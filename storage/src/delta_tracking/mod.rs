// This file is part of midnight-ledger.
// Copyright (C) 2025 Midnight Foundation
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

//! Delta tracking module for write and delete costing
//!
//! This module provides the `RcMap` data structure and associated functions
//! for implementing write and delete costing in the Midnight ledger.

mod rcmap;

pub use rcmap::RcMap;

use crate::arena::ArenaKey;
use crate::db::DB;
use base_crypto::cost_model::{CostDuration, RunningCost};
use std::collections::HashSet as StdHashSet;

/// Result of write and delete cost computation
pub struct WriteDeleteResults<D: DB> {
    /// Total bytes that would be written to storage
    pub bytes_written: u64,
    /// Total bytes that would be deleted from storage
    pub bytes_deleted: u64,
    /// The amount of nodes created
    pub nodes_written: u64,
    /// The amount of nodes deleted
    pub nodes_deleted: u64,
    /// The CPU cost of the write/delete processing
    pub processing_cost: RunningCost,
    /// Updated charged keys map (`K1`) after write and delete computation
    pub updated_charged_keys: RcMap<D>,
}

/// Return costed initial `RcMap` (`K0`) for a root set `r0`.
///
/// For costing initial state of new contract.
///
/// WARNING: this requires the keys in `r0` to be in the back-end; see similar
/// warning in `incremental_write_delete_costs` for more details.
pub fn initial_write_delete_costs<D: DB>(
    r0: &StdHashSet<ArenaKey<D::Hasher>>,
    cpu_cost: impl Fn(u64, u64) -> RunningCost,
) -> WriteDeleteResults<D> {
    let rcmap = RcMap::default();
    let keys_reachable_from_r0 = get_writes(&rcmap, &r0);
    let keys_removed = StdHashSet::new();
    let k0 = update_rcmap(&rcmap, &keys_reachable_from_r0);
    WriteDeleteResults::new(keys_reachable_from_r0, keys_removed, k0, cpu_cost)
}

/// Compute write and delete costs from old charged keys `k0` and new root set `r1`,
/// returning costed updated charged keys (`K1`).
///
/// For costing a call to an existing contract.
///
/// This implements the complete costing algorithm from the spec.
///
/// WARNING: this requires the keys in `r1` to be in the back-end, which in turn
/// requires them to be `persist()`ed or currently loaded in `Sp`s. The assumption
/// is that callers of this function will do so while `Sp`s are in scope for the
/// `r1` keys, which happens naturally when calling this function on keys taken
/// from the output `StateValue` of the VM.
pub fn incremental_write_delete_costs<D: DB>(
    k0: &RcMap<D>,
    r1: &StdHashSet<ArenaKey<D::Hasher>>,
    cpu_cost: impl Fn(u64, u64) -> RunningCost,
    gc_limit: impl FnOnce(RunningCost) -> usize,
) -> WriteDeleteResults<D> {
    // Step 1: Find keys that need to be written (reachable from R1 but not in K0)
    let keys_added = get_writes(k0, r1);
    let added_cost = cpu_cost(keys_added.len() as u64, 0);
    // Step 2: Update reference counts by adding the new keys
    let k = update_rcmap(k0, &keys_added);
    // Step 3: Garbage collect unreachable keys
    let (k1, keys_removed) = gc_rcmap(&k, r1, gc_limit(added_cost));
    WriteDeleteResults::new(keys_added, keys_removed, k1, cpu_cost)
}

/// Compute total bytes from a set of keys by summing their node sizes.
fn compute_bytes_from_keys<D: DB>(keys: &StdHashSet<ArenaKey<D::Hasher>>) -> u64 {
    let arena = &crate::storage::default_storage::<D>().arena;
    arena.with_backend(|backend| {
        keys.iter()
            .map(|key| {
                backend
                    .get(key)
                    // WARNING: this requires the keys to be in the backend,
                    // which in turn requires them to be `persist()`ed or
                    // currently loaded in sps. We ensure this by storing refs
                    // to charged keys in the RcMap, which itself gets persisted
                    // as part of the ContractState's ChargedState.
                    .expect("key should exist in arena when computing bytes")
                    .size() as u64
                    // Overhead of storing the refcount itself
                    + 32 + 4
            })
            .sum()
    })
}

