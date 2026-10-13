// This file is part of midnight-ledger.
// Copyright (C) 2026 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");
// You may not use this file except in compliance with the License.
// You may obtain a copy of the License at
// http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::{
    collections::HashSet,
    time::{Duration, Instant},
};

use crate::{
    arena::ArenaHash,
    backend::OnDiskObject,
    db::{DB, Update},
};

// A through-origin linear regression, we just keep the sum of x and y values.
#[derive(Debug, Default)]
struct IncrementalLinRegress {
    sum_x: f64,
    sum_y: f64,
}

impl IncrementalLinRegress {
    fn measure(&mut self, x: f64, y: f64) {
        self.sum_x += x;
        self.sum_y += y;
    }
    fn predict(&self, x: f64) -> Option<f64> {
        let a = self.sum_y / self.sum_x;
        let y = a * x;
        y.is_finite().then_some(y)
    }
}

#[derive(Debug, Default)]
struct RunningBenchmark {
    read_model: IncrementalLinRegress,
    scan_model: IncrementalLinRegress,
}

const DEFAULT_BATCH_SIZE: usize = 128;
const BATCH_LIMIT: usize = 4096;

impl RunningBenchmark {
    // During mark phase, how many children to scan *on disk* in one batch
    fn read_batch_size(&self, budget: Duration) -> Option<usize> {
        if budget.is_zero() {
            return None;
        }
        match self.read_model.predict(budget.as_micros() as f64) {
            Some(batch) if batch > BATCH_LIMIT as f64 => Some(BATCH_LIMIT),
            Some(batch) if batch > 0f64 => Some(batch.ceil() as usize),
            None => Some(DEFAULT_BATCH_SIZE),
            Some(_) => None,
        }
    }

    fn read_batch_measurement(&mut self, batch_size: usize, took: Duration) {
        self.read_model
            .measure(took.as_micros() as f64, batch_size as f64);
    }

    // During sweep phase, how many children to scan *on disk* in one batch, *including* deletions
    // for this batch
    fn scan_batch_size(&self, budget: Duration) -> Option<usize> {
        if budget.is_zero() {
            return None;
        }
        match self.scan_model.predict(budget.as_micros() as f64) {
            Some(batch) if batch > BATCH_LIMIT as f64 => Some(BATCH_LIMIT),
            Some(batch) if batch > 1f64 => Some(batch.ceil() as usize),
            None => Some(DEFAULT_BATCH_SIZE),
            Some(_) => None,
        }
    }

    fn scan_batch_measurement(&mut self, batch_size: usize, took: Duration) {
        self.scan_model
            .measure(took.as_micros() as f64, batch_size as f64);
    }
}

/// Incremental mark & sweep state.
///
/// The root set (`last_roots`) is maintained incrementally between cycle boundaries: root-count
/// changes flushed to the DB are reported via `note_root_count`, and in-memory roots are added on
/// every `run`. A full read of all DB roots (`get_roots`) happens only at init and once per
/// completed sweep cycle, not after each flush.
#[derive(Debug)]
pub(crate) struct GcState<D: DB> {
    rescan: bool,
    last_roots: HashSet<ArenaHash<D::Hasher>>,
    grey_set: HashSet<ArenaHash<D::Hasher>>,
    mark_set: HashSet<ArenaHash<D::Hasher>>,
    sweep_resume: Option<D::ScanResumeHandle>,
    running_bench: RunningBenchmark,
}

impl<D: DB> Default for GcState<D> {
    fn default() -> Self {
        GcState {
            rescan: true,
            last_roots: Default::default(),
            grey_set: Default::default(),
            mark_set: Default::default(),
            sweep_resume: None,
            running_bench: Default::default(),
        }
    }
}

impl<D: DB> GcState<D> {
    /// Notes that the root count of `key` has been set to `new_count` in the DB.
    ///
    /// A positive count adds the node to the root set, and greys it if not yet marked, so the next
    /// `run` marks its subtree before sweeping. A zero count removes it from the root set only;
    /// anything it kept alive is reconciled by the full rescan after a completed sweep. Does not
    /// trigger a rescan. While a rescan is pending this is a no-op, as the rescan reads the DB.
    pub(crate) fn note_root_count(&mut self, key: &ArenaHash<D::Hasher>, new_count: u32) {
        if self.rescan {
            return;
        }
        if new_count > 0 {
            self.last_roots.insert(key.clone());
            if !self.mark_set.contains(key) {
                self.grey_set.insert(key.clone());
            }
        } else {
            self.last_roots.remove(key);
        }
    }

