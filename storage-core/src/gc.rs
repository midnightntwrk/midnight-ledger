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

/// Debug builds only: panics if a reported child list differs from the stored object's, or a
/// requested key with children is left out. Empty lists pass, as pinned nodes report none.
#[cfg(debug_assertions)]
fn check_reported_children<D: DB>(
    db: &D,
    batch: &[ArenaHash<D::Hasher>],
    reported: &[(ArenaHash<D::Hasher>, Vec<ArenaHash<D::Hasher>>)],
) {
    use std::collections::HashMap;
    let objects = db.batch_get_nodes(batch.iter().cloned());
    let stored = objects
        .iter()
        .map(|(key, object)| {
            let children = object
                .iter()
                .flat_map(|object| object.children.iter())
                .flat_map(|child| child.refs_iter())
                .collect::<HashSet<_>>();
            (key, children)
        })
        .collect::<HashMap<_, _>>();
    for (key, children) in reported {
        let Some(stored) = stored.get(key) else {
            continue;
        };
        let children = children.iter().collect::<HashSet<_>>();
        assert!(
            children.is_empty() || children == *stored,
            "batch_get_children reported children for {key:?} that differ from its stored object's"
        );
    }
    let reported = reported.iter().map(|(key, _)| key).collect::<HashSet<_>>();
    for (key, children) in &stored {
        assert!(
            reported.contains(key) || children.is_empty(),
            "batch_get_children left out {key:?}, whose stored object has children"
        );
    }
}

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
    pub(crate) fn force_rescan(&mut self) {
        self.rescan = true;
    }

    pub(crate) fn run<'a>(
        &mut self,
        roots: impl Iterator<Item = ArenaHash<D::Hasher>>,
        remaining_budget: impl Fn() -> Duration,
        db: &mut D,
        cache_read: impl Fn(ArenaHash<D::Hasher>) -> Option<&'a OnDiskObject<D::Hasher>>,
        db_roots: impl FnOnce(&mut D) -> Vec<ArenaHash<D::Hasher>>,
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
                    for child in obj.children.iter().flat_map(|c| c.refs_iter()) {
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
            let batch_read = db.batch_get_children(batch.iter().cloned());
            #[cfg(debug_assertions)]
            check_reported_children(db, &batch, &batch_read);
            // Mark every requested key, returned or not, so none stays grey forever.
            for key in &batch {
                self.grey_set.remove(key);
            }
            self.mark_set.extend(batch);
            for (parent, children) in batch_read {
                self.mark_set.insert(parent);
                for child in children {
                    if !self.mark_set.contains(&child) && !self.grey_set.contains(&child) {
                        self.grey_set.insert(child);
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
        while let Some(batch_size) = self.running_bench.scan_batch_size(remaining_budget()) {
            let batch_start = Instant::now();
            let (keys, handle) = db.scan_keys(self.sweep_resume.take(), batch_size);
            self.sweep_resume = handle;
            let batch_size = keys.len();
            // Cull all nodes in the batch that are *not* marked.
            let to_delete = keys
                .into_iter()
                .filter(|k| !self.mark_set.contains(k))
                .collect::<Vec<_>>();
            db.batch_update(to_delete.iter().map(|k| (k.clone(), Update::DeleteNode)));
            cull_set.extend(to_delete);
            self.running_bench
                .scan_batch_measurement(batch_size, batch_start.elapsed());

            // We have finished!
            if self.sweep_resume.is_none() {
                break;
            }
        }
        //eprintln!("SWEEP PHASE END: {} (resume: {:?})", cull_set.len(), self.sweep_resume);
        // We ran against our bound, return.
        if self.sweep_resume.is_some() {
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
    use crate::{
        DefaultHasher,
        arena::{ArenaHash, ArenaKey},
        backend::{OnDiskObject, StorageBackend},
        db::{DB, DummyArbitrary, InMemoryDB, Update},
    };
    #[cfg(feature = "proptest")]
    use proptest::{arbitrary::Arbitrary, strategy::Just};
    use serialize::Deserializable;
    use std::sync::Mutex;
    use std::{
        collections::{HashMap, HashSet},
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering::Relaxed},
        },
        time::Duration,
    };

    fn hash(n: u8) -> ArenaHash<DefaultHasher> {
        ArenaHash::deserialize(&mut [n; 32].as_slice(), 0).unwrap()
    }

    fn node(n: u8, children: &[u8]) -> (ArenaHash<DefaultHasher>, OnDiskObject<DefaultHasher>) {
        let object = OnDiskObject {
            data: vec![n],
            children: children.iter().map(|c| ArenaKey::Ref(hash(*c))).collect(),
        };
        (hash(n), object)
    }

    // 1 is the gc root: 1 -> 2 -> 3 and 2 -> 4 -> 5. 6 -> 7 is garbage.
    const NODES: [(u8, &[u8]); 7] = [
        (1, &[2]),
        (2, &[3, 4]),
        (3, &[]),
        (4, &[5]),
        (5, &[]),
        (6, &[7]),
        (7, &[]),
    ];

    /// A backend over [`NODES`] with only 1 a root, and a cache of one node, which keeps the mark
    /// phase on disk.
    fn backend<D: DB<Hasher = DefaultHasher>>(mut db: D) -> StorageBackend<D> {
        db.batch_update(NODES.iter().map(|(n, children)| {
            let (key, object) = node(*n, children);
            (key, Update::InsertNode(object))
        }));
        db.set_root_count(hash(1), 1);
        StorageBackend::new(1, db)
    }

    fn survivors<D: DB<Hasher = DefaultHasher>>(backend: &StorageBackend<D>) -> HashSet<u8> {
        NODES
            .iter()
            .map(|(n, _)| *n)
            .filter(|n| backend.database.get_node(&hash(*n)).is_some())
            .collect()
    }

    /// Reads keys and children without objects, counting object reads. `pinned` nodes are
    /// unscanned and childless; `stale`, `omit` and `noisy` simulate faulty backends.
    #[derive(Clone, Debug, Default)]
    struct TestDb {
        inner: InMemoryDB,
        pinned: Arc<Mutex<Vec<u8>>>,
        stale: Option<u8>,
        omit: Option<u8>,
        noisy: bool,
        object_scans: Arc<AtomicUsize>,
        node_reads: Arc<AtomicUsize>,
        children_asked: Arc<Mutex<Vec<ArenaHash<DefaultHasher>>>>,
    }

    impl TestDb {
        fn pinned(&self) -> Vec<ArenaHash<DefaultHasher>> {
            self.pinned
                .lock()
                .unwrap()
                .iter()
                .map(|n| hash(*n))
                .collect()
        }
    }

    impl DummyArbitrary for TestDb {}

    #[cfg(feature = "proptest")]
    impl Arbitrary for TestDb {
        type Parameters = ();
        type Strategy = Just<Self>;

        fn arbitrary_with(_: ()) -> Self::Strategy {
            unimplemented!()
        }
    }

    impl DB for TestDb {
        type Hasher = DefaultHasher;
        type ScanResumeHandle = <InMemoryDB as DB>::ScanResumeHandle;

        fn get_node(&self, key: &ArenaHash<DefaultHasher>) -> Option<OnDiskObject<DefaultHasher>> {
            self.inner.get_node(key)
        }

        fn insert_node(
            &mut self,
            key: ArenaHash<DefaultHasher>,
            object: OnDiskObject<DefaultHasher>,
        ) {
            self.inner.insert_node(key, object)
        }

        fn delete_node(&mut self, key: &ArenaHash<DefaultHasher>) {
            self.inner.delete_node(key)
        }

        fn batch_update<I>(&mut self, iter: I)
        where
            I: Iterator<Item = (ArenaHash<DefaultHasher>, Update<DefaultHasher>)>,
        {
            self.inner.batch_update(iter)
        }

        fn batch_get_nodes<I>(
            &self,
            keys: I,
        ) -> Vec<(
            ArenaHash<DefaultHasher>,
            Option<OnDiskObject<DefaultHasher>>,
        )>
        where
            I: Iterator<Item = ArenaHash<DefaultHasher>>,
        {
            self.node_reads.fetch_add(1, Relaxed);
            self.inner.batch_get_nodes(keys)
        }

        fn get_root_count(&self, key: &ArenaHash<DefaultHasher>) -> u32 {
            self.inner.get_root_count(key)
        }

        fn set_root_count(&mut self, key: ArenaHash<DefaultHasher>, count: u32) {
            self.inner.set_root_count(key, count)
        }

        fn get_roots(&self) -> HashMap<ArenaHash<DefaultHasher>, u32> {
            self.inner.get_roots()
        }

        fn size(&self) -> usize {
            self.inner.size()
        }

        fn scan(
            &self,
            resume_from: Option<Self::ScanResumeHandle>,
            batch_size: usize,
        ) -> (
            Vec<(ArenaHash<DefaultHasher>, OnDiskObject<DefaultHasher>)>,
            Option<Self::ScanResumeHandle>,
        ) {
            self.object_scans.fetch_add(1, Relaxed);
            self.inner.scan(resume_from, batch_size)
        }

        fn scan_keys(
            &self,
            resume_from: Option<Self::ScanResumeHandle>,
            batch_size: usize,
        ) -> (
            Vec<ArenaHash<DefaultHasher>>,
            Option<Self::ScanResumeHandle>,
        ) {
            let pinned = self.pinned();
            let (batch, handle) = self.inner.scan(resume_from, batch_size);
            let keys = batch
                .into_iter()
                .map(|(key, _)| key)
                .filter(|key| !pinned.contains(key))
                .collect();
            (keys, handle)
        }

        fn batch_get_children<I>(
            &self,
            keys: I,
        ) -> Vec<(ArenaHash<DefaultHasher>, Vec<ArenaHash<DefaultHasher>>)>
        where
            I: Iterator<Item = ArenaHash<DefaultHasher>>,
        {
            let pinned = self.pinned();
            keys.filter(|key| self.omit.map(hash).as_ref() != Some(key))
                .map(|key| {
                    self.children_asked.lock().unwrap().push(key.clone());
                    let mut children = match self.inner.get_node(&key) {
                        Some(_) if pinned.contains(&key) => vec![],
                        Some(object) => object
                            .children
                            .iter()
                            .flat_map(|child| child.refs_iter().cloned())
                            .collect(),
                        None => vec![],
                    };
                    if self.stale.map(hash).as_ref() == Some(&key) {
                        children.pop();
                    }
                    (key, children)
                })
                .flat_map(|entry| {
                    let copies = if self.noisy { 2 } else { 1 };
                    std::iter::repeat_n(entry, copies)
                })
                .chain(self.noisy.then(|| (hash(2), vec![hash(3), hash(4)])))
                .collect()
        }
    }

    #[test]
    fn gc_with_default_scan_keys() {
        let mut backend = backend(InMemoryDB::<DefaultHasher>::default());
        assert_eq!(backend.gc(Duration::from_secs(3600)), 2);
        assert_eq!(survivors(&backend), HashSet::from([1, 2, 3, 4, 5]));
    }

    #[test]
    fn gc_sweeps_with_scan_keys() {
        let db = TestDb::default();
        let object_scans = db.object_scans.clone();
        let mut backend = backend(db);
        assert_eq!(backend.gc(Duration::from_secs(3600)), 2);
        assert_eq!(survivors(&backend), HashSet::from([1, 2, 3, 4, 5]));
        assert_eq!(object_scans.load(Relaxed), 0);
    }

    #[test]
    fn gc_marks_with_batch_get_children() {
        let db = TestDb::default();
        let node_reads = db.node_reads.clone();
        let mut backend = backend(db);
        assert_eq!(backend.gc(Duration::from_secs(3600)), 2);
        assert_eq!(survivors(&backend), HashSet::from([1, 2, 3, 4, 5]));
        // Debug builds read objects to check the reported children against them.
        if !cfg!(debug_assertions) {
            assert_eq!(node_reads.load(Relaxed), 0);
        }
    }

    #[test]
    fn gc_neither_sweeps_nor_marks_below_childless_unscanned_nodes() {
        let db = TestDb {
            pinned: Arc::new(Mutex::new(vec![4, 5])),
            ..Default::default()
        };
        let children_asked = db.children_asked.clone();
        let mut backend = backend(db);
        assert_eq!(backend.gc(Duration::from_secs(3600)), 2);
        assert_eq!(survivors(&backend), HashSet::from([1, 2, 3, 4, 5]));
        // 4 is reached and reports no children, so the mark phase never asks for 5's.
        let asked = children_asked.lock().unwrap();
        assert!(asked.contains(&hash(4)));
        assert!(!asked.contains(&hash(5)));
    }

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

    /// Runs gc passes of `batches` on-disk batches each until a cycle completes, calling `between`
    /// after each. With this small graph a batch always covers a whole level of the mark.
    fn gc_in_passes<D: DB<Hasher = DefaultHasher>>(
        backend: &mut StorageBackend<D>,
        batches: usize,
        mut between: impl FnMut(usize),
    ) -> usize {
        let calls = std::cell::Cell::new(0);
        let budget = || {
            calls.set(calls.get() + 1);
            if calls.get() <= batches {
                Duration::MAX
            } else {
                Duration::ZERO
            }
        };
        let mut culled = 0;
        for pass in 0..20 {
            calls.set(0);
            culled += backend.gc_budgeted(budget);
            between(pass);
            if backend.database.size() == NODES.len() - 2 {
                break;
            }
        }
        culled
    }

    #[test]
    fn gc_keeps_a_subtree_pinned_during_a_bounded_cycle() {
        let db = TestDb::default();
        let (pinned, children_asked) = (db.pinned.clone(), db.children_asked.clone());
        let mut backend = backend(db);
        // Pinning 4 and 5 mid-mark makes the mark skip 5, which must survive the sweep.
        let culled = gc_in_passes(&mut backend, 2, |pass| {
            if pass == 0 {
                let asked = children_asked.lock().unwrap().clone();
                assert_eq!(
                    asked,
                    vec![hash(1), hash(2)],
                    "the first pass stops mid-mark"
                );
                pinned.lock().unwrap().extend([4, 5]);
            }
        });
        assert_eq!(culled, 2);
        assert_eq!(survivors(&backend), HashSet::from([1, 2, 3, 4, 5]));
        assert!(!children_asked.lock().unwrap().contains(&hash(5)));
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "reported children for")]
    fn gc_catches_a_stale_child_list() {
        // 2 is reported without 4, which would leave 4 and 5 unmarked and deleted.
        let db = TestDb {
            stale: Some(2),
            ..Default::default()
        };
        backend(db).gc(Duration::from_secs(3600));
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "whose stored object has children")]
    fn gc_catches_a_stored_node_left_out() {
        // Leaving out 2 would leave 3, 4 and 5 unmarked and deleted.
        let db = TestDb {
            omit: Some(2),
            ..Default::default()
        };
        backend(db).gc(Duration::from_secs(3600));
    }

    #[test]
    fn gc_completes_when_a_leaf_is_left_out() {
        // A left-out key is marked like a missing node, so the cycle still completes.
        let db = TestDb {
            omit: Some(3),
            ..Default::default()
        };
        let mut backend = backend(db);
        assert_eq!(gc_in_passes(&mut backend, 10, |_| {}), 2);
        assert_eq!(survivors(&backend), HashSet::from([1, 2, 3, 4, 5]));
    }

    #[test]
    fn gc_follows_references_through_direct_children() {
        use crate::arena::DirectChildNode;
        // 20's only child is embedded, and references 21; 22 is garbage.
        let mut db = InMemoryDB::<DefaultHasher>::default();
        let embedded = DirectChildNode::new(vec![0], vec![ArenaKey::Ref(hash(21))]);
        db.batch_update(
            [
                (
                    hash(20),
                    OnDiskObject {
                        data: vec![20],
                        children: vec![ArenaKey::Direct(embedded)],
                    },
                ),
                node(21, &[]),
                node(22, &[]),
            ]
            .into_iter()
            .map(|(key, object)| (key, Update::InsertNode(object))),
        );
        db.set_root_count(hash(20), 1);
        let mut backend = StorageBackend::new(1, db);
        assert_eq!(backend.gc(Duration::from_secs(3600)), 1);
        assert!(backend.database.get_node(&hash(21)).is_some());
        assert!(backend.database.get_node(&hash(22)).is_none());
    }

    #[test]
    fn gc_accepts_repeated_and_unrequested_keys() {
        // Correct lists, but every key twice, and 2 also before it was requested.
        let db = TestDb {
            noisy: true,
            ..Default::default()
        };
        let mut backend = backend(db);
        assert_eq!(backend.gc(Duration::from_secs(3600)), 2);
        assert_eq!(survivors(&backend), HashSet::from([1, 2, 3, 4, 5]));
    }
}
