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

#[allow(deprecated)]
use sha2::digest::generic_array::GenericArray;

use nomt::{hasher::Blake3Hasher, trie::KeyPath, KeyReadWrite, Nomt, Options, SessionParams};

use crate::{DefaultHasher, WellBehavedHasher, arena::ArenaHash, backend::OnDiskObject};

#[cfg(feature = "proptest")]
use super::DummyDBStrategy;
use super::{DB, DummyArbitrary, Update};

/// Key prefix bytes for namespace separation in NOMT's flat `[u8; 32]` key space.
const PREFIX_NODE: u8 = 0x00;
const PREFIX_GC_ROOT: u8 = 0x01;
const PREFIX_META_NODE_COUNT: u8 = 0xFF;

/// The fixed metadata key for persisting node count.
const META_NODE_COUNT_KEY: KeyPath = {
    let mut k = [0u8; 32];
    k[0] = PREFIX_META_NODE_COUNT;
    k[1] = 0x01;
    k
};

/// The fixed metadata key for persisting the roots index.
const META_ROOTS_INDEX_KEY: KeyPath = {
    let mut k = [0u8; 32];
    k[0] = PREFIX_META_NODE_COUNT;
    k[1] = 0x02;
    k
};

/// A database back-end using NOMT (Nearly Optimal Merkle Trie).
///
/// NOMT uses fixed `[u8; 32]` keys. We map `ArenaHash` keys into this space
/// using a prefix byte for namespace separation (nodes, GC roots, metadata),
/// followed by the first 31 bytes of the `ArenaHash`.
pub struct NomtDb<H: WellBehavedHasher = DefaultHasher> {
    inner: Arc<RwLock<NomtDbInner<H>>>,
}

struct NomtDbInner<H: WellBehavedHasher> {
    nomt: Nomt<Blake3Hasher>,
    /// Write buffer: None value means delete.
    pending_nodes: HashMap<KeyPath, Option<Vec<u8>>>,
    /// Track pending GC root count changes.
    pending_roots: HashMap<KeyPath, Option<Vec<u8>>>,
    /// In-memory index of all roots with their counts (mirrors persisted state).
    roots_index: HashMap<ArenaHash<H>, u32>,
    /// In-memory node count tracker.
    node_count: usize,
    _phantom: PhantomData<H>,
}

/// Serialize the roots index as: [hash_size (u32 LE)] then repeated [hash_bytes | count (u32 LE)].
fn serialize_roots_index<H: WellBehavedHasher>(roots: &HashMap<ArenaHash<H>, u32>) -> Vec<u8> {
    let hash_size = <H as OutputSizeUser>::output_size();
    let entry_size = hash_size + 4;
    let mut buf = Vec::with_capacity(4 + roots.len() * entry_size);
    buf.extend_from_slice(&(hash_size as u32).to_le_bytes());
    for (key, count) in roots {
        #[allow(deprecated)]
        buf.extend_from_slice(key.0.as_slice());
        buf.extend_from_slice(&count.to_le_bytes());
    }
    buf
}

/// Deserialize the roots index.
fn deserialize_roots_index<H: WellBehavedHasher>(data: &[u8]) -> HashMap<ArenaHash<H>, u32> {
    if data.len() < 4 {
        return HashMap::new();
    }
    let hash_size = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
    let entry_size = hash_size + 4;
    let entries_data = &data[4..];
    let mut map = HashMap::new();
    let mut offset = 0;
    while offset + entry_size <= entries_data.len() {
        let hash_bytes = &entries_data[offset..offset + hash_size];
        let count_bytes = &entries_data[offset + hash_size..offset + entry_size];
        let count = u32::from_le_bytes(count_bytes.try_into().unwrap());
        #[allow(deprecated)]
        let key = ArenaHash(GenericArray::from_iter(hash_bytes.iter().copied()));
        map.insert(key, count);
        offset += entry_size;
    }
    map
}

fn serialize_node<H: WellBehavedHasher>(node: &OnDiskObject<H>) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(<OnDiskObject<H> as Serializable>::serialized_size(node));
    <OnDiskObject<H> as Serializable>::serialize(node, &mut bytes)
        .expect("Failed to serialize OnDiskObject");
    bytes
}

/// Construct a 32-byte NOMT key from a prefix byte and an `ArenaHash`.
/// Uses the first 31 bytes of the hash to fill bytes 1..32.
fn make_key<H: WellBehavedHasher>(prefix: u8, hash: &ArenaHash<H>) -> KeyPath {
    let mut key = [0u8; 32];
    key[0] = prefix;
    #[allow(deprecated)]
    let hash_bytes = hash.0.as_slice();
    let copy_len = hash_bytes.len().min(31);
    key[1..1 + copy_len].copy_from_slice(&hash_bytes[..copy_len]);
    key
}

