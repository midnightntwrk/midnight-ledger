// This file is part of midnight-ledger.
// Copyright (C) Midnight Foundation
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

use digest::OutputSizeUser;
use std::{collections::HashMap, fmt::Debug, ops::Deref, sync::Arc};

#[cfg(feature = "proptest")]
use proptest::prelude::*;
use serialize::{Deserializable, Serializable};

#[allow(deprecated)]
use digest::common::array::Array;
use parity_db;

#[cfg(feature = "layout-v2")]
use crate::arena::ArenaKey;
use crate::{DefaultHasher, WellBehavedHasher, arena::ArenaHash, backend::OnDiskObject};

#[cfg(feature = "proptest")]
use super::DummyDBStrategy;
use super::{DB, DummyArbitrary, Update};

// Different value to Substrate: polkadot-sdk/substrate/client/db/src/utils.rs
// This means the `storage` database must be stored in a different file
// NOTE: We stay at 3 columns even with layout v2, to reserve a column for future GC purposes
/// Number of columns used for ParityDb instance
pub const NUM_COLUMNS: u8 = 3;
/// Column index for storing storage nodes
pub const NODE_COLUMN: u8 = 0;
/// Column index for storing reference counts
pub const GC_ROOT_COLUMN: u8 = 1;
#[cfg(not(feature = "layout-v2"))]
/// Column to track which nodes have a ref count of zero
pub const REF_COUNT_ZERO: u8 = 2;
#[cfg(feature = "layout-v2")]
/// Column holding, per node hash, the node's serialized `children` (the same encoding as inside
/// [`OnDiskObject`]). A backend-internal index, fully derivable from the node column, so the
/// garbage collector can enumerate keys and read child references without loading node data.
/// Reuses the column that `REF_COUNT_ZERO` occupies without `layout-v2`.
pub const CHILDREN_COLUMN: u8 = 2;
#[cfg(feature = "layout-v2")]
/// Key of the completeness marker in [`CHILDREN_COLUMN`]. Its presence means every node in the
/// node column has a children entry. It is deliberately not hash-sized, so it can never collide
/// with a node key.
const CHILDREN_COMPLETE_MARKER: &[u8] = b"children-complete";

/// A wrapper around `Arc<parity_db::Db>` that owns a database instance.
pub struct OwnedDb(pub Arc<parity_db::Db>);

impl OwnedDb {
    /// Open a new `ParityDB` at the given *directory* path.
    ///
    /// The on-disk representation of the database is a collection of files, so
    /// `path`, if it already exists, must be a directory, not a file. If the
    /// directory at `path` doesn't already exist it will be created.
    ///
    /// Note: This is an exclusive open. This method will panic if the database at the path is
    /// already open.
    pub fn new(path: &std::path::Path) -> Self {
        if path.exists() && path.is_file() {
            panic!(
                "path '{}' is an existing file, but it must be a directory if it already exists",
                path.display()
            );
        }

        let mut options = parity_db::Options::with_columns(path, NUM_COLUMNS);
        set_init_options(&mut options, 0, false);

        let db = parity_db::Db::open_or_create(&options).unwrap_or_else(|e| {
            panic!(
                "parity-db open error: {e}. Note: Check db isn't already open. Path: {}",
                path.display()
            )
        });

        OwnedDb(Arc::new(db))
    }
}

