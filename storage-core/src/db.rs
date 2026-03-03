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

//! A database of content-addressed DAG nodes.

#[cfg(all(feature = "sqlite", not(feature = "layout-v2")))]
mod sql;
#[cfg(all(feature = "sqlite", not(feature = "layout-v2")))]
pub use sql::SqlDB;
#[cfg(feature = "parity-db")]
mod paritydb;
#[cfg(feature = "parity-db")]
pub use paritydb::ParityDb;
#[cfg(feature = "parity-db")]
pub use paritydb::ParityDbTree;

use crate::DefaultHasher;
use crate::backend::OnDiskObject;
use crate::{
    WellBehavedHasher,
    arena::{ArenaHash, ArenaKey, NodeAddress},
};
#[cfg(feature = "proptest")]
use proptest::{
    prelude::*,
    strategy::{NewTree, ValueTree},
    test_runner::TestRunner,
};
use std::collections::{HashMap, HashSet};
use std::fmt::Debug;
#[cfg(feature = "proptest")]
use std::marker::PhantomData;
use std::sync::{Arc, Mutex};

/// An update to the database, for use with `DB::batch_update`.
#[derive(Clone, Debug)]
pub enum Update<H: WellBehavedHasher> {
    /// Insert a DAG node.
    InsertNode(OnDiskObject<H>),
    /// Delete a DAG node.
    DeleteNode,
    /// Set the root count of a DAG node. Setting this to zero means the node is
    /// no longer a GC root.
    SetRootCount(u32),
}

#[cfg(feature = "proptest")]
/// Arbitrary is required on DB to be able to easily derive Arbitrary on Sp types, depending on
/// feature flag "proptest"
pub trait DummyArbitrary: Arbitrary {}
#[cfg(not(feature = "proptest"))]
/// Arbitrary is required on DB to be able to easily derive Arbitrary on Sp types, depending on
/// feature flag "proptest"
pub trait DummyArbitrary {}

// ============================================================================
// Tree node types for ParityDB multitree operations
// ============================================================================

/// A node being inserted into a tree. Contains the node's data and references
/// to its children, which may themselves be new nodes or existing nodes
/// identified by their `NodeAddress`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewTreeNode {
    /// Encoded node data: `[hash || payload]`.
    ///
    /// The hash is the content hash of this node. The payload is the binary
    /// representation of the stored value. Child hashes are not embedded;
    /// they can be obtained by reading child nodes.
    pub data: Vec<u8>,
    /// References to child nodes.
    pub children: Vec<TreeChildRef>,
}

/// A reference to a child node within a tree being inserted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TreeChildRef {
    /// A new node being inserted for the first time.
    New(NewTreeNode),
    /// An existing node already in the database, referenced by address.
    Existing(NodeAddress),
}

/// A node read back from the tree database.
///
/// Children are returned as bare `NodeAddress` values. The caller
/// (typically `StorageBackend`) is responsible for pairing addresses with
/// hashes — either from its own cache or by reading the child nodes.
#[derive(Clone, Debug)]
pub struct TreeReadNode<H: WellBehavedHasher> {
    /// The content hash of this node.
    pub hash: ArenaHash<H>,
    /// The payload data (the binary representation of the stored value).
    pub data: Vec<u8>,
    /// Addresses of this node's children in the tree database.
    pub children: Vec<NodeAddress>,
    /// This node's own address in the tree database.
    pub addr: NodeAddress,
}

