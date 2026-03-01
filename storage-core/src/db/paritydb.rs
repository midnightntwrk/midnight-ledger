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

use crypto::digest::OutputSizeUser;
use std::{collections::HashMap, fmt::Debug, marker::PhantomData};

#[cfg(feature = "proptest")]
use proptest::prelude::*;
use serialize::{Deserializable, Serializable};

use parity_db;
#[allow(deprecated)]
use sha2::digest::generic_array::GenericArray;

use crate::{DefaultHasher, WellBehavedHasher, arena::ArenaHash, backend::OnDiskObject};
use crate::arena::NodeAddress;

#[cfg(feature = "proptest")]
use super::DummyDBStrategy;
use super::{DB, DummyArbitrary, Update};

// Different value to Substrate: polkadot-sdk/substrate/client/db/src/utils.rs
// This means the `storage` database must be stored in a different file
// NOTE: We stay at 3 columns even with layout v2, to reserve a column for future GC purposes
const NUM_COLUMNS: u8 = 3;
const NODE_COLUMN: u8 = 0;
const GC_ROOT_COLUMN: u8 = 1;
#[cfg(not(feature = "layout-v2"))]
// Column to track which nodes have a ref count of zero
const REF_COUNT_ZERO: u8 = 2;

/// A database back-end using the `ParityDB` library.
pub struct ParityDb<H: WellBehavedHasher = DefaultHasher> {
    db: parity_db::Db,
    _phantom: std::marker::PhantomData<H>,
}

impl<H: WellBehavedHasher> Default for ParityDb<H> {
    fn default() -> Self {
        let dir = tempfile::TempDir::new().unwrap().keep();
        Self::open(&dir)
    }
}

impl<H: WellBehavedHasher> Debug for ParityDb<H> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ParityDb")
            .field("db", &"no-debug".to_string())
            .finish()
    }
}

fn serialize_node<H: WellBehavedHasher>(node: &OnDiskObject<H>) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(<OnDiskObject<H> as Serializable>::serialized_size(node));
    <OnDiskObject<H> as Serializable>::serialize(node, &mut bytes)
        .expect("Failed to serialize OnDiskObject");
    bytes
}

fn bytes_to_arena_key<H: WellBehavedHasher>(key_bytes: Vec<u8>) -> ArenaHash<H> {
    if key_bytes.len() != <H as OutputSizeUser>::output_size() {
        panic!(
            "incorrect length for gc_root key: found {}, expected {}",
            key_bytes.len(),
            <H as OutputSizeUser>::output_size()
        );
    }

    #[allow(deprecated)]
    ArenaHash(GenericArray::from_iter(key_bytes))
}

impl<H: WellBehavedHasher> ParityDb<H> {
    /// Open a new `ParityDB` at the given *directory* path.
    ///
    /// The on-disk representation of the database is a collection of files, so
    /// `path`, if it already exists, must be a directory, not a file. If the
    /// directory at `path` doesn't already exist it will be created.
    ///
    /// Note: This is an exclusive open. This method will panic if the database at the path is
    /// already open.
    pub fn open(path: &std::path::Path) -> Self {
        // The error message provided by `parity_db::DB::open_or_create` is not
        // very helpful in this case.
        if path.exists() && path.is_file() {
            panic!(
                "path '{}' is an existing file, but it must be a directory if it already exists",
                path.display()
            );
        }

        let mut options = parity_db::Options::with_columns(path, NUM_COLUMNS);
        // Add indexes to all columns - we need this to be able to iterate over them
        options.columns[GC_ROOT_COLUMN as usize].btree_index = true;
        // NOTE: Hardcoded because the constant is behind a feature flag.
        options.columns[2].btree_index = true;
        options.columns[NODE_COLUMN as usize].btree_index = true;
        let db = parity_db::Db::open_or_create(&options).unwrap_or_else(|e| {
            panic!(
                "parity-db open error: {e}. Note: Check db isn't already open. Path: {}",
                path.display()
            )
        });

        ParityDb {
            db,
            _phantom: PhantomData,
        }
    }
}