impl<D: DB> WriteDeleteResults<D> {
    /// Compute `WriteDeleteResults` for new `RcMap` and key deltas.
    fn new(
        keys_added: StdHashSet<ArenaKey<D::Hasher>>,
        keys_removed: StdHashSet<ArenaKey<D::Hasher>>,
        new_charged_keys: RcMap<D>,
        cpu_cost: impl Fn(u64, u64) -> RunningCost,
    ) -> Self {
        let nodes_written = keys_added.len() as u64;
        let nodes_deleted = keys_removed.len() as u64;
        Self {
            bytes_written: compute_bytes_from_keys::<D>(&keys_added),
            bytes_deleted: compute_bytes_from_keys::<D>(&keys_removed),
            nodes_written,
            nodes_deleted,
            processing_cost: cpu_cost(nodes_written, nodes_deleted),
            updated_charged_keys: new_charged_keys,
        }
    }

    /// Get `RunningCost` of these results.
    pub fn running_cost(&self) -> RunningCost {
        RunningCost {
            read_time: CostDuration::ZERO,
            compute_time: CostDuration::ZERO,
            bytes_written: self.bytes_written,
            bytes_deleted: self.bytes_deleted,
        } + self.processing_cost
    }
}

/// Compute keys reachable from `roots` that are not currently charged in the
/// `RcMap`.
///
/// Assumes: `rcmap` is child closed.
///
/// Ensures: the return value union `rcmap` is child closed, and contains all
/// keys in `roots`.
pub fn get_writes<D: DB>(
    rcmap: &RcMap<D>,
    roots: &StdHashSet<ArenaKey<D::Hasher>>,
) -> StdHashSet<ArenaKey<D::Hasher>> {
    let arena = &crate::storage::default_storage::<D>().arena;
    let mut queue: Vec<ArenaKey<D::Hasher>> = roots.iter().cloned().collect();
    let mut keys_added = StdHashSet::new();

    while let Some(key) = queue.pop() {
        if !rcmap.contains(&key) && !keys_added.contains(&key) {
            let children = arena
                .children(&key)
                .expect("children for write update should be loadable");
            queue.extend(children);
            keys_added.insert(key);
        }
    }
    keys_added
}

/// Update an `RcMap` by adding reference counts for the provided keys and all
/// their children.  Returns a new `RcMap` with the updated reference counts.
///
/// Assumes:
/// - `rcmap` union `keys_added` is child closed.
/// - `rcmap` has internally accurate reference counts.
///
/// Ensures: the returned `RcMap` is child closed, and has internally accurate
/// reference counts.
#[must_use]
pub fn update_rcmap<D: DB>(
    rcmap: &RcMap<D>,
    keys_added: &StdHashSet<ArenaKey<D::Hasher>>,
) -> RcMap<D> {
    let arena = &crate::storage::default_storage::<D>().arena;
    let mut rcmap = rcmap.clone();

    // Count the new refs locally first to minimize the expensive-ish rcmap increments
    let mut inc_map = keys_added
        .iter()
        .map(|k| (k.clone(), 0))
        .collect::<std::collections::HashMap<_, _>>();
    // Initialize all new keys with rc = 0
    // Update reference counts for all edges from new keys
    for key in keys_added {
        let children = arena.children(key).expect("children should be loadable");
        for child in children {
            *inc_map.entry(child).or_default() += 1;
        }
    }
    for (k, by) in inc_map.into_iter() {
        let old_rc = rcmap.get_rc(&k).unwrap_or(0);
        rcmap = rcmap.modify_rc(&k, old_rc + by);
    }

    rcmap
}

