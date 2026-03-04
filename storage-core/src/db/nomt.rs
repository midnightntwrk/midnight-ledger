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

use std::{
    collections::HashMap,
    fmt::Debug,
    marker::PhantomData,
    path::Path,
    sync::{Arc, RwLock},
};

#[cfg(feature = "proptest")]
use proptest::prelude::*;
use serialize::{Deserializable, Serializable};

use nomt::{hasher::Blake3Hasher, trie::KeyPath, KeyReadWrite, Nomt, Options, SessionParams};

use crate::{DefaultHasher, WellBehavedHasher, arena::ArenaHash, backend::OnDiskObject};

#[cfg(feature = "proptest")]
use super::DummyDBStrategy;
use super::{DB, DummyArbitrary, Update};

/// A database back-end using NOMT (Nearly Optimal Merkle Trie).
///
/// Only node data is stored in NOMT. ArenaHash keys are used directly as
/// NOMT KeyPaths (padded/truncated to 32 bytes). GC root counts, node count,
/// and other metadata are kept purely in memory.
pub struct NomtDb<H: WellBehavedHasher = DefaultHasher> {
    inner: Arc<RwLock<NomtDbInner<H>>>,
}

struct NomtDbInner<H: WellBehavedHasher> {
    nomt: Nomt<Blake3Hasher>,
    /// Write buffer: None value means delete.
    pending_nodes: HashMap<KeyPath, Option<Vec<u8>>>,
    /// In-memory index of all roots with their counts.
    roots_index: HashMap<ArenaHash<H>, u32>,
    /// In-memory node count tracker.
    node_count: usize,
    _phantom: PhantomData<H>,
}

fn serialize_node<H: WellBehavedHasher>(node: &OnDiskObject<H>) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(<OnDiskObject<H> as Serializable>::serialized_size(node));
    <OnDiskObject<H> as Serializable>::serialize(node, &mut bytes)
        .expect("Failed to serialize OnDiskObject");
    bytes
}

/// Construct a 32-byte NOMT KeyPath directly from an `ArenaHash`.
/// For Sha256 (32 bytes) this is a direct copy. For other hash sizes
/// the result is padded with zeros or truncated.
fn make_key<H: WellBehavedHasher>(hash: &ArenaHash<H>) -> KeyPath {
    let mut key = [0u8; 32];
    #[allow(deprecated)]
    let hash_bytes = hash.0.as_slice();
    let copy_len = hash_bytes.len().min(32);
    key[..copy_len].copy_from_slice(&hash_bytes[..copy_len]);
    key
}

impl<H: WellBehavedHasher> NomtDb<H> {
    /// Open a NOMT database at the given directory path.
    pub fn open(path: &Path) -> Self {
        let mut opts = Options::new();
        opts.path(path);
        let nomt = Nomt::<Blake3Hasher>::open(opts).expect("Failed to open NOMT database");

        NomtDb {
            inner: Arc::new(RwLock::new(NomtDbInner {
                nomt,
                pending_nodes: HashMap::new(),
                roots_index: HashMap::new(),
                node_count: 0,
                _phantom: PhantomData,
            })),
        }
    }

    /// Flush all pending writes to NOMT in a single session.
    fn flush(inner: &mut NomtDbInner<H>) {
        if inner.pending_nodes.is_empty() {
            return;
        }

        let mut actuals: Vec<(KeyPath, KeyReadWrite)> = Vec::new();

        for (key, value) in inner.pending_nodes.drain() {
            let prev = inner
                .nomt
                .read(key)
                .expect("Failed to read from NOMT during flush");
            actuals.push((key, KeyReadWrite::ReadThenWrite(prev, value)));
        }

        // Sort actuals by key — NOMT requires sorted key order.
        actuals.sort_by(|a, b| a.0.cmp(&b.0));

        let session = inner.nomt.begin_session(SessionParams::default());
        let finished = session
            .finish(actuals)
            .expect("Failed to finish NOMT session");
        finished
            .commit(&inner.nomt)
            .expect("Failed to commit NOMT session");
    }
}

impl<H: WellBehavedHasher> Default for NomtDb<H> {
    fn default() -> Self {
        let dir = tempfile::TempDir::new().unwrap().keep();
        Self::open(&dir)
    }
}

impl<H: WellBehavedHasher> Debug for NomtDb<H> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NomtDb")
            .field("db", &"nomt-instance")
            .finish()
    }
}

impl<H: WellBehavedHasher> DummyArbitrary for NomtDb<H> {}

#[cfg(feature = "proptest")]
impl<H: WellBehavedHasher> Arbitrary for NomtDb<H> {
    type Parameters = ();
    type Strategy = DummyDBStrategy<Self>;

    fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
        DummyDBStrategy::<Self>(PhantomData)
    }
}

impl<H: WellBehavedHasher> DB for NomtDb<H> {
    type Hasher = H;