/// A database of Merkle DAG nodes.
///
/// The DAG node representation stored in the db is [`OnDiskObject`], comprising
/// a binary payload, a set of child keys, and a ref count (number of
/// parent->child DAG references with this node as child).
///
/// In addition to DAG nodes, the DB also stores gc-root counts, to allow
/// marking nodes as persisted / not subject to GC.
///
/// The DAG nodes are keyed by [`ArenaHash`] hashes, which are content-addresses
/// at the level of the Merkle DAG, but NOT at the level of the DB. Details
/// below.
///
/// # Warning: DB nodes are NOT content addressed from DB point of view
///
/// This may be surprising, since the job of the DB is to store
/// content-addressed Merkle DAGs, and the DB keys are the Merkle DAG
/// keys. However, from the point of the view of the DB this keying is NOT
/// content addressing, because DB nodes include the ref-count, which is not
/// included in the Merkle DAG key computation (a hash of the node payload and
/// child keys). Indeed, the ref-count is meta information as far as the Merkle
/// DAGs are concerned, and it would make no sense to include the ref-count in
/// the hash, since this would mean adding a new reference to a node would
/// require transitively updating all of its ancestors!
///
/// # Warning: DB implementations must not enforce logical consistency
///
/// Database implementations should be "dumb", in that they just track state computed by
/// the `StorageBackend` layer above, which is responsible for logical
/// consistency concerns (see examples below). There is no expectation that the
/// `StorageBackend` will write updates to the db in a logically consistent
/// order, or that the db will ever be in a logically consistent state, unless
/// `StorageBackend::flush_all_changes_to_db` has been called (and even then,
/// logical inconsistencies during the flush would be expected). In particular,
/// the `StorageBackend::flush_cache_evictions_to_db` API may write some
/// arbitrary, incomplete set of updates to the DB, and the DB is expected to
/// support this use case. The motivation here is that the `StorageBackend` is
/// expected to support large Merkle DAGs that don't fit in memory, and so may
/// "swap" parts of these DAGs to disk via the DB, but there is no reason to
/// force the DB state itself to be logically consistent.
///
/// Possibly non-exhaustive list of logical inconsistencies the DB must support:
///
/// - the child keys in a node need not point to nodes that exist in the DB
///
/// - the ref-count in a node need not equal the number of other nodes in the DB
///   which reference said node
///
/// - a node may be deleted while its gc-root-count is still stored as non-zero
///   in the DB
///
/// - a non-zero gc-root-count may be set on a node that is not present in the
///   DB
///
/// # Warning: Foot-gun
///
/// If adding a method with a default implementation, be sure to add a pass-thru
/// in the `DB` impl for [`crate::storage::WrappedDB`].
pub trait DB: Default + Sync + Send + Debug + DummyArbitrary + 'static {
    /// The hasher used in this DB.
    type Hasher: WellBehavedHasher;

    // ====================================================================
    // Shared methods (required)
    // ====================================================================

    /// Return a mapping from key to root count, for all the roots in this
    /// DB. All mapped root counts will be positive.
    fn get_roots(&self) -> HashMap<ArenaHash<Self::Hasher>, u32>;

    /// Return the number of nodes in this DB.
    ///
    /// Only used for diagnostics/testing. Returns 0 by default.
    fn size(&self) -> usize {
        0
    }

    /// Get the number of times the node with key `key` has been marked as a GC
    /// root. Returns 0 if the node is not a GC root.
    fn get_root_count(&self, key: &ArenaHash<Self::Hasher>) -> u32 {
        self.get_roots().get(key).copied().unwrap_or(0)
    }

    // ====================================================================
    // Flat KV methods (legacy; panicking defaults for tree-only impls)
    // ====================================================================

    /// Get node in DAG with key `key`.
    fn get_node(&self, _key: &ArenaHash<Self::Hasher>) -> Option<OnDiskObject<Self::Hasher>> {
        unimplemented!("flat KV operations not supported by this DB implementation")
    }

    #[cfg(not(feature = "layout-v2"))]
    /// Get the keys for all the unreachable nodes, i.e. the nodes with
    /// `ref_count == 0`, which aren't marked as GC roots.
    fn get_unreachable_keys(&self) -> std::vec::Vec<ArenaHash<Self::Hasher>> {
        unimplemented!("flat KV operations not supported by this DB implementation")
    }

    /// Insert a DAG node with key `key`.
    fn insert_node(
        &mut self,
        _key: ArenaHash<Self::Hasher>,
        _object: OnDiskObject<Self::Hasher>,
    ) {
        unimplemented!("flat KV operations not supported by this DB implementation")
    }

    /// Remove the DAG node with key `key`.
    fn delete_node(&mut self, _key: &ArenaHash<Self::Hasher>) {
        unimplemented!("flat KV operations not supported by this DB implementation")
    }

    /// Set the root count of the node with key `key` to `count`. If `count` is
    /// 0, the node will no longer be a GC root.
    fn set_root_count(&mut self, _key: ArenaHash<Self::Hasher>, _count: u32) {
        unimplemented!("flat KV operations not supported by this DB implementation")
    }

    /// Batch update the database.
    ///
    /// For `DB`s that use expensive write transactions, implementors should
    /// combine many updates in a single transaction.
    fn batch_update<I>(&mut self, _iter: I)
    where
        I: Iterator<Item = (ArenaHash<Self::Hasher>, Update<Self::Hasher>)>,
    {
        unimplemented!("flat KV operations not supported by this DB implementation")
    }

    /// Batch get nodes.
    ///
    /// For `DB`s that use expensive transactions, implementors should combine
    /// many gets into a single transaction.
    #[allow(clippy::type_complexity)]
    fn batch_get_nodes<I>(
        &self,
        _keys: I,
    ) -> std::vec::Vec<(ArenaHash<Self::Hasher>, Option<OnDiskObject<Self::Hasher>>)>
    where
        I: Iterator<Item = ArenaHash<Self::Hasher>>,
    {
        unimplemented!("flat KV operations not supported by this DB implementation")
    }

    /// Get all nodes reachable from the node with key `key` using a breadth
    /// first search.
    ///
    /// The `cache_get` function should return nodes for keys that are already
    /// in memory. Such keys and nodes will *not* be included in the result.  If
    /// the caller wishes to update the cache ordering for nodes returned by
    /// `cache_get`, they should provide a `cache_get` implementation that does
    /// this.
    ///
    /// If `truncate` is true, then the search is truncated at nodes returned by
    /// `cache_get`. If false, then the search will continue past cached nodes.
    ///
    /// If `max_depth` is `Some(n)`, only nodes at depth `n` or less will be
    /// retrieved, where the node for argument `key` is at depth 0.
    ///
    /// If `max_count` is `Some(n)`, only the first `n` nodes will be retrieved.
    ///
    /// Returns a sequence of `(key, node)` pairs, containing all keys which
    /// were read from the db during this search, in the order they were read
    /// from the DB (the traversal order, ignoring cache hits). Note that keys
    /// that are already in the cache are not included in the returned map.
    ///
    /// Note: this default implementation could be improved for `SqlDB`, by
    /// combining all the lookups in a single transaction. How much this buys us
    /// depends on how many nodes we see at each level -- see the
    /// `db::sql::tests::bulk_read_file` test for details -- but a potential way
    /// to avoid having to re-implement from scratch, would be to expose a way to
    /// manually manage the transactions for `SqlDB`s.
    #[allow(clippy::type_complexity)]
    fn bfs_get_nodes<C>(
        &self,
        key: &ArenaHash<Self::Hasher>,
        cache_get: C,
        truncate: bool,
        max_depth: Option<usize>,
        max_count: Option<usize>,
    ) -> std::vec::Vec<(ArenaHash<Self::Hasher>, OnDiskObject<Self::Hasher>)>
    where
        C: Fn(&ArenaHash<Self::Hasher>) -> Option<OnDiskObject<Self::Hasher>>,
    {
        // The key-value pairs to return.
        let mut kvs = vec![];
        // The keys that we've already checked for cache membership. We check
        // for membership here to avoid processing duplicates.
        let mut visited = HashSet::new();
        let mut current_depth = 0;
        // The to-be-processed keys at the current depth.
        let mut current_keys = vec![key.clone()];
        while !current_keys.is_empty()
            && max_depth.is_none_or(|max_depth| current_depth <= max_depth)
        {
            // The unvisited keys at the next depth.
            let mut next_keys = vec![];
            // The `current_keys` that aren't already in `kvs` or the cache, in
            // the order bfs observed them, with duplicates eliminated.
            let mut unknown_keys = vec![];

            // Attempt to get keys from `cache_get` if not already in `nodes`,
            // collecting misses into `unknown_keys`.
            for k in current_keys {
                if !visited.contains(&k) {
                    visited.insert(k.clone());
                    match cache_get(&k) {
                        Some(node) => {
                            if !truncate {
                                next_keys
                                    .extend(node.children.iter().flat_map(ArenaKey::refs).cloned());
                            }
                        }
                        _ => {
                            unknown_keys.push(k);
                        }
                    }
                }
            }

            // For remaining, unknown keys, try to batch get them from db, being
            // careful not to end up with more than `max_count` results.
            if let Some(max_count) = max_count {
                // This is a no-op if `unknown_keys.len() + kvs.len() <= max_count`.
                unknown_keys.truncate(max_count - kvs.len());
            }
            for (k, v) in self.batch_get_nodes(unknown_keys.into_iter()) {
                match v {
                    Some(node) => {
                        next_keys.extend(node.children.iter().flat_map(ArenaKey::refs).cloned());
                        kvs.push((k, node));
                    }
                    None => {
                        // We allow the root key to not be found, but not any
                        // descendant keys.
                        if current_depth > 0 {
                            panic!("child key {k:?} must be in memory or db");
                        }
                    }
                }
            }

            // Prepare next iteration.
            current_depth += 1;
            current_keys = next_keys;
        }
        kvs
    }

    // ====================================================================
    // Tree methods for ParityDB multitree operations
    // (panicking defaults for flat-only impls)
    // ====================================================================

    /// Insert a tree under the given root key.
    ///
    /// Returns `NodeAddress` values for all `TreeChildRef::New` nodes in the
    /// tree, in DFS pre-order (skipping `Existing` refs). The root node's
    /// address is the first element.
    fn insert_tree(
        &mut self,
        _key: ArenaHash<Self::Hasher>,
        _root: NewTreeNode,
    ) -> Vec<NodeAddress> {
        unimplemented!("tree operations not supported by this DB implementation")
    }

    /// Increment the reference count of the tree at the given root key.
    fn reference_tree(&mut self, _key: &ArenaHash<Self::Hasher>) {
        unimplemented!("tree operations not supported by this DB implementation")
    }

    /// Decrement the reference count of the tree at the given root key.
    /// The tree (and any exclusively-owned subtrees) is removed when the
    /// count reaches zero.
    fn dereference_tree(&mut self, _key: &ArenaHash<Self::Hasher>) {
        unimplemented!("tree operations not supported by this DB implementation")
    }

    /// Read a tree's root node by its root key.
    ///
    /// Returns the decoded node with hash, payload, and children as
    /// bare `NodeAddress` values.
    fn get_tree_root(
        &self,
        _key: &ArenaHash<Self::Hasher>,
    ) -> Option<TreeReadNode<Self::Hasher>> {
        unimplemented!("tree operations not supported by this DB implementation")
    }

    /// Read a node by its `NodeAddress`.
    fn get_node_by_addr(&self, _addr: NodeAddress) -> Option<TreeReadNode<Self::Hasher>> {
        unimplemented!("tree operations not supported by this DB implementation")
    }

    /// Batch read nodes by their `NodeAddress` values.
    fn batch_get_nodes_by_addr(
        &self,
        addrs: &[NodeAddress],
    ) -> Vec<Option<TreeReadNode<Self::Hasher>>> {
        addrs
            .iter()
            .map(|addr| self.get_node_by_addr(*addr))
            .collect()
    }

    /// Flush any pending writes to stable storage.
    ///
    /// For databases with write-ahead logs (e.g. ParityDB), this ensures
    /// logged operations are applied to the underlying data files. No-op
    /// by default.
    fn flush(&mut self) {}
}


