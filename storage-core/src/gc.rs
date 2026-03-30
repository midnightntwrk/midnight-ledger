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

    pub(crate) fn run<'a, 'b: 'a>(
        &'b mut self,
        roots: impl Iterator<Item = ArenaHash<D::Hasher>>,
        bound: Duration,
        db: &'b mut D,
        cache_read: impl Fn(ArenaHash<D::Hasher>) -> Option<&'a OnDiskObject<D::Hasher>>,
        db_roots: impl for<'c> FnOnce(&'c mut D) -> Vec<ArenaHash<D::Hasher>>,
    ) -> Vec<ArenaHash<D::Hasher>> {
        let t0 = Instant::now();
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
            && let Some(batch_size) = self
                .running_bench
                .read_batch_size(bound.saturating_sub(t0.elapsed()))
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
        while let Some(batch_size) = self
            .running_bench
            .scan_batch_size(bound.saturating_sub(t0.elapsed()))
        {
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
        assert_eq!(
            arena.with_backend(|b| b.gc(Duration::from_hours(1))),
            1 * CHUNK
        );
        assert_eq!(size(), 9 * CHUNK);
        // However, if we *just* drop references from memory, they are still protected due to
        // persistence.
        refs.truncate(8 * CHUNK);
        assert_eq!(arena.with_backend(|b| b.gc(Duration::from_hours(1))), 0);
        assert_eq!(size(), 9 * CHUNK);
        // If we give a small budget for the gc, it will not complete in one run through
        refs[5 * CHUNK..8 * CHUNK]
            .iter_mut()
            .for_each(|r| r.unpersist());
        arena.with_backend(|b| b.flush_all_changes_to_db());
        refs.truncate(5 * CHUNK);
        assert_eq!(arena.with_backend(|b| b.gc(Duration::from_millis(25))), 0);
        assert_eq!(size(), 9 * CHUNK);
        // But if ran repeatedly, it *will* run through
        let mut culled = 0;
        for _ in 0..100 {
            // Increased budget because of a dilemma: On some optimisation levels having too high a
            // budget above would *always* immediately complete, while a too low one here would
            // *never* complete, due to running out of budget in the in-memory mark phase.
            culled += arena.with_backend(|b| b.gc(Duration::from_millis(100)));
        }
        assert_eq!(culled, 3 * CHUNK);
        assert_eq!(size(), 6 * CHUNK);
    }
}