#[cfg(feature = "proptest")]
/// A dummy Arbitrary impl for `ParityDb` to allow for deriving Arbitrary on Sp<T, D>
impl<H: WellBehavedHasher> Arbitrary for ParityDb<H> {
    type Parameters = ();
    type Strategy = DummyDBStrategy<Self>;

    fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
        DummyDBStrategy::<Self>(PhantomData)
    }
}

impl<H: WellBehavedHasher> DummyArbitrary for ParityDb<H> {}

impl<H: WellBehavedHasher> DB for ParityDb<H> {
    type Hasher = H;

    /// Note: If the key was recently deleted, this may still return Some(_).
    fn get_node(&self, key: &ArenaHash<Self::Hasher>) -> Option<OnDiskObject<Self::Hasher>> {
        self.db
            .get(NODE_COLUMN, &key.0)
            .expect("failed to get from db")
            .map(|bytes| {
                OnDiskObject::<Self::Hasher>::deserialize(&mut &bytes[..], 0)
                    .expect("Failed to deserialize OnDiskObject")
            })
    }

    #[cfg(not(feature = "layout-v2"))]
    fn get_unreachable_keys(&self) -> Vec<ArenaHash<Self::Hasher>> {
        let mut it = self
            .db
            .iter(REF_COUNT_ZERO)
            .expect("Failed to iterate over db");

        let mut keys = Vec::new();
        while let Some((key, _)) = it.next().expect("Failed to get next from iterator") {
            let k = bytes_to_arena_key(key);
            if self.get_root_count(&k) == 0 {
                keys.push(k);
            }
        }
        keys
    }

    fn insert_node(
        &mut self,
        key: crate::arena::ArenaHash<Self::Hasher>,
        object: OnDiskObject<Self::Hasher>,
    ) {
        #[allow(unused_mut, reason = "for feature flags")]
        let mut ops = vec![(
            NODE_COLUMN,
            parity_db::Operation::Set(key.0.to_vec(), serialize_node(&object)),
        )];
        #[cfg(not(feature = "layout-v2"))]
        ops.push((
            REF_COUNT_ZERO,
            if object.ref_count == 0 {
                parity_db::Operation::Set(key.0.to_vec(), vec![])
            } else {
                parity_db::Operation::Dereference(key.0.to_vec())
            },
        ));
        self.db.commit_changes(ops).expect("Failed to commit to db");
    }

    fn delete_node(&mut self, key: &crate::arena::ArenaHash<Self::Hasher>) {
        #[allow(unused_mut, reason = "for feature flags")]
        let mut ops = vec![(
            NODE_COLUMN,
            parity_db::Operation::Dereference(key.0.to_vec()),
        )];
        #[cfg(not(feature = "layout-v2"))]
        ops.push((
            REF_COUNT_ZERO,
            parity_db::Operation::Dereference(key.0.to_vec()),
        ));
        self.db.commit_changes(ops).expect("Failed to commit to db");
    }

    fn batch_update<I>(&mut self, iter: I)
    where
        I: Iterator<Item = (ArenaHash<Self::Hasher>, Update<Self::Hasher>)>,
    {
        let mut ops = Vec::new();
        for (key, update) in iter {
            match update {
                Update::InsertNode(object) => {
                    ops.push((
                        NODE_COLUMN,
                        parity_db::Operation::Set(key.0.to_vec(), serialize_node(&object)),
                    ));
                    #[cfg(not(feature = "layout-v2"))]
                    ops.push((
                        REF_COUNT_ZERO,
                        if object.ref_count == 0 {
                            parity_db::Operation::Set(key.0.to_vec(), vec![])
                        } else {
                            parity_db::Operation::Dereference(key.0.to_vec())
                        },
                    ));
                }
                Update::SetRootCount(count) => {
                    ops.push((
                        GC_ROOT_COLUMN,
                        if count == 0 {
                            parity_db::Operation::Dereference(key.0.to_vec())
                        } else {
                            parity_db::Operation::Set(key.0.to_vec(), count.to_le_bytes().to_vec())
                        },
                    ));
                }
                Update::DeleteNode => {
                    ops.push((
                        NODE_COLUMN,
                        parity_db::Operation::Dereference(key.0.to_vec()),
                    ));
                    #[cfg(not(feature = "layout-v2"))]
                    ops.push((
                        REF_COUNT_ZERO,
                        parity_db::Operation::Dereference(key.0.to_vec()),
                    ));
                }
            }
        }
        self.db.commit_changes(ops).expect("Failed to commit to db");
    }