impl Deref for OwnedDb {
    type Target = parity_db::Db;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Default for OwnedDb {
    fn default() -> Self {
        let dir = tempfile::TempDir::new().unwrap().keep();
        Self::new(&dir)
    }
}

/// Sets parity_db Options for midnight-storage compatibility.
pub fn set_init_options(
    options: &mut parity_db::Options,
    column_offset: u8,
    use_compression: bool,
) {
    // Add indexes to all columns - we need this to be able to iterate over them
    options.columns[(column_offset + GC_ROOT_COLUMN) as usize].btree_index = true;
    // NOTE: Hardcoded because the constant is behind a feature flag.
    options.columns[(column_offset + 2) as usize].btree_index = true;
    options.columns[(column_offset + NODE_COLUMN) as usize].btree_index = true;
    if use_compression {
        options.columns[(column_offset + NODE_COLUMN) as usize].compression =
            parity_db::CompressionType::Lz4;
    }
}

/// A database back-end using the `ParityDB` library.
pub struct ParityDb<
    H: WellBehavedHasher = DefaultHasher,
    D: Deref<Target = parity_db::Db> + Send + Sync + 'static = OwnedDb,
    const COLUMN_OFFSET: u8 = 0,
> {
    db: D,
    _phantom: std::marker::PhantomData<H>,
}

impl<
    H: WellBehavedHasher,
    D: Deref<Target = parity_db::Db> + Default + Send + Sync + 'static,
    const COLUMN_OFFSET: u8,
> Default for ParityDb<H, D, COLUMN_OFFSET>
{
    fn default() -> Self {
        Self::from_existing_db(Default::default())
    }
}

impl<
    H: WellBehavedHasher,
    D: Deref<Target = parity_db::Db> + Send + Sync + 'static,
    const COLUMN_OFFSET: u8,
> Debug for ParityDb<H, D, COLUMN_OFFSET>
{
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

#[cfg(feature = "layout-v2")]
fn serialize_children<H: WellBehavedHasher>(children: &Vec<ArenaKey<H>>) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(<Vec<ArenaKey<H>> as Serializable>::serialized_size(
        children,
    ));
    <Vec<ArenaKey<H>> as Serializable>::serialize(children, &mut bytes)
        .expect("Failed to serialize node children");
    bytes
}

/// `key + 1` as a fixed-width big-endian number, or `None` if `key` is all `0xff`: the resume
/// handle of a scan batch whose last key is `key`.
#[cfg(feature = "gc-v1")]
fn next_key(key: &[u8]) -> Option<Vec<u8>> {
    let mut key = key.to_vec();
    let mut i = key.len() - 1;
    while i > 0 && key[i] == 0xff {
        key[i] = 0x00;
        i -= 1;
    }
    if i == 0 && key[i] == 0xff {
        None
    } else {
        key[i] += 1;
        Some(key)
    }
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
    ArenaHash(Array::from_iter(key_bytes))
}

impl<H: WellBehavedHasher, const COLUMN_OFFSET: u8> ParityDb<H, OwnedDb, COLUMN_OFFSET> {
    /// Open a new `ParityDB` at the given *directory* path.
    ///
    /// The on-disk representation of the database is a collection of files, so
    /// `path`, if it already exists, must be a directory, not a file. If the
    /// directory at `path` doesn't already exist it will be created.
    ///
    /// Note: This is an exclusive open. This method will panic if the database at the path is
    /// already open.
    pub fn open(path: &std::path::Path) -> Self {
        Self::from_existing_db(OwnedDb::new(path))
    }
}

impl<
    H: WellBehavedHasher,
    D: Deref<Target = parity_db::Db> + Send + Sync + 'static,
    const COLUMN_OFFSET: u8,
> ParityDb<H, D, COLUMN_OFFSET>
{
    /// Initialize using an existing ParityDB database instance. Database options MUST first be
    /// set using `set_init_options`.
    pub fn from_existing_db(db: D) -> Self {
        let res = Self {
            db,
            _phantom: Default::default(),
        };
        #[cfg(feature = "layout-v2")]
        res.init_children_marker();
        res
    }

    /// If the node column is empty, record in the children column that it is complete: every
    /// node written from now on gets a children entry alongside it, so a children-column scan
    /// enumerates every node. A database that already holds nodes (possibly written before the
    /// children column was maintained) never gets the marker, and keeps using node-column scans.
    #[cfg(feature = "layout-v2")]
    fn init_children_marker(&self) {
        let empty = self
            .db
            .iter(COLUMN_OFFSET + NODE_COLUMN)
            .expect("Failed to iterate over db")
            .next()
            .expect("Failed to get next from iterator")
            .is_none();
        if empty {
            self.db
                .commit_changes([(
                    COLUMN_OFFSET + CHILDREN_COLUMN,
                    parity_db::Operation::Set(CHILDREN_COMPLETE_MARKER.to_vec(), vec![]),
                )])
                .expect("Failed to commit to db");
        }
    }

    #[cfg(feature = "gc-v1")]
    fn children_complete(&self) -> bool {
        self.db
            .get(COLUMN_OFFSET + CHILDREN_COLUMN, CHILDREN_COMPLETE_MARKER)
            .expect("failed to get from db")
            .is_some()
    }
}

#[cfg(feature = "proptest")]
/// A dummy Arbitrary impl for `ParityDb` to allow for deriving Arbitrary on Sp<T, D>
impl<
    H: WellBehavedHasher,
    D: Deref<Target = parity_db::Db> + Default + Send + Sync + 'static,
    const COLUMN_OFFSET: u8,
> Arbitrary for ParityDb<H, D, COLUMN_OFFSET>
{
    type Parameters = ();
    type Strategy = DummyDBStrategy<Self>;

    fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
        DummyDBStrategy::<Self>(std::marker::PhantomData)
    }
}