/// Perform garbage collection by removing keys with zero reference counts that
/// are not in `roots`.
///
/// Returns a tuple of `(updated_rcmap, keys_removed)`.
#[must_use]
pub fn gc_rcmap<D: DB>(
    orig_rcmap: &RcMap<D>,
    roots: &StdHashSet<ArenaKey<D::Hasher>>,
    step_limit: usize,
) -> (RcMap<D>, StdHashSet<ArenaKey<D::Hasher>>) {
    let arena = &crate::storage::default_storage::<D>().arena;
    let mut rcmap = orig_rcmap.clone();
    let mut keys_removed = StdHashSet::new();
    let mut step = 0;
    let mut storage_queue = orig_rcmap.get_unreachable_keys_not_in(roots);
    // Invariant: keys in queue have rc == 0 and aren't in r1.
    let mut queue: Vec<ArenaKey<D::Hasher>> = Vec::new();
    let mut rc_cache = std::collections::HashMap::new();
    let mut update_queue = std::collections::HashMap::new();

    // First, accumulate update information (to churn storage less)
    while let Some(key) = storage_queue.next().or_else(|| queue.pop()) {
        if step >= step_limit {
            break;
        }
        step += 1;

        // Decrement reference counts of key's children
        let children = arena.children(&key).expect("children should be loadable");
        for child in children {
            let existing = rc_cache
                .entry(child.clone())
                .or_insert_with(|| rcmap.get_rc(&child).unwrap_or(0));
            let sub = update_queue.entry(child.clone()).or_default();
            *sub += 1;
            // If child's rc became 0 and it's not in roots, add it to the gc queue
            if *sub >= *existing && !roots.contains(&child) {
                queue.push(child);
            }
        }
        keys_removed.insert(key);
    }

    // Execute on the update information
    for (key, update) in update_queue.into_iter() {
        let original = rc_cache
            .get(&key)
            .expect("must have cached decremented key");
        let updated = original.saturating_sub(update);
        rcmap = rcmap.modify_rc(&key, updated);
    }

    for key in keys_removed.iter() {
        // Remove key.
        //
        // WARNING: the order here is important, i.e. first decrementing
        // children, and then removing their parents. The reason is that the
        // RcMap only holds backend references to the keys with refcount zero,
        // where here in these high-level functions that use the RcMap we
        // maintain the invariant that RcMap is ref-count accurate for the roots
        // it was constructed from. So, if we remove a parent key from rc_0
        // before decrementing its children, its children could become
        // unreachable!
        //
        // NOTE: we don't have the same concerns in `get_writes` and
        // `update_rcmap`, since there we're concerned with increasing rcs
        // corresponding to nodes that are currently in the arena in existing
        // sps.
        rcmap = rcmap
            .remove_unreachable_key(key)
            .expect("keys in queue have rc == 0");
    }

    (rcmap, keys_removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate as storage;
    use crate::arena::Sp;
    use crate::db::DB;
    use crate::storable::Loader;
    use crate::storage::set_default_storage;
    use crate::{DefaultDB, Storable};
    use derive_where::derive_where;
    use serialize::Tagged;
    use std::collections::HashMap;

    // Simple test node that can form arbitrary DAGs
    #[derive(Storable, Debug, Hash)]
    #[derive_where(Clone, PartialEq, Eq)]
    #[storable(db = D)]
    #[tag = "test_node[v1]"]
    struct Node<D: DB = DefaultDB> {
        id: u64, // Encode (layer, node_id) as layer * 256 + node_id
        children: Vec<Sp<Node<D>, D>>,
    }

    impl<D: DB> Node<D> {
        fn new(id: (u8, u8), children: &[Sp<Node<D>, D>]) -> Sp<Node<D>, D> {
            let encoded_id = (id.0 as u64) * 256 + (id.1 as u64);
            Sp::new(Node {
                id: encoded_id,
                children: children.to_vec(),
            })
        }
    }

    struct Dag<D: DB = DefaultDB> {
        nodes: HashMap<(u8, u8), ArenaKey<D::Hasher>>,
        // Sps of the roots to keep them in backend.
        _roots: Vec<Sp<Node<D>, D>>,
    }

    // Test DAG specified as adjacency list.
    //
    // This is pretty arbitrary, chosen for being "kind of complicated" and
    // "doesn't have sideways or back edges". The tests are mostly agnostic to
    // the structure here, altho they often assume that (0,1) and (0,2) are
    // valid nodes with large descendant subgraphs.
    fn test_dag_adjacency() -> Vec<((u8, u8), Vec<(u8, u8)>)> {
        vec![
            // Layer 0: roots
            ((0, 1), vec![(1, 1), (1, 2), (2, 1), (3, 1)]),
            ((0, 2), vec![(1, 2), (1, 3)]),
            // Layer 1
            ((1, 1), vec![(2, 1), (2, 2)]),
            ((1, 2), vec![(2, 2), (3, 1)]),
            ((1, 3), vec![(2, 3), (3, 2)]),
            // Layer 2
            ((2, 1), vec![(3, 1), (3, 2), (3, 3)]),
            ((2, 2), vec![(3, 2), (4, 1)]),
            ((2, 3), vec![(3, 3), (3, 4)]),
            // Layer 3
            ((3, 1), vec![(4, 1), (4, 2)]),
            ((3, 2), vec![(4, 1), (4, 2), (4, 3)]),
            ((3, 3), vec![]),
            ((3, 4), vec![(4, 3), (5, 1)]),
            // Layer 4
            ((4, 1), vec![(5, 1)]),
            ((4, 2), vec![(5, 1), (5, 2)]),
            ((4, 3), vec![(5, 2)]),
            // Layer 5: leaves
            ((5, 1), vec![]),
            ((5, 2), vec![]),
        ]
    }

    // Build the actual DAG from the adjacency list
    fn build_test_dag<D: DB>() -> Dag<D> {
        let adjacency = test_dag_adjacency();
        let mut nodes: HashMap<(u8, u8), Sp<Node<D>, D>> = HashMap::new();

        // Build bottom-up: iterate adjacency list in reverse order, because
        // there are no back or sideways edges
        for ((layer, id), children_ids) in adjacency.iter().rev() {
            let node_id = (*layer, *id);
            let children: Vec<_> = children_ids
                .iter()
                .map(|child_id| nodes[child_id].clone())
                .collect();
            nodes.insert(node_id, Node::new(node_id, &children));
        }

        // Convert to ArenaKey map for easier test access
        let mut arena_nodes = HashMap::new();
        for ((layer, id), node) in &nodes {
            arena_nodes.insert((*layer, *id), node.root.clone());
        }

        Dag {
            nodes: arena_nodes,
            _roots: vec![nodes[&(0, 1)].clone(), nodes[&(0, 2)].clone()],
        }
    }

    // Compute all nodes reachable from given roots using adjacency list
    fn compute_reachable_nodes(roots: &[(u8, u8)]) -> StdHashSet<(u8, u8)> {
        let adjacency = test_dag_adjacency();
        let mut reachable = StdHashSet::new();
        let mut queue: Vec<(u8, u8)> = roots.iter().copied().collect();

        while let Some(node_id) = queue.pop() {
            if !reachable.insert(node_id) {
                continue;
            }
            // Add children to queue
            let children = adjacency
                .iter()
                .find(|(k, _)| *k == node_id)
                .expect("nodes must be in adjacency")
                .1
                .clone();
            queue.extend(children);
        }

        reachable
    }

    // Get subgraph reference counts for nodes reachable from roots
    fn get_subgraph_rcs(roots: &[(u8, u8)]) -> HashMap<(u8, u8), u64> {
        let adjacency = test_dag_adjacency();
        let reachable = compute_reachable_nodes(roots);
        let mut rcs = HashMap::new();

        // Initialize all reachable nodes with rc=0
        for node_id in &reachable {
            rcs.insert(*node_id, 0);
        }

        // Count incoming edges from nodes in the subgraph
        for (parent_id, children) in &adjacency {
            if reachable.contains(parent_id) {
                for child_id in children {
                    let rc = rcs.get_mut(child_id).unwrap();
                    *rc += 1;
                }
            }
        }

        rcs
    }

    // Convert node IDs to ArenaKeys using the DAG
    fn to_keys<'a, I>(node_ids: I) -> StdHashSet<ArenaKey<crate::DefaultHasher>>
    where
        I: IntoIterator<Item = &'a (u8, u8)>,
    {
        let dag = build_test_dag::<DefaultDB>();
        node_ids
            .into_iter()
            .map(|id| dag.nodes[id].clone())
            .collect()
    }

    use super::rcmap::tests::get_rcmap_descendants;

    #[test]
    fn get_writes() {
        // Need this alive to be sure that nodes stay in backend.
        let _dag = build_test_dag::<DefaultDB>();

        // Test 1: Empty K0 should return all reachable nodes from R1
        let k0: RcMap<DefaultDB> = RcMap::default();
        let roots = [(0, 1)];
        let r1 = to_keys(roots.iter());

        let writes = super::get_writes(&k0, &r1);
        let expected_reachable = compute_reachable_nodes(&roots);
        let expected_keys = to_keys(expected_reachable.iter());

        assert_eq!(
            writes, expected_keys,
            "Write set should contain exactly the reachable nodes"
        );

        // Test 2: K0 contains bottom layers (3,4,5) - should truncate traversal
        // Layers 3, 4, 5
        let k0_node_ids: Vec<_> = test_dag_adjacency()
            .iter()
            .map(|((layer, id), _)| (*layer, *id))
            .filter(|(layer, _)| *layer >= 3 && *layer <= 5)
            .collect();
        let k0_keys = to_keys(k0_node_ids.iter());
        let k0_writes = super::get_writes::<DefaultDB>(&RcMap::default(), &k0_keys);
        let k0: RcMap<DefaultDB> = super::update_rcmap(&RcMap::default(), &k0_writes);

        let writes = super::get_writes(&k0, &r1);

        // Compute what should be written: reachable from roots minus what's in K0
        let reachable_from_r1 = compute_reachable_nodes(&roots);
        let k0_set: StdHashSet<_> = k0_node_ids.iter().copied().collect();
        let expected_writes = &reachable_from_r1 - &k0_set;
        let expected_writes_keys = to_keys(expected_writes.iter());

        assert_eq!(
            writes, expected_writes_keys,
            "Write set should exclude K0 nodes"
        );

        // Test 3: Multiple roots
        let multi_roots = [(0, 1), (0, 2)];
        let r1_multi = to_keys(multi_roots.iter());
        let writes_multi = super::get_writes::<DefaultDB>(&RcMap::default(), &r1_multi);
        let expected_multi = compute_reachable_nodes(&multi_roots);
        let expected_multi_keys = to_keys(expected_multi.iter());

        assert_eq!(
            writes_multi, expected_multi_keys,
            "Multiple roots should give union of reachable sets"
        );
    }

    #[test]
    fn update_rcmap() {
        let dag = build_test_dag::<DefaultDB>();

        // Test updating empty RcMap with nodes reachable from specific roots
        let roots = [(0, 1)];
        let reachable = compute_reachable_nodes(&roots);

        let k0: RcMap<DefaultDB> = RcMap::default();
        let writes = to_keys(reachable.iter());

        let k1 = super::update_rcmap(&k0, &writes);

        // Compute expected reference counts based on adjacency
        let expected_rcs = get_subgraph_rcs(&roots);

        // Verify reference counts match expectations
        for (node_id, expected_rc) in expected_rcs {
            let actual_rc = k1.get_rc(&dag.nodes[&node_id]).unwrap();
            assert_eq!(
                actual_rc, expected_rc,
                "Node {:?} should have rc={}, got {}",
                node_id, expected_rc, actual_rc
            );
        }
    }

    #[test]
    fn gc_rcmap() {
        let dag = build_test_dag::<DefaultDB>();

        // Build initial RcMap with nodes reachable from both root types
        let full_roots = [(0, 1), (0, 2)];
        let full_reachable = compute_reachable_nodes(&full_roots);
        let all_writes = to_keys(full_reachable.iter());
        let k0: RcMap<DefaultDB> = super::update_rcmap(&RcMap::default(), &all_writes);

        // Test GC with limited root set: only one root type
        let limited_roots = [(0, 1)];
        let roots = to_keys(limited_roots.iter());

        let step_limit = 1000;
        let (k1, removed) = super::gc_rcmap(&k0, &roots, step_limit);

        // Compute what should remain vs be removed
        let kept_nodes = compute_reachable_nodes(&limited_roots);
        let expected_removed: StdHashSet<_> = &full_reachable - &kept_nodes;

        // Verify removed nodes
        assert_eq!(
            removed.len(),
            expected_removed.len(),
            "Should remove exactly the unreachable nodes"
        );

        for node_id in &expected_removed {
            assert!(
                removed.contains(&dag.nodes[node_id]),
                "Node {:?} should be removed as unreachable",
                node_id
            );
            assert_eq!(
                k1.get_rc(&dag.nodes[node_id]),
                None,
                "Removed node {:?} should not have rc in new map",
                node_id
            );
        }

        // Verify kept nodes
        for node_id in &kept_nodes {
            assert!(
                !removed.contains(&dag.nodes[node_id]),
                "Node {:?} should not be removed as it's reachable",
                node_id
            );
            assert!(
                k1.get_rc(&dag.nodes[node_id]).is_some(),
                "Remaining node {:?} should have rc in new map",
                node_id
            );
        }

        // Test step limit: should stop GC early with limited steps
        let (k2, removed2) = super::gc_rcmap(&k0, &roots, 2);
        assert!(
            removed2.len() == 2,
            "With step_limit=2, should remove 2 nodes"
        );

        // Test resuming GC
        let (_k3, removed3) = super::gc_rcmap(&k2, &roots, expected_removed.len());
        let total_removed: StdHashSet<_> = removed2.union(&removed3).cloned().collect();
        assert!(
            total_removed.len() == expected_removed.len(),
            "Resuming GC should make progress"
        );

        // Run gc one step at a time until no progress is possible
        let empty_roots = StdHashSet::new();
        let mut current_rcmap = k0.clone();
        let mut total_single_step_removed = StdHashSet::new();
        loop {
            let (new_rcmap, removed_single) = super::gc_rcmap(&current_rcmap, &empty_roots, 1);
            if removed_single.is_empty() {
                break; // No more progress
            }
            total_single_step_removed.extend(removed_single);
            current_rcmap = new_rcmap;
        }

        assert_eq!(
            total_single_step_removed.len(),
            full_reachable.len(),
            "Single-step GC should eventually remove all nodes with empty root set"
        );
    }

    // Test that GC operations don't crash when RcMap holds only references to data
    #[test]
    fn rcmap_survives_gc_with_only_references() {
        use crate::db::InMemoryDB;
        use crate::storage::WrappedDB;
        use std::collections::HashSet as StdHashSet;

        struct Tag;
        type W = WrappedDB<InMemoryDB, Tag>;
        set_default_storage(crate::Storage::<W>::default).unwrap();

        // Here rcmap has the only references to the underlying objects
        let rcmap: RcMap<W> = {
            // Build test DAG with isolated DB and create RcMap for full DAG
            let dag = build_test_dag::<W>();
            let full_roots = [(0, 1), (0, 2)];
            let all_reachable = compute_reachable_nodes(&full_roots);
            let all_writes: StdHashSet<_> = all_reachable
                .iter()
                .map(|id| dag.nodes[id].clone())
                .collect();
            super::update_rcmap(&RcMap::default(), &all_writes)
        };

        // Run GC from empty root set - should GC everything but not crash
        let empty_roots = StdHashSet::new();
        let (_final_rcmap, _removed) = super::gc_rcmap(&rcmap, &empty_roots, 1000);

        // If we reach here without panics, RcMap successfully survived GC
        // with only references to the data
    }

    // Test that initial_write_delete_costs and incremental_write_delete_costs
    // work correctly for various randomly chosen contract state
    // transitions.
    #[test]
    fn write_delete_costs() {
        use rand::rngs::StdRng;
        use rand::{Rng, SeedableRng};
        use std::collections::{HashMap, HashSet as StdHashSet};

        let dag = build_test_dag::<DefaultDB>();

        // Collect all node IDs from the DAG for random selection
        let all_node_ids: Vec<(u8, u8)> = dag.nodes.keys().cloned().collect();

        // Use a fixed seed for reproducibility
        let mut rng = StdRng::seed_from_u64(42);

        // Generate 100 random root sets with varying sizes
        let mut root_sets = Vec::new();
        for _ in 0..100 {
            let root_set_size = {
                // Size distribution:
                // - 40% small (0-3 nodes)
                // - 40% medium (4-8 nodes)
                // - 15% large (9-15 nodes)
                // - 5% very large (16-25 nodes)
                let p = rng.gen_range(0..100);
                if p < 40 {
                    rng.gen_range(0..=3)
                } else if p < 80 {
                    rng.gen_range(4..=8)
                } else if p < 95 {
                    rng.gen_range(9..=15)
                } else {
                    rng.gen_range(16..=25.min(all_node_ids.len()))
                }
            };

            // Randomly select nodes for this root set
            let mut selected_nodes = StdHashSet::new();
            while selected_nodes.len() < root_set_size {
                let idx = rng.gen_range(0..all_node_ids.len());
                selected_nodes.insert(all_node_ids[idx]);
            }

            root_sets.push(selected_nodes.into_iter().collect::<Vec<_>>());
        }

        // Convert root sets to ArenaKey sets
        let root_sets_as_keys: Vec<StdHashSet<_>> = root_sets.iter().map(to_keys).collect();

        // Compute initial_write_delete_costs for each root set.
        for i in 0..root_sets.len() {
            let results =
                super::initial_write_delete_costs::<DefaultDB>(&root_sets_as_keys[i], |_, _| {
                    Default::default()
                });

            // Verify the RcMap matches what get_subgraph_rcs predicts
            let expected_rcs = get_subgraph_rcs(&root_sets[i]);
            let actual_rcs = results.updated_charged_keys.get_rcs();

            // Convert expected_rcs node IDs to ArenaKeys for comparison
            let expected_rcs_as_keys: HashMap<_, _> = expected_rcs
                .into_iter()
                .map(|(node_id, rc)| (dag.nodes[&node_id].clone(), rc))
                .collect();

            assert_eq!(
                actual_rcs, expected_rcs_as_keys,
                "Initial costs for root set {} should have correct reference counts",
                i
            );

            // Verify that all keys in the rootset are descendants of the RcMap
            let rcmap_descendants = get_rcmap_descendants(&results.updated_charged_keys);
            for root_key in &root_sets_as_keys[i] {
                assert!(
                    rcmap_descendants.contains(root_key),
                    "Root key {:?} should be a descendant of RcMap after initial_write_delete_costs",
                    root_key
                );
            }
        }

        // Initialize from the first root set, and then iterate over each
        // subsequent root set and compute incremental_write_delete_costs.
        let initial_roots = &root_sets_as_keys[0];
        let initial_results =
            super::initial_write_delete_costs(initial_roots, |_, _| Default::default());
        let mut current_charged_keys = initial_results.updated_charged_keys;
        for i in 1..root_sets.len() {
            let next_roots = &root_sets_as_keys[i];
            let results = super::incremental_write_delete_costs::<DefaultDB>(
                &current_charged_keys,
                next_roots,
                |_, _| Default::default(),
                |_| 1000, // High step limit for complete GC
            );

            // Verify the RcMap matches what get_subgraph_rcs predicts for the new root set
            let expected_rcs = get_subgraph_rcs(&root_sets[i]);
            let actual_rcs = results.updated_charged_keys.get_rcs();

            // Convert expected_rcs node IDs to ArenaKeys for comparison
            let expected_rcs_as_keys: HashMap<_, _> = expected_rcs
                .into_iter()
                .map(|(node_id, rc)| (dag.nodes[&node_id].clone(), rc))
                .collect();

            assert_eq!(
                actual_rcs, expected_rcs_as_keys,
                "Incremental transition {} should have correct reference counts",
                i
            );

            // Verify that all keys in the rootset are descendants of the RcMap
            let rcmap_descendants = get_rcmap_descendants(&results.updated_charged_keys);
            for root_key in next_roots {
                assert!(
                    rcmap_descendants.contains(root_key),
                    "Root key {:?} should be a descendant of RcMap after incremental_write_delete_costs",
                    root_key
                );
            }

            // Update for next iteration
            current_charged_keys = results.updated_charged_keys;
        }
    }
}
