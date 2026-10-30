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
    backend::{GcLimits, OnDiskObject},
    db::{DB, Update},
};
// `OnDiskObject` is still needed for the in-cache mark (`cache_read`).

/// Maximum number of keys handed to the database in one call, in both the on-disk mark phase
/// (`batch_get_children`) and the sweep phase (`scan_keys` plus the matching deletions). The time
/// budget is checked before every such call, so this bounds how far one call can run past its
/// budget: by one chunk's worth of database work, even when the per-key cost estimate is stale
/// (for example under sudden disk contention).
pub(crate) const CHUNK: usize = 256;

/// Number of grey nodes the in-cache mark phase visits between two budget checks. Visiting a node
/// is a cache lookup plus a few hash-set operations, so this is far cheaper than a disk chunk.
pub(crate) const CACHE_CHUNK: usize = 1024;

/// Weight of the newest measurement in [`CostEstimate`].
const EWMA_ALPHA: f64 = 0.25;

/// Exponentially weighted estimate of the per-key cost of one kind of database call.
///
/// We keep weighted averages of both the elapsed time and the number of keys and predict with
/// their ratio, which is a through-origin regression over recent chunks. Unlike the previous
/// all-time regression it follows changes in disk latency within a few chunks, and unlike a
/// per-chunk `time / keys` average a small final chunk (with its fixed per-call overhead) does not
/// skew the estimate.
#[derive(Debug, Default)]
struct CostEstimate {
    // `None` until the first measurement.
    ewma: Option<(f64, f64)>,
}

impl CostEstimate {
    /// How many keys to process in the next database call, given the budget left. `None` means
    /// stop: the budget is exhausted. Otherwise at least one key, at most [`CHUNK`], and no more
    /// than the estimate says fits in the remaining budget.
    fn chunk_size(&self, remaining: Duration) -> Option<usize> {
        if remaining.is_zero() {
            return None;
        }
        let Some((us, keys)) = self.ewma else {
            return Some(CHUNK);
        };
        let fit = remaining.as_micros() as f64 * keys / us;
        if !fit.is_finite() || fit >= CHUNK as f64 {
            return Some(CHUNK);
        }
        Some((fit.ceil() as usize).clamp(1, CHUNK))
    }

    fn measure(&mut self, keys: usize, took: Duration) {
        if keys == 0 {
            return;
        }
        // Floor at 1µs so a sub-microsecond chunk does not make the estimate infinite.
        let sample = ((took.as_micros() as f64).max(1.0), keys as f64);
        self.ewma = Some(match self.ewma {
            None => sample,
            Some((us, k)) => (
                us + EWMA_ALPHA * (sample.0 - us),
                k + EWMA_ALPHA * (sample.1 - k),
            ),
        });
    }
}

#[derive(Debug, Default)]
struct RunningBenchmark {
    read: CostEstimate,
    scan: CostEstimate,
}

#[derive(Debug)]
pub(crate) struct GcState<D: DB> {
    rescan: bool,
    last_roots: HashSet<ArenaHash<D::Hasher>>,
    /// All reached-but-unprocessed nodes. Source of truth for the mark phase.
    grey_set: HashSet<ArenaHash<D::Hasher>>,
    /// Grey nodes still to be tried against the cache, as a depth-first work stack.
    ///
    /// Invariant: every entry is also in `grey_set` (nodes are inserted into `grey_set` before
    /// being pushed, and only leave `grey_set` by being popped here or, once this stack is empty,
    /// by the on-disk mark). Stopping with a non-empty stack therefore loses nothing, and the
    /// stack is kept across calls so the next call resumes where this one stopped instead of
    /// re-seeding from the whole grey set.
    cache_stack: Vec<ArenaHash<D::Hasher>>,
    mark_set: HashSet<ArenaHash<D::Hasher>>,
    sweep_resume: Option<D::ScanResumeHandle>,
    running_bench: RunningBenchmark,
    /// Number of keys handed to the database (mark reads plus sweep scans), for tests.
    #[cfg(test)]
    disk_keys_processed: usize,
    /// Number of grey nodes visited by the in-cache mark, for tests.
    #[cfg(test)]
    cache_nodes_visited: usize,
}