#[allow(dead_code)]
fn bytes_to_arena_key<H: WellBehavedHasher>(nomt_key: &KeyPath) -> ArenaHash<H> {
    // Reconstruct ArenaHash from bytes 1..32 of the NOMT key, padding with zeros if needed.
    let hash_size = <H as OutputSizeUser>::output_size();
    let mut hash_bytes = vec![0u8; hash_size];
    let copy_len = hash_size.min(31);
    hash_bytes[..copy_len].copy_from_slice(&nomt_key[1..1 + copy_len]);
    #[allow(deprecated)]
    ArenaHash(GenericArray::from_iter(hash_bytes))
}

impl<H: WellBehavedHasher> NomtDb<H> {
    /// Open a NOMT database at the given directory path.
    pub fn open(path: &Path) -> Self {
        let mut opts = Options::new();
        opts.path(path);
        let nomt = Nomt::<Blake3Hasher>::open(opts).expect("Failed to open NOMT database");

        // Load persisted node count from metadata key.
        let node_count = nomt
            .read(META_NODE_COUNT_KEY)
            .expect("Failed to read NOMT metadata")
            .map(|bytes| {
                if bytes.len() == 8 {
                    usize::from_le_bytes(bytes.try_into().unwrap())
                } else {
                    0
                }
            })
            .unwrap_or(0);

        // Load persisted roots index.
        let roots_index = nomt
            .read(META_ROOTS_INDEX_KEY)
            .expect("Failed to read NOMT roots index")
            .map(|bytes| deserialize_roots_index::<H>(&bytes))
            .unwrap_or_default();

        NomtDb {
            inner: Arc::new(RwLock::new(NomtDbInner {
                nomt,
                pending_nodes: HashMap::new(),
                pending_roots: HashMap::new(),
                roots_index,
                node_count,
                _phantom: PhantomData,
            })),
        }
    }

    /// Flush all pending writes to NOMT in a single session.
    fn flush(inner: &mut NomtDbInner<H>) {
        if inner.pending_nodes.is_empty() && inner.pending_roots.is_empty() {
            return;
        }

        // Collect all actuals (key + read-then-write operations).
        let mut actuals: Vec<(KeyPath, KeyReadWrite)> = Vec::new();

        for (key, value) in inner.pending_nodes.drain() {
            // Read the current value first.
            let prev = inner
                .nomt
                .read(key)
                .expect("Failed to read from NOMT during flush");
            actuals.push((key, KeyReadWrite::ReadThenWrite(prev, value)));
        }

        for (key, value) in inner.pending_roots.drain() {
            let prev = inner
                .nomt
                .read(key)
                .expect("Failed to read from NOMT during flush");
            actuals.push((key, KeyReadWrite::ReadThenWrite(prev, value)));
        }

        // Persist metadata: node count and roots index.
        {
            let prev = inner
                .nomt
                .read(META_NODE_COUNT_KEY)
                .expect("Failed to read NOMT metadata during flush");
            let new_val = inner.node_count.to_le_bytes().to_vec();
            actuals.push((
                META_NODE_COUNT_KEY,
                KeyReadWrite::ReadThenWrite(prev, Some(new_val)),
            ));
        }
        {
            let prev = inner
                .nomt
                .read(META_ROOTS_INDEX_KEY)
                .expect("Failed to read NOMT roots index during flush");
            let new_val = serialize_roots_index(&inner.roots_index);
            actuals.push((
                META_ROOTS_INDEX_KEY,
                KeyReadWrite::ReadThenWrite(prev, Some(new_val)),
            ));
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
        let nomt_key = make_key(PREFIX_NODE, key);

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

    fn insert_node(
        &mut self,
        key: ArenaHash<Self::Hasher>,
        object: OnDiskObject<Self::Hasher>,
    ) {
        let mut inner = self.inner.write().expect("lock poisoned");
        let nomt_key = make_key(PREFIX_NODE, &key);

        // Check if this is a new insert (not overwrite) for node count tracking.
        let is_new = if let Some(pending) = inner.pending_nodes.get(&nomt_key) {
            pending.is_none() // was deleted, so re-inserting counts as new
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
        let nomt_key = make_key(PREFIX_NODE, key);

        // Check if node actually exists for count tracking.
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
                    let nomt_key = make_key(PREFIX_NODE, &key);

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
                    let nomt_key = make_key(PREFIX_NODE, &key);

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
                    let nomt_key = make_key(PREFIX_GC_ROOT, &key);
                    if count == 0 {
                        inner.pending_roots.insert(nomt_key, None);
                        inner.roots_index.remove(&key);
                    } else {
                        inner
                            .pending_roots
                            .insert(nomt_key, Some(count.to_le_bytes().to_vec()));
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
        let nomt_key = make_key(PREFIX_GC_ROOT, &key);

        if count == 0 {
            inner.pending_roots.insert(nomt_key, None);
            inner.roots_index.remove(&key);
        } else {
            inner
                .pending_roots
                .insert(nomt_key, Some(count.to_le_bytes().to_vec()));
            inner.roots_index.insert(key, count);
        }

        Self::flush(&mut inner);
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