    fn batch_get_nodes<I>(
        &self,
        keys: I,
    ) -> Vec<(ArenaHash<Self::Hasher>, Option<OnDiskObject<Self::Hasher>>)>
    where
        I: Iterator<Item = ArenaHash<Self::Hasher>>,
    {
        crate::db::dubious_batch_get_nodes(self, keys)
    }

    fn get_root_count(&self, key: &crate::arena::ArenaHash<Self::Hasher>) -> u32 {
        self.db
            .get(GC_ROOT_COLUMN, &key.0)
            .expect("failed to get from db")
            .map(|bytes| {
                u32::from_le_bytes(bytes.try_into().expect("gc root count should be 4 bytes"))
            })
            .unwrap_or(0)
    }

    fn set_root_count(&mut self, key: ArenaHash<Self::Hasher>, count: u32) {
        let ops = vec![(
            GC_ROOT_COLUMN,
            if count == 0 {
                parity_db::Operation::Dereference(key.0.to_vec())
            } else {
                parity_db::Operation::Set(key.0.to_vec(), count.to_le_bytes().to_vec())
            },
        )];
        self.db.commit_changes(ops).expect("Failed to commit to db");
    }

    fn get_roots(&self) -> HashMap<ArenaHash<Self::Hasher>, u32> {
        let mut it = self
            .db
            .iter(GC_ROOT_COLUMN)
            .expect("Failed to iterate over db");

        let mut map = HashMap::new();
        while let Some((key, value)) = it.next().expect("Failed to get next from iterator") {
            let k = bytes_to_arena_key(key);
            let v = u32::from_le_bytes(value.try_into().expect("gc root count should be 4 bytes"));
            map.insert(k, v);
        }
        map
    }

    fn size(&self) -> usize {
        let mut it = self
            .db
            .iter(NODE_COLUMN)
            .expect("Failed to iterate over db");

        let mut count = 0;
        while it
            .next()
            .expect("Failed to get next from iterator")
            .is_some()
        {
            count += 1;
        }
        count
    }
}

// ============================================================================
// ParityDb TreeDB implementation using multitree columns
// ============================================================================

use super::{
    NewTreeNode, TreeChildRef, TreeReadNode, decode_tree_node_data,
};

/// Column layout for the tree-oriented ParityDb.
const TREE_NUM_COLUMNS: u8 = 1;
const TREE_COLUMN: u8 = 0;

/// A ParityDB backend configured for multitree operations.
///
/// Uses a single multitree column where trees are stored under root hash keys
/// with built-in reference counting. Each tree has a synthetic wrapper root
/// (with empty data) whose single child is the actual root — this gives the
/// actual root a `NodeAddress` so other trees can reference it.
pub struct ParityDbTree<H: WellBehavedHasher = DefaultHasher> {
    db: parity_db::Db,
    _phantom: PhantomData<H>,
}

impl<H: WellBehavedHasher> Default for ParityDbTree<H> {
    fn default() -> Self {
        let dir = tempfile::TempDir::new().unwrap().keep();
        Self::open(&dir)
    }
}

impl<H: WellBehavedHasher> Debug for ParityDbTree<H> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ParityDbTree")
            .field("db", &"no-debug".to_string())
            .finish()
    }
}

impl<H: WellBehavedHasher> DummyArbitrary for ParityDbTree<H> {}

#[cfg(feature = "proptest")]
impl<H: WellBehavedHasher> Arbitrary for ParityDbTree<H> {
    type Parameters = ();
    type Strategy = DummyDBStrategy<Self>;

    fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
        DummyDBStrategy::<Self>(PhantomData)
    }
}