impl<D: DB> Default for GcState<D> {
    fn default() -> Self {
        GcState {
            rescan: true,
            last_roots: Default::default(),
            grey_set: Default::default(),
            cache_stack: Default::default(),
            mark_set: Default::default(),
            sweep_resume: None,
            running_bench: Default::default(),
            #[cfg(test)]
            disk_keys_processed: 0,
            #[cfg(test)]
            cache_nodes_visited: 0,
        }
    }
}

impl<D: DB> GcState<D> {
    pub(crate) fn force_rescan(&mut self) {
        self.rescan = true;
    }

    /// Adds `node` to the grey set if it is neither marked nor already grey, and queues it for the
    /// in-cache mark.
    fn shade(&mut self, node: ArenaHash<D::Hasher>) {
        if !self.mark_set.contains(&node) && self.grey_set.insert(node.clone()) {
            self.cache_stack.push(node);
        }
    }

    pub(crate) fn run<'a, 'b: 'a>(
        &'b mut self,
        roots: impl Iterator<Item = ArenaHash<D::Hasher>>,
        remaining_budget: impl Fn() -> Duration,
        limits: GcLimits,
        db: &'b mut D,
        cache_read: impl Fn(ArenaHash<D::Hasher>) -> Option<&'a OnDiskObject<D::Hasher>>,
        db_roots: impl for<'c> FnOnce(&'c mut D) -> Vec<ArenaHash<D::Hasher>>,
    ) -> Vec<ArenaHash<D::Hasher>> {
        // First, we need to update our root set. We take the new root set `roots`, and note any
        // additions to add to the grey set. Deletions are not handled, as they may still be on
        // disk.
        //
        // If `rescan` is true, we instead go fetch the full root set from disk, and init
        // `last_roots` entirely from there. `clear` + `extend` keeps the set's allocation.
        if self.rescan {
            self.last_roots.clear();
            self.last_roots.extend(roots.chain(db_roots(db)));
            let last_roots = std::mem::take(&mut self.last_roots);
            for root in last_roots.iter() {
                self.shade(root.clone());
            }
            self.last_roots = last_roots;
            self.rescan = false;
        } else {
            for root in roots {
                if self.last_roots.insert(root.clone()) {
                    self.shade(root);
                }
            }
        }

        // Next, we do the mark phase. In-cache grey nodes are processed first, depth-first from
        // `cache_stack`; whatever misses the cache stays in the grey set for the on-disk mark.
        // The budget is checked every `CACHE_CHUNK` nodes.
        //
        // If we run out of budget here we must return straight away, and not fall through to the
        // on-disk mark: a node that is only in the write cache (not yet flushed) reads as missing
        // on disk, and would be marked without its children being traversed.
        while !self.cache_stack.is_empty() {
            if remaining_budget().is_zero() {
                return vec![];
            }
            for _ in 0..CACHE_CHUNK {
                let Some(hash) = self.cache_stack.pop() else {
                    break;
                };
                #[cfg(test)]
                {
                    self.cache_nodes_visited += 1;
                }
                if let Some(obj) = cache_read(hash.clone()) {
                    self.grey_set.remove(&hash);
                    self.mark_set.insert(hash);
                    for child in obj.children.iter().flat_map(|c| c.refs().into_iter()) {
                        self.shade(child.clone());
                    }
                }
            }
        }

        // Now the grey set is purely on disk. Heuristically, we assume that all references on disk
        // are *also* on disk, just because it simplifies the flow, so children found here go to
        // the grey set only, not to the cache stack.
        while !self.grey_set.is_empty()
            && let Some(chunk_size) = self.running_bench.read.chunk_size(remaining_budget())
        {
            let chunk_start = Instant::now();
            let chunk: Vec<_> = self.grey_set.iter().take(chunk_size).cloned().collect();
            #[cfg(test)]
            {
                self.disk_keys_processed += chunk.len();
            }
            // Children only: marking never needs an object's data.
            let chunk_read = db.batch_get_children(chunk.iter().cloned());
            // Every key we asked for is marked, including keys the database does not have (read as
            // `None`), so a missing node can never stay grey forever.
            for key in chunk.iter() {
                self.grey_set.remove(key);
            }
            let chunk_len = chunk.len();
            self.mark_set.extend(chunk);
            for (_, children) in chunk_read {
                for child in children.iter().flatten().flat_map(|c| c.refs().into_iter()) {
                    if !self.mark_set.contains(child) && !self.grey_set.contains(child) {
                        self.grey_set.insert(child.clone());
                    }
                }
            }
            self.running_bench
                .read
                .measure(chunk_len, chunk_start.elapsed());
        }

        // If after all this, the grey set is not empty, we've ran up to our bound, and stop for
        // now. No sweep chunk may run while anything is grey.
        if !self.grey_set.is_empty() {
            return vec![];
        }

        // Whew. We're now done with the mark phase, and we want to do sweeping. We use the resume
        // handle if we have one, and stop when we run out of budget, hit the deletion cap, or the
        // scan completes.
        //
        // Deletion cap and the resume handle: the handle `scan_keys` returns is derived by the
        // backend from the *whole* returned batch (it points after its last key), and its type is
        // backend specific, so we cannot resume from the middle of a returned batch. We therefore
        // never scan more keys than we may still delete: each scan asks for at most
        // `max_deletes - deleted` keys. Every scanned key is then fully processed (kept if marked,
        // deleted otherwise), the deletions of one call never exceed the cap, and the stored
        // handle always points just after the last processed key, so nothing is skipped or
        // visited twice. The price is that marked keys in a scan count against the cap as scan
        // budget, so a call may delete fewer than `max_deletes` keys.
        let mut cull_set = vec![];
        let mut sweep_done = false;
        loop {
            let cap_left = limits
                .max_deletes
                .map(|cap| cap.saturating_sub(cull_set.len()));
            if cap_left == Some(0) {
                break;
            }
            let Some(chunk_size) = self.running_bench.scan.chunk_size(remaining_budget()) else {
                break;
            };
            let chunk_size = cap_left.map_or(chunk_size, |cap_left| chunk_size.min(cap_left));
            let chunk_start = Instant::now();
            // Keys only: the sweep never needs the objects.
            let (keys, handle) = db.scan_keys(self.sweep_resume.clone(), chunk_size);
            self.sweep_resume = handle;
            let chunk_len = keys.len();
            #[cfg(test)]
            {
                self.disk_keys_processed += chunk_len;
            }
            // Cull all nodes in the chunk that are *not* marked.
            let to_delete = keys
                .into_iter()
                .filter(|k| !self.mark_set.contains(k))
                .collect::<Vec<_>>();
            db.batch_update(to_delete.iter().map(|k| (k.clone(), Update::DeleteNode)));
            cull_set.extend(to_delete);
            self.running_bench
                .scan
                .measure(chunk_len, chunk_start.elapsed());

            // We have finished!
            if self.sweep_resume.is_none() {
                sweep_done = true;
                break;
            }
        }
        //eprintln!("SWEEP PHASE END: {} (resume: {:?})", cull_set.len(), self.sweep_resume);
        // We ran against our bound or the deletion cap, return. Note that `sweep_resume` is also
        // `None` when the sweep has not started yet, so only a scan that actually came back
        // without a handle counts as completion; otherwise a call whose budget ran out right
        // after the mark phase would throw the finished mark set away.
        if !sweep_done {
            return cull_set;
        }

        // We completed a sweep! We reset to a clean state. We must rescan on
        // the next cycle, because `roots` (live_inserts) may not include all DB
        // roots (nodes with root_count > 0 that have no in-memory Sp).
        // `clear` keeps the allocations, so the next cycle does not regrow (and rehash) them.
        self.rescan = true;
        self.last_roots.clear();
        self.mark_set.clear();
        // NOTE: grey_set and cache_stack are already known empty, sweep_resume is known None.
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

    use super::{CACHE_CHUNK, CHUNK, GcState};
    use crate::{
        arena::{ArenaHash, ArenaKey},
        backend::{GcLimits, OnDiskObject},
        db::{DB, InMemoryDB, Update},
    };
    use std::{
        cell::Cell,
        collections::{HashMap, HashSet},
        time::Duration,
    };

    type H = crate::DefaultHasher;
    type Db = InMemoryDB<H>;
    type Cache = HashMap<ArenaHash<H>, OnDiskObject<H>>;

    fn node(tag: u64, children: &[ArenaHash<H>]) -> (ArenaHash<H>, OnDiskObject<H>) {
        let data = tag.to_le_bytes().to_vec();
        let key = crate::arena::hash(&data, children.iter());
        let children = children.iter().cloned().map(ArenaKey::Ref).collect();
        (key, OnDiskObject { data, children })
    }

    /// A budget that allows `k` chunks of work, then nothing.
    fn first_k(k: usize) -> impl Fn() -> Duration {
        let calls = Cell::new(0usize);
        move || {
            let n = calls.get();
            calls.set(n + 1);
            if n < k { Duration::MAX } else { Duration::ZERO }
        }
    }

    /// Test graph: one root (passed as a live insert) whose `fanout` children are interior nodes,
    /// each pointing at its own on-disk-only subtree of `depth` levels. With `root_in_cache`, the
    /// root and the interior nodes are cache-only (like unflushed write-cache entries, never on
    /// disk); otherwise they are on disk, and the interior nodes are also cached (like read-cache
    /// entries), since an on-disk node's children are always on disk. A persisted on-disk root with an on-disk
    /// chain, and `garbage` unreachable on-disk nodes, some of which point into live nodes.
    struct Graph {
        db: Db,
        cache: Cache,
        live_root: ArenaHash<H>,
        live: HashSet<ArenaHash<H>>,
        garbage: HashSet<ArenaHash<H>>,
    }

    fn graph(fanout: u64, depth: u64, garbage: u64, root_in_cache: bool) -> Graph {
        let mut db = Db::default();
        let mut cache = Cache::new();
        // Insertion order, so that two graphs built with the same arguments are identical.
        let mut live = vec![];
        let mut tag = 0u64;
        let mut fresh = || {
            tag += 1;
            tag
        };
        let mut interior = vec![];
        for _ in 0..fanout {
            let mut below: Vec<ArenaHash<H>> = vec![];
            for _ in 0..depth {
                let (k, o) = node(fresh(), &below);
                db.insert_node(k.clone(), o);
                live.push(k.clone());
                below = vec![k];
            }
            let (k, o) = node(fresh(), &below);
            if !root_in_cache {
                db.insert_node(k.clone(), o.clone());
            }
            cache.insert(k.clone(), o);
            live.push(k.clone());
            interior.push(k);
        }
        let (live_root, o) = node(fresh(), &interior);
        if root_in_cache {
            cache.insert(live_root.clone(), o);
        } else {
            db.insert_node(live_root.clone(), o);
        }
        live.push(live_root.clone());
        // A persisted on-disk root with a chain below it.
        let mut below: Vec<ArenaHash<H>> = vec![];
        for _ in 0..50 {
            let (k, o) = node(fresh(), &below);
            db.insert_node(k.clone(), o);
            live.push(k.clone());
            below = vec![k];
        }
        db.batch_update(std::iter::once((below[0].clone(), Update::SetRootCount(1))));
        let mut garbage_set = HashSet::new();
        for i in 0..garbage {
            let children: Vec<_> = if i % 10 == 0 {
                vec![live[i as usize % live.len()].clone()]
            } else {
                vec![]
            };
            let (k, o) = node(fresh(), &children);
            db.insert_node(k.clone(), o);
            garbage_set.insert(k);
        }
        Graph {
            db,
            cache,
            live_root,
            live: live.into_iter().collect(),
            garbage: garbage_set,
        }
    }

    fn step(
        state: &mut GcState<Db>,
        g: &mut Graph,
        budget: impl Fn() -> Duration,
        limits: GcLimits,
    ) -> Vec<ArenaHash<H>> {
        let cache = &g.cache;
        state.run(
            std::iter::once(g.live_root.clone()),
            budget,
            limits,
            &mut g.db,
            |k| cache.get(&k),
            |db| db.get_roots().keys().cloned().collect(),
        )
    }

    /// Runs `step` until a sweep completes (signalled by `rescan` being set again), returning
    /// everything deleted, in order.
    fn run_to_completion(
        state: &mut GcState<Db>,
        g: &mut Graph,
        budget: impl Fn() -> Box<dyn Fn() -> Duration>,
        limits: GcLimits,
        max_calls: usize,
    ) -> Vec<ArenaHash<H>> {
        let mut deleted = vec![];
        for _ in 0..max_calls {
            deleted.extend(step(state, g, budget(), limits));
            if state.rescan {
                return deleted;
            }
        }
        panic!("gc cycle did not complete in {max_calls} calls");
    }

    fn assert_exactly_garbage_deleted(g: &Graph, deleted: &[ArenaHash<H>]) {
        let deleted_set: HashSet<_> = deleted.iter().cloned().collect();
        assert_eq!(deleted_set.len(), deleted.len(), "a node was deleted twice");
        assert_eq!(deleted_set, g.garbage);
        for k in g.live.iter() {
            assert!(
                g.cache.contains_key(k) || g.db.get_node(k).is_some(),
                "live node deleted"
            );
        }
    }

    #[test]
    fn budget_bounds_work_per_call() {
        for k in [0usize, 1, 2, 5] {
            let mut g = graph(1500, 2, 3000, true);
            let mut state = GcState::<Db>::default();
            let mut deleted = vec![];
            let mut completed = false;
            for _ in 0..10_000 {
                let (disk0, cache0) = (state.disk_keys_processed, state.cache_nodes_visited);
                deleted.extend(step(&mut state, &mut g, first_k(k), GcLimits::default()));
                let disk = state.disk_keys_processed - disk0;
                let cache = state.cache_nodes_visited - cache0;
                assert!(disk <= k * CHUNK, "k={k}: {disk} disk keys in one call");
                assert!(
                    cache <= k * CACHE_CHUNK,
                    "k={k}: {cache} cache nodes in one call"
                );
                if k == 0 {
                    assert_eq!(disk + cache, 0);
                }
                if state.rescan {
                    completed = true;
                    break;
                }
            }
            if k == 0 {
                assert!(!completed);
                assert!(deleted.is_empty());
            } else {
                assert!(completed, "k={k}: gc cycle did not complete");
                assert_exactly_garbage_deleted(&g, &deleted);
            }
        }
    }

    #[test]
    fn max_deletes_caps_deletions_per_call() {
        for cap in [1usize, 7, 300, 5000] {
            let mut g = graph(40, 3, 2000, false);
            let mut state = GcState::<Db>::default();
            let limits = GcLimits {
                max_deletes: Some(cap),
            };
            let mut deleted = vec![];
            let mut calls = 0;
            while deleted.len() < g.garbage.len() {
                calls += 1;
                assert!(calls < 10_000, "cap={cap}: no progress");
                let culled = step(&mut state, &mut g, || Duration::MAX, limits);
                assert!(culled.len() <= cap, "cap={cap}: {} deletions", culled.len());
                deleted.extend(culled);
            }
            if cap < 2000 {
                assert!(calls >= 2000 / cap, "cap={cap}: only {calls} calls");
            }
            // A further full cycle deletes nothing more.
            deleted.extend(run_to_completion(
                &mut state,
                &mut g,
                || Box::new(|| Duration::MAX),
                limits,
                10_000,
            ));
            deleted.extend(run_to_completion(
                &mut state,
                &mut g,
                || Box::new(|| Duration::MAX),
                limits,
                10_000,
            ));
            assert_exactly_garbage_deleted(&g, &deleted);
        }
    }

    #[test]
    fn max_deletes_zero_keeps_the_mark() {
        let mut g = graph(10, 2, 100, false);
        let mut state = GcState::<Db>::default();
        let limits = GcLimits {
            max_deletes: Some(0),
        };
        assert!(step(&mut state, &mut g, || Duration::MAX, limits).is_empty());
        // Mark finished, sweep not started: the mark set must survive.
        assert!(state.grey_set.is_empty());
        assert!(!state.mark_set.is_empty());
        assert!(!state.rescan);
        let deleted = run_to_completion(
            &mut state,
            &mut g,
            || Box::new(|| Duration::MAX),
            GcLimits::default(),
            1,
        );
        assert_exactly_garbage_deleted(&g, &deleted);
    }

    #[test]
    fn in_cache_mark_stop_loses_no_grey_node() {
        // More cache-resident nodes than one `CACHE_CHUNK`, so a one-chunk budget stops the
        // in-cache mark half way. The cache-only interior nodes are absent from disk, so if the
        // collector fell through to the on-disk mark they would read as missing and their
        // on-disk subtrees would be swept.
        let fanout = 3 * CACHE_CHUNK as u64;
        let mut reference = graph(fanout, 2, 500, true);
        let mut state = GcState::<Db>::default();
        let expected = run_to_completion(
            &mut state,
            &mut reference,
            || Box::new(|| Duration::MAX),
            GcLimits::default(),
            1,
        );
        assert_exactly_garbage_deleted(&reference, &expected);

        let mut g = graph(fanout, 2, 500, true);
        let mut state = GcState::<Db>::default();
        assert!(step(&mut state, &mut g, first_k(1), GcLimits::default()).is_empty());
        assert!(
            !state.cache_stack.is_empty(),
            "the in-cache mark did not stop"
        );
        assert!(
            state.cache_stack.iter().all(|k| state.grey_set.contains(k)),
            "a stacked node is not grey"
        );
        assert_eq!(state.disk_keys_processed, 0);
        let deleted = run_to_completion(
            &mut state,
            &mut g,
            || Box::new(|| Duration::MAX),
            GcLimits::default(),
            1,
        );
        let deleted: HashSet<_> = deleted.into_iter().collect();
        assert_eq!(deleted, expected.into_iter().collect::<HashSet<_>>());
        assert_eq!(g.db.size(), reference.db.size());
    }

    #[test]
    fn backend_gc_with_limits_caps_deletions() {
        let arena = crate::arena::Arena::new_from_backend(crate::backend::StorageBackend::new(
            1,
            crate::db::InMemoryDB::<crate::DefaultHasher>::default(),
        ));
        let size = || arena.with_backend(|b| b.database.size());
        let mut refs = (0..3000u64).map(|i| arena.alloc(i)).collect::<Vec<_>>();
        refs.iter_mut().for_each(|r| r.persist());
        arena.with_backend(|b| b.flush_all_changes_to_db());
        refs[1000..].iter_mut().for_each(|r| r.unpersist());
        arena.with_backend(|b| b.flush_all_changes_to_db());
        refs.truncate(1000);
        let limits = GcLimits {
            max_deletes: Some(150),
        };
        let mut culled = 0;
        for _ in 0..100 {
            let n = arena.with_backend(|b| b.gc_with_limits(Duration::from_hours(1), limits));
            assert!(n <= 150);
            culled += n;
        }
        assert_eq!(culled, 2000);
        assert_eq!(size(), 1000);
    }
}