/// A dubious default implementation of `DB::batch_update`.
///
/// Note: this implementation is super slow in the case of `DB`s which
/// use expensive write transactions behind the scenes.
pub fn dubious_batch_update<D: DB, I>(db: &mut D, iter: I)
where
    I: Iterator<Item = (ArenaHash<D::Hasher>, Update<D::Hasher>)>,
{
    use Update::*;
    for (k, v) in iter {
        match v {
            InsertNode(value) => db.insert_node(k, value),
            DeleteNode => db.delete_node(&k),
            SetRootCount(count) => db.set_root_count(k, count),
        }
    }
}

/// A dubious default implementation of `DB::batch_get_nodes`.
///
/// Note: this implementation is probably slow for `DB`s which use a
/// separate transaction for each read.
#[allow(clippy::type_complexity)]
pub fn dubious_batch_get_nodes<D: DB, I>(
    db: &D,
    keys: I,
) -> Vec<(ArenaHash<D::Hasher>, Option<OnDiskObject<D::Hasher>>)>
where
    I: Iterator<Item = ArenaHash<D::Hasher>>,
{
    keys.map(|k| (k.clone(), db.get_node(&k))).collect()
}

#[derive(Clone, Debug)]
/// An in-memory database
pub struct InMemoryDB<H: WellBehavedHasher = DefaultHasher> {
    nodes: Arc<Mutex<HashMap<ArenaHash<H>, OnDiskObject<H>>>>,
    roots: Arc<Mutex<HashMap<ArenaHash<H>, u32>>>,
}

impl<H: WellBehavedHasher> DummyArbitrary for InMemoryDB<H> {}

#[cfg(feature = "proptest")]
/// A dummy DB Tree for proptesting
pub struct DummyDBTree<D: DB>(PhantomData<D>);

#[cfg(feature = "proptest")]
impl<D: DB> ValueTree for DummyDBTree<D> {
    type Value = D;

    fn current(&self) -> Self::Value {
        D::default()
    }

    fn simplify(&mut self) -> bool {
        false
    }

    fn complicate(&mut self) -> bool {
        false
    }
}

#[cfg(feature = "proptest")]
#[derive(Debug)]
/// A dummy DB Strategy for proptesting
pub struct DummyDBStrategy<D: DB>(PhantomData<D>);

#[cfg(feature = "proptest")]
impl<D: DB> Strategy for DummyDBStrategy<D> {
    type Tree = DummyDBTree<D>;
    type Value = D;

    fn new_tree(&self, _runner: &mut TestRunner) -> NewTree<Self> {
        Ok(DummyDBTree(PhantomData))
    }
}

#[cfg(feature = "proptest")]
/// A dummy Arbitrary impl for `InMemoryDB` to allow for deriving Arbitrary on Sp<T, D>
impl<H: WellBehavedHasher> Arbitrary for InMemoryDB<H> {
    type Parameters = ();
    type Strategy = DummyDBStrategy<Self>;

    fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
        DummyDBStrategy::<Self>(PhantomData)
    }
}

impl<H: WellBehavedHasher> InMemoryDB<H> {
    fn lock_nodes(&self) -> std::sync::MutexGuard<'_, HashMap<ArenaHash<H>, OnDiskObject<H>>> {
        self.nodes.lock().expect("db lock poisoned")
    }

    fn lock_roots(&self) -> std::sync::MutexGuard<'_, HashMap<ArenaHash<H>, u32>> {
        self.roots.lock().expect("db lock poisoned")
    }
}

impl<H: WellBehavedHasher> DB for InMemoryDB<H> {
    type Hasher = H;

    fn get_node(&self, key: &ArenaHash<H>) -> Option<OnDiskObject<H>> {
        self.lock_nodes().get(key).cloned()
    }

    #[cfg(not(feature = "layout-v2"))]
    fn get_unreachable_keys(&self) -> std::vec::Vec<ArenaHash<Self::Hasher>> {
        let nodes_guard = self.lock_nodes();
        let roots_guard = self.lock_roots();
        let mut unreachable = vec![];
        for (key, node) in nodes_guard.iter() {
            if node.ref_count == 0 && !roots_guard.contains_key(key) {
                unreachable.push(key.clone());
            }
        }
        unreachable
    }

    fn insert_node(&mut self, key: ArenaHash<H>, object: OnDiskObject<H>) {
        self.lock_nodes().insert(key, object);
    }

    fn delete_node(&mut self, key: &ArenaHash<H>) {
        self.lock_nodes().remove(key);
    }

    fn get_root_count(&self, key: &ArenaHash<Self::Hasher>) -> u32 {
        self.lock_roots().get(key).cloned().unwrap_or(0)
    }

    fn set_root_count(&mut self, key: ArenaHash<Self::Hasher>, count: u32) {
        if count > 0 {
            self.lock_roots().insert(key, count);
        } else {
            self.lock_roots().remove(&key);
        }
    }

    fn get_roots(&self) -> HashMap<ArenaHash<Self::Hasher>, u32> {
        self.lock_roots().clone()
    }

    fn size(&self) -> usize {
        self.lock_nodes().len()
    }

    fn batch_update<I>(&mut self, iter: I)
    where
        I: Iterator<Item = (ArenaHash<Self::Hasher>, Update<Self::Hasher>)>,
    {
        dubious_batch_update(self, iter);
    }

    fn batch_get_nodes<I>(
        &self,
        keys: I,
    ) -> Vec<(ArenaHash<Self::Hasher>, Option<OnDiskObject<Self::Hasher>>)>
    where
        I: Iterator<Item = ArenaHash<Self::Hasher>>,
    {
        dubious_batch_get_nodes(self, keys)
    }
}

impl<H: WellBehavedHasher> Default for InMemoryDB<H> {
    fn default() -> Self {
        Self {
            nodes: Arc::default(),
            roots: Arc::default(),
        }
    }
}

// ============================================================================
// Tree node data encoding/decoding helpers
// ============================================================================