    fn get_node(&self, key: &ArenaHash<Self::Hasher>) -> Option<OnDiskObject<Self::Hasher>> {
        let inner = self.inner.read().expect("lock poisoned");
        let nomt_key = make_key(key);

        // Check pending writes first.
        if let Some(pending) = inner.pending_nodes.get(&nomt_key) {
            return pending.as_ref().map(|bytes| {
                OnDiskObject::<Self::Hasher>::deserialize(&mut &bytes[..], 0)
                    .expect("Failed to deserialize OnDiskObject")
            });
        }

        // Fall through to NOMT.
        inner
            .nomt
            .read(nomt_key)
            .expect("Failed to read from NOMT")
            .map(|bytes| {
                OnDiskObject::<Self::Hasher>::deserialize(&mut &bytes[..], 0)
                    .expect("Failed to deserialize OnDiskObject")
            })
    }

    fn get_unreachable_keys(&self) -> Vec<ArenaHash<Self::Hasher>> {
        Vec::new()
    }

    fn insert_node(
        &mut self,
        key: ArenaHash<Self::Hasher>,
        object: OnDiskObject<Self::Hasher>,
    ) {
        let mut inner = self.inner.write().expect("lock poisoned");
        let nomt_key = make_key(&key);

        let is_new = if let Some(pending) = inner.pending_nodes.get(&nomt_key) {
            pending.is_none()
        } else {
            inner
                .nomt
                .read(nomt_key)
                .expect("Failed to read from NOMT")
                .is_none()
        };

        inner
            .pending_nodes
            .insert(nomt_key, Some(serialize_node(&object)));

        if is_new {
            inner.node_count += 1;
        }

        Self::flush(&mut inner);
    }

    fn delete_node(&mut self, key: &ArenaHash<Self::Hasher>) {
        let mut inner = self.inner.write().expect("lock poisoned");
        let nomt_key = make_key(key);

        let exists = if let Some(pending) = inner.pending_nodes.get(&nomt_key) {
            pending.is_some()
        } else {
            inner
                .nomt
                .read(nomt_key)
                .expect("Failed to read from NOMT")
                .is_some()
        };

        inner.pending_nodes.insert(nomt_key, None);

        if exists {
            inner.node_count = inner.node_count.saturating_sub(1);
        }

        Self::flush(&mut inner);
    }

    fn batch_update<I>(&mut self, iter: I)
    where
        I: Iterator<Item = (ArenaHash<Self::Hasher>, Update<Self::Hasher>)>,
    {
        let mut inner = self.inner.write().expect("lock poisoned");

        for (key, update) in iter {
            match update {
                Update::InsertNode(object) => {
                    let nomt_key = make_key(&key);

                    let is_new = if let Some(pending) = inner.pending_nodes.get(&nomt_key) {
                        pending.is_none()
                    } else {
                        inner
                            .nomt
                            .read(nomt_key)
                            .expect("Failed to read from NOMT")
                            .is_none()
                    };

                    inner
                        .pending_nodes
                        .insert(nomt_key, Some(serialize_node(&object)));

                    if is_new {
                        inner.node_count += 1;
                    }
                }
                Update::DeleteNode => {
                    let nomt_key = make_key(&key);

                    let exists = if let Some(pending) = inner.pending_nodes.get(&nomt_key) {
                        pending.is_some()
                    } else {
                        inner
                            .nomt
                            .read(nomt_key)
                            .expect("Failed to read from NOMT")
                            .is_some()
                    };

                    inner.pending_nodes.insert(nomt_key, None);

                    if exists {
                        inner.node_count = inner.node_count.saturating_sub(1);
                    }
                }
                Update::SetRootCount(count) => {
                    if count == 0 {
                        inner.roots_index.remove(&key);
                    } else {
                        inner.roots_index.insert(key.clone(), count);
                    }
                }
            }
        }

        Self::flush(&mut inner);
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

    fn get_root_count(&self, key: &ArenaHash<Self::Hasher>) -> u32 {
        let inner = self.inner.read().expect("lock poisoned");
        inner.roots_index.get(key).copied().unwrap_or(0)
    }

    fn set_root_count(&mut self, key: ArenaHash<Self::Hasher>, count: u32) {
        let mut inner = self.inner.write().expect("lock poisoned");

        if count == 0 {
            inner.roots_index.remove(&key);
        } else {
            inner.roots_index.insert(key, count);
        }
    }

    fn get_roots(&self) -> HashMap<ArenaHash<Self::Hasher>, u32> {
        let inner = self.inner.read().expect("lock poisoned");
        inner.roots_index.clone()
    }

    fn size(&self) -> usize {
        let inner = self.inner.read().expect("lock poisoned");
        inner.node_count
    }
}

#[cfg(test)]
mod tests {
    use super::NomtDb;

    #[test]
    fn basic_open_and_default() {
        let _db: NomtDb<sha2::Sha256> = NomtDb::default();
    }

    #[test]
    fn allow_serial_access() {
        let path = tempfile::TempDir::new().unwrap().keep();
        let db: NomtDb<sha2::Sha256> = NomtDb::open(&path);
        drop(db);
        let _db2: NomtDb<sha2::Sha256> = NomtDb::open(&path);
    }
}