impl<H: WellBehavedHasher> ParityDbTree<H> {
    /// Open a multitree-mode ParityDB at the given directory path.
    pub fn open(path: &std::path::Path) -> Self {
        if path.exists() && path.is_file() {
            panic!(
                "path '{}' is an existing file, but it must be a directory if it already exists",
                path.display()
            );
        }

        let mut options = parity_db::Options::with_columns(path, TREE_NUM_COLUMNS);
        options.columns[TREE_COLUMN as usize].multitree = true;
        options.columns[TREE_COLUMN as usize].allow_direct_node_access = true;
        // ref_counted is needed for ReferenceTree/DereferenceTree operations
        options.columns[TREE_COLUMN as usize].ref_counted = true;
        // preimage is required when ref_counted is enabled
        options.columns[TREE_COLUMN as usize].preimage = true;
        // Multitree columns don't support compression
        options.columns[TREE_COLUMN as usize].compression = parity_db::CompressionType::NoCompression;

        let db = parity_db::Db::open_or_create(&options).unwrap_or_else(|e| {
            panic!(
                "parity-db open error: {e}. Note: Check db isn't already open. Path: {}",
                path.display()
            )
        });

        ParityDbTree {
            db,
            _phantom: PhantomData,
        }
    }

    /// Convert a `NewTreeNode` tree into a parity-db `NewNode` tree,
    /// wrapped in a synthetic root.
    fn to_parity_tree(root: NewTreeNode) -> parity_db::NewNode {
        fn convert_node(node: NewTreeNode) -> parity_db::NewNode {
            let children = node
                .children
                .into_iter()
                .map(|child| match child {
                    TreeChildRef::New(child_node) => {
                        parity_db::NodeRef::New(convert_node(child_node))
                    }
                    TreeChildRef::Existing(addr) => {
                        parity_db::NodeRef::Existing(addr)
                    }
                })
                .collect();
            parity_db::NewNode {
                data: node.data,
                children,
            }
        }

        // Synthetic wrapper: empty data, single child = actual root
        parity_db::NewNode {
            data: vec![],
            children: vec![parity_db::NodeRef::New(convert_node(root))],
        }
    }

    /// Walk a just-written tree via `get_node` in DFS pre-order, collecting
    /// `NodeAddress` values for all `New` nodes. We skip `Existing` refs since
    /// they already have known addresses.
    ///
    /// `template` is the original `NewTreeNode` tree used to know which children
    /// were `New` vs `Existing`. `wrapper_children` is the children list from
    /// the wrapper root (should be `[actual_root_addr]`).
    fn collect_new_addresses(
        &self,
        template: &NewTreeNode,
        actual_root_addr: NodeAddress,
    ) -> Vec<NodeAddress> {
        let mut addrs = Vec::new();
        self.collect_addrs_recursive(template, actual_root_addr, &mut addrs);
        addrs
    }

    fn collect_addrs_recursive(
        &self,
        template: &NewTreeNode,
        addr: NodeAddress,
        addrs: &mut Vec<NodeAddress>,
    ) {
        addrs.push(addr);

        // Read this node to get its child addresses
        let (_, child_addrs) = self
            .db
            .get_node(TREE_COLUMN, addr)
            .expect("failed to read node from db")
            .expect("just-written node should exist");

        assert_eq!(
            child_addrs.len(),
            template.children.len(),
            "child count mismatch between template and DB"
        );

        for (child_ref, &child_addr) in template.children.iter().zip(child_addrs.iter()) {
            if let TreeChildRef::New(child_template) = child_ref {
                self.collect_addrs_recursive(child_template, child_addr, addrs);
            }
        }
    }

    /// Decode a node read from parity-db into a `TreeReadNode`.
    fn decode_parity_node(
        data: &[u8],
        child_addrs: Vec<NodeAddress>,
        addr: NodeAddress,
    ) -> Option<TreeReadNode<H>> {
        decode_tree_node_data::<H>(data, child_addrs, addr)
    }
}

impl<H: WellBehavedHasher> DB for ParityDbTree<H> {
    type Hasher = H;

