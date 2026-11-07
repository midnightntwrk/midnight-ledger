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

use crypto::digest::OutputSizeUser;
use std::{collections::HashMap, fmt::Debug, marker::PhantomData, ops::Deref, sync::Arc};

#[cfg(feature = "proptest")]
use proptest::prelude::*;
use serialize::{Deserializable, Serializable};

use parity_db;
#[allow(deprecated)]
use sha2::digest::generic_array::GenericArray;

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
        Self {
            db: Default::default(),
            _phantom: Default::default(),
        }
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
        ParityDb {
            db: OwnedDb::new(path),
            _phantom: PhantomData,
        }
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
        Self {
            db,
            _phantom: Default::default(),
        }
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
        DummyDBStrategy::<Self>(PhantomData)
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
        let key = res.last().and_then(|(k, _)| {
            let mut key = k.0.to_vec();
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
        });
        (res, key)
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
}
