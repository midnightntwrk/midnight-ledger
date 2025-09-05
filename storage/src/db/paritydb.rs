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
use sha2::digest::generic_array::GenericArray;

use crate::{DefaultHasher, WellBehavedHasher, arena::ArenaKey, backend::OnDiskObject};

#[cfg(feature = "proptest")]
use super::DummyDBStrategy;
use super::{DB, DummyArbitrary, Update};

// Different value to Substrate: polkadot-sdk/substrate/client/db/src/utils.rs
// This means the `storage` database must be stored in a different file
const NUM_COLUMNS: u8 = 3;
const NODE_COLUMN: u8 = 0;
const GC_ROOT_COLUMN: u8 = 1;
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

fn bytes_to_arena_key<H: WellBehavedHasher>(key_bytes: Vec<u8>) -> ArenaKey<H> {
    if key_bytes.len() != <H as OutputSizeUser>::output_size() {
        panic!(
            "incorrect length for gc_root key: found {}, expected {}",
            key_bytes.len(),
            <H as OutputSizeUser>::output_size()
        );
    }
    ArenaKey(GenericArray::from_iter(key_bytes))
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
        options.columns[REF_COUNT_ZERO as usize].btree_index = true;
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
    fn get_node(&self, key: &ArenaKey<Self::Hasher>) -> Option<OnDiskObject<Self::Hasher>> {
        self.db
            .get(NODE_COLUMN, &key.0)
            .expect("failed to get from db")
            .map(|bytes| {
                OnDiskObject::<Self::Hasher>::deserialize(&mut &bytes[..], 0)
                    .expect("Failed to deserialize OnDiskObject")
            })
    }

    fn get_unreachable_keys(&self) -> Vec<ArenaKey<Self::Hasher>> {
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
        key: crate::arena::ArenaKey<Self::Hasher>,
        object: OnDiskObject<Self::Hasher>,
    ) {
        let mut ops = vec![(
            NODE_COLUMN,
            parity_db::Operation::Set(key.0.to_vec(), serialize_node(&object)),
        )];
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

    fn delete_node(&mut self, key: &crate::arena::ArenaKey<Self::Hasher>) {
        let ops = vec![
            (
                NODE_COLUMN,
                parity_db::Operation::Dereference(key.0.to_vec()),
            ),
            (
                REF_COUNT_ZERO,
                parity_db::Operation::Dereference(key.0.to_vec()),
            ),
        ];
        self.db.commit_changes(ops).expect("Failed to commit to db");
    }

    fn batch_update<I>(&mut self, iter: I)
    where
        I: Iterator<Item = (ArenaKey<Self::Hasher>, Update<Self::Hasher>)>,
    {
        let mut ops = Vec::new();
        for (key, update) in iter {
            match update {
                Update::InsertNode(object) => {
                    ops.push((
                        NODE_COLUMN,
                        parity_db::Operation::Set(key.0.to_vec(), serialize_node(&object)),
                    ));
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
    ) -> Vec<(ArenaKey<Self::Hasher>, Option<OnDiskObject<Self::Hasher>>)>
    where
        I: Iterator<Item = ArenaKey<Self::Hasher>>,
    {
        crate::db::dubious_batch_get_nodes(self, keys)
    }

    fn get_root_count(&self, key: &crate::arena::ArenaKey<Self::Hasher>) -> u32 {
        self.db
            .get(GC_ROOT_COLUMN, &key.0)
            .expect("failed to get from db")
            .map(|bytes| {
                u32::from_le_bytes(bytes.try_into().expect("gc root count should be 4 bytes"))
            })
            .unwrap_or(0)
    }

    fn set_root_count(&mut self, key: ArenaKey<Self::Hasher>, count: u32) {
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

    fn get_roots(&self) -> HashMap<ArenaKey<Self::Hasher>, u32> {
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
