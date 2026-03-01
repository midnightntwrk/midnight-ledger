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
use std::{collections::HashMap, fmt::Debug, marker::PhantomData, sync::atomic::AtomicBool};

#[cfg(feature = "proptest")]
use proptest::prelude::*;
use serialize::{Deserializable, Serializable};

use parity_db::{self, CompressionType, Operation};
#[allow(deprecated)]
use sha2::digest::generic_array::GenericArray;

use crate::{DefaultHasher, WellBehavedHasher, arena::ArenaHash, backend::OnDiskObject};

#[cfg(feature = "proptest")]
use super::DummyDBStrategy;
use super::{DB, DummyArbitrary, Update};

const NUM_COLUMNS: u8 = 5;
const NODE_COLUMN: u8 = 0;
const GC_ROOT_COLUMN: u8 = 1;
#[cfg(not(feature = "layout-v2"))]
// Column to track which nodes have a ref count of zero
const REF_COUNT_ZERO: u8 = 2;
// Incremental GC columns
const GC_GREY: u8 = 2;
const GC_MARK: u8 = 3;
const GC_META: u8 = 4;

const GC_META_KEY: &[u8] = b"gc_meta";

/// Phase of the incremental garbage collector.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GcPhase {
    /// No GC in progress.
    Clean = 0,
    /// Mark phase: processing grey (frontier) nodes.
    Mark = 1,
    /// Sweep phase: deleting unreachable (white) nodes.
    Sweep = 2,
}

impl GcPhase {
    fn from_byte(b: u8) -> Self {
        match b {
            0 => GcPhase::Clean,
            1 => GcPhase::Mark,
            2 => GcPhase::Sweep,
            _ => panic!("invalid GC phase byte: {b}"),
        }
    }
}

/// Persisted GC metadata: phase + generation + optional sweep cursor.
///
/// The generation alternates between 0 and 1 across GC cycles. A node is
/// "black" (reached) if its GC_MARK entry matches the current generation, and
/// "white" (unreached) otherwise. Flipping the generation at the start of each
/// cycle avoids a costly O(live_nodes) reset of GC_MARK.
///
/// The sweep cursor tracks the last NODE_COLUMN key examined during an
/// incremental sweep step, so subsequent steps skip past already-checked keys.
#[derive(Debug, Clone)]
struct GcMeta {
    phase: GcPhase,
    generation: u8,
    /// Last key examined during sweep. `None` means start from the beginning.
    sweep_cursor: Option<Vec<u8>>,
}

impl GcMeta {
    fn to_bytes(&self) -> Vec<u8> {
        let mut buf = vec![self.phase as u8, self.generation];
        if let Some(ref cursor) = self.sweep_cursor {
            let len = cursor.len() as u16;
            buf.extend_from_slice(&len.to_le_bytes());
            buf.extend_from_slice(cursor);
        }
        buf
    }

    fn from_bytes(bytes: &[u8]) -> Self {
        assert!(bytes.len() >= 2, "GcMeta needs at least 2 bytes");
        let sweep_cursor = if bytes.len() > 2 {
            let len = u16::from_le_bytes([bytes[2], bytes[3]]) as usize;
            Some(bytes[4..4 + len].to_vec())
        } else {
            None
        };
        GcMeta {
            phase: GcPhase::from_byte(bytes[0]),
            generation: bytes[1],
            sweep_cursor,
        }
    }
}

/// A database back-end using the `ParityDB` library.
pub struct ParityDb<H: WellBehavedHasher = DefaultHasher> {
    db: parity_db::Db,
    /// Cached flag: is a GC cycle currently in progress? The authoritative
    /// state lives in GC_META on disk; this avoids an extra point lookup on
    /// every `batch_update` / `set_root_count`.
    gc_active: AtomicBool,
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
            .field("db", &"no-debug")
            .field("gc_active", &self.gc_active)
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
        for col in options.columns.iter_mut() {
            col.uniform = true;
        }
        options.columns[GC_ROOT_COLUMN as usize].btree_index = true;
        options.columns[NODE_COLUMN as usize].btree_index = true;
        options.columns[NODE_COLUMN as usize].preimage = true;
        options.columns[NODE_COLUMN as usize].compression = CompressionType::Lz4;
        // GC_GREY needs btree_index for iteration (finding frontier work)
        options.columns[GC_GREY as usize].btree_index = true;
        // GC_MARK only needs point lookups, no btree_index
        // GC_META stores a single small entry; not uniform-key-length
        options.columns[GC_META as usize].uniform = false;
        let db = parity_db::Db::open_or_create(&options).unwrap_or_else(|e| {
            panic!(
                "parity-db open error: {e}. Note: Check db isn't already open. Path: {}",
                path.display()
            )
        });