    #[cfg(test)]
    pub(crate) fn sweep_in_progress(&self) -> bool {
        self.sweep_resume.is_some()
    }

    #[cfg(test)]
    pub(crate) fn rescan_pending(&self) -> bool {
        self.rescan
    }

    pub(crate) fn run<'a, 'b: 'a>(
        &'b mut self,
        roots: impl Iterator<Item = ArenaHash<D::Hasher>>,
        remaining_budget: impl Fn() -> Duration,
        db: &'b mut D,
        cache_read: impl Fn(ArenaHash<D::Hasher>) -> Option<&'a OnDiskObject<D::Hasher>>,
        db_roots: impl for<'c> FnOnce(&'c mut D) -> Vec<ArenaHash<D::Hasher>>,
    ) -> Vec<ArenaHash<D::Hasher>> {
        // First, we need to update our root set. We take the new root set `roots`, and note any
        // additions to add to the grey set. Deletions are not handled, as they may still be on
        // disk.
        //
        // If `rescan` is true, we instead go fetch the full root set from disk, and init
        // `last_roots` entirely from there.
        if self.rescan {
            self.last_roots = roots.chain(db_roots(db)).collect();
            self.grey_set.extend(
                self.last_roots
                    .iter()
                    .filter(|r| !self.mark_set.contains(r))
                    .cloned(),
            );
            self.rescan = false;
        } else {
            for root in roots {
                if self.last_roots.insert(root.clone()) {
                    self.grey_set.insert(root);
                }
            }
        }

        // Next, we do the mark phase. We *always* do some marking and some sweeping, although one
        // or the other may get zero budget. In the case of mark, we process any grey nodes, which
        // will eventually be empty or processable within one bound due to the delta between roots
        // being small.
        //
        // We operate by first splitting the grey set into a in-cache portion, and an out-of-cache
        // portion. We assume that our bound is high enough to always process the full in-cache
        // portion.
        //
        // We iteratively process the in-cache grey set until *only* the out-of-cache grey set
        // remains. Then we iteratively process this in batch sizes provided by `RunningBenchmark`
        // until we hit our bound.
        let mut to_process = self.grey_set.clone();
        while !to_process.is_empty() {
            let mut next = HashSet::new();
            for hash in to_process.into_iter() {
                if let Some(obj) = cache_read(hash.clone()) {
                    self.mark_set.insert(hash.clone());
                    self.grey_set.remove(&hash);
                    for child in obj.children.iter().flat_map(|c| c.refs().into_iter()) {
                        if !self.mark_set.contains(child) && !self.grey_set.contains(child) {
                            self.grey_set.insert(child.clone());
                            next.insert(child.clone());
                        }
                    }
                }
            }
            to_process = next;
        }
        // Now the grey set is purely on disk. Heuristically, we assume that all references on disk
        // are *also* on disk, just because it simplifies the flow.
        while !self.grey_set.is_empty()
            && let Some(batch_size) = self.running_bench.read_batch_size(remaining_budget())
        {
            let batch_start = Instant::now();
            let batch: Vec<_> = self.grey_set.iter().take(batch_size).cloned().collect();
            let batch_size = batch.len();
            let batch_read = db.batch_get_nodes(batch.iter().cloned());
            self.mark_set.extend(batch);
            for (parent, obj) in batch_read {
                self.grey_set.remove(&parent);
                self.mark_set.insert(parent);
                for child in obj
                    .iter()
                    .flat_map(|o| o.children.iter())
                    .flat_map(|c| c.refs().into_iter())
                {
                    if !self.mark_set.contains(child) && !self.grey_set.contains(child) {
                        self.grey_set.insert(child.clone());
                    }
                }
            }
            self.running_bench
                .read_batch_measurement(batch_size, batch_start.elapsed());
        }
        //eprintln!("MARK PHASE END: {}", self.grey_set.len());

        // If after all this, the grey set is not empty, we've ran up to our bound, and stop for
        // now.
        if !self.grey_set.is_empty() {
            return vec![];
        }

        // Whew. We're now done with the mark phase, and we want to do sweeping. We use the resume
        // handle if we have one, and stop when we run out of budget or the scan completes.
        let mut cull_set = vec![];
        // Set only once a scan reports the end of the node set. A sweep that gets no budget at
        // all leaves `sweep_resume` at `None` too, and must not be taken for a completed one.
        let mut sweep_finished = false;
        while let Some(batch_size) = self.running_bench.scan_batch_size(remaining_budget()) {
            let batch_start = Instant::now();
            let (nodes, handle) = db.scan(self.sweep_resume.clone(), batch_size);
            self.sweep_resume = handle;
            let batch_size = nodes.len();
            // Cull all nodes in the batch that are *not* marked.
            let to_delete = nodes
                .into_iter()
                .map(|(k, _)| k)
                .filter(|k| !self.mark_set.contains(k))
                .collect::<Vec<_>>();
            db.batch_update(to_delete.iter().map(|k| (k.clone(), Update::DeleteNode)));
            cull_set.extend(to_delete);
            self.running_bench
                .scan_batch_measurement(batch_size, batch_start.elapsed());

            // We have finished!
            if self.sweep_resume.is_none() {
                sweep_finished = true;
                break;
            }
        }
        //eprintln!("SWEEP PHASE END: {} (resume: {:?})", cull_set.len(), self.sweep_resume);
        // We ran against our bound, return.
        if !sweep_finished {
            return cull_set;
        }

        // We completed a sweep! We reset to a clean state. We must rescan on
        // the next cycle, because `roots` (live_inserts) may not include all DB
        // roots (nodes with root_count > 0 that have no in-memory Sp).
        self.rescan = true;
        self.last_roots = HashSet::new();
        self.mark_set = HashSet::new();
        // NOTE: grey_set is already known empyt, sweep_resume is known None
        // running_bench is deliberately kept.
        cull_set
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_gc() {
        use crate::db::DB;
        use std::time::Duration;
        let arena = crate::arena::Arena::new_from_backend(crate::backend::StorageBackend::new(
            1,
            crate::db::InMemoryDB::<crate::DefaultHasher>::default(),
        ));
        let size = || arena.with_backend(|b| b.database.size());
        const CHUNK: usize = 10_000;
        let mut refs = (0..10 * CHUNK)
            .map(|i| arena.alloc(i as u64))
            .collect::<Vec<_>>();
        refs.iter_mut().for_each(|r| r.persist());
        arena.with_backend(|b| b.flush_all_changes_to_db());
        assert_eq!(size(), 10 * CHUNK);
        // Because everything's persisted, gc does nothing.
        assert_eq!(arena.with_backend(|b| b.gc(Duration::from_hours(1))), 0);
        assert_eq!(size(), 10 * CHUNK);
        // If we unpersist the last 1000 entries, gc *still* does nothing, because they are still
        // referenced in memory.
        refs[9 * CHUNK..10 * CHUNK]
            .iter_mut()
            .for_each(|r| r.unpersist());
        arena.with_backend(|b| b.flush_all_changes_to_db());
        assert_eq!(arena.with_backend(|b| b.gc(Duration::from_hours(1))), 0);
        assert_eq!(size(), 10 * CHUNK);
        // If we now drop them from in-memory, they *will* be gc'd
        refs.truncate(9 * CHUNK);
        assert_eq!(arena.with_backend(|b| b.gc(Duration::from_hours(1))), CHUNK);
        assert_eq!(size(), 9 * CHUNK);
        // However, if we *just* drop references from memory, they are still protected due to
        // persistence.
        refs.truncate(8 * CHUNK);
        assert_eq!(arena.with_backend(|b| b.gc(Duration::from_hours(1))), 0);
        assert_eq!(size(), 9 * CHUNK);
        // If we give a small budget for the gc, it will not complete in one run through.
        // We use gc_budgeted with a closure controlling the batch count, avoiding any
        // real-time dependency. Duration::ZERO prevents all on-disk work (mark and sweep),
        // while with a cache size of 1 the in-cache mark phase processes almost nothing.
        refs[5 * CHUNK..8 * CHUNK]
            .iter_mut()
            .for_each(|r| r.unpersist());
        arena.with_backend(|b| b.flush_all_changes_to_db());
        refs.truncate(5 * CHUNK);
        assert_eq!(arena.with_backend(|b| b.gc_budgeted(|| Duration::ZERO)), 0);
        assert_eq!(size(), 9 * CHUNK);
        // But if ran repeatedly with a small per-call batch budget, it *will* run through.
        let batch_counter = std::cell::Cell::new(0usize);
        let budget = || {
            let n = batch_counter.get();
            batch_counter.set(n + 1);
            if n < 10 {
                Duration::MAX
            } else {
                Duration::ZERO
            }
        };
        let mut culled = 0;
        for _ in 0..100 {
            batch_counter.set(0);
            culled += arena.with_backend(|b| b.gc_budgeted(budget));
        }
        assert_eq!(culled, 3 * CHUNK);
        assert_eq!(size(), 6 * CHUNK);
    }

    mod late_roots {
        use crate::arena::{Arena, ArenaHash, Sp, bin_tree::BinTree};
        use crate::backend::StorageBackend;
        use crate::db::{DB, InMemoryDB};
        use std::collections::HashSet;
        use std::time::Duration;

        type Db = InMemoryDB<crate::DefaultHasher>;
        type A = Arena<Db>;
        type H = ArenaHash<crate::DefaultHasher>;

        fn new_arena() -> A {
            Arena::new_from_backend(StorageBackend::new(1, Db::default()))
        }

        fn size(a: &A) -> usize {
            a.with_backend(|b| b.database.size())
        }

        /// A budget allowing only a couple of batches per call.
        fn small_budget() -> impl Fn() -> Duration {
            let n = std::cell::Cell::new(0usize);
            move || {
                let i = n.get();
                n.set(i + 1);
                if i < 2 { Duration::MAX } else { Duration::ZERO }
            }
        }

        /// Persisted nodes (roots, kept) plus unreferenced garbage, all flushed and dropped.
        fn populate(a: &A, kept: usize, garbage: usize) {
            let mut sps = (0..(kept + garbage) as u64)
                .map(|i| a.alloc(i))
                .collect::<Vec<_>>();
            sps.iter_mut().for_each(|s| s.persist());
            a.with_backend(|b| b.flush_all_changes_to_db());
            sps[kept..].iter_mut().for_each(|s| s.unpersist());
            a.with_backend(|b| b.flush_all_changes_to_db());
        }

        /// Drive small-budget gc calls until a sweep is mid-way.
        fn drive_to_mid_sweep(a: &A) {
            for _ in 0..1000 {
                a.with_backend(|b| b.gc_budgeted(small_budget()));
                if a.with_backend(|b| b.gc_state().sweep_in_progress()) {
                    return;
                }
            }
            panic!("never reached mid-sweep");
        }

        /// Runs two full cycles (the second collects anything protected only by the first).
        fn gc_to_completion(a: &A) {
            for _ in 0..2 {
                for _ in 0..100_000 {
                    a.with_backend(|b| b.gc_budgeted(small_budget()));
                    if a.with_backend(|b| b.gc_state().rescan_pending()) {
                        break;
                    }
                }
                assert!(a.with_backend(|b| b.gc_state().rescan_pending()));
            }
        }

        fn subtree(a: &A, root: &H) -> HashSet<H> {
            let mut seen = HashSet::new();
            let mut todo = vec![root.clone()];
            while let Some(h) = todo.pop() {
                if !seen.insert(h.clone()) {
                    continue;
                }
                if let Some(obj) = a.with_backend(|b| b.database.get_node(&h)) {
                    for c in obj.children.iter().flat_map(|c| c.refs().into_iter()) {
                        todo.push(c.clone());
                    }
                }
            }
            seen
        }

        fn assert_all_in_db(a: &A, nodes: &HashSet<H>) {
            for h in nodes {
                assert!(
                    a.with_backend(|b| b.database.get_node(h)).is_some(),
                    "node {h:?} was deleted"
                );
            }
        }

        /// Persist a wrapper `W` over a fresh subtree, flush, and drop every `Sp`.
        fn persist_wrapper(a: &A, seed: u64) -> H {
            let leaf = |v| Some(a.alloc(BinTree::<Db>::new(v, None, None)));
            let mut w: Sp<BinTree<Db>, Db> = a.alloc(BinTree::new(
                seed,
                Some(a.alloc(BinTree::new(seed + 1, leaf(seed + 2), leaf(seed + 3)))),
                leaf(seed + 4),
            ));
            w.persist();
            let h = w.hash();
            a.with_backend(|b| b.flush_all_changes_to_db());
            h
        }

        #[test]
        fn late_root_survives_in_progress_sweep() {
            let a = new_arena();
            populate(&a, 2_000, 6_000);
            drive_to_mid_sweep(&a);
            let w = persist_wrapper(&a, 1_000_000);
            let reachable = subtree(&a, &w);
            assert!(reachable.len() >= 5);
            gc_to_completion(&a);
            assert_all_in_db(&a, &reachable);
            assert_eq!(a.with_backend(|b| b.get_root_count(&w)), 1);
            assert!(a.with_backend(|b| b.get_roots()).contains_key(&w));
            assert_eq!(size(&a), 2_000 + reachable.len());
        }

        #[test]
        fn late_root_children_sorting_before_root_survive() {
            let a = new_arena();
            populate(&a, 2_000, 6_000);
            drive_to_mid_sweep(&a);
            // Persist several wrappers, and require that for at least one, a descendant's key sorts
            // before the root's key.
            let mut all = HashSet::new();
            let mut roots = vec![];
            let mut child_before_root = false;
            for i in 0..20u64 {
                let w = persist_wrapper(&a, 10_000_000 + i * 10);
                let sub = subtree(&a, &w);
                child_before_root |= sub.iter().any(|h| *h < w);
                all.extend(sub);
                roots.push(w);
            }
            assert!(child_before_root);
            gc_to_completion(&a);
            assert_all_in_db(&a, &all);
            let db_roots = a.with_backend(|b| b.get_roots());
            for r in &roots {
                assert!(db_roots.contains_key(r));
            }
            assert_eq!(size(&a), 2_000 + all.len());
        }

        /// Excluding persisted roots via `gc_override_gc_roots` collects them together with their
        /// whole subtree: no surviving node may reference a deleted one.
        #[test]
        fn override_excluded_roots_are_collected_with_their_subtrees() {
            let a = new_arena();
            populate(&a, 2_000, 6_000);
            let mut excluded = HashSet::new();
            let mut child_before_root = false;
            for i in 0..20u64 {
                let w = persist_wrapper(&a, 10_000_000 + i * 10);
                child_before_root |= subtree(&a, &w).iter().any(|h| *h < w);
                excluded.insert(w);
            }
            assert!(child_before_root);
            for _ in 0..2 {
                for _ in 0..100_000 {
                    let ex = excluded.clone();
                    a.with_backend(|b| {
                        b.gc_budgeted_with_roots(small_budget(), move |db| {
                            db.get_roots()
                                .keys()
                                .filter(|k| !ex.contains(*k))
                                .cloned()
                                .collect()
                        })
                    });
                    if a.with_backend(|b| b.gc_state().rescan_pending()) {
                        break;
                    }
                }
                assert!(a.with_backend(|b| b.gc_state().rescan_pending()));
            }
            for w in &excluded {
                assert!(a.with_backend(|b| b.database.get_node(w)).is_none());
            }
            assert_eq!(size(&a), 2_000);
        }

        /// A mark phase that uses up the whole budget leaves no budget for the sweep. That must
        /// not count as a completed cycle: the marks are kept, and the next call sweeps without
        /// re-reading the roots.
        #[test]
        fn mark_survives_sweep_with_no_budget() {
            // A cache large enough that marking happens entirely in memory, with zero budget.
            let a: A = Arena::new_from_backend(StorageBackend::new(100_000, Db::default()));
            populate(&a, 100, 900);
            let calls = std::cell::Cell::new(0usize);
            a.with_backend(|b| {
                b.gc_budgeted_with_roots(
                    || Duration::ZERO,
                    |db| {
                        calls.set(calls.get() + 1);
                        db.get_roots().keys().cloned().collect()
                    },
                )
            });
            assert_eq!(calls.get(), 1);
            assert_eq!(size(&a), 1_000, "no budget, so nothing swept");
            assert!(
                !a.with_backend(|b| b.gc_state().rescan_pending()),
                "an unstarted sweep must not complete the cycle"
            );
            for _ in 0..100_000 {
                run_counting(&a, &calls);
                if a.with_backend(|b| b.gc_state().rescan_pending()) {
                    break;
                }
            }
            assert!(a.with_backend(|b| b.gc_state().rescan_pending()));
            assert_eq!(calls.get(), 1, "the cycle completed on the first root read");
            assert_eq!(size(&a), 100);
        }

        /// The first completed cycle on a fresh GC state has actually swept, with one root read.
        #[test]
        fn first_cycle_sweeps_with_one_root_read() {
            let a = new_arena();
            populate(&a, 2_000, 6_000);
            let calls = std::cell::Cell::new(0usize);
            for _ in 0..100_000 {
                run_counting(&a, &calls);
                if a.with_backend(|b| b.gc_state().rescan_pending()) {
                    break;
                }
            }
            assert!(a.with_backend(|b| b.gc_state().rescan_pending()));
            assert_eq!(calls.get(), 1);
            assert_eq!(size(&a), 2_000);
        }

        fn run_counting(a: &A, calls: &std::cell::Cell<usize>) {
            a.with_backend(|b| {
                b.gc_budgeted_with_roots(small_budget(), |db| {
                    calls.set(calls.get() + 1);
                    db.get_roots().keys().cloned().collect()
                })
            });
        }

        #[test]
        fn root_persisted_mid_cycle_is_marked_without_full_rescan() {
            let a = new_arena();
            populate(&a, 2_000, 6_000);
            let calls = std::cell::Cell::new(0usize);
            for _ in 0..1000 {
                run_counting(&a, &calls);
                if a.with_backend(|b| b.gc_state().sweep_in_progress()) {
                    break;
                }
            }
            assert!(a.with_backend(|b| b.gc_state().sweep_in_progress()));
            // One full get_roots per cycle started so far (a small cycle may complete early).
            let at_mid = calls.get();
            let w = persist_wrapper(&a, 1_000_000);
            let reachable = subtree(&a, &w);
            // The flush must not have forced a rescan.
            assert!(!a.with_backend(|b| b.gc_state().rescan_pending()));
            run_counting(&a, &calls);
            assert_eq!(
                calls.get(),
                at_mid,
                "no get_roots right after a mid-cycle flush"
            );
            // Finish the cycle.
            for _ in 0..100_000 {
                if a.with_backend(|b| b.gc_state().rescan_pending()) {
                    break;
                }
                run_counting(&a, &calls);
            }
            assert!(a.with_backend(|b| b.gc_state().rescan_pending()));
            assert_eq!(calls.get(), at_mid, "no get_roots until the cycle boundary");
            assert_all_in_db(&a, &reachable);
            assert_eq!(a.with_backend(|b| b.get_root_count(&w)), 1);
            // The next call starts a new cycle, with exactly one more full read.
            run_counting(&a, &calls);
            assert_eq!(calls.get(), at_mid + 1);
            eprintln!(
                "get_roots invocations: mid-cycle {at_mid}, final {}",
                calls.get()
            );
        }

        #[test]
        fn root_released_mid_cycle_is_collected_next_cycle() {
            let a = new_arena();
            let mut sps = (0..8_000u64).map(|i| a.alloc(i)).collect::<Vec<_>>();
            sps.iter_mut().for_each(|s| s.persist());
            a.with_backend(|b| b.flush_all_changes_to_db());
            sps[2_000..].iter_mut().for_each(|s| s.unpersist());
            a.with_backend(|b| b.flush_all_changes_to_db());
            sps.truncate(2_000);
            let victim = sps.pop().unwrap();
            let victim_hash = victim.hash();
            drive_to_mid_sweep(&a);
            victim.unpersist();
            a.with_backend(|b| b.flush_all_changes_to_db());
            drop(victim);
            drop(sps);
            gc_to_completion(&a);
            assert!(
                a.with_backend(|b| b.database.get_node(&victim_hash))
                    .is_none()
            );
            assert_eq!(size(&a), 1_999);
        }

        #[test]
        fn unpersisted_garbage_is_still_collected() {
            let a = new_arena();
            populate(&a, 100, 900);
            assert_eq!(size(&a), 1_000);
            gc_to_completion(&a);
            assert_eq!(size(&a), 100);
        }
    }
}