/// Encode a tree node's data in the format: `[hash || payload]`.
///
/// The hash is the content hash of the node. The payload is the binary
/// representation of the stored value. Child information is stored
/// separately by the tree database (as `NodeAddress` values).
pub fn encode_tree_node_data<H: WellBehavedHasher>(
    hash: &ArenaHash<H>,
    payload: &[u8],
) -> Vec<u8> {
    let hash_size = <H as crypto::digest::OutputSizeUser>::output_size();
    let mut data = Vec::with_capacity(hash_size + payload.len());
    data.extend_from_slice(&hash.0);
    data.extend_from_slice(payload);
    data
}

/// Decode a tree node's data from the format produced by [`encode_tree_node_data`].
///
/// Returns the hash and payload. The child addresses and this node's own
/// address come separately from the tree database.
pub fn decode_tree_node_data<H: WellBehavedHasher>(
    data: &[u8],
    child_addresses: Vec<NodeAddress>,
    addr: NodeAddress,
) -> Option<TreeReadNode<H>> {
    use crypto::digest::OutputSizeUser;
    let hash_size = <H as OutputSizeUser>::output_size();

    if data.len() < hash_size {
        return None;
    }

    #[allow(deprecated)]
    let hash = ArenaHash(
        crypto::digest::crypto_common::generic_array::GenericArray::clone_from_slice(
            &data[..hash_size],
        ),
    );

    let payload = data[hash_size..].to_vec();

    Some(TreeReadNode {
        hash,
        data: payload,
        children: child_addresses,
        addr,
    })
}

// ============================================================================
// InMemoryTreeDB: in-memory implementation of TreeDB for testing
// ============================================================================

/// Internal representation of a stored tree node in `InMemoryTreeDB`.
#[derive(Clone, Debug)]
struct StoredTreeNode {
    /// The encoded data blob (same format as would be stored in ParityDB).
    data: Vec<u8>,
    /// Addresses of child nodes.
    child_addresses: Vec<NodeAddress>,
    /// Reference count (number of parent nodes referencing this node).
    ref_count: u32,
}

#[derive(Clone, Debug)]
/// An in-memory tree database, simulating ParityDB's multitree semantics.
pub struct InMemoryTreeDB<H: WellBehavedHasher = crate::DefaultHasher> {
    /// All stored nodes, keyed by their assigned `NodeAddress`.
    nodes: Arc<Mutex<HashMap<NodeAddress, StoredTreeNode>>>,
    /// Root keys mapped to (wrapper_node_address, ref_count).
    /// The wrapper node has a single child which is the actual root.
    roots: Arc<Mutex<HashMap<ArenaHash<H>, (NodeAddress, u32)>>>,
    /// Next address to assign.
    next_addr: Arc<Mutex<NodeAddress>>,
}

impl<H: WellBehavedHasher> Default for InMemoryTreeDB<H> {
    fn default() -> Self {
        Self {
            nodes: Arc::default(),
            roots: Arc::default(),
            next_addr: Arc::new(Mutex::new(1)), // start at 1 so 0 is never used
        }
    }
}

impl<H: WellBehavedHasher> DummyArbitrary for InMemoryTreeDB<H> {}

#[cfg(feature = "proptest")]
impl<H: WellBehavedHasher> Arbitrary for InMemoryTreeDB<H> {
    type Parameters = ();
    type Strategy = DummyDBStrategy<Self>;

    fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
        DummyDBStrategy::<Self>(PhantomData)
    }
}

impl<H: WellBehavedHasher> InMemoryTreeDB<H> {
    fn lock_nodes(&self) -> std::sync::MutexGuard<'_, HashMap<NodeAddress, StoredTreeNode>> {
        self.nodes.lock().expect("db lock poisoned")
    }

    fn lock_roots(&self) -> std::sync::MutexGuard<'_, HashMap<ArenaHash<H>, (NodeAddress, u32)>> {
        self.roots.lock().expect("db lock poisoned")
    }

    fn alloc_addr(&self) -> NodeAddress {
        let mut next = self.next_addr.lock().expect("db lock poisoned");
        let addr = *next;
        *next += 1;
        addr
    }

    /// Recursively insert a `NewTreeNode` tree, assigning addresses to all new
    /// nodes and incrementing ref counts for existing ones. Returns the
    /// addresses of all `New` nodes in DFS pre-order.
    fn insert_tree_recursive(
        &self,
        node: NewTreeNode,
        new_addrs: &mut Vec<NodeAddress>,
        nodes: &mut std::sync::MutexGuard<'_, HashMap<NodeAddress, StoredTreeNode>>,
    ) -> NodeAddress {
        let addr = self.alloc_addr();
        new_addrs.push(addr);

        let mut child_addresses = Vec::with_capacity(node.children.len());
        for child_ref in node.children {
            match child_ref {
                TreeChildRef::New(child_node) => {
                    let child_addr =
                        self.insert_tree_recursive(child_node, new_addrs, nodes);
                    // New nodes start with ref_count = 1 (from this parent).
                    // The ref_count was already set to 1 when the child was inserted.
                    child_addresses.push(child_addr);
                }
                TreeChildRef::Existing(child_addr) => {
                    // Increment ref count of existing node.
                    if let Some(existing) = nodes.get_mut(&child_addr) {
                        existing.ref_count += 1;
                    }
                    child_addresses.push(child_addr);
                }
            }
        }

        nodes.insert(
            addr,
            StoredTreeNode {
                data: node.data,
                child_addresses,
                ref_count: 1, // one reference from parent (or root table)
            },
        );

        addr
    }

    /// Recursively decrement ref counts and remove unreferenced nodes.
    fn dereference_recursive(
        &self,
        addr: NodeAddress,
        nodes: &mut std::sync::MutexGuard<'_, HashMap<NodeAddress, StoredTreeNode>>,
    ) {
        let should_remove = if let Some(node) = nodes.get_mut(&addr) {
            node.ref_count = node.ref_count.saturating_sub(1);
            node.ref_count == 0
        } else {
            return;
        };

        if should_remove {
            let node = nodes.remove(&addr).unwrap();
            for child_addr in node.child_addresses {
                self.dereference_recursive(child_addr, nodes);
            }
        }
    }

    fn read_node(
        &self,
        addr: NodeAddress,
        nodes: &std::sync::MutexGuard<'_, HashMap<NodeAddress, StoredTreeNode>>,
    ) -> Option<TreeReadNode<H>> {
        let stored = nodes.get(&addr)?;
        decode_tree_node_data(&stored.data, stored.child_addresses.clone(), addr)
    }
}

impl<H: WellBehavedHasher> DB for InMemoryTreeDB<H> {
    type Hasher = H;

    fn get_roots(&self) -> HashMap<ArenaHash<Self::Hasher>, u32> {
        self.lock_roots()
            .iter()
            .map(|(k, (_, count))| (k.clone(), *count))
            .collect()
    }

    fn size(&self) -> usize {
        self.lock_nodes().len()
    }

    fn insert_tree(
        &mut self,
        key: ArenaHash<Self::Hasher>,
        root: NewTreeNode,
    ) -> Vec<NodeAddress> {
        let mut new_addrs = Vec::new();
        let mut nodes = self.lock_nodes();
        let root_addr = self.insert_tree_recursive(root, &mut new_addrs, &mut nodes);
        drop(nodes);

        let mut roots = self.lock_roots();
        // If tree already exists for this key, dereference old one
        if let Some((old_addr, _)) = roots.remove(&key) {
            let mut nodes = self.lock_nodes();
            self.dereference_recursive(old_addr, &mut nodes);
        }
        roots.insert(key, (root_addr, 1));

        new_addrs
    }