    fn get_roots(&self) -> HashMap<ArenaHash<Self::Hasher>, u32> {
        // ParityDB's multitree doesn't expose root counts directly via iteration.
        // We need to track roots externally or use btree_index iteration.
        // For now, since we don't have btree_index on the multitree column,
        // this is a limitation. We'll need a separate column or external tracking.
        //
        // TODO: This needs a secondary index. For now, return empty.
        // The StorageBackend will need to track roots itself when using DB.
        HashMap::new()
    }

    fn insert_tree(
        &mut self,
        key: ArenaHash<Self::Hasher>,
        root: NewTreeNode,
    ) -> Vec<NodeAddress> {
        let parity_tree = Self::to_parity_tree(root.clone());

        let ops = vec![(
            TREE_COLUMN,
            parity_db::Operation::InsertTree(key.0.to_vec(), parity_tree),
        )];
        self.db
            .commit_changes(ops)
            .expect("Failed to commit tree to db");

        // Read back the wrapper root to get the actual root's address
        let (_, wrapper_children) = self
            .db
            .get_root(TREE_COLUMN, &key.0)
            .expect("failed to read wrapper root")
            .expect("just-written wrapper root should exist");

        assert_eq!(
            wrapper_children.len(),
            1,
            "wrapper root should have exactly one child"
        );
        let actual_root_addr = wrapper_children[0];

        self.collect_new_addresses(&root, actual_root_addr)
    }

    fn reference_tree(&mut self, key: &ArenaHash<Self::Hasher>) {
        let ops = vec![(
            TREE_COLUMN,
            parity_db::Operation::ReferenceTree(key.0.to_vec()),
        )];
        self.db
            .commit_changes(ops)
            .expect("Failed to reference tree in db");
    }

    fn dereference_tree(&mut self, key: &ArenaHash<Self::Hasher>) {
        let ops = vec![(
            TREE_COLUMN,
            parity_db::Operation::DereferenceTree(key.0.to_vec()),
        )];
        self.db
            .commit_changes(ops)
            .expect("Failed to dereference tree in db");
    }

    fn get_tree_root(
        &self,
        key: &ArenaHash<Self::Hasher>,
    ) -> Option<TreeReadNode<Self::Hasher>> {
        // Read the wrapper root
        let (_, wrapper_children) = self
            .db
            .get_root(TREE_COLUMN, &key.0)
            .expect("failed to read from db")?;

        if wrapper_children.is_empty() {
            return None;
        }

        let actual_root_addr = wrapper_children[0];
        self.get_node_by_addr(actual_root_addr)
    }

    fn get_node_by_addr(&self, addr: NodeAddress) -> Option<TreeReadNode<Self::Hasher>> {
        let (data, child_addrs) = self
            .db
            .get_node(TREE_COLUMN, addr)
            .expect("failed to read node from db")?;

        Self::decode_parity_node(&data, child_addrs, addr)
    }
}

#[cfg(test)]
mod tests {
    use super::ParityDb;

    /// Disallow two open parity-db instances pointing to the same directory.
    #[test]
    fn disallow_concurrent_access_file() {
        let path: tempfile::TempDir = tempfile::TempDir::new().unwrap();
        let mk_db = || ParityDb::open(path.path());

        let _first_db: ParityDb<sha2::Sha256> = mk_db();
        let second_db = std::panic::catch_unwind(mk_db);
        assert!(second_db.is_err());
    }

    /// Allow opening a new instance pointing to the same directory once any
    /// existing instance has been dropped.
    #[test]
    fn allow_serial_access_file() {
        let path = tempfile::TempDir::new().unwrap().keep();
        let mk_db = || ParityDb::open(&path);
        let first_db: ParityDb<sha2::Sha256> = mk_db();
        drop(first_db);
        mk_db();
    }

    // TreeDB tests for ParityDbTree
    use super::ParityDbTree;
    use crate::DefaultHasher;
    use crate::arena::ArenaHash;
    use crate::db::{DB, NewTreeNode, TreeChildRef, encode_tree_node_data};