        // Recover GC state: if a previous cycle was interrupted, gc_active
        // will be true so the next gc_step() call resumes where it left off.
        let gc_active = db
            .get(GC_META, GC_META_KEY)
            .expect("failed to read gc_meta")
            .map(|bytes| GcMeta::from_bytes(&bytes).phase != GcPhase::Clean)
            .unwrap_or(false);

        ParityDb {
            db,
            gc_active: AtomicBool::new(gc_active),
            _phantom: PhantomData,
        }
    }

    /// Run a full GC cycle. Blocks until complete.
    pub fn gc(&self) {
        self.gc_start();
        while self.gc_step(usize::MAX) {}
    }

    /// Returns true if a GC cycle is currently in progress.
    pub fn gc_is_active(&self) -> bool {
        self.gc_active.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Start a new GC cycle. No-op if one is already in progress.
    pub fn gc_start(&self) {
        let meta = self.gc_read_meta();
        if meta.phase != GcPhase::Clean {
            return; // Already active — resume via gc_step().
        }

        // Flip the generation so all previous GC_MARK entries become "white".
        let new_gen = 1 - meta.generation;

        // Seed GC_GREY with all current GC roots.
        let mut roots_iter = self
            .db
            .iter(GC_ROOT_COLUMN)
            .expect("Failed to iterate gc roots");
        let mut ops = Vec::new();
        while let Some((k, _)) = roots_iter.next().expect("Failed to get next root") {
            ops.push((GC_GREY, Operation::Set(k, vec![])));
        }
        drop(roots_iter);

        if !ops.is_empty() {
            self.db
                .commit_changes(ops)
                .expect("Failed to seed grey set");
        }

        self.gc_write_meta(&GcMeta {
            phase: GcPhase::Mark,
            generation: new_gen,
            sweep_cursor: None,
        });
    }

    /// Perform up to `budget` units of GC work.
    ///
    /// Returns `true` if the GC cycle is still in progress and more work
    /// remains, or `false` if the cycle has completed (or was never started).
    pub fn gc_step(&self, budget: usize) -> bool {
        let meta = self.gc_read_meta();
        match meta.phase {
            GcPhase::Clean => false,
            GcPhase::Mark => {
                let (_processed, grey_empty) = self.gc_mark_step(&meta, budget);
                if grey_empty {
                    self.gc_write_meta(&GcMeta {
                        phase: GcPhase::Sweep,
                        generation: meta.generation,
                        sweep_cursor: None,
                    });
                }
                true
            }
            GcPhase::Sweep => {
                let (_deleted, sweep_done, new_cursor) = self.gc_sweep_step(&meta, budget);
                if sweep_done {
                    self.gc_write_meta(&GcMeta {
                        phase: GcPhase::Clean,
                        generation: meta.generation,
                        sweep_cursor: None,
                    });
                    self.db.flush();
                    false
                } else {
                    self.gc_write_meta(&GcMeta {
                        phase: GcPhase::Sweep,
                        generation: meta.generation,
                        sweep_cursor: new_cursor,
                    });
                    true
                }
            }
        }
    }

    // -- internal helpers --------------------------------------------------

    fn gc_read_meta(&self) -> GcMeta {
        self.db
            .get(GC_META, GC_META_KEY)
            .expect("failed to read gc_meta")
            .map(|bytes| GcMeta::from_bytes(&bytes))
            .unwrap_or(GcMeta {
                phase: GcPhase::Clean,
                generation: 0,
                sweep_cursor: None,
            })
    }

    fn gc_write_meta(&self, meta: &GcMeta) {
        self.db
            .commit_changes(vec![(
                GC_META,
                Operation::Set(GC_META_KEY.to_vec(), meta.to_bytes()),
            )])
            .expect("failed to write gc_meta");
        self.gc_active
            .store(meta.phase != GcPhase::Clean, std::sync::atomic::Ordering::Relaxed);
    }

    /// Process up to `budget` grey nodes. Returns `(processed, grey_empty)`.
    fn gc_mark_step(&self, meta: &GcMeta, budget: usize) -> (usize, bool) {
        // Collect up to `budget` grey keys.
        let mut grey_iter = self.db.iter(GC_GREY).expect("Failed to iterate GC_GREY");
        let mut grey_batch: Vec<Vec<u8>> = Vec::with_capacity(budget.min(1024));
        while grey_batch.len() < budget {
            match grey_iter
                .next()
                .expect("Failed to get next from GC_GREY iterator")
            {
                Some((k, _)) => grey_batch.push(k),
                None => break,
            }
        }
        drop(grey_iter);

        if grey_batch.is_empty() {
            return (0, true);
        }

        let current_gen = meta.generation;
        let mut ops = Vec::new();

        for grey_key in &grey_batch {
            // Look up the node to find its children.
            if let Some(node) = self
                .db
                .get(NODE_COLUMN, grey_key)
                .expect("failed to get node")
                .map(|bytes| {
                    OnDiskObject::<H>::deserialize(&mut &bytes[..], 0)
                        .expect("Failed to deserialize OnDiskObject")
                })
            {
                for child_ref in node.children.iter().flat_map(|k| k.refs()) {
                    let child_bytes = child_ref.0.to_vec();
                    let already_marked = self
                        .db
                        .get(GC_MARK, &child_bytes)
                        .expect("failed to check gc_mark")
                        .is_some_and(|v| v.first() == Some(&current_gen));
                    if !already_marked {
                        ops.push((GC_GREY, Operation::Set(child_bytes, vec![])));
                    }
                }
            }

            // Mark this node black (reached with current generation).
            ops.push((
                GC_MARK,
                Operation::Set(grey_key.clone(), vec![current_gen]),
            ));
            // Remove from the grey frontier.
            ops.push((GC_GREY, Operation::Dereference(grey_key.clone())));
        }

        let processed = grey_batch.len();
        self.db
            .commit_changes(ops)
            .expect("Failed to commit gc mark step");

        // The mark step itself may have added new grey entries (children of the
        // nodes we just processed). Re-check whether grey is truly empty.
        let mut check = self.db.iter(GC_GREY).expect("Failed to iterate GC_GREY");
        let grey_empty = check
            .next()
            .expect("Failed to check GC_GREY")
            .is_none();
        drop(check);

        (processed, grey_empty)
    }

    /// Delete up to `budget` unreachable nodes. Returns `(deleted, sweep_done, new_cursor)`.
    ///
    /// The sweep cursor (`meta.sweep_cursor`) tracks the last NODE_COLUMN key
    /// examined in a previous step. Keys at or before the cursor are skipped
    /// cheaply (byte comparison only, no GC_MARK lookup), so total sweep work
    /// across all incremental steps is O(n) iterations rather than O(n * steps).
    fn gc_sweep_step(
        &self,
        meta: &GcMeta,
        budget: usize,
    ) -> (usize, bool, Option<Vec<u8>>) {
        let current_gen = meta.generation;
        let cursor = meta.sweep_cursor.as_deref();
        let mut node_iter = self
            .db
            .iter(NODE_COLUMN)
            .expect("Failed to iterate NODE_COLUMN");
        let mut ops = Vec::new();
        let mut deleted = 0;
        let mut last_key: Option<Vec<u8>>;

        loop {
            match node_iter
                .next()
                .expect("Failed to get next from NODE_COLUMN iterator")
            {
                Some((k, _)) => {
                    // Skip keys we already examined in a previous step.
                    if let Some(c) = cursor {
                        if k.as_slice() <= c {
                            continue;
                        }
                    }
                    last_key = Some(k.clone());
                    let is_marked = self
                        .db
                        .get(GC_MARK, &k)
                        .expect("failed to check gc_mark")
                        .is_some_and(|v| v.first() == Some(&current_gen));
                    if !is_marked {
                        ops.push((NODE_COLUMN, Operation::Dereference(k.clone())));
                        // Also clean up any stale GC_MARK entry.
                        ops.push((GC_MARK, Operation::Dereference(k)));
                        deleted += 1;
                        if deleted >= budget {
                            break;
                        }
                    }
                }
                None => {
                    // Iteration exhausted — sweep is complete.
                    drop(node_iter);
                    if !ops.is_empty() {
                        self.db
                            .commit_changes(ops)
                            .expect("Failed to commit gc sweep step");
                    }
                    return (deleted, true, None);
                }
            }
        }

        drop(node_iter);
        if !ops.is_empty() {
            self.db
                .commit_changes(ops)
                .expect("Failed to commit gc sweep step");
        }
        (deleted, false, last_key)
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
        self.db.flush();
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
        self.db.flush();
    }

    fn batch_update<I>(&mut self, iter: I)
    where
        I: Iterator<Item = (ArenaHash<Self::Hasher>, Update<Self::Hasher>)>,
    {
        let gc_active = self.gc_is_active();
        let mut added_grey = false;
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
                    // Write barrier: new roots must enter the grey frontier so
                    // the mark phase discovers their children.
                    if count > 0 && gc_active {
                        ops.push((
                            GC_GREY,
                            parity_db::Operation::Set(key.0.to_vec(), vec![]),
                        ));
                        added_grey = true;
                    }
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
        self.db.flush();

        // If we injected grey entries while in the sweep phase, revert to mark
        // so the new roots get fully traced before sweeping continues.
        if added_grey {
            let meta = self.gc_read_meta();
            if meta.phase == GcPhase::Sweep {
                self.gc_write_meta(&GcMeta {
                    phase: GcPhase::Mark,
                    generation: meta.generation,
                    sweep_cursor: None,
                });
            }
        }
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
        let gc_active = self.gc_is_active();
        let mut ops = vec![(
            GC_ROOT_COLUMN,
            if count == 0 {
                parity_db::Operation::Dereference(key.0.to_vec())
            } else {
                parity_db::Operation::Set(key.0.to_vec(), count.to_le_bytes().to_vec())
            },
        )];
        if count > 0 && gc_active {
            ops.push((
                GC_GREY,
                parity_db::Operation::Set(key.0.to_vec(), vec![]),
            ));
        }
        self.db.commit_changes(ops).expect("Failed to commit to db");

        if count > 0 && gc_active {
            let meta = self.gc_read_meta();
            if meta.phase == GcPhase::Sweep {
                self.gc_write_meta(&GcMeta {
                    phase: GcPhase::Mark,
                    generation: meta.generation,
                    sweep_cursor: None,
                });
            }
        }
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

#[cfg(test)]
mod tests {
    use super::ParityDb;
    use crate::backend::raw_node::RawNode;
    use crate::db::{DB, Update};

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

    // The incremental GC tests require layout-v2 because GC_GREY (column 2)
    // shares an index with REF_COUNT_ZERO (column 2, layout-v1 only). In
    // layout-v1 the insert_node path writes ref-count data to column 2 which
    // would corrupt the grey frontier.
    #[cfg(feature = "layout-v2")]
    /// GC with no roots removes everything.
    #[test]
    fn incremental_gc_removes_all_unreachable() {
        let mut db = ParityDb::<sha2::Sha256>::default();
        let n1 = RawNode::new(&[1], 0, vec![]);
        let n2 = RawNode::new(&[2], 0, vec![&n1]);
        n1.insert_into_db(&mut db);
        n2.insert_into_db(&mut db);
        assert_eq!(db.size(), 2);

        db.gc();
        assert_eq!(db.size(), 0);
    }

    #[cfg(feature = "layout-v2")]
    /// GC preserves rooted subtrees and deletes orphans.
    #[test]
    fn incremental_gc_preserves_roots() {
        let mut db = ParityDb::<sha2::Sha256>::default();
        let leaf = RawNode::new(&[1, 1], 0, vec![]);
        let root = RawNode::new(&[1, 2], 0, vec![&leaf]);
        let orphan = RawNode::new(&[2, 1], 0, vec![]);
        leaf.insert_into_db(&mut db);
        root.insert_into_db(&mut db);
        orphan.insert_into_db(&mut db);
        db.set_root_count(root.key.clone(), 1);
        assert_eq!(db.size(), 3);

        db.gc();
        assert_eq!(db.size(), 2);
        assert!(db.get_node(&root.key).is_some());
        assert!(db.get_node(&leaf.key).is_some());
        assert!(db.get_node(&orphan.key).is_none());
    }

    #[cfg(feature = "layout-v2")]
    /// Stepping through GC one node at a time gives the same result.
    #[test]
    fn incremental_gc_stepping() {
        let mut db = ParityDb::<sha2::Sha256>::default();
        let leaf = RawNode::new(&[1, 1], 0, vec![]);
        let root = RawNode::new(&[1, 2], 0, vec![&leaf]);
        let orphan = RawNode::new(&[2, 1], 0, vec![]);
        leaf.insert_into_db(&mut db);
        root.insert_into_db(&mut db);
        orphan.insert_into_db(&mut db);
        db.set_root_count(root.key.clone(), 1);

        db.gc_start();
        assert!(db.gc_is_active());
        while db.gc_step(1) {}
        assert!(!db.gc_is_active());

        assert_eq!(db.size(), 2);
        assert!(db.get_node(&root.key).is_some());
        assert!(db.get_node(&leaf.key).is_some());
        assert!(db.get_node(&orphan.key).is_none());
    }

    #[cfg(feature = "layout-v2")]
    /// A new root added via batch_update during the mark phase is preserved.
    #[test]
    fn incremental_gc_new_root_during_mark() {
        let mut db = ParityDb::<sha2::Sha256>::default();

        let leaf = RawNode::new(&[1, 1], 0, vec![]);
        let root = RawNode::new(&[1, 2], 0, vec![&leaf]);
        leaf.insert_into_db(&mut db);
        root.insert_into_db(&mut db);
        db.set_root_count(root.key.clone(), 1);

        // Start GC, do some mark work.
        db.gc_start();
        db.gc_step(1);

        // Insert a new tree during GC.
        let new_leaf = RawNode::new(&[3, 1], 0, vec![]);
        let new_root = RawNode::new(&[3, 2], 0, vec![&new_leaf]);
        db.batch_update(
            vec![
                (
                    new_leaf.key.clone(),
                    Update::InsertNode(new_leaf.clone().into_obj()),
                ),
                (
                    new_root.key.clone(),
                    Update::InsertNode(new_root.clone().into_obj()),
                ),
                (new_root.key.clone(), Update::SetRootCount(1)),
            ]
            .into_iter(),
        );

        // Finish GC.
        while db.gc_step(1024) {}

        // Both trees should survive.
        assert!(db.get_node(&root.key).is_some());
        assert!(db.get_node(&leaf.key).is_some());
        assert!(db.get_node(&new_root.key).is_some());
        assert!(db.get_node(&new_leaf.key).is_some());
    }

    #[cfg(feature = "layout-v2")]
    /// A new root added during the sweep phase triggers a return to mark and is
    /// preserved.
    #[test]
    fn incremental_gc_new_root_during_sweep() {
        let mut db = ParityDb::<sha2::Sha256>::default();

        let leaf = RawNode::new(&[1, 1], 0, vec![]);
        let root = RawNode::new(&[1, 2], 0, vec![&leaf]);
        leaf.insert_into_db(&mut db);
        root.insert_into_db(&mut db);
        db.set_root_count(root.key.clone(), 1);

        // Run mark to completion, enter sweep.
        db.gc_start();
        while db.gc_step(1024) {
            let meta = db.gc_read_meta();
            if meta.phase == super::GcPhase::Sweep {
                break;
            }
        }
        assert!(db.gc_is_active());

        // Insert a new tree during the sweep phase.
        let new_leaf = RawNode::new(&[3, 1], 0, vec![]);
        let new_root = RawNode::new(&[3, 2], 0, vec![&new_leaf]);
        db.batch_update(
            vec![
                (
                    new_leaf.key.clone(),
                    Update::InsertNode(new_leaf.clone().into_obj()),
                ),
                (
                    new_root.key.clone(),
                    Update::InsertNode(new_root.clone().into_obj()),
                ),
                (new_root.key.clone(), Update::SetRootCount(1)),
            ]
            .into_iter(),
        );

        // Phase should have reverted to Mark.
        let meta = db.gc_read_meta();
        assert_eq!(meta.phase, super::GcPhase::Mark);

        // Finish GC.
        while db.gc_step(1024) {}

        assert!(db.get_node(&root.key).is_some());
        assert!(db.get_node(&leaf.key).is_some());
        assert!(db.get_node(&new_root.key).is_some());
        assert!(db.get_node(&new_leaf.key).is_some());
    }

    #[cfg(feature = "layout-v2")]
    /// Running GC twice works correctly (generation flip).
    #[test]
    fn incremental_gc_two_cycles() {
        let mut db = ParityDb::<sha2::Sha256>::default();

        let leaf = RawNode::new(&[1, 1], 0, vec![]);
        let root = RawNode::new(&[1, 2], 0, vec![&leaf]);
        let orphan = RawNode::new(&[2, 1], 0, vec![]);
        leaf.insert_into_db(&mut db);
        root.insert_into_db(&mut db);
        orphan.insert_into_db(&mut db);
        db.set_root_count(root.key.clone(), 1);

        // First cycle.
        db.gc();
        assert_eq!(db.size(), 2);

        // Add another orphan and run again.
        let orphan2 = RawNode::new(&[3, 1], 0, vec![]);
        orphan2.insert_into_db(&mut db);
        assert_eq!(db.size(), 3);

        // Second cycle.
        db.gc();
        assert_eq!(db.size(), 2);
        assert!(db.get_node(&root.key).is_some());
        assert!(db.get_node(&leaf.key).is_some());
        assert!(db.get_node(&orphan2.key).is_none());
    }

    #[cfg(feature = "layout-v2")]
    /// Crash recovery: a GC interrupted mid-cycle can be resumed after reopening.
    #[test]
    fn incremental_gc_crash_recovery() {
        let path = tempfile::TempDir::new().unwrap().keep();

        let leaf = RawNode::<sha2::Sha256>::new(&[1, 1], 0, vec![]);
        let orphan = RawNode::<sha2::Sha256>::new(&[2, 1], 0, vec![]);

        // Setup: insert nodes, start GC, do partial work, then drop (simulating
        // a crash).
        {
            let mut db = ParityDb::<sha2::Sha256>::open(&path);
            leaf.insert_into_db(&mut db);
            orphan.insert_into_db(&mut db);
            db.set_root_count(leaf.key.clone(), 1);
            db.gc_start();
            db.gc_step(1); // Partial mark.
            db.db.flush();
        }

        // Reopen and finish GC.
        {
            let db = ParityDb::<sha2::Sha256>::open(&path);
            assert!(db.gc_is_active());
            while db.gc_step(1024) {}
            assert!(!db.gc_is_active());
            assert_eq!(db.size(), 1);
            assert!(db.get_node(&leaf.key).is_some());
            assert!(db.get_node(&orphan.key).is_none());
        }
    }

    #[cfg(feature = "layout-v2")]
    /// gc_start is a no-op if GC is already active.
    #[test]
    fn incremental_gc_start_is_idempotent() {
        let mut db = ParityDb::<sha2::Sha256>::default();
        let leaf = RawNode::new(&[1, 1], 0, vec![]);
        leaf.insert_into_db(&mut db);
        db.set_root_count(leaf.key.clone(), 1);

        db.gc_start();
        let meta1 = db.gc_read_meta();
        db.gc_start(); // Should be a no-op.
        let meta2 = db.gc_read_meta();
        assert_eq!(meta1.generation, meta2.generation);
        assert_eq!(meta1.phase, meta2.phase);

        while db.gc_step(1024) {}
        assert_eq!(db.size(), 1);
    }

    #[cfg(feature = "layout-v2")]
    /// Deeper tree: GC correctly traverses multi-level DAGs.
    #[test]
    fn incremental_gc_deep_tree() {
        let mut db = ParityDb::<sha2::Sha256>::default();

        let n41 = RawNode::new(&[1, 4, 1], 0, vec![]);
        let n42 = RawNode::new(&[1, 4, 2], 0, vec![]);
        let n43 = RawNode::new(&[1, 4, 3], 0, vec![]);
        let n31 = RawNode::new(&[1, 3, 1], 0, vec![&n41, &n42]);
        let n32 = RawNode::new(&[1, 3, 2], 0, vec![&n42, &n43]);
        let n21 = RawNode::new(&[1, 2, 1], 0, vec![&n31, &n42, &n32]);
        let orphan = RawNode::new(&[2, 1, 1], 0, vec![]);

        for n in [&n41, &n42, &n43, &n31, &n32, &n21, &orphan] {
            n.insert_into_db(&mut db);
        }
        db.set_root_count(n21.key.clone(), 1);
        assert_eq!(db.size(), 7);

        db.gc();

        // Everything reachable from n21 survives; orphan is deleted.
        assert_eq!(db.size(), 6);
        for n in [&n41, &n42, &n43, &n31, &n32, &n21] {
            assert!(db.get_node(&n.key).is_some(), "expected {:?} to survive", n.key);
        }
        assert!(db.get_node(&orphan.key).is_none());
    }
}