    fn reference_tree(&mut self, key: &ArenaHash<Self::Hasher>) {
        let mut roots = self.lock_roots();
        if let Some((_, ref_count)) = roots.get_mut(key) {
            *ref_count += 1;
        }
    }

    fn dereference_tree(&mut self, key: &ArenaHash<Self::Hasher>) {
        let mut roots = self.lock_roots();
        let should_remove = if let Some((_, ref_count)) = roots.get_mut(key) {
            *ref_count = ref_count.saturating_sub(1);
            *ref_count == 0
        } else {
            return;
        };

        if should_remove {
            let (root_addr, _) = roots.remove(key).unwrap();
            drop(roots);
            let mut nodes = self.lock_nodes();
            self.dereference_recursive(root_addr, &mut nodes);
        }
    }

    fn get_tree_root(
        &self,
        key: &ArenaHash<Self::Hasher>,
    ) -> Option<TreeReadNode<Self::Hasher>> {
        let roots = self.lock_roots();
        let (root_addr, _) = roots.get(key)?;
        let root_addr = *root_addr;
        drop(roots);
        let nodes = self.lock_nodes();
        self.read_node(root_addr, &nodes)
    }

    fn get_node_by_addr(&self, addr: NodeAddress) -> Option<TreeReadNode<Self::Hasher>> {
        let nodes = self.lock_nodes();
        self.read_node(addr, &nodes)
    }
}

#[cfg(test)]
mod tests {
    use super::Update::*;
    use crate::backend::raw_node::RawNode;
    use crate::{
        DefaultHasher,
        arena::ArenaHash,
        backend::OnDiskObject,
        db::{DB, InMemoryDB},
    };
    use rand::Rng;
    use std::collections::{HashMap, HashSet};

    // Number of keys to use for bulk_read tests. Don't leave this at a high
    // value, because for tests built without --release, they can take minutes
    // to finish. For actual benchmarking, 100,000 or 1,000,000 are good values.
    const BULK_READ_NUM_KVS: usize = 1000;