    fn make_leaf(hash_bytes: &[u8], payload: &[u8]) -> (ArenaHash<DefaultHasher>, NewTreeNode) {
        let hash = ArenaHash::<DefaultHasher>::_from_bytes(hash_bytes);
        let data = encode_tree_node_data::<DefaultHasher>(&hash, payload);
        (hash, NewTreeNode { data, children: vec![] })
    }

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
    fn parity_tree_insert_and_read_leaf() {
        let mut db = ParityDbTree::<DefaultHasher>::default();
        let (hash, leaf) = make_leaf(&[1, 2, 3], &[10, 20, 30]);

        let addrs = db.insert_tree(hash.clone(), leaf);
        assert_eq!(addrs.len(), 1);

        let read_back = db.get_tree_root(&hash).expect("root should exist");
        assert_eq!(read_back.hash, hash);
        assert_eq!(read_back.data, vec![10, 20, 30]);
        assert!(read_back.children.is_empty());
    }

    #[test]
    fn parity_tree_insert_and_read_tree() {
        let mut db = ParityDbTree::<DefaultHasher>::default();

        let (leaf_hash, leaf) = make_leaf(&[1, 0, 0], &[1]);
        let (root_hash, root) = make_branch(
            &[2, 0, 0],
            &[2],
            vec![TreeChildRef::New(leaf)],
        );

        let addrs = db.insert_tree(root_hash.clone(), root);
        assert_eq!(addrs.len(), 2);

        let root_node = db.get_tree_root(&root_hash).expect("root should exist");
        assert_eq!(root_node.hash, root_hash);
        assert_eq!(root_node.data, vec![2]);
        assert_eq!(root_node.children.len(), 1);

        let child_addr = root_node.children[0];
        let child_node = db.get_node_by_addr(child_addr).expect("child should exist");
        assert_eq!(child_node.hash, leaf_hash);
        assert_eq!(child_node.data, vec![1]);
        assert!(child_node.children.is_empty());
    }

    #[test]
    fn parity_tree_shared_subtree() {
        let mut db = ParityDbTree::<DefaultHasher>::default();

        // Insert first tree: root1 -> shared_leaf
        let (_shared_hash, shared_leaf) = make_leaf(&[1, 0, 0], &[1]);
        let (root1_hash, root1) = make_branch(
            &[2, 0, 0],
            &[2],
            vec![TreeChildRef::New(shared_leaf)],
        );

        let addrs1 = db.insert_tree(root1_hash.clone(), root1);
        assert_eq!(addrs1.len(), 2);

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

        // Both roots should be readable
        let r1 = db.get_tree_root(&root1_hash).expect("root1 should exist");
        let r2 = db.get_tree_root(&root2_hash).expect("root2 should exist");
        assert_eq!(r1.data, vec![2]);
        assert_eq!(r2.data, vec![3]);

        // Both should reference the shared leaf by address
        let shared_node = db.get_node_by_addr(shared_addr).expect("shared should exist");
        assert_eq!(shared_node.data, vec![1]);

        // Dereference tree1 - shared leaf should survive (via tree2)
        db.dereference_tree(&root1_hash);
        assert!(db.get_node_by_addr(shared_addr).is_some(), "shared leaf should survive");

        // Dereference tree2 - everything should be cleaned up
        db.dereference_tree(&root2_hash);
        // Note: parity-db may not immediately reclaim nodes, so we can't
        // necessarily assert that get_node returns None here.
    }

    #[test]
    fn parity_tree_reference_dereference() {
        let mut db = ParityDbTree::<DefaultHasher>::default();
        let (hash, leaf) = make_leaf(&[1, 2, 3], &[42]);

        db.insert_tree(hash.clone(), leaf);
        assert!(db.get_tree_root(&hash).is_some());

        // Reference again
        db.reference_tree(&hash);

        // Dereference once - should still exist
        db.dereference_tree(&hash);
        assert!(db.get_tree_root(&hash).is_some());

        // Dereference again - logically removed, but parity-db may not
        // immediately reclaim data (deleted nodes may still be readable briefly).
        db.dereference_tree(&hash);
        // Note: We can't assert get_tree_root returns None here because
        // parity-db's eventual consistency model means recently deleted data
        // may still be readable. The InMemoryTreeDB test covers this assertion.
    }
}