impl<
    H: WellBehavedHasher,
    D: Deref<Target = parity_db::Db> + Default + Send + Sync + 'static,
    const COLUMN_OFFSET: u8,
> DummyArbitrary for ParityDb<H, D, COLUMN_OFFSET>
{
}

impl<
    H: WellBehavedHasher,
    D: Deref<Target = parity_db::Db> + Default + Sync + Send + 'static,
    const COLUMN_OFFSET: u8,
> DB for ParityDb<H, D, COLUMN_OFFSET>
{
    type Hasher = H;
    #[cfg(feature = "gc-v1")]
    type ScanResumeHandle = Vec<u8>;

    /// Note: If the key was recently deleted, this may still return Some(_).
    fn get_node(&self, key: &ArenaHash<Self::Hasher>) -> Option<OnDiskObject<Self::Hasher>> {
        self.db
            .get(COLUMN_OFFSET + NODE_COLUMN, &key.0)
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
            .iter(COLUMN_OFFSET + REF_COUNT_ZERO)
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
            COLUMN_OFFSET + NODE_COLUMN,
            parity_db::Operation::Set(key.0.to_vec(), serialize_node(&object)),
        )];
        #[cfg(not(feature = "layout-v2"))]
        ops.push((
            COLUMN_OFFSET + REF_COUNT_ZERO,
            if object.ref_count == 0 {
                parity_db::Operation::Set(key.0.to_vec(), vec![])
            } else {
                parity_db::Operation::Dereference(key.0.to_vec())
            },
        ));
        #[cfg(feature = "layout-v2")]
        ops.push((
            COLUMN_OFFSET + CHILDREN_COLUMN,
            parity_db::Operation::Set(key.0.to_vec(), serialize_children(&object.children)),
        ));
        self.db.commit_changes(ops).expect("Failed to commit to db");
    }

    fn delete_node(&mut self, key: &crate::arena::ArenaHash<Self::Hasher>) {
        #[allow(unused_mut, reason = "for feature flags")]
        let mut ops = vec![(
            COLUMN_OFFSET + NODE_COLUMN,
            parity_db::Operation::Dereference(key.0.to_vec()),
        )];
        #[cfg(not(feature = "layout-v2"))]
        ops.push((
            COLUMN_OFFSET + REF_COUNT_ZERO,
            parity_db::Operation::Dereference(key.0.to_vec()),
        ));
        #[cfg(feature = "layout-v2")]
        ops.push((
            COLUMN_OFFSET + CHILDREN_COLUMN,
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
                        COLUMN_OFFSET + NODE_COLUMN,
                        parity_db::Operation::Set(key.0.to_vec(), serialize_node(&object)),
                    ));
                    #[cfg(not(feature = "layout-v2"))]
                    ops.push((
                        COLUMN_OFFSET + REF_COUNT_ZERO,
                        if object.ref_count == 0 {
                            parity_db::Operation::Set(key.0.to_vec(), vec![])
                        } else {
                            parity_db::Operation::Dereference(key.0.to_vec())
                        },
                    ));
                    #[cfg(feature = "layout-v2")]
                    ops.push((
                        COLUMN_OFFSET + CHILDREN_COLUMN,
                        parity_db::Operation::Set(
                            key.0.to_vec(),
                            serialize_children(&object.children),
                        ),
                    ));
                }
                Update::SetRootCount(count) => {
                    ops.push((
                        COLUMN_OFFSET + GC_ROOT_COLUMN,
                        if count == 0 {
                            parity_db::Operation::Dereference(key.0.to_vec())
                        } else {
                            parity_db::Operation::Set(key.0.to_vec(), count.to_le_bytes().to_vec())
                        },
                    ));
                }
                Update::DeleteNode => {
                    ops.push((
                        COLUMN_OFFSET + NODE_COLUMN,
                        parity_db::Operation::Dereference(key.0.to_vec()),
                    ));
                    #[cfg(not(feature = "layout-v2"))]
                    ops.push((
                        COLUMN_OFFSET + REF_COUNT_ZERO,
                        parity_db::Operation::Dereference(key.0.to_vec()),
                    ));
                    #[cfg(feature = "layout-v2")]
                    ops.push((
                        COLUMN_OFFSET + CHILDREN_COLUMN,
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
            .get(COLUMN_OFFSET + GC_ROOT_COLUMN, &key.0)
            .expect("failed to get from db")
            .map(|bytes| {
                u32::from_le_bytes(bytes.try_into().expect("gc root count should be 4 bytes"))
            })
            .unwrap_or(0)
    }

    fn set_root_count(&mut self, key: ArenaHash<Self::Hasher>, count: u32) {
        let ops = vec![(
            COLUMN_OFFSET + GC_ROOT_COLUMN,
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
            .iter(COLUMN_OFFSET + GC_ROOT_COLUMN)
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
            .iter(COLUMN_OFFSET + NODE_COLUMN)
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

    #[cfg(feature = "gc-v1")]
    fn scan(
        &self,
        resume_from: Option<Self::ScanResumeHandle>,
        batch_size: usize,
    ) -> (
        Vec<(ArenaHash<Self::Hasher>, OnDiskObject<Self::Hasher>)>,
        Option<Self::ScanResumeHandle>,
    ) {
        let mut it = self
            .db
            .iter(COLUMN_OFFSET + NODE_COLUMN)
            .expect("Failed to iterate over db");
        if let Some(handle) = resume_from {
            it.seek(&handle).expect("Failed to seek db iterator");
        }
        let mut res = Vec::with_capacity(batch_size);
        for _ in 0..batch_size {
            if let Some((k, v)) = it.next().expect("Failed db iteration step") {
                res.push((
                    bytes_to_arena_key(k),
                    OnDiskObject::<Self::Hasher>::deserialize(&mut &v[..], 0)
                        .expect("Failed to deserialize OnDiskObject"),
                ));
            } else {
                // We ran out, return
                return (res, None);
            }
        }
        let key = res.last().and_then(|(k, _)| next_key(&k.0));
        (res, key)
    }

    /// Served from the children column when it is known to be complete (see
    /// [`CHILDREN_COLUMN`]), so no node value is read or decompressed. Otherwise (a database
    /// that held nodes before the children column was maintained) falls back to scanning the
    /// node column. Both columns are ordered by key, so resume handles are interchangeable.
    #[cfg(feature = "gc-v1")]
    fn scan_keys(
        &self,
        resume_from: Option<Self::ScanResumeHandle>,
        batch_size: usize,
    ) -> (Vec<ArenaHash<Self::Hasher>>, Option<Self::ScanResumeHandle>) {
        if !self.children_complete() {
            let (nodes, handle) = self.scan(resume_from, batch_size);
            return (nodes.into_iter().map(|(k, _)| k).collect(), handle);
        }
        let mut it = self
            .db
            .iter(COLUMN_OFFSET + CHILDREN_COLUMN)
            .expect("Failed to iterate over db");
        if let Some(handle) = resume_from {
            it.seek(&handle).expect("Failed to seek db iterator");
        }
        let key_len = <H as OutputSizeUser>::output_size();
        let mut res = Vec::with_capacity(batch_size);
        while res.len() < batch_size {
            match it.next().expect("Failed db iteration step") {
                // We ran out, return
                None => return (res, None),
                // The completeness marker, not a node.
                Some((k, _)) if k.len() != key_len => continue,
                Some((k, _)) => res.push(bytes_to_arena_key(k)),
            }
        }
        let key = res.last().and_then(|k| next_key(&k.0));
        (res, key)
    }

    /// Served from the children column, so no node value is read or decompressed. A key
    /// without a children entry (a node written before the children column was maintained)
    /// falls back to decoding the node.
    #[cfg(feature = "gc-v1")]
    fn batch_get_children<I>(
        &self,
        keys: I,
    ) -> Vec<(ArenaHash<Self::Hasher>, Option<Vec<ArenaKey<Self::Hasher>>>)>
    where
        I: Iterator<Item = ArenaHash<Self::Hasher>>,
    {
        keys.map(|key| {
            let children = match self
                .db
                .get(COLUMN_OFFSET + CHILDREN_COLUMN, &key.0)
                .expect("failed to get from db")
            {
                Some(bytes) => Some(
                    <Vec<ArenaKey<Self::Hasher>>>::deserialize(&mut &bytes[..], 0)
                        .expect("Failed to deserialize node children"),
                ),
                None => self.get_node(&key).map(|node| node.children),
            };
            (key, children)
        })
        .collect()
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

    #[cfg(feature = "gc-v1")]
    mod children_sidecar {
        use super::super::{
            CHILDREN_COLUMN, CHILDREN_COMPLETE_MARKER, NODE_COLUMN, OwnedDb, ParityDb,
            serialize_node,
        };
        use crate::{
            DefaultHasher,
            arena::{ArenaHash, ArenaKey},
            backend::raw_node::RawNode,
            db::{DB, Update},
        };
        use std::collections::HashSet;

        type Db = ParityDb<DefaultHasher>;

        /// A 60-node chain plus a few nodes with several children, so child lists of
        /// lengths 0, 1, 2 and 3 are all exercised.
        fn sample_nodes() -> Vec<RawNode> {
            let mut nodes = vec![RawNode::new(&[0], 0, vec![])];
            for i in 1..60u8 {
                let n = RawNode::new(&[i], 0, vec![&nodes[nodes.len() - 1]]);
                nodes.push(n);
            }
            let wide2 = RawNode::new(&[200], 0, vec![&nodes[3], &nodes[7]]);
            let wide3 = RawNode::new(&[201], 0, vec![&nodes[1], &nodes[2], &wide2]);
            nodes.push(wide2);
            nodes.push(wide3);
            nodes
        }

        fn sidecar(db: &Db, key: &[u8]) -> Option<Vec<u8>> {
            db.db.get(CHILDREN_COLUMN, key).unwrap()
        }

        fn has_marker(db: &Db) -> bool {
            sidecar(db, CHILDREN_COMPLETE_MARKER).is_some()
        }

        fn all_scan_keys(db: &Db, batch: usize) -> Vec<ArenaHash<DefaultHasher>> {
            let (mut keys, mut handle) = (vec![], None);
            loop {
                let (ks, h) = db.scan_keys(handle, batch);
                keys.extend(ks);
                handle = h;
                if handle.is_none() {
                    return keys;
                }
            }
        }

        fn assert_children_match_nodes(db: &Db, keys: &[ArenaHash<DefaultHasher>]) {
            let objs = db.batch_get_nodes(keys.iter().cloned());
            let children = db.batch_get_children(keys.iter().cloned());
            assert_eq!(objs.len(), children.len());
            for ((k1, obj), (k2, ch)) in objs.into_iter().zip(children) {
                assert_eq!(k1, k2);
                assert_eq!(obj.map(|o| o.children), ch);
            }
        }

        /// The shared `DB` contract test (inserts through `insert_node`) holds for parity-db.
        #[test]
        fn keys_and_children_match_objects_paritydb() {
            let dir = tempfile::TempDir::new().unwrap();
            let mut db = Db::open(dir.path());
            crate::db::tests::test_keys_and_children_match_objects(&mut db);
            assert!(has_marker(&db));
        }

        /// A fresh database gets the marker, `batch_update` keeps the children column in step
        /// with the node column, and `scan_keys` (served from the children column) matches
        /// `scan` across resumes without ever yielding the marker.
        #[test]
        fn fresh_db_serves_keys_and_children_from_sidecar() {
            let dir = tempfile::TempDir::new().unwrap();
            let mut db = Db::open(dir.path());
            assert!(has_marker(&db), "an empty database must get the marker");
            assert!(db.children_complete());

            let nodes = sample_nodes();
            db.batch_update(
                nodes
                    .iter()
                    .map(|n| (n.key.clone(), Update::InsertNode(n.clone().into_obj()))),
            );

            // Every node has a children entry, encoded exactly as inside `OnDiskObject`.
            for n in &nodes {
                let bytes = sidecar(&db, &n.key.0).expect("missing children entry");
                let decoded =
                    <Vec<ArenaKey<DefaultHasher>> as serialize::Deserializable>::deserialize(
                        &mut &bytes[..],
                        0,
                    )
                    .unwrap();
                assert_eq!(decoded, n.children);
            }
            // Bytes per entry, by number of `Ref` children.
            for n in [&nodes[0], &nodes[1], &nodes[60], &nodes[61]] {
                let len = sidecar(&db, &n.key.0).unwrap().len();
                println!(
                    "children entry: {} children -> {len} bytes",
                    n.children.len()
                );
            }

            let mut keys: Vec<_> = nodes.iter().map(|n| n.key.clone()).collect();
            keys.push(ArenaHash::_from_bytes(&[0xff; 32]));
            assert_children_match_nodes(&db, &keys);

            let expected: HashSet<_> = nodes.iter().map(|n| n.key.clone()).collect();
            for batch in [1usize, 7, 1000] {
                let (mut from_scan, mut from_keys) = (vec![], vec![]);
                let (mut h_scan, mut h_keys) = (None, None);
                loop {
                    let (objs, h) = db.scan(h_scan, batch);
                    from_scan.extend(objs.into_iter().map(|(k, _)| k));
                    h_scan = h;
                    let (ks, h) = db.scan_keys(h_keys.clone(), batch);
                    from_keys.extend(ks);
                    assert_eq!(h, h_scan, "resume handles diverge, batch {batch}");
                    h_keys = h;
                    if h_scan.is_none() {
                        break;
                    }
                }
                assert_eq!(from_scan, from_keys, "batch {batch}");
                assert_eq!(from_keys.iter().cloned().collect::<HashSet<_>>(), expected);
                assert_eq!(from_keys.len(), nodes.len());
            }

            // Reopening a database that holds nodes keeps the marker it already has.
            drop(db);
            let db = Db::open(dir.path());
            assert!(has_marker(&db));
            assert_eq!(all_scan_keys(&db, 7).len(), nodes.len());
        }

        /// A database whose nodes were written without the children column (an older writer)
        /// never gets the marker; `scan_keys` and `batch_get_children` fall back to the node
        /// column and still see every node, and deletes clear both columns.
        #[test]
        fn pre_existing_db_falls_back_to_node_column() {
            let dir = tempfile::TempDir::new().unwrap();
            let nodes = sample_nodes();
            {
                // Simulate an older writer: node column only, no `ParityDb` (so no marker).
                let owned = OwnedDb::new(dir.path());
                owned
                    .commit_changes(nodes.iter().map(|n| {
                        (
                            NODE_COLUMN,
                            parity_db::Operation::Set(
                                n.key.0.to_vec(),
                                serialize_node(&n.clone().into_obj()),
                            ),
                        )
                    }))
                    .unwrap();
            }

            let mut db = Db::open(dir.path());
            assert!(
                !has_marker(&db),
                "a database that already holds nodes must not get the marker"
            );
            assert!(!db.children_complete());
            for n in &nodes {
                assert!(sidecar(&db, &n.key.0).is_none());
            }

            let expected: Vec<_> = {
                let mut ks: Vec<_> = nodes.iter().map(|n| n.key.clone()).collect();
                ks.sort();
                ks
            };
            for batch in [1usize, 7, 1000] {
                let mut got = all_scan_keys(&db, batch);
                got.sort();
                assert_eq!(got, expected, "batch {batch}");
            }
            let keys: Vec<_> = nodes.iter().map(|n| n.key.clone()).collect();
            assert_children_match_nodes(&db, &keys);
            for (n, (_, ch)) in nodes
                .iter()
                .zip(db.batch_get_children(keys.iter().cloned()))
            {
                assert_eq!(ch.as_ref(), Some(&n.children));
            }

            // New writes maintain the children column; scans still fall back (no marker).
            let extra = RawNode::new(&[250], 0, vec![&nodes[5]]);
            db.batch_update(std::iter::once((
                extra.key.clone(),
                Update::InsertNode(extra.clone().into_obj()),
            )));
            assert!(sidecar(&db, &extra.key.0).is_some());
            assert!(!has_marker(&db));
            assert_eq!(all_scan_keys(&db, 7).len(), nodes.len() + 1);

            // Deletes clear both columns, for old and new nodes alike.
            let old = nodes[10].key.clone();
            db.batch_update(
                [
                    (old.clone(), Update::DeleteNode),
                    (extra.key.clone(), Update::DeleteNode),
                ]
                .into_iter(),
            );
            for k in [&old, &extra.key] {
                assert!(db.db.get(NODE_COLUMN, &k.0).unwrap().is_none());
                assert!(sidecar(&db, &k.0).is_none());
            }
            let got: HashSet<_> = all_scan_keys(&db, 7).into_iter().collect();
            assert_eq!(got.len(), nodes.len() - 1);
            assert!(!got.contains(&old) && !got.contains(&extra.key));
            assert_eq!(
                db.batch_get_children([old].into_iter()),
                vec![(nodes[10].key.clone(), None)]
            );

            // Reopening still does not add the marker.
            drop(db);
            assert!(!has_marker(&Db::open(dir.path())));
        }

        /// `gc.rs::test_gc`, over parity-db: the garbage collector culls the same counts, and
        /// culled nodes leave no children entry behind.
        #[test]
        fn gc_end_to_end_paritydb() {
            use std::time::Duration;
            let dir = tempfile::TempDir::new().unwrap();
            let arena = crate::arena::Arena::new_from_backend(crate::backend::StorageBackend::new(
                1,
                Db::open(dir.path()),
            ));
            let size = || arena.with_backend(|b| b.database.size());
            let all_keys = || {
                arena.with_backend(|b| {
                    let (mut keys, mut h) = (HashSet::new(), None);
                    loop {
                        let (objs, next) = b.database.scan(h, 1000);
                        keys.extend(objs.into_iter().map(|(k, _)| k));
                        h = next;
                        if h.is_none() {
                            return keys;
                        }
                    }
                })
            };
            let assert_sidecar_consistent = |before: &HashSet<ArenaHash<DefaultHasher>>| {
                let now = all_keys();
                arena.with_backend(|b| {
                    assert!(b.database.children_complete());
                    for k in before.difference(&now) {
                        assert!(
                            b.database.db.get(CHILDREN_COLUMN, &k.0).unwrap().is_none(),
                            "culled node kept its children entry"
                        );
                    }
                    let from_sidecar: HashSet<_> =
                        all_scan_keys(&b.database, 1000).into_iter().collect();
                    assert_eq!(from_sidecar, now);
                });
            };

            const CHUNK: usize = 1_000;
            let mut refs = (0..10 * CHUNK)
                .map(|i| arena.alloc(i as u64))
                .collect::<Vec<_>>();
            refs.iter_mut().for_each(|r| r.persist());
            arena.with_backend(|b| b.flush_all_changes_to_db());
            assert_eq!(size(), 10 * CHUNK);
            assert_eq!(arena.with_backend(|b| b.gc(Duration::from_hours(1))), 0);
            assert_eq!(size(), 10 * CHUNK);
            refs[9 * CHUNK..10 * CHUNK]
                .iter_mut()
                .for_each(|r| r.unpersist());
            arena.with_backend(|b| b.flush_all_changes_to_db());
            assert_eq!(arena.with_backend(|b| b.gc(Duration::from_hours(1))), 0);
            assert_eq!(size(), 10 * CHUNK);

            let before = all_keys();
            refs.truncate(9 * CHUNK);
            assert_eq!(arena.with_backend(|b| b.gc(Duration::from_hours(1))), CHUNK);
            assert_eq!(size(), 9 * CHUNK);
            assert_sidecar_consistent(&before);

            refs.truncate(8 * CHUNK);
            assert_eq!(arena.with_backend(|b| b.gc(Duration::from_hours(1))), 0);
            assert_eq!(size(), 9 * CHUNK);

            refs[5 * CHUNK..8 * CHUNK]
                .iter_mut()
                .for_each(|r| r.unpersist());
            arena.with_backend(|b| b.flush_all_changes_to_db());
            refs.truncate(5 * CHUNK);
            assert_eq!(arena.with_backend(|b| b.gc_budgeted(|| Duration::ZERO)), 0);
            assert_eq!(size(), 9 * CHUNK);
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
            let before = all_keys();
            let mut culled = 0;
            for _ in 0..100 {
                batch_counter.set(0);
                culled += arena.with_backend(|b| b.gc_budgeted(budget));
            }
            assert_eq!(culled, 3 * CHUNK);
            assert_eq!(size(), 6 * CHUNK);
            assert_sidecar_consistent(&before);
        }
    }
}