    /// Not bothering to record stats here, since we don't care about relative
    /// performance of toy implementation.
    #[test]
    fn bulk_read_inmemorydb() {
        for chunk_size in [10, 100, 1000] {
            // For in-memory we can't actually reopen the db each time, since then
            // we'd lose its contents :)
            let db = InMemoryDB::<DefaultHasher>::default();
            let mk_db = || db.clone();
            let num_kvs = BULK_READ_NUM_KVS;
            test_bulk_read(num_kvs, chunk_size, mk_db);
        }
    }
    /// Speedups for various chunk sizes, for 100,000 keys, 3 runs:
    ///
    /// 10: 3.2 times, 2.9 times, 3.1 times
    /// 100: 4.5 times, 3.7 times, 4.3 times
    /// 1000: 4.8 times, 4.6 times, 5.3 times
    ///
    /// The above is for tests compiled with optimization, e.g.
    ///
    ///    cargo test --all-features --release -p midnight-storage --lib -- tests::bulk_read_sqldb_file --nocapture
    #[cfg(all(feature = "sqlite", not(feature = "layout-v2")))]
    #[test]
    fn bulk_read_sqldb_memory() {
        for chunk_size in [10, 100, 1000] {
            // For in-memory we can't actually reopen the db each time, since then
            // we'd lose its contents :)
            let db = crate::db::SqlDB::<DefaultHasher>::memory();
            let mk_db = || db.clone_memory_db();
            let num_kvs = BULK_READ_NUM_KVS;
            test_bulk_read(num_kvs, chunk_size, mk_db);
        }
    }
    /// Speedups for various chunk sizes, for 100,000 keys, 3 runs:
    ///
    /// 10: 3.2 times, 3.2 times, 3.0 times
    /// 100: 4.2 times, 4.4 times, 4.4 times
    /// 1000: 4.6 times, 4.8 times, 4.3 times
    ///
    /// Total time for each test is about 3.5 seconds.
    ///
    /// The above is for tests compiled with optimization, e.g.
    ///
    ///    cargo test --all-features --release -p midnight-storage --lib -- tests::bulk_read_sqldb_file --nocapture
    #[cfg(all(feature = "sqlite", not(feature = "layout-v2")))]
    #[test]
    fn bulk_read_sqldb_file() {
        for chunk_size in [10, 100, 1000] {
            let path = tempfile::NamedTempFile::new().unwrap().into_temp_path();
            let mk_db = || crate::db::SqlDB::<DefaultHasher>::exclusive_file(&path);
            let num_kvs = BULK_READ_NUM_KVS;
            test_bulk_read(num_kvs, chunk_size, mk_db);
        }
    }
    /// Speedups for various chunk sizes, for 100,000 keys, 3 runs:
    ///
    /// 10: 1.0 times, 1.0 times, 0.9 times
    /// 100: 0.9 times, 0.9 times, 0.9 times
    /// 1000: 0.9 times, 0.8 times, 1.0 times
    ///
    /// Total time for each test is about 3.5 seconds.
    ///
    /// This is very surprising: performance is *worse* with bulk reads!?
    /// FIGURED IT OUT: `ParityDB` is using the default implementation of
    /// `DB::batch_get_nodes`, which is just a loop over `DB::get_node`. TODO: add a
    /// proper `batch_get_nodes` implementation to `ParityDB` and see if this
    /// improves ...
    ///
    /// The above is for tests compiled with optimization, e.g.
    ///
    ///    cargo test --all-features --release -p midnight-storage --lib -- tests::bulk_read_paritydb --nocapture
    #[cfg(feature = "parity-db")]
    #[test]
    fn bulk_read_paritydb() {
        for chunk_size in [10, 100, 1000] {
            let path = tempfile::TempDir::new().unwrap().keep();
            let mk_db = || crate::db::ParityDb::<DefaultHasher>::open(&path);
            let num_kvs = BULK_READ_NUM_KVS;
            test_bulk_read(num_kvs, chunk_size, mk_db);
        }
    }
    /// Compare bulk reading to reading one-by-one.
    ///
    /// The `open_db` argument should open a connection to the *same* db every
    /// time it's called.
    ///
    /// To get a more reliable result from this, it should be a proper
    /// benchmark, but the speedups observed here are already good enough to
    /// justify implementing the bulk read functionality for `SqlDB`. However, for
    /// `ParityDb`, we actually see *worse* performance with batched reads!
    ///
    /// Naively, we might be concerned about the order in which the bulk and
    /// one-by-one lookups are performed, because of caching. But changing the
    /// order of the steps, or running the first step multiple times, doesn't
    /// noticeably change how long each step takes for `SqlDB`.
    fn test_bulk_read<D: DB, F: Fn() -> D>(num_kvs: usize, chunk_size: usize, open_db: F) {
        let mut db = open_db();
        let mut rng = rand::thread_rng();
        let kvs = (0..num_kvs)
            .map(|_| rng.r#gen())
            .collect::<Vec<(ArenaHash<_>, OnDiskObject<_>)>>();

        let mut t = crate::test::Timer::new("test_bulk_read");

        let iter = kvs.iter().map(|(k, v)| (k.clone(), InsertNode(v.clone())));
        db.batch_update(iter);

        t.delta("batch insert kvs");

        // Open the DB again, to avoid any caching effects as far as possible
        // (but there may also be disk cache effects we can't avoid).
        drop(db);
        let db = open_db();

        t.delta("reopen db");

        for (k, _) in &kvs {
            db.get_node(k).unwrap();
        }

        let delta_1by1 = t.delta("read kvs one-by-one");

        // Open the DB again, to avoid any caching effects as far as possible
        // (but there may also be disk cache effects we can't avoid).
        drop(db);
        let db = open_db();

        t.delta("reopen db");

        let iter = kvs.clone().into_iter().map(|(k, _)| k);
        use itertools::Itertools;
        let chunks = iter.chunks(chunk_size);
        for chunk in chunks.into_iter() {
            for (_, v) in db.batch_get_nodes(chunk) {
                v.unwrap();
            }
        }

        let delta_batch = t.delta("batch read kvs");

        println!(
            "Speedup for num_kvs={}, chunk_size={}: {:.1}",
            num_kvs,
            chunk_size,
            delta_1by1 / delta_batch
        );
    }

    const ALL_OPS_NUM_KVS: usize = 100;

    /// Run time, 3 runs, 10,000 kvs: 0.04 s, 0.04 s, 0.05 s
    #[test]
    fn all_ops_inmemorydb() {
        let mut db = InMemoryDB::<DefaultHasher>::default();
        test_all_ops(ALL_OPS_NUM_KVS, &mut db);
    }
    /// Run time, 3 runs, 10,000 kvs: 1.73 s, 1.78 s, 1.76 s
    #[cfg(all(feature = "sqlite", not(feature = "layout-v2")))]
    #[test]
    fn all_ops_sqldb_memory() {
        let mut db = crate::db::SqlDB::<DefaultHasher>::memory();
        test_all_ops(ALL_OPS_NUM_KVS, &mut db);
    }
    /// Run time, 3 runs, 10,000 kvs: 2.51 s, 2.52 s, 2.62 s
    #[cfg(all(feature = "sqlite", not(feature = "layout-v2")))]
    #[test]
    fn all_ops_sqldb_file() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let mut db = crate::db::SqlDB::<DefaultHasher>::exclusive_file(file.into_temp_path());

        // Speedups compared to default sqlite configuration, not sure how many
        // kvs:
        //
        // Turning off synchronous transactions reduces test time from 9s
        // to 0.10s.
        //
        // Turning on WAL reduces test time from 9s to 3.2s.
        test_all_ops(ALL_OPS_NUM_KVS, &mut db);
    }
    /// Run time, 3 runs, 10,000 kvs: 0.33 s, 0.39 s, 0.35 s
    #[cfg(feature = "parity-db")]
    #[test]
    fn all_ops_paritydb() {
        let mut db = crate::db::ParityDb::<DefaultHasher>::default();
        test_all_ops(ALL_OPS_NUM_KVS, &mut db);
    }
    /// Test all db operations, without concurrency.
    ///
    /// Prints progress/timing info.
    ///
    /// This is essentially a benchmark, so better to compile tests with
    /// --release, and run tests one at a time. E.g.
    ///
    ///     cargo test --all-features -p midnight-storage --lib --release -- tests::all_ops --nocapture --test-threads=1
    fn test_all_ops<D: DB>(num_kvs: usize, db: &mut D) {
        let mut t = crate::test::Timer::new("test_all_ops");

        let mut rng = rand::thread_rng();
        let kvs = (0..num_kvs)
            .map(|_| rng.r#gen())
            .collect::<Vec<(ArenaHash<_>, OnDiskObject<_>)>>();

        t.delta("gen kvs");

        for (k, v) in kvs.clone() {
            db.insert_node(k, v);
        }

        t.delta("insert kvs");

        for (k, v) in kvs.clone() {
            assert_eq!(db.get_node(&k), Some(v));
        }
        assert_eq!(db.size(), num_kvs);

        t.delta("get kvs");

        for (i, (k, _)) in kvs.clone().into_iter().enumerate() {
            db.set_root_count(k, i as u32);
        }
        assert_eq!(db.size(), num_kvs);

        t.delta("set root counts");

        for (i, (k, _)) in kvs.iter().enumerate() {
            assert_eq!(db.get_root_count(k), i as u32);
        }

        t.delta("get root counts");

        for (k, v) in kvs.clone() {
            assert_eq!(db.get_node(&k), Some(v));
            db.delete_node(&k);
            assert_eq!(db.get_node(&k), None);
        }
        assert_eq!(db.size(), 0);

        t.delta("get, delete, and get kvs");

        let iter = kvs.iter().enumerate().flat_map(|(i, (k, v))| {
            [
                (k.clone(), InsertNode(v.clone())),
                (k.clone(), SetRootCount(i as u32)),
            ]
        });
        db.batch_update(iter);
        for (i, (k, v)) in kvs.clone().into_iter().enumerate() {
            assert_eq!(db.get_node(&k), Some(v));
            assert_eq!(db.get_root_count(&k), i as u32);
        }
        assert_eq!(db.size(), num_kvs);

        t.delta("batch insert and get kvs and root counts");

        let root_counts_golden: HashMap<_, _> = kvs
            .iter()
            .enumerate()
            .map(|(i, (k, _))| (k.clone(), i as u32))
            // Skip `i == 0`, which is not a root 😭
            .skip(1)
            .collect();
        let root_counts_db = db.get_roots();
        assert_eq!(root_counts_golden.len(), root_counts_db.len());
        assert_eq!(root_counts_golden, root_counts_db);

        t.delta("batch get all roots");

        let iter = kvs
            .iter()
            .flat_map(|(k, _)| [(k.clone(), DeleteNode), (k.clone(), SetRootCount(0))]);
        db.batch_update(iter);
        for (k, _) in kvs.clone() {
            assert_eq!(db.get_node(&k), None);
            assert_eq!(db.get_root_count(&k), 0);
        }
        assert_eq!(db.size(), 0);

        t.delta("batch delete and get kvs and root counts");
    }

    #[cfg(not(feature = "layout-v2"))]
    #[test]
    fn bfs_get_nodes_inmemorydb() {
        test_bfs_get_nodes::<InMemoryDB>();
    }
    #[cfg(all(feature = "sqlite", not(feature = "layout-v2")))]
    #[test]
    fn bfs_get_nodes_sqldb() {
        test_bfs_get_nodes::<crate::db::SqlDB>();
    }
    #[cfg(all(feature = "parity-db", not(feature = "layout-v2")))]
    #[test]
    fn bfs_get_nodes_paritydb() {
        test_bfs_get_nodes::<crate::db::ParityDb>();
    }
    #[cfg(not(feature = "layout-v2"))]
    fn test_bfs_get_nodes<D: DB<Hasher = DefaultHasher>>() {
        use crate::backend::raw_node::RawNode;
        // Arranging the nodes in layers, the variables names here are
        // `n<layer><column>` for the`column`th node in layer `layer`.
        let n41 = RawNode::new(&[1, 4, 1], 1, vec![]);
        let n42 = RawNode::new(&[1, 4, 2], 3, vec![]);
        let n43 = RawNode::new(&[1, 4, 3], 2, vec![]);
        let n44 = RawNode::new(&[1, 4, 4], 2, vec![]);
        let n31 = RawNode::new(&[1, 3, 1], 2, vec![&n41, &n42]);
        let n32 = RawNode::new(&[1, 3, 2], 2, vec![&n42, &n43]);
        let n33 = RawNode::new(&[1, 3, 3], 1, vec![&n43, &n44]);
        let n21 = RawNode::new(&[1, 2, 1], 2, vec![&n31, &n42, &n32]);
        let n22 = RawNode::new(&[1, 2, 2], 1, vec![&n32, &n33]);
        let n11 = RawNode::new(&[1, 1, 1], 0, vec![&n31, &n21, &n22]);

        let o31 = RawNode::new(&[2, 3, 1], 1, vec![]);
        let o32 = RawNode::new(&[2, 3, 2], 1, vec![]);
        let o21 = RawNode::new(&[2, 2, 1], 1, vec![&o31, &o32]);
        let o11 = RawNode::new(&[2, 1, 1], 0, vec![&n21, &n44, &o21]);

        let n_nodes = [&n41, &n42, &n43, &n44, &n31, &n32, &n33, &n21, &n22, &n11];
        let o_nodes: [&RawNode; 4] = [&o31, &o32, &o21, &o11];

        let mut db = D::default();
        for n in n_nodes.iter().chain(o_nodes.iter()) {
            n.insert_into_db(&mut db);
        }

        // Test that getting root of uncached object recovers whole graph, and
        // that the result is returned in traversal order. In the other tests
        // here we don't bother checking the order of results.
        let kvs = db.bfs_get_nodes(&n11.key, |_| None, false, None, None);
        let keys: std::vec::Vec<_> = kvs.clone().into_iter().map(|(k, _)| k).collect();
        let expected_keys: std::vec::Vec<_> =
            [&n11, &n31, &n21, &n22, &n41, &n42, &n32, &n33, &n43, &n44]
                .map(|n| n.key.clone())
                .into_iter()
                .collect();
        assert_eq!(keys, expected_keys);

        // Test that getting root of overlapping object only db fetches new nodes.
        let cache: HashMap<_, _> = kvs.into_iter().collect();
        let kvs = db.bfs_get_nodes(&o11.key, |key| cache.get(key).cloned(), false, None, None);
        let keys: HashSet<_> = kvs.into_iter().map(|(k, _)| k).collect();
        let expected_keys: HashSet<_> = o_nodes.iter().map(|n| n.key.clone()).collect();
        assert_eq!(keys, expected_keys);

        // Test cache that contains some intermediate nodes, but not their descendents.
        let cache: HashMap<_, _> = [&n21, &n22, &n41]
            .map(|n| (n.key.clone(), n.clone().into_obj()))
            .into_iter()
            .collect();
        let kvs = db.bfs_get_nodes(&n11.key, |key| cache.get(key).cloned(), false, None, None);
        let keys: HashSet<_> = kvs.into_iter().map(|(k, _)| k).collect();
        let expected_keys: HashSet<_> = n_nodes
            .iter()
            .filter(|n| !cache.contains_key(&n.key))
            .map(|n| n.key.clone())
            .collect();
        assert_eq!(keys, expected_keys);
        // Again, but truncating.
        let kvs = db.bfs_get_nodes(&n11.key, |key| cache.get(key).cloned(), true, None, None);
        let keys: HashSet<_> = kvs.into_iter().map(|(k, _)| k).collect();
        let expected_keys: HashSet<_> = [&n11, &n31, &n42]
            .map(|n| n.key.clone())
            .into_iter()
            .collect();
        assert_eq!(keys, expected_keys);

        // Test max_depth. We could check the specific nodes returned, but I
        // don't think we necessarily want that to be part of the spec.
        let kvs = db.bfs_get_nodes(&n11.key, |_| None, false, Some(2), None);
        let mut keys: std::vec::Vec<_> = kvs.into_iter().map(|(k, _)| k).collect();
        let mut expected_keys: std::vec::Vec<_> = [
            &n11.key, &n21.key, &n22.key, &n31.key, &n32.key, &n33.key, &n41.key, &n42.key,
        ]
        .into_iter()
        .cloned()
        .collect();
        keys.sort();
        expected_keys.sort();
        assert_eq!(keys, expected_keys);

        // Test max_count.
        let kvs = db.bfs_get_nodes(&n11.key, |_| None, false, None, Some(5));
        let keys: std::vec::Vec<_> = kvs.into_iter().map(|(k, _)| k).collect();
        assert_eq!(keys.len(), 5);
    }

    #[cfg(not(feature = "layout-v2"))]
    #[test]
    fn get_unreachable_keys_inmemorydb() {
        test_get_unreachable_keys::<InMemoryDB>();
    }
    #[cfg(all(feature = "sqlite", not(feature = "layout-v2")))]
    #[test]
    fn get_unreachable_keys_sqldb() {
        test_get_unreachable_keys::<crate::db::SqlDB>();
    }
    #[cfg(all(feature = "parity-db", not(feature = "layout-v2")))]
    #[test]
    fn get_unreachable_keys_paritydb() {
        test_get_unreachable_keys::<crate::db::ParityDb>();
    }
    /// Helper for creating DB-specific tests of the `DB::get_unreachable_keys`
    /// API.
    ///
    /// This is also called in `crate::db::sql::tests::get_unreachable_keys`.
    #[cfg(not(feature = "layout-v2"))]
    fn test_get_unreachable_keys<D: DB<Hasher = DefaultHasher>>() {
        let mut db = D::default();
        let n41 = RawNode::new(&[4, 1], 0, vec![]);
        let n31 = RawNode::new(&[3, 1], 1, vec![]);
        let n32 = RawNode::new(&[3, 2], 0, vec![]);
        let n33 = RawNode::new(&[3, 3], 1, vec![]);
        let n21 = RawNode::new(&[2, 1], 0, vec![&n31, &n33]);
        let n22 = RawNode::new(&[2, 2], 1, vec![]);
        let n11 = RawNode::new(&[1, 1], 0, vec![&n22]);
        let nodes = [&n41, &n31, &n32, &n33, &n21, &n22, &n11];
        for n in nodes {
            n.insert_into_db(&mut db);
        }

        ////////////////////////////////////////////////////////////////

        let keys: HashSet<_> = [&n11, &n21, &n32, &n41]
            .map(|n| n.key.clone())
            .into_iter()
            .collect();
        assert_eq!(keys, db.get_unreachable_keys().into_iter().collect());

        ////////////////////////////////////////////////////////////////

        db.set_root_count(n11.key.clone(), 1);
        db.set_root_count(n22.key.clone(), 1);
        let keys: HashSet<_> = [&n21, &n32, &n41]
            .map(|n| n.key.clone())
            .into_iter()
            .collect();
        assert_eq!(keys, db.get_unreachable_keys().into_iter().collect());
        db.set_root_count(n11.key.clone(), 0);
        db.set_root_count(n22.key.clone(), 0);
    }

    #[cfg(not(feature = "layout-v2"))]
    #[test]
    fn update_ref_count_inmemorydb() {
        test_update_ref_count::<InMemoryDB>();
    }
    #[cfg(all(feature = "sqlite", not(feature = "layout-v2")))]
    #[test]
    fn update_ref_count_sqldb() {
        test_update_ref_count::<crate::db::SqlDB>();
    }
    #[cfg(all(feature = "parity-db", not(feature = "layout-v2")))]
    #[test]
    fn update_ref_count_paritydb() {
        test_update_ref_count::<crate::db::ParityDb>();
    }
    /// Test that updating the ref count of an existing node works
    /// correctly. This is of interest because ref-counts are not included in
    /// node key hashes, so an implementation that accidentally assumes content
    /// addressing may get this wrong.
    #[cfg(not(feature = "layout-v2"))]
    fn test_update_ref_count<D: DB>() {
        let mut db = D::default();
        let n1 = RawNode::new(&[1], 0, vec![]);
        let k1 = n1.key.clone();
        let n2 = RawNode::new(&[1], 1, vec![]);
        let k2 = n2.key.clone();
        assert_eq!(k1, k2);
        n1.insert_into_db(&mut db);
        assert_eq!(db.get_node(&k1).unwrap(), n1.into_obj());
        n2.insert_into_db(&mut db);
        assert_eq!(db.get_node(&k1).unwrap(), n2.into_obj());
    }

    // ========================================================================
    // TreeDB tests
    // ========================================================================

    use super::{
        InMemoryTreeDB, NewTreeNode, TreeChildRef, encode_tree_node_data,
    };

    /// Helper to create a leaf `NewTreeNode` with a given hash and payload.
    fn make_leaf(hash_bytes: &[u8], payload: &[u8]) -> (ArenaHash<DefaultHasher>, NewTreeNode) {
        let hash = ArenaHash::<DefaultHasher>::_from_bytes(hash_bytes);
        let data = encode_tree_node_data::<DefaultHasher>(&hash, payload);
        (hash, NewTreeNode { data, children: vec![] })
    }

    /// Helper to create a branch `NewTreeNode` with children.
    fn make_branch(
        hash_bytes: &[u8],
        payload: &[u8],
        children: Vec<TreeChildRef>,
    ) -> (ArenaHash<DefaultHasher>, NewTreeNode) {
        let hash = ArenaHash::<DefaultHasher>::_from_bytes(hash_bytes);
        let data = encode_tree_node_data::<DefaultHasher>(&hash, payload);
        (hash, NewTreeNode { data, children })
    }

    #[test]
    fn tree_db_insert_and_read_leaf() {
        let mut db = InMemoryTreeDB::<DefaultHasher>::default();
        let (hash, leaf) = make_leaf(&[1, 2, 3], &[10, 20, 30]);

        let addrs = db.insert_tree(hash.clone(), leaf);
        assert_eq!(addrs.len(), 1);

        let read_back = db.get_tree_root(&hash).expect("root should exist");
        assert_eq!(read_back.hash, hash);
        assert_eq!(read_back.data, vec![10, 20, 30]);
        assert!(read_back.children.is_empty());
    }

    #[test]
    fn tree_db_insert_and_read_tree() {
        let mut db = InMemoryTreeDB::<DefaultHasher>::default();

        let (leaf_hash, leaf) = make_leaf(&[1, 0, 0], &[1]);
        let (root_hash, root) = make_branch(
            &[2, 0, 0],
            &[2],
            vec![TreeChildRef::New(leaf)],
        );

        let addrs = db.insert_tree(root_hash.clone(), root);
        // Should return addresses for root + leaf = 2 new nodes
        assert_eq!(addrs.len(), 2);

        let root_node = db.get_tree_root(&root_hash).expect("root should exist");
        assert_eq!(root_node.hash, root_hash);
        assert_eq!(root_node.data, vec![2]);
        assert_eq!(root_node.children.len(), 1);

        // Read child by its address
        let child_addr = root_node.children[0];
        let child_node = db.get_node_by_addr(child_addr).expect("child should exist");
        assert_eq!(child_node.hash, leaf_hash);
        assert_eq!(child_node.data, vec![1]);
        assert!(child_node.children.is_empty());
    }

    #[test]
    fn tree_db_reference_counting() {
        let mut db = InMemoryTreeDB::<DefaultHasher>::default();
        let (hash, leaf) = make_leaf(&[1, 2, 3], &[42]);

        db.insert_tree(hash.clone(), leaf);
        assert_eq!(db.size(), 1);
        assert_eq!(db.get_roots().len(), 1);
        assert_eq!(*db.get_roots().get(&hash).unwrap(), 1);

        // Reference again
        db.reference_tree(&hash);
        assert_eq!(*db.get_roots().get(&hash).unwrap(), 2);

        // Dereference once - should still exist
        db.dereference_tree(&hash);
        assert_eq!(db.size(), 1);
        assert_eq!(*db.get_roots().get(&hash).unwrap(), 1);

        // Dereference again - should be removed
        db.dereference_tree(&hash);
        assert_eq!(db.size(), 0);
        assert!(db.get_roots().is_empty());
        assert!(db.get_tree_root(&hash).is_none());
    }

    #[test]
    fn tree_db_shared_subtree() {
        let mut db = InMemoryTreeDB::<DefaultHasher>::default();

        // Insert first tree: root1 -> shared_leaf
        let (_shared_hash, shared_leaf) = make_leaf(&[1, 0, 0], &[1]);
        let (root1_hash, root1) = make_branch(
            &[2, 0, 0],
            &[2],
            vec![TreeChildRef::New(shared_leaf)],
        );

        let addrs1 = db.insert_tree(root1_hash.clone(), root1);
        assert_eq!(addrs1.len(), 2);
        assert_eq!(db.size(), 2); // root1 + shared_leaf

        // Get the shared leaf's address
        let root1_node = db.get_tree_root(&root1_hash).unwrap();
        let shared_addr = root1_node.children[0];

        // Insert second tree: root2 -> shared_leaf (existing)
        let (root2_hash, root2) = make_branch(
            &[3, 0, 0],
            &[3],
            vec![TreeChildRef::Existing(shared_addr)],
        );

        let addrs2 = db.insert_tree(root2_hash.clone(), root2);
        assert_eq!(addrs2.len(), 1); // only root2 is new
        assert_eq!(db.size(), 3); // root1 + shared_leaf + root2

        // Dereference tree1 - shared leaf should survive
        db.dereference_tree(&root1_hash);
        assert_eq!(db.size(), 2); // shared_leaf + root2
        assert!(db.get_tree_root(&root1_hash).is_none());
        assert!(db.get_node_by_addr(shared_addr).is_some());

        // Dereference tree2 - everything should be gone
        db.dereference_tree(&root2_hash);
        assert_eq!(db.size(), 0);
        assert!(db.get_node_by_addr(shared_addr).is_none());
    }

    #[test]
    fn tree_db_encode_decode_roundtrip() {
        let hash = ArenaHash::<DefaultHasher>::_from_bytes(&[1, 2, 3]);
        let payload = vec![10, 20, 30, 40];

        let encoded = encode_tree_node_data::<DefaultHasher>(&hash, &payload);

        let child_addrs = vec![100, 200];
        let node_addr = 42;
        let decoded = super::decode_tree_node_data::<DefaultHasher>(&encoded, child_addrs.clone(), node_addr)
            .expect("decode should succeed");

        assert_eq!(decoded.hash, hash);
        assert_eq!(decoded.data, payload);
        assert_eq!(decoded.children, child_addrs);
        assert_eq!(decoded.addr, node_addr);
    }
}
