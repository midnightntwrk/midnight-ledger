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

#![allow(rustdoc::private_intra_doc_links)]
//! A storage layer that intermediates between the in-memory
//! [`Arena`](crate::arena::Arena) and the persistent
//! [database](crate::db::DB), managing reference counts for the Merkle-ized
//! DAGs we store in the DB, and providing a caching layer.
//!
//! See [`StorageBackend`] for a detailed overview and the public API.

use crate::{
    WellBehavedHasher,
    arena::ArenaKey,
    cache::Cache,
    db::{DB, Update},
};
use rand::distributions::{Distribution, Standard};
use serialize::{Deserializable, Serializable};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
};

#[derive(PartialEq, Debug, Clone, Copy)]
/// A non-trivial update delta for a value stored under an [`ArenaKey`] in
/// memory in [`StorageBackend`].
///
/// Invariant: at least one of `ref_delta` and `root_delta` are non-zero. This
/// is the "non-trivial" part.
struct Delta {
    ref_delta: i32,
    root_delta: i32,
}
impl Delta {
    /// Create a new delta that increments the ref count by `ref_delta`.
    fn new_ref_delta(ref_delta: i32) -> Self {
        assert!(ref_delta != 0, "ref delta must be non-zero");
        Self {
            ref_delta,
            root_delta: 0,
        }
    }

    /// Create a new delta that increments the root count by `root_delta`.
    fn new_root_delta(root_delta: i32) -> Self {
        assert!(root_delta != 0, "root delta must be non-zero");
        Self {
            ref_delta: 0,
            root_delta,
        }
    }

    /// Combine two deltas, returning `None` if the result would be a trivial
    /// no-op delta.
    fn combine(self, other: Self) -> Option<Self> {
        let ref_delta = self.ref_delta + other.ref_delta;
        let root_delta = self.root_delta + other.root_delta;
        if ref_delta == 0 && root_delta == 0 {
            None
        } else {
            Some(Self {
                ref_delta,
                root_delta,
            })
        }
    }
}

#[derive(PartialEq, Debug, Clone)]
/// A value stored under an [`ArenaKey`] in memory in [`StorageBackend`].
///
/// Contains an underlying object, which is the canonical version of that
/// object, and includes any pending updates specified by `delta`.
///
/// Invariants:
/// - `delta` is always non-zero.
/// - `obj` includes ref-count changes prescribed by `delta`.
enum CacheValue<H: WellBehavedHasher> {
    /// Existing object, read from DB, but not mutated.
    Read { obj: OnDiskObject<H> },
    /// Existing object, mutated due to new parent or gc-root
    /// references.
    ///
    /// Possible optimization: we could avoid storing the `obj` here, and so
    /// also avoid looking it up in the DB until we were ready to apply the
    /// update (which will be never for temp updates). However, the trade-off is
    /// that this would prevent us from failing fast if there was a bug whereby
    /// a non-existent object was referenced via a child key, or a ref count
    /// became negative. I.e. by having the object here, we're able to do sanity
    /// checks.
    Update { delta: Delta, obj: OnDiskObject<H> },
    /// Existing object in a combination of `Read` and `Update` states. We keep
    /// this separate from `Read` and `Update`, because when a sequence of
    /// `Update`s combine to have no effect we drop them, whereas a
    /// `ReadAndUpdate` with no updates devolves into a `Read`, but stays in the
    /// cache.
    ReadAndUpdate { delta: Delta, obj: OnDiskObject<H> },
    /// New object, not in DB, resulting from calling `cache` on an unknown
    /// key.
    Create { obj: OnDiskObject<H> },
    /// Like `Create`, but additionally with non-zero ref or root counts.
    CreateAndUpdate { obj: OnDiskObject<H>, delta: Delta },
    /// New object, not in DB, no longer of interest to arena, but still
    /// referenced. This is the result of `uncache`ing a node in the
    /// `CreateAndUpdate` state. A more accurate name might be
    /// `CreateAndUpdateAndDelete` :)
    ///
    /// We need the `delta` to track the root counts, if any; the ref counts are
    /// already in `obj`.
    CreateAndDelete { obj: OnDiskObject<H>, delta: Delta },
    /// A dummy value used for memory swaps
    Dummy,
}

impl<H: WellBehavedHasher> CacheValue<H> {
    /// Return reference to contained `OnDiskObject`.
    fn get_obj(&self) -> &OnDiskObject<H> {
        match self {
            CacheValue::Read { obj }
            | CacheValue::Update { obj, .. }
            | CacheValue::ReadAndUpdate { obj, .. }
            | CacheValue::Create { obj, .. }
            | CacheValue::CreateAndUpdate { obj, .. }
            | CacheValue::CreateAndDelete { obj, .. } => obj,
            CacheValue::Dummy => unreachable!(),
        }
    }

    /// This value contains pending mutations, either as a new object, or
    /// updates to an existing object.
    fn is_pending(&self) -> bool {
        match self {
            CacheValue::Read { .. } | CacheValue::Dummy => false,
            CacheValue::Update { .. }
            | CacheValue::ReadAndUpdate { .. }
            | CacheValue::Create { .. }
            | CacheValue::CreateAndUpdate { .. }
            | CacheValue::CreateAndDelete { .. } => true,
        }
    }
}

#[derive(Debug)]
/// A storage back-end, that wraps interactions with the database and provides
/// in-memory caching. Its public API provides a mapping from `ArenaKey` keys
/// to `OnDiskObject` objects, and a way to persist such objects to the DB,
/// taking care of reference counting along the way.
///
/// # Pub access to `StorageBackend` objects
///
/// There is no `pub` API to construct `StorageBackend`. Rather, lib users are
/// expected to construct a [`crate::Storage`], and then access the
/// `StorageBackend` via [`crate::arena::Arena::with_backend`] method of the
/// [`crate::Storage::arena`] field.
///
/// # Overview
///
/// This module intermediates between the in-memory arena and the persistent
/// database, managing reference counts for the Merkle-ized DAGs we store in the
/// DB, and providing a caching layer. The cache addresses several concerns:
///
/// 1) reducing disk reads: by keeping data read from the on-disk DB in memory,
///    we can avoid going to disk again the next time that data is accessed.
///
/// 2) reducing disk writes: the arena creates a lot of *temporary* data
///    structures that will never be persisted -- i.e. be marked as a gc-root or
///    be in the transitive closure of a GC root -- and so instead of eagerly
///    writing these data to disk, we keep them in memory in case they get
///    dropped right away. Also, we track reference counts in the DB, and so by
///    doing reference count updates only in cache where possible, we avoid
///    having to write intermediate states to disk.
///
/// 3) bulking disk writes: transactions in SQLite (used by
///    [`crate::db::SqlDB`]) are very expensive, and so we want to do many
///    writes at once when possible. By collecting the potential writes in
///    memory in the cache, we can then flush them periodically en masse.
///
/// However, this caching layer also adds complexity, because we now have
/// multiple potential sources of truth, the DB and the cache. The main
/// complexity here arises from concern (2), reducing disk writes. We optimize
/// the common case, where the arena creates temporary data structures and
/// [`StorageBackend::cache`]s them into the cache, only to drop them soon
/// after. The tricky thing here is deducing that the net change of a bunch of
/// arena cache mutations is the identity, i.e. that we don't need to write
/// anything to the DB. In the case of new object creation -- meaning objects
/// that aren't already in the DB -- this is easy: when the arena announces it's
/// done with a particular key -- by calling [`StorageBackend::uncache`] on that
/// key -- we simply check if that key has any remaining references to it, and
/// drop it if not. The harder case is when the arena changes the reference
/// counts for keys that already exist in the DB, by creating larger structures
/// that reference these existing keys. To handle this case, we track reference
/// count deltas instead of cardinal reference counts: if the net effect of all
/// the deltas is zero, then we know the key can safely be dropped from the
/// cache.
///
/// Another source of complexity arises from a concern that has nothing to do
/// with the goals of caching, and instead conflicts with caching: we want to
/// support the creation of data structures that are too large to fit in memory,
/// without the user needing to carefully checkpoint the construction. To deal
/// with this, we track the size of the write cache, and provide
/// [`StorageBackend::flush_cache_evictions_to_db`] to bulk-write mutations to disk
/// when the write cache has become too large. In these cases we can't know at the time
/// of disk-writing if the written mutations will persist. So, we may end up
/// with unused temporary values in the DB, and a separate GC operation is
/// responsible for cleaning these up periodically.
///
/// # Assumptions
///
/// - the database is not changing under our feet, meaning in particular that
///   there is only one back-end. When an object `obj` is `cache`d into the
///   back-end, what happens depends on whether `obj` is already in the
///   database. To support the database changing under our feet, we would need
///   to handle the case where `obj` was in the database when it was `cache`d,
///   but was then removed from or modified in the database by another thread
///   before `obj` was `uncache`d.
///
/// - there is only one "logical" arena calling into / manipulating the
///   back-end. Here "one logical arena" includes multiple clones of a single
///   initial arena, since cloned arenas share their metadata structures and
///   avoid `cache`ing the same object more than once. Indeed, the key
///   assumption we make here is that no object will be `cache`ed more than
///   once, independently. It would be possibly to support multiple, distinct
///   arenas -- non-clones, with distinct metadata -- manipulating the back-end,
///   but would require more careful tracking of `cache` calls, to handle the case
///   where two different arenas `cache` the same object, and we need to be
///   careful to keep it around until all these arena's are done using it.
///
/// - objects are `cache`d only after all of their children have been `cache`d
///   or are already in the db. In particular, this means the arena is
///   responsible for sanitizing user controlled inputs before passing them to
///   the back-end, to make sure they are well formed, in terms of children
///   references.
///
/// - if the caller `cache`s an object `obj`, and wants
///   `obj` to continue to exist in the back-end, then before `uncache`ing `obj`,
///   the user needs to first do either of:
///
///   - `cache` but not `uncache` another object which has `obj` in its
///     transitive closure.
///
///   - `persist` an object that has `obj` in its transitive closure.
///
///   Note that it's okay for the caller to e.g. `uncache` interior nodes of a
///   large data structure, as long as a reference to the root has been
///   `persist`ed or `cache`d-but-not-`uncache`d.
///
/// # Terminology and APIs
///
/// A key will never be in `read_cache` and `write_cache` at the same time, but
/// may be in `database` and either cache at the same time. If a key is in
/// either cache, then we say it's "in memory". If a key is in memory, then the
/// value stored in memory under that key describes the canonical version of the
/// object.
///
/// The back-end provides various public APIs related to manipulating in-memory
/// representations of objects. The `get` API brings an object into memory from
/// the database. The `cache` API attempts to create a new object in memory,
/// but falls back on any existing version already in memory or the DB. The
/// `uncache` API informs the back-end that an object is no longer of interest to
/// the caller, which allows the back-end to remove it from memory if it has no
/// references or pending updates in memory. These APIs act on `ArenaKey` keys
/// and `OnDiskObject` values, but internally manipulate more complex states, in
/// the form of `CacheValue` values.
pub struct StorageBackend<D: DB> {
    /// Persistent backing storage.
    database: D,
    /// The size of `read_cache`, and size to which `write_cache` will be
    /// truncated on flush. If zero, then caches are unbounded.
    ///
    /// This is the *number* of cached objects, not the memory consumed by them.
    cache_size: usize,
    /// Bounded, in-memory LRU read cache. Objects automatically fall off the
    /// end when the cache is at capacity.
    ///
    /// This cache *only* contains `CacheValue::Read` values.
    read_cache: Cache<ArenaKey<D::Hasher>, CacheValue<D::Hasher>>,
    /// Un-bounded, in-memory LRU write cache. The place where new in-memory
    /// objects go initially. This cache is brought down to `self.cache_size`
    /// when a `self.flush_*` operation is run, and we refer to objects flushed
    /// this way as "evictions".
    ///
    /// This cache *never* contains `CacheValue::Read` values.
    write_cache: Cache<ArenaKey<D::Hasher>, CacheValue<D::Hasher>>,
    /// Keys that have been `cache`d but not `uncached`. Used as additional,
    /// temporary GC roots.
    live_inserts: HashSet<ArenaKey<D::Hasher>>,
    /// Run-time stats to help with performance tuning.
    // Use interior mutability to allow updating stats in "pure" functions.
    stats: RefCell<StorageBackendStats>,
}

/// Run-time stats to help with performance tuning.
#[derive(Debug, Clone, Copy)]
pub struct StorageBackendStats {
    /// Number of times `get` was called and the requested object was found in
    /// memory.
    pub get_cache_hits: usize,
    /// Number of times `get` was called, the requested object was not found in
    /// memory, and the back-end attempted to read it from the DB.
    pub get_cache_misses: usize,
}

impl<D: DB> StorageBackend<D> {
    /// Create a new `StorageBackend` with cache bound `cache_size` and optional
    /// database.
    ///
    /// If `cache_size` is 0, then the read cache is unbounded.
    ///
    /// The cache bound `cache_size` is strictly enforced for the read cache,
    /// but the write cache is allowed to grow beyond this bound, and is then
    /// truncated to `cache_size` on flush.
    ///
    /// Note: here `cache_size` is the *number* of cached objects, not size of
    /// memory consumed by them!
    pub(crate) fn new(cache_size: usize, database: D) -> Self {
        let read_cache = if cache_size > 0 {
            Cache::new(cache_size)
        } else {
            Cache::unbounded()
        };
        Self {
            database,
            cache_size,
            read_cache,
            write_cache: Cache::unbounded(),
            live_inserts: HashSet::new(),
            stats: RefCell::new(StorageBackendStats {
                get_cache_hits: 0,
                get_cache_misses: 0,
            }),
        }
    }

    /// Get object with key, trying to get object from memory and falling back
    /// to database if necessary.
    ///
    /// If an object is found, then it ends up in the front of the cache.
    ///
    /// Callers may want to call [`Self::pre_fetch`] on the root of the DAG
    /// once, before calling this `get` function many times, if they expect to
    /// call `get` for many nodes from the same DAG.
    ///
    /// Calling `get` on a temporary object that has already been `uncache`d,
    /// but is still in memory because it's still referenced, will not cause
    /// that temp object to continue to exist if its ref count goes to zero. If
    /// you want that, then call `cache` instead!
    pub(crate) fn get(&mut self, key: &ArenaKey<D::Hasher>) -> Option<&OnDiskObject<D::Hasher>> {
        // If already in memory, move to the front of cache.
        if self.peek_from_memory(key).is_some() {
            self.stats.borrow_mut().get_cache_hits += 1;
            let value = self.remove_from_memory(key);
            let value = match value {
                CacheValue::Update {
                    delta,
                    obj,
                } => CacheValue::ReadAndUpdate {
                    delta,
                    obj,
                },
                CacheValue::Read { .. }
                | CacheValue::ReadAndUpdate { .. }
                | CacheValue::Create { .. }
                | CacheValue::CreateAndUpdate { .. }
                // Note: `get`ing an object in the `CreateAndDelete` state is
                // not nec a bug, since e.g. it could be in the closure of a new
                // GC root, and awaiting persistence along with that root.
                | CacheValue::CreateAndDelete { .. } | CacheValue::Dummy => value,
            };
            self.cache_insert_new_key(key.clone(), value);
            return Some(self.peek_from_memory(key).unwrap().get_obj());
        }

        // Attempt to read from DB.
        self.stats.borrow_mut().get_cache_misses += 1;
        if let Some(obj) = self.database.get_node(key) {
            self.cache_insert_new_key(key.clone(), CacheValue::Read { obj });
        }
        self.peek_from_memory(key).map(|cv| cv.get_obj())
    }

    /// Get the root count for `key`, incorporating any pending in-memory updates.
    pub(crate) fn get_root_count(&self, key: &ArenaKey<D::Hasher>) -> u32 {
        let db_root_count = self.database.get_root_count(key);
        let mem_root_delta = match self.peek_from_memory(key) {
            Some(CacheValue::Read { .. }) => 0,
            Some(CacheValue::Update { delta, .. }) => delta.root_delta,
            Some(CacheValue::ReadAndUpdate { delta, .. }) => delta.root_delta,
            Some(CacheValue::Create { .. }) => 0,
            Some(CacheValue::CreateAndUpdate { delta, .. }) => delta.root_delta,
            Some(CacheValue::CreateAndDelete { delta, .. }) => delta.root_delta,
            None | Some(CacheValue::Dummy) => 0,
        };
        let root_count = db_root_count as i32 + mem_root_delta;
        assert!(root_count >= 0, "root count must be non-negative");
        root_count as u32
    }

    /// Get a copy of the current stats.
    ///
    /// Public API users are expected to access this via
    /// `Arena::with_backend(|b| b.get_stats())`.
    pub fn get_stats(&self) -> StorageBackendStats {
        *self.stats.borrow()
    }

    /// Get collection of all GC roots and their root counts. Returned root
    /// counts are positive.
    ///
    /// As implemented, this function assumes no concurrent modification of the
    /// roots in the DB while the root collection is being computed.
    pub fn get_roots(&self) -> HashMap<ArenaKey<D::Hasher>, u32> {
        // To be a root, a key must be stored in the DB as a root, or have
        // pending root-count updates in memory. However, we need to be careful
        // to handle the case where a key is a root in the DB, but it also has
        // in-memory updates that reduce its root count back to zero, making it
        // no longer a root. So, we start with the collection of DB roots, and
        // then extend and update that according to the pending updates in
        // memory.
        let mut roots_map = self.database.get_roots();
        for (key, _) in self.write_cache.iter() {
            let root_count = self.get_root_count(key);
            if root_count > 0 {
                roots_map.insert(key.clone(), root_count);
            } else {
                roots_map.remove(key);
            }
        }
        roots_map
    }

    /// Cache an object with a given key into memory, assumes all children
    /// have already been `cache`d, or are already in the DB.
    ///
    /// If the object is not in the DB, then it will continue to exist at least
    /// until `uncache`d, although it may get written to disk if evicted from the
    /// write cache.
    ///
    /// In all cases, `cache`ing an object moves it to the front of the cache.
    ///
    /// This function is responsible for maintaining "natural" reference counts,
    /// arising from data parent->child relationships. See `persist` for creating
    /// GC roots.
    ///
    /// # Note
    ///
    /// A `cache`d object is considered "live"/reachable for the purposes of
    /// GC, until it is subsequently `uncache`d.
    ///
    /// # Panics
    ///
    /// It's an error to `cache` the same object more than once without `uncache`ing
    /// first.
    pub(crate) fn cache(
        &mut self,
        key: ArenaKey<D::Hasher>,
        data: std::vec::Vec<u8>,
        children: std::vec::Vec<ArenaKey<D::Hasher>>,
    ) {
        assert!(
            !self.live_inserts.contains(&key),
            "a key can't be cached more than once without being uncached"
        );
        self.live_inserts.insert(key.clone());

        // If this object is already in memory then there's nothing to change
        // about the object itself, since only reference counts can vary for a
        // given key. However, we move it to the front of the cache, and move it
        // out of `CreateAndDelete` state if necessary, since `cache` promises
        // that the object will stick around at least until being `uncache`d
        // again.
        if self.peek_from_memory(&key).is_some() {
            let value = self.remove_from_memory(&key);
            let value = match value {
                // We don't upgrade this to `Create`, because `Create` implies
                // that we incremented the reference counts for this node's
                // children, but we didn't and won't do that, since the object
                // already exists.
                CacheValue::Read { .. }
                | CacheValue::ReadAndUpdate { .. }
                | CacheValue::Create { .. }
                | CacheValue::CreateAndUpdate { .. }
                | CacheValue::Dummy => value,
                CacheValue::Update { delta, obj } => CacheValue::ReadAndUpdate { delta, obj },
                CacheValue::CreateAndDelete { obj, delta } => {
                    CacheValue::CreateAndUpdate { obj, delta }
                }
            };
            self.cache_insert_new_key(key, value);
            return;
        }
        // If this object is not in memory, but already in the DB, then pull it
        // into the cache.
        //
        // Note: this check is actually quite expensive for SqlDB! For the
        // `arena::load_large_tree` stress test, commenting out this check
        // reduces the time taking to build a height 20 binary tree from 20.5s
        // to 3.5s, i.e. an 80%+ improvement. For ParityDb, this check seems to
        // have no performance effect. Note that the database is empty in that
        // test, and performance could be different for a non-empty db.
        //
        // If it later turns out that slowdown due to this db check matters, we
        // could eliminate it by refactoring the backend to only track ref-count
        // deltas, instead of full ref-counts like we track currently. Then we
        // either compute the full ref-counts only when flushing, or make the db
        // responsible for applying the ref-count deltas directly.
        if let Some(obj) = self.database.get_node(&key) {
            self.cache_insert_new_key(key.clone(), CacheValue::Read { obj });
            return;
        }
        // Otherwise, this is a new object, so we need to update the ref counts of all the
        // children, and insert a `Create`.
        self.update_counts(&children, Delta::new_ref_delta(1));
        self.cache_insert_new_key(
            key,
            CacheValue::Create {
                obj: OnDiskObject {
                    data,
                    ref_count: 0,
                    children,
                },
            },
        );
    }

    /// Mark object for `key` as no longer live, allowing it to fall out of the
    /// cache later if possible. If the object for `key` still has pending
    /// mutations, or is a new object with a non-zero reference count, then we
    /// can't actually drop it
    ///
    /// # Panics
    ///
    /// It's an error to `uncache` an object that hasn't been `cache`d first,
    /// and an error to `uncache` an `cache`d object a second time without
    /// re-`cache`ing it.
    pub(crate) fn uncache(&mut self, key: &ArenaKey<D::Hasher>) {
        assert!(
            self.live_inserts.contains(key),
            "a key can't be uncached more times than it was cached (0 or 1)"
        );
        self.live_inserts.remove(key);

        if let Some(value) = self.peek_from_memory(key).cloned() {
            match value {
                // We need to remove `Create` values, since we don't want them to
                // overflow to disk. In practice this shouldn't matter for cache
                // reuse, since this only concerns roots of temporary DAGs, and
                // we don't expect the Arena actually unmark the roots until
                // they're done with the DAG. Rather, for WIP DAGs, the Arena is
                // expected to unload and uncache the descendents only, and those
                // will be preserved by this function, since they're still
                // reachable.
                CacheValue::Create { obj } => {
                    assert_eq!(
                        obj.ref_count, 0,
                        "CacheValue::Create values must have zero ref counts"
                    );
                    self.remove_from_memory(key);
                    self.update_counts(&obj.children, Delta::new_ref_delta(-1))
                }
                CacheValue::CreateAndUpdate { obj, delta } => self
                    .write_cache
                    .update_in_place(key.clone(), CacheValue::CreateAndDelete { obj, delta }),
                CacheValue::CreateAndDelete { .. } => (),
                CacheValue::Read { .. } => (),
                CacheValue::Update { .. } => (),
                CacheValue::ReadAndUpdate { .. } => (),
                CacheValue::Dummy => (),
            };
        }
    }

    /// Un-mark `key` as GC root. See [`Self::persist`].
    pub fn unpersist(&mut self, key: &ArenaKey<D::Hasher>) {
        self.update_counts(&[key.clone()], Delta::new_root_delta(-1));
    }

    /// Mark `key` as a GC root, meaning it will be persisted across GC runs,
    /// even if no other data references it. Use `unpersist` to un-mark.
    ///
    /// NOTE: The same `key` can be `persist`ed multiple times, in which case it
    /// needs to be `unpersist`ed the same number of times to stop being treated as
    /// a GC root. In other words, the GC-root status of a key is a non-negative
    /// int, not a bool.
    pub fn persist(&mut self, key: &ArenaKey<D::Hasher>) {
        self.update_counts(&[key.clone()], Delta::new_root_delta(1));
    }

    /// Load DAG rooted at `key` into the cache from the DB.
    ///
    /// Nodes up to and including 0-indexed depth `depth`, i.e. where `key` is
    /// for node at depth 0.
    ///
    /// The `truncate` argument, if true, means the pre-fetch BFS will be
    /// truncated at nodes that are already in memory. Note that if using
    /// `truncate == false`, and calling `pre_fetch` many times, you can end
    /// up doing `O(n^2)` work when getting `n` nodes, if the nodes are pre-fetched
    /// from the leaves up to the root (simplest case: a linear chain of `n`
    /// nodes).
    pub fn pre_fetch(
        &mut self,
        key: &ArenaKey<D::Hasher>,
        max_depth: Option<usize>,
        truncate: bool,
    ) {
        let max_count = if self.cache_size > 0 {
            Some(self.cache_size)
        } else {
            None
        };
        let mut kvs = self.database.bfs_get_nodes(
            key,
            |key| self.peek_from_memory(key).map(|cv| cv.get_obj().clone()),
            truncate,
            max_depth,
            max_count,
        );
        // The `bfs_get_nodes` to return `kvs` in traversal order. We insert
        // them here in reverse traversal order, so that the root is lru, and
        // lru-age increases with depth.
        kvs.reverse();
        for (k, v) in kvs {
            self.cache_insert_new_key(k, CacheValue::Read { obj: v })
        }
    }

    /// Flush writes implied by `CacheValue` values to DB.
    ///
    /// # Panics
    ///
    /// This function assumes all values in `writes` have been removed from
    /// memory already, and none of them are `CacheValue::Read`s, and panics if
    /// this is not true.
    ///
    /// # Note
    ///
    /// Mutating `CacheValue`s will have their objects inserted into the read
    /// cache as `CacheValue::Read` values, in order to keep them cached in
    /// memory if possible. In particular, this means they must be removed from
    /// the cache before calling this function!
    fn flush_to_db<I>(&mut self, writes: I)
    where
        I: Iterator<Item = (ArenaKey<D::Hasher>, CacheValue<D::Hasher>)>,
    {
        let mut updates = vec![];
        for (k, v) in writes {
            // Check for reads first, to give better error messages.
            if matches!(v, CacheValue::Read { .. }) {
                panic!("BUG: unexpected CacheValue::Read!")
            }
            // Insert objects for flushed writes into the read cache.
            //
            // It might make sense to not recache `CreateAndDelete`
            // values here, but that's a question to be answered as part
            // of optimization. For example, one way that
            // `CreateAndDelete` arises is when the `Arena` unloads an
            // object into "lazy" form. If the unloaded data gets loaded
            // again, then it will be better for it to still be in the
            self.cache_insert_new_key(
                k.clone(),
                CacheValue::Read {
                    obj: v.get_obj().clone(),
                },
            );
            // Calculate DB updates corresponding to pending writes.
            match v {
                CacheValue::Read { .. } => unreachable!("already handled Read above"),
                CacheValue::Update { delta, obj } | CacheValue::ReadAndUpdate { delta, obj } => {
                    if delta.ref_delta != 0 {
                        updates.push((k.clone(), Update::InsertNode(obj)));
                    }
                    if delta.root_delta != 0 {
                        // Can't use self.get_root_count here, since `v` has
                        // already been removed from DB.
                        let db_root_count = self.database.get_root_count(&k) as i32;
                        let root_count = db_root_count + delta.root_delta;
                        assert!(root_count >= 0, "roots counts can't be negative!");
                        updates.push((k, Update::SetRootCount(root_count as u32)));
                    }
                }
                CacheValue::CreateAndUpdate { obj, delta }
                | CacheValue::CreateAndDelete { obj, delta } => {
                    updates.push((k.clone(), Update::InsertNode(obj)));
                    if delta.root_delta != 0 {
                        // This a new object that's not yet in the DB.
                        assert!(delta.root_delta > 0, "root count can't be negative");
                        let root_count = delta.root_delta as u32;
                        updates.push((k, Update::SetRootCount(root_count)));
                    }
                }
                CacheValue::Create { obj } => updates.push((k, Update::InsertNode(obj))),
                CacheValue::Dummy => {}
            }
        }
        self.database.batch_update(updates.into_iter());
    }

    /// Push pending writes to shrink the write cache to `self.cache_size`.
    ///
    /// # Note
    ///
    /// This doesn't flush all pending writes in `self.write_cache` --
    /// that's what the `flush_all_changes_to_db` does -- the point of this
    /// function is to help avoid the write cache becoming too large. But what
    /// we may want is an intermediate version, that flushes all *referenced*
    /// pending writes in the write cache.
    pub fn flush_cache_evictions_to_db(&mut self) {
        if self.cache_size == 0 {
            return;
        }
        let mut evictions = HashMap::new();
        while self.write_cache.len() > self.cache_size {
            let (k, v) = self.write_cache.pop_lru().unwrap();
            evictions.insert(k, v);
        }
        self.flush_to_db(evictions.into_iter());
    }

    /// Push all pending writes to the DB.
    pub fn flush_all_changes_to_db(&mut self) {
        let iter = self
            .write_cache
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect::<std::vec::Vec<_>>()
            .into_iter();
        self.write_cache.clear();
        self.flush_to_db(iter);
    }

    /// Remove all unreachable nodes from memory and the DB.
    ///
    /// Here "unreachable" nodes are nodes with `ref_count == 0` and `root_count
    /// == 0`, which are not currently "live inserts", meaning they have been
    /// `cache`d but not `uncache`d.
    ///
    /// # Note
    ///
    /// This GC implementation assumes the correctness of the reference counts
    /// stored in the db and memory, and doesn't actually do a reachability
    /// search from the roots. This is much faster than searching the entire db
    /// from the roots, but means this function is not sufficient to clean up
    /// the db after a crash which left the db in an inconsistent state, in
    /// terms of db-stored reference counts.
    // # Possible optimization
    //
    // We could batch load the `unreachable_keys` nodes
    // from the db, altho it's unclear what the size bound should be here in
    // relation to the read cache size, since we need to bring in their children
    // as well (which could be a second bulk pass), and so we'd probably need
    // some idea of the average number of children of a typical node.
    //
    // Alternatively, and much more generally, we could eliminate most concerns
    // about bulk fetching and deleting from the db here (and elsewhere!) by
    // instead exposing a db transaction interface. Then we would just start a
    // write transaction at the beginning of gc, and commit it at the end. To
    // get the same performance as the existing db batch operations we need to
    // also cache prepared statements (I have no idea how much these buy us or
    // if this caching would be worth the trouble).
    //
    // # Alternative implementation: time bounded, incremental
    //
    // The current implementation does a full gc, cleaning up as much as
    // possible. However, since the backend is unusable while gc is running, we
    // might instead prefer a time bounded, incremental version. To get a time
    // bounded version, assuming we have enough time to calculate the full
    // initial set of `unreachable_keys`, we could just partially process this
    // set, instead of fully processing it as we do now.
    pub fn gc(&mut self) {
        // Compute unreachable keys, taking memory into account.
        let db_unreachable_keys = self.database.get_unreachable_keys();
        let mut mem_unreachable_keys = vec![];
        for (k, v) in self.write_cache.iter() {
            if v.get_obj().ref_count == 0 {
                mem_unreachable_keys.push(k.clone());
            }
        }
        // For the purposes of GC, root keys include live inserts.
        let root_keys: HashSet<_> = self
            .get_roots()
            .into_keys()
            .chain(self.live_inserts.clone())
            .collect();
        let mut unreachable_keys: std::vec::Vec<_> = db_unreachable_keys
            .into_iter()
            .chain(mem_unreachable_keys)
            .filter(|k| !root_keys.contains(k))
            .collect();

        // Mark unreachable keys for deletion, decrementing child ref counts,
        // and marking children that become unreachable this way.
        let mut keys_to_delete = vec![];
        while let Some(key) = unreachable_keys.pop() {
            // Load node for `key` and all its children.
            let max_depth = 1;
            let truncate = false;
            self.pre_fetch(&key, Some(max_depth), truncate);
            let node = self.peek_from_memory(&key).unwrap().get_obj().clone();

            // Decrement child ref counts, and mark unreachable any children
            // whose ref count goes to zero.
            self.update_counts(&node.children, Delta::new_ref_delta(-1));
            for child_key in &node.children {
                // Unless the cache is unreasonably small, all of the children
                // will already be in memory from the `pre_fetch` above, so this
                // `get` won't trigger any full pre-fetches.
                if self.get(child_key).unwrap().ref_count == 0 && !root_keys.contains(child_key) {
                    unreachable_keys.push(child_key.clone());
                }
            }
            keys_to_delete.push(key);
        }

        // Delete all unreachable keys from memory and db.
        for key in &keys_to_delete {
            self.write_cache.remove(key);
            self.read_cache.remove(key);
        }
        let batch_deletes = keys_to_delete.into_iter().map(|k| (k, Update::DeleteNode));
        self.database.batch_update(batch_deletes);
    }

    /// Attempt to get in-memory value, without updating cache ordering.
    fn peek_from_memory(&self, key: &ArenaKey<D::Hasher>) -> Option<&CacheValue<D::Hasher>> {
        // Here `peek` gets a key from the cache without moving it to the front.
        if let Some(value) = self.write_cache.peek(key) {
            return Some(value);
        }
        if let Some(value) = self.read_cache.peek(key) {
            return Some(value);
        }
        None
    }

    /// Attempt to get in-memory value, without updating cache ordering.
    fn peek_mut_from_memory(
        &mut self,
        key: &ArenaKey<D::Hasher>,
    ) -> Option<&mut CacheValue<D::Hasher>> {
        // Here `peek` gets a key from the cache without moving it to the front.
        if let Some(value) = self.write_cache.peek_mut(key) {
            return Some(value);
        }
        if let Some(value) = self.read_cache.peek_mut(key) {
            return Some(value);
        }
        None
    }

    /// Remove value stored in memory under `key`. Panics if `key` is not in
    /// memory.
    fn remove_from_memory(&mut self, key: &ArenaKey<D::Hasher>) -> CacheValue<D::Hasher> {
        self.write_cache
            .remove(key)
            .or_else(|| self.read_cache.remove(key))
            .unwrap_or_else(|| panic!("key must be in memory"))
            .1
    }

    /// Update ref and root counts for `keys`, e.g. for `key = obj.children` when `obj`
    /// is created or destroyed.
    fn update_counts(&mut self, keys: &[ArenaKey<D::Hasher>], delta: Delta) {
        for key in keys {
            if let Some(cache_val) = self.peek_mut_from_memory(key) {
                // Safe because we will only use this is get an owned copy of cache_val before
                // overwriting it.
                let mut tmp = CacheValue::Dummy;
                std::mem::swap(&mut tmp, cache_val);
                let was_pending = tmp.is_pending();
                enum Action<H: WellBehavedHasher> {
                    Replace(CacheValue<H>),
                    Remove,
                    RemoveWithChildren(OnDiskObject<H>),
                }
                use Action::*;
                let action = match tmp {
                    CacheValue::Read { obj } => {
                        let obj = obj.apply_delta(delta);
                        Replace(CacheValue::ReadAndUpdate { obj, delta })
                    }
                    CacheValue::Update {
                        delta: old_delta,
                        obj,
                    } => {
                        let obj = obj.apply_delta(delta);
                        #[allow(clippy::manual_map)]
                        if let Some(delta) = delta.combine(old_delta) {
                            Replace(CacheValue::Update { delta, obj })
                        } else {
                            Remove
                        }
                    }
                    CacheValue::ReadAndUpdate {
                        obj,
                        delta: old_delta,
                    } => {
                        let obj = obj.apply_delta(delta);
                        if let Some(delta) = delta.combine(old_delta) {
                            Replace(CacheValue::ReadAndUpdate { obj, delta })
                        } else {
                            Replace(CacheValue::Read { obj })
                        }
                    }
                    CacheValue::Create { obj } => {
                        let obj = obj.apply_delta(delta);
                        Replace(CacheValue::CreateAndUpdate { obj, delta })
                    }
                    CacheValue::CreateAndUpdate {
                        obj,
                        delta: old_delta,
                    } => {
                        let obj = obj.apply_delta(delta);
                        if let Some(delta) = delta.combine(old_delta) {
                            Replace(CacheValue::CreateAndUpdate { obj, delta })
                        } else {
                            Replace(CacheValue::Create { obj })
                        }
                    }
                    CacheValue::CreateAndDelete {
                        obj,
                        delta: old_delta,
                    } => {
                        let obj = obj.apply_delta(delta);
                        if let Some(delta) = delta.combine(old_delta) {
                            Replace(CacheValue::CreateAndDelete { obj, delta })
                        } else {
                            RemoveWithChildren(obj)
                        }
                    }
                    CacheValue::Dummy => Remove,
                };
                match action {
                    Replace(obj) => {
                        *cache_val = obj;
                        if was_pending != cache_val.is_pending() {
                            let value = self.remove_from_memory(key);
                            self.cache_insert_new_key(key.clone(), value);
                        } else {
                            self.promote(key);
                        }
                    }
                    Remove => {
                        self.remove_from_memory(key);
                    }
                    RemoveWithChildren(obj) => {
                        // Now we can finally delete `obj`, because it's no
                        // longer referenced.
                        self.remove_from_memory(key);
                        self.update_counts(&obj.children, Delta::new_ref_delta(-1));
                    }
                }
            } else {
                // Possible small optimization: could batch get nodes here for
                // `keys` that aren't already in memory.
                let obj = self
                    .database
                    .get_node(key)
                    .expect("can't update unknown object");
                let obj = obj.apply_delta(delta);
                self.cache_insert_new_key(key.clone(), CacheValue::Update { delta, obj })
            }
        }
    }

    fn promote(&mut self, key: &ArenaKey<D::Hasher>) {
        let _ = self.write_cache.promote(key) || self.read_cache.promote(key);
    }

    /// Add key-value pair to the appropriate (read or write) cache, taking care
    /// of corresponding `pending_write` book keeping.
    ///
    /// # Panics
    ///
    /// Panics if `key` is already in memory.
    fn cache_insert_new_key(&mut self, key: ArenaKey<D::Hasher>, value: CacheValue<D::Hasher>) {
        debug_assert!(
            self.peek_from_memory(&key).is_none(),
            "key must not already be in memory"
        );
        if value.is_pending() {
            assert!(
                self.write_cache.set(key, value).is_none(),
                "write cache is unbounded, it can't evict"
            );
        } else {
            match self.read_cache.set(key, value) {
                Some((_, v)) => {
                    debug_assert!(!v.is_pending(), "read cache shouldn't contain writes");
                }
                _ => {}
            }
        }
    }

    /// Return the number of elements in the write cache.
    ///
    /// # Note
    ///
    /// The elements in the cache are potentially of wildly varying sizes; use
    /// `get_write_cache_obj_bytes` if you're worried about the actual memory
    /// usage.
    pub fn get_write_cache_len(&self) -> usize {
        self.write_cache.len()
    }

    /// Return the number of bytes in the `OnDiskObject`s in the write cache.
    ///
    /// # Note
    ///
    /// This ignores the write-cache data other than `OnDiskObject`s, which
    /// includes the ref deltas. But that data should be relatively small.
    pub fn get_write_cache_obj_bytes(&self) -> usize {
        self.write_cache
            .iter()
            .map(|(_, cv)| cv.get_obj().size())
            .sum()
    }
}

#[derive(Debug, Clone)]
/// The on-disk/DB representation of an object.
///
/// This is `pub` because some `DB` APIs expose it, but we don't anticipate any
/// other public use.
pub struct OnDiskObject<H: WellBehavedHasher> {
    /// Binary representation of the object
    pub(crate) data: std::vec::Vec<u8>,
    /// The number of parent->child references to this object. The object may be
    /// deleted by GC when `ref_count` is zero and the object is not
    /// `persist`ed. Note that the `persist` counts are stored separately -- see
    /// `StorageBackend::get_root_count` for details -- but not here in the
    /// object!
    pub(crate) ref_count: u32,
    pub(crate) children: std::vec::Vec<ArenaKey<H>>,
}

impl<H: WellBehavedHasher> Serializable for OnDiskObject<H> {
    fn serialize(&self, writer: &mut impl std::io::Write) -> std::io::Result<()> {
        self.data.serialize(writer)?;
        self.ref_count.serialize(writer)?;
        self.children.serialize(writer)?;
        Ok(())
    }

    fn serialized_size(&self) -> usize {
        self.data.serialized_size()
            + self.ref_count.serialized_size()
            + self.children.serialized_size()
    }
}

impl<H: WellBehavedHasher> Deserializable for OnDiskObject<H> {
    fn deserialize(
        reader: &mut impl std::io::Read,
        recursion_depth: u32,
    ) -> Result<Self, std::io::Error> {
        let data = Deserializable::deserialize(reader, recursion_depth)?;
        let ref_count = Deserializable::deserialize(reader, recursion_depth)?;
        let children = Deserializable::deserialize(reader, recursion_depth)?;
        Ok(OnDiskObject {
            data,
            ref_count,
            children,
        })
    }
}

impl<H: WellBehavedHasher> OnDiskObject<H> {
    /// Compute new obj with `delta.ref_delta` applied to `ref_count`.
    ///
    /// This ignores any `delta.root_delta`, since the `OnDiskObject` is not
    /// concerned with root counts.
    fn apply_delta(self, delta: Delta) -> Self {
        let ref_count = self.ref_count as i32 + delta.ref_delta;
        assert!(ref_count >= 0, "ref count can't be negative");
        let ref_count = ref_count as u32;
        OnDiskObject {
            data: self.data,
            children: self.children,
            ref_count,
        }
    }

    /// Return the size in bytes of this object.
    pub(crate) fn size(&self) -> usize {
        let data_size = self.data.len();
        let ref_count_size = 4;
        let bytes_per_arena_key = <H as crypto::digest::OutputSizeUser>::output_size();
        let children_refs_size = self.children.len() * bytes_per_arena_key;
        data_size + ref_count_size + children_refs_size
    }
}

// Unsure why this a manual implementation necessary: deriving `PartialEq`
// compiles, but then usage fails!?
impl<H: WellBehavedHasher> PartialEq for OnDiskObject<H> {
    fn eq(&self, other: &Self) -> bool {
        self.data.eq(&other.data)
            && self.ref_count.eq(&other.ref_count)
            && self.children.eq(&other.children)
    }
}

impl<H: WellBehavedHasher> Distribution<OnDiskObject<H>> for Standard {
    /// Generate a random `OnDiskObject` with small internal vectors.
    fn sample<R: rand::prelude::Rng + ?Sized>(&self, rng: &mut R) -> OnDiskObject<H> {
        // Generate a vector of length at most 10.
        fn rand_vec<T, R: rand::prelude::Rng + ?Sized>(rng: &mut R) -> std::vec::Vec<T>
        where
            Standard: Distribution<T>,
        {
            const MAX_LEN: usize = 10;
            let len = rng.gen_range(0..MAX_LEN);
            let mut v = std::vec::Vec::with_capacity(len);
            for _ in 0..len {
                v.push(rng.r#gen());
            }
            v
        }
        OnDiskObject {
            data: rand_vec(rng),
            ref_count: rng.r#gen(),
            children: rand_vec(rng),
        }
    }
}

/// Raw DAG nodes for testing.
#[cfg(test)]
pub(crate) mod raw_node {
    use super::*;
    use crate::DefaultHasher;

    /// Intent: there is no hash relationship between the `key` and the `data`,
    /// i.e. we're not actually enforcing content addressing. In fact, in `new`
    /// we derive the `data` from the `key`.
    #[derive(Debug, Clone)]
    pub(crate) struct RawNode<H: WellBehavedHasher = DefaultHasher> {
        pub(crate) key: ArenaKey<H>,
        // This field is useful for debugging.
        #[allow(dead_code)]
        pub(crate) data: std::vec::Vec<u8>,
        pub(crate) children: std::vec::Vec<ArenaKey<H>>,
        pub(crate) ref_count: u32,
    }

    impl<H: WellBehavedHasher> RawNode<H> {
        pub(crate) fn new(
            key: &[u8],
            ref_count: u32,
            children: std::vec::Vec<&RawNode<H>>,
        ) -> Self {
            let data = key.to_vec();
            let key = ArenaKey::_from_bytes(key);
            let children = children.into_iter().map(|n| n.key.clone()).collect();
            RawNode {
                key,
                data,
                children,
                ref_count,
            }
        }

        /// Cache insert node into back-end, which will calculate correct ref counts.
        pub(crate) fn cache_into_backend<D: DB<Hasher = H>>(
            &self,
            backend: &mut StorageBackend<D>,
        ) {
            backend.cache(self.key.clone(), self.data.clone(), self.children.clone());
        }

        /// Insert node into DB with zero ref count.
        pub(crate) fn insert_into_db<D: DB<Hasher = H>>(&self, db: &mut D) {
            db.insert_node(self.key.clone(), self.clone().into_obj());
        }

        /// Compute corresponding `OnDiskObject` for this `RawNode`.
        pub(crate) fn into_obj(self) -> OnDiskObject<H> {
            OnDiskObject {
                data: self.data,
                ref_count: self.ref_count,
                children: self.children,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate as storage;
    use crypto::digest::Digest;
    use derive_where::derive_where;
    use raw_node::RawNode;

    use crate::{
        Storable,
        arena::{IntermediateRepr, IrLoader, Sp, hash},
        db::InMemoryDB,
        storable::Loader,
    };

    use super::*;

    fn childless_hash<D: DB, T: Storable<D>>(val: &T) -> ArenaKey<D::Hasher> {
        let mut hasher = D::Hasher::default();
        let mut bytes: std::vec::Vec<u8> = std::vec::Vec::new();
        val.to_binary_repr(&mut bytes)
            .expect("Storable data should be able to be represented in binary");

        hasher.update(bytes);

        ArenaKey(hasher.finalize())
    }

    #[test]
    fn cache_overflow_into_db_inmemorydb() {
        test_cache_overflow_into_db::<InMemoryDB>();
    }
    #[cfg(feature = "sqlite")]
    #[test]
    fn cache_overflow_into_db_sqldb() {
        test_cache_overflow_into_db::<crate::db::SqlDB>();
    }
    #[cfg(feature = "parity-db")]
    #[test]
    fn cache_overflow_into_db_paritydb() {
        test_cache_overflow_into_db::<crate::db::ParityDb>();
    }
    fn test_cache_overflow_into_db<D: DB + Default>() {
        let mut backend = StorageBackend::new(2, D::default());
        let (k1, d1) = (childless_hash::<D, u8>(&0), vec![0]);
        let (k2, d2) = (childless_hash::<D, u8>(&1), vec![1]);
        let (k3, d3) = (childless_hash::<D, u8>(&2), vec![2]);

        // Put all keys in cache.
        backend.cache(k1.clone(), d1.clone(), vec![]);
        backend.cache(k2.clone(), d2.clone(), vec![]);
        backend.cache(k3.clone(), d3.clone(), vec![]);
        assert!(backend.write_cache.peek(&k1).is_some());
        assert!(backend.write_cache.peek(&k2).is_some());
        assert!(backend.write_cache.peek(&k3).is_some());
        assert_eq!(backend.write_cache.len(), 3);

        // Flush lru key, i.e. k1, to db.
        backend.flush_cache_evictions_to_db();
        assert_eq!(backend.write_cache.len(), 2);

        // Check that k1 is no longer pending when cached, but k2 and k3 still
        // are.
        assert_eq!(backend.get(&k1).unwrap().data, d1);
        assert!(backend.write_cache.peek(&k1).is_none());
        assert!(backend.read_cache.peek(&k1).is_some());
        assert_eq!(backend.get(&k2).unwrap().data, d2);
        assert!(backend.write_cache.peek(&k2).is_some());
        assert_eq!(backend.get(&k3).unwrap().data, d3);
        assert!(backend.write_cache.peek(&k3).is_some());
    }

    fn in_database_repr<D: DB, T: Storable<D>>(val: T) -> (ArenaKey<D::Hasher>, std::vec::Vec<u8>) {
        let mut bytes: std::vec::Vec<u8> = std::vec::Vec::new();
        Storable::to_binary_repr(&val, &mut bytes)
            .expect("Failed to serialize to 'std::vec::Vec<u8>'!");
        let key = hash::<D>(&bytes, &val.children());
        (key, bytes)
    }

    #[test]
    fn storage_backend_inmemorydb() {
        test_storage_backend::<InMemoryDB>();
    }
    #[cfg(feature = "sqlite")]
    #[test]
    fn storage_backend_sqldb() {
        test_storage_backend::<crate::db::SqlDB>();
    }
    #[cfg(feature = "parity-db")]
    #[test]
    fn storage_backend_paritydb() {
        test_storage_backend::<crate::db::ParityDb>();
    }
    fn test_storage_backend<D: DB>() {
        let mut storage_backend = StorageBackend::new(16, D::default());
        let (key, bytes) = in_database_repr::<D, u32>(10);

        // Caching and retrieving an object
        storage_backend.cache(key.clone(), bytes.clone(), vec![]);
        assert_eq!(storage_backend.get(&key).unwrap().data, bytes);
        assert_eq!(storage_backend.get(&key).unwrap().ref_count, 0);
        assert_eq!(storage_backend.database.size(), 0);
        assert!(storage_backend.peek_from_memory(&key).unwrap().is_pending());
        storage_backend.pre_fetch(&key, None, true);
        assert!(storage_backend.write_cache.peek(&key).is_some());

        // Persisting an object, but not yet writing it to db
        storage_backend.persist(&key);
        assert!(storage_backend.peek_from_memory(&key).unwrap().is_pending());
        assert_eq!(storage_backend.get(&key).unwrap().ref_count, 0);
        assert_eq!(storage_backend.get_root_count(&key), 1);
        assert_eq!(storage_backend.database.size(), 0);

        // Writing persisted objects to db
        storage_backend.flush_all_changes_to_db();
        assert_eq!(storage_backend.database.size(), 1);
        assert!(storage_backend.write_cache.peek(&key).is_none());
        assert!(storage_backend.read_cache.peek(&key).is_some());
        assert!(storage_backend.database.get_node(&key).is_some());

        // Caching a duplicate object, a no-op in terms of mutation and ref counts
        storage_backend.uncache(&key);
        storage_backend.cache(key.clone(), bytes.clone(), vec![]);
        assert_eq!(storage_backend.get(&key).unwrap().data, bytes);
        assert_eq!(storage_backend.get(&key).unwrap().ref_count, 0);
        assert_eq!(storage_backend.get_root_count(&key), 1);
        assert!(!storage_backend.peek_from_memory(&key).unwrap().is_pending());

        // Flush cache, go to DB for object
        storage_backend.flush_all_changes_to_db();
        storage_backend.write_cache.clear();
        assert_eq!(storage_backend.get(&key).unwrap().ref_count, 0);
        assert_eq!(storage_backend.get_root_count(&key), 1);
        storage_backend.write_cache.clear();
        assert!(!storage_backend.peek_from_memory(&key).unwrap().is_pending());

        // Persist and unpersist an object twice. The object has already been
        // persisted once. First we persist and unpersist in memory, which has no
        // effect on db.
        storage_backend.persist(&key);
        assert_eq!(storage_backend.database.size(), 1);
        assert_eq!(storage_backend.get(&key).unwrap().ref_count, 0);
        assert_eq!(
            storage_backend.database.get_node(&key).unwrap().ref_count,
            0
        );
        assert_eq!(storage_backend.get_root_count(&key), 2);
        storage_backend.unpersist(&key);
        assert_eq!(storage_backend.get_root_count(&key), 1);
        storage_backend.uncache(&key);

        // Second we unpersist a second time, and see that the object goes away.
        storage_backend.unpersist(&key);
        assert_eq!(storage_backend.get(&key).unwrap().ref_count, 0);
        assert_eq!(storage_backend.get_root_count(&key), 0);
        storage_backend.flush_all_changes_to_db();
        storage_backend.gc();
        assert_eq!(storage_backend.database.size(), 0);
        assert!(storage_backend.get(&key).is_none());
    }

    // A dag with labeled nodes. This is in the style of arena stored dags, with
    // `Sp` child pointers and a `Storable` instance.
    #[derive(Debug, Storable)]
    #[derive_where(Clone)]
    #[storable(db = D)]
    struct LabeledNode<D: DB> {
        label: u32,
        children: std::vec::Vec<Sp<Self, D>>,
    }

    impl<D: DB> Eq for LabeledNode<D> {}

    impl<D: DB> PartialEq for LabeledNode<D> {
        fn eq(&self, other: &Self) -> bool {
            self.label == other.label && self.children == other.children
        }
    }

    #[test]
    fn storage_backend_trees_inmemorydb() {
        test_storage_backend_trees::<InMemoryDB>();
    }
    #[cfg(feature = "sqlite")]
    #[test]
    fn storage_backend_trees_sqldb() {
        test_storage_backend_trees::<crate::db::SqlDB>();
    }
    #[cfg(feature = "parity-db")]
    #[test]
    fn storage_backend_trees_paritydb() {
        test_storage_backend_trees::<crate::db::ParityDb>();
    }
    fn test_storage_backend_trees<D: DB>() {
        // The first half of this test computes various objects and keys, with
        // respect to a first storage. The second half starts over with a new
        // storage, but using the objects and keys from the first half.

        // First half.

        let storage = crate::Storage::<D>::default();
        let arena = &storage.arena;

        let child = LabeledNode {
            label: 0,
            children: vec![],
        };
        let parent = LabeledNode {
            label: 1,
            children: vec![arena.alloc(child.clone())],
        };
        let gp = LabeledNode {
            label: 2,
            children: vec![arena.alloc(parent.clone()), arena.alloc(child.clone())],
        };
        let (child_key, child_bytes) = in_database_repr(child.clone());
        let (parent_key, parent_bytes) = in_database_repr(parent.clone());
        let (_gp_key, gp_bytes) = in_database_repr(gp.clone());

        // Test that `Storable` is implemented correctly on `TestNode`.
        let child_reconstructed = <LabeledNode<D> as Storable<D>>::from_binary_repr(
            &mut child_bytes.clone().as_slice(),
            &mut vec![].into_iter(),
            &IrLoader::new(arena, &HashMap::new()),
        )
        .unwrap();
        assert_eq!(child_reconstructed, child);
        let all: HashMap<ArenaKey<D::Hasher>, IntermediateRepr<D>> = HashMap::from([(
            child_key.clone(),
            IntermediateRepr::<D>::from_storable(&child),
        )]);
        let parent_reconstructed = <LabeledNode<D> as Storable<D>>::from_binary_repr(
            &mut parent_bytes.clone().as_slice(),
            &mut vec![child_key.clone()].into_iter(),
            &IrLoader::new(arena, &all),
        )
        .unwrap();
        assert_eq!(parent_reconstructed, parent);
        let all: HashMap<ArenaKey<D::Hasher>, IntermediateRepr<D>> = HashMap::from([
            (child_key.clone(), IntermediateRepr::from_storable(&child)),
            (parent_key.clone(), IntermediateRepr::from_storable(&parent)),
        ]);
        let gp_reconstructed = <LabeledNode<D> as Storable<D>>::from_binary_repr(
            &mut gp_bytes.clone().as_slice(),
            &mut vec![parent_key.clone(), child_key.clone()].into_iter(),
            &IrLoader::new(arena, &all),
        )
        .unwrap();
        assert_eq!(gp_reconstructed, gp);

        // Second half.

        let mut storage_backend = StorageBackend::<D>::new(16, D::default());

        // Caching and retrieving an object
        storage_backend.cache(child_key.clone(), child_bytes.clone(), vec![]);
        storage_backend.cache(
            parent_key.clone(),
            parent_bytes.clone(),
            vec![child_key.clone()],
        );

        assert_eq!(storage_backend.get(&child_key).unwrap().data, child_bytes);
        assert_eq!(storage_backend.get(&parent_key).unwrap().data, parent_bytes);
        assert_eq!(storage_backend.get(&child_key).unwrap().ref_count, 1);
        assert_eq!(storage_backend.get(&parent_key).unwrap().ref_count, 0);
        assert_eq!(storage_backend.database.size(), 0);
        assert!(storage_backend.write_cache.peek(&child_key).is_some());
        assert!(storage_backend.write_cache.peek(&parent_key).is_some());

        // Persisting a parent object
        storage_backend.persist(&parent_key);
        assert_eq!(storage_backend.get(&child_key).unwrap().ref_count, 1);
        assert_eq!(storage_backend.get(&parent_key).unwrap().ref_count, 0);
        assert_eq!(storage_backend.get_root_count(&child_key), 0);
        assert_eq!(storage_backend.get_root_count(&parent_key), 1);
        assert_eq!(storage_backend.database.size(), 0);
        assert!(storage_backend.write_cache.peek(&parent_key).is_some());
        assert!(storage_backend.write_cache.peek(&child_key).is_some());

        // Writing persisted objects to db
        storage_backend.flush_all_changes_to_db();
        assert_eq!(storage_backend.database.size(), 2);
        assert!(storage_backend.write_cache.peek(&parent_key).is_none());
        assert!(storage_backend.read_cache.peek(&parent_key).is_some());
        assert!(storage_backend.write_cache.peek(&child_key).is_none());
        assert!(storage_backend.read_cache.peek(&child_key).is_some());

        // Caching a duplicate child object, no-op on ref counts and db
        storage_backend.uncache(&child_key);
        storage_backend.cache(child_key.clone(), child_bytes, vec![]);
        assert_eq!(storage_backend.get(&child_key).unwrap().ref_count, 1);
        assert!(
            !storage_backend
                .peek_from_memory(&child_key)
                .unwrap()
                .is_pending()
        );

        // Uncache the child object, no-op on ref counts
        storage_backend.uncache(&child_key);
        assert_eq!(storage_backend.get(&child_key).unwrap().ref_count, 1);

        // Unpersist the root object
        storage_backend.unpersist(&parent_key);
        storage_backend.uncache(&parent_key);
        assert_eq!(storage_backend.database.size(), 2);
        assert_eq!(storage_backend.get(&parent_key).unwrap().ref_count, 0);
        assert_eq!(storage_backend.get(&child_key).unwrap().ref_count, 1);
        assert_eq!(storage_backend.get_root_count(&parent_key), 0);
        assert_eq!(storage_backend.get_root_count(&child_key), 0);
        storage_backend.flush_all_changes_to_db();
        assert_eq!(storage_backend.database.size(), 2);
        storage_backend.gc();
        assert_eq!(storage_backend.database.size(), 0);
    }

    /// Test that reference counts and root counts are calculated correctly,
    /// that various orderings on `cache` and `uncache` are handled correctly,
    /// and that persisting an object to db in the middle of a sequence a ref
    /// and root count updates is handled correctly.
    #[test]
    fn ref_counting_inmemorydb() {
        test_ref_counting::<InMemoryDB>();
    }
    #[cfg(feature = "sqlite")]
    #[test]
    fn ref_counting_sqldb() {
        test_ref_counting::<crate::db::SqlDB>();
    }
    #[cfg(feature = "parity-db")]
    #[test]
    fn ref_counting_paritydb() {
        test_ref_counting::<crate::db::ParityDb>();
    }
    fn test_ref_counting<D: DB>() {
        // Arranging the nodes in layers, the variables names here are
        // `n<layer><column>` for the`column`th node in layer `layer`.
        let n41 = RawNode::new(&[4, 1], 4, vec![]);
        let n31 = RawNode::new(&[3, 1], 2, vec![&n41]);
        let n32 = RawNode::new(&[3, 2], 3, vec![&n41]);
        let n33 = RawNode::new(&[3, 3], 2, vec![&n41]);
        let n21 = RawNode::new(&[2, 1], 1, vec![&n31, &n32]);
        let n22 = RawNode::new(&[2, 2], 1, vec![&n32, &n33]);
        let n11 = RawNode::new(&[1, 1], 0, vec![&n41, &n31, &n32, &n33, &n21, &n22]);
        let nodes = [&n41, &n31, &n32, &n33, &n21, &n22, &n11];

        let cache_size = 16;
        let init_backend = || {
            let mut backend = StorageBackend::new(cache_size, D::default());
            for n in nodes {
                n.cache_into_backend(&mut backend);
            }
            backend
        };

        ////////////////////////////////////////////////////////////////
        // Build the graph only in memory, and uncache in cache-insertion order.
        ////////////////////////////////////////////////////////////////

        let mut backend = init_backend();
        for n in nodes {
            assert_eq!(backend.get(&n.key).unwrap().ref_count, n.ref_count);
            assert_eq!(backend.get_root_count(&n.key), 0);
        }
        // Uncache in cache-insertion order, except for root, and see that all nodes
        // remain unchanged.
        for n in [&n41, &n31, &n32, &n33, &n21, &n22] {
            backend.uncache(&n.key);
        }
        for n in nodes {
            assert_eq!(
                backend
                    .peek_from_memory(&n.key)
                    .unwrap()
                    .get_obj()
                    .ref_count,
                n.ref_count
            );
            assert_eq!(backend.get_root_count(&n.key), 0);
        }
        // Uncache the root and see that everything gets cascade deleted.
        backend.uncache(&n11.key);
        for n in nodes {
            assert!(backend.peek_from_memory(&n.key).is_none());
            assert_eq!(backend.get_root_count(&n.key), 0);
        }

        ////////////////////////////////////////////////////////////////
        // Build the graph only in memory, testing multiple `cache` and
        // `uncache` calls.
        ////////////////////////////////////////////////////////////////

        let mut backend = init_backend();
        // Uncache in cache-insertion order, except for root.
        for n in [&n41, &n31, &n32, &n33, &n21, &n22] {
            backend.uncache(&n.key);
        }
        // Re-cache a few nodes.
        n31.cache_into_backend(&mut backend);
        n33.cache_into_backend(&mut backend);
        // Uncache the root and see that as much as possible gets cascade
        // deleted, while things that are still referenced by `cache`d nodes
        // stick around.
        backend.uncache(&n11.key);
        for (n, r) in [(&n41, 2), (&n31, 0), (&n33, 0)] {
            assert_eq!(
                backend
                    .peek_from_memory(&n.key)
                    .unwrap()
                    .get_obj()
                    .ref_count,
                r
            );
            assert_eq!(backend.get_root_count(&n.key), 0);
        }
        for n in [&n32, &n21, &n22, &n11] {
            assert!(backend.get(&n.key).is_none());
            assert_eq!(backend.get_root_count(&n.key), 0);
        }
        // Now uncache the re-caches, and see that everything disappears.
        backend.uncache(&n31.key);
        backend.uncache(&n33.key);
        for n in nodes {
            assert!(backend.get(&n.key).is_none());
            assert_eq!(backend.get_root_count(&n.key), 0);
        }

        ////////////////////////////////////////////////////////////////
        // Build the graph only in memory, and uncache in
        // reverse-cache-insertion order.
        ////////////////////////////////////////////////////////////////

        let mut backend = init_backend();
        for n in nodes {
            assert_eq!(backend.get(&n.key).unwrap().ref_count, n.ref_count);
            assert_eq!(backend.get_root_count(&n.key), 0);
        }
        // Uncache in reverse-cache-insertion order, and see that all nodes get
        // removed as they're `uncache`d, but not before.
        let mut reversed_nodes = nodes;
        reversed_nodes.reverse();
        for (i, n) in reversed_nodes.iter().enumerate() {
            backend.uncache(&n.key);
            assert!(backend.peek_from_memory(&n.key).is_none());
            for m in &reversed_nodes[i + 1..] {
                assert!(backend.peek_from_memory(&m.key).is_some());
            }
            assert_eq!(backend.get_root_count(&n.key), 0);
        }

        ////////////////////////////////////////////////////////////////
        // Build the graph only in memory, persist the root, and uncache all.
        ////////////////////////////////////////////////////////////////

        let mut backend = init_backend();
        backend.persist(&n11.key);
        for (n, rt) in [
            (&n41, 0),
            (&n31, 0),
            (&n32, 0),
            (&n33, 0),
            (&n21, 0),
            (&n22, 0),
            (&n11, 1),
        ] {
            assert_eq!(backend.get(&n.key).unwrap().ref_count, n.ref_count);
            assert_eq!(backend.get_root_count(&n.key), rt);
        }
        // Uncache everything.
        for n in nodes {
            backend.uncache(&n.key);
        }
        // Confirm that nothing changed.
        for (n, rt) in [
            (&n41, 0),
            (&n31, 0),
            (&n32, 0),
            (&n33, 0),
            (&n21, 0),
            (&n22, 0),
            (&n11, 1),
        ] {
            assert_eq!(backend.get(&n.key).unwrap().ref_count, n.ref_count);
            assert_eq!(backend.get_root_count(&n.key), rt);
        }
        // Unpersist the root and see that everything gets deleted.
        backend.unpersist(&n11.key);
        for n in nodes {
            assert!(backend.peek_from_memory(&n.key).is_none());
            assert_eq!(backend.get_root_count(&n.key), 0);
        }

        ////////////////////////////////////////////////////////////////
        // Flush the graph to DB after each cache insertion.
        ////////////////////////////////////////////////////////////////

        // Can't use `init_backend` here because we init differently, flushing
        // after each cache insertion.
        let mut backend = StorageBackend::new(cache_size, D::default());
        for n in nodes {
            n.cache_into_backend(&mut backend);
            backend.flush_all_changes_to_db();
            assert!(backend.write_cache.peek(&n.key).is_none());
            assert!(backend.read_cache.peek(&n.key).is_some());
        }
        for n in nodes {
            assert_eq!(backend.get(&n.key).unwrap().ref_count, n.ref_count);
            assert_eq!(backend.get_root_count(&n.key), 0);
        }

        ////////////////////////////////////////////////////////////////
        // Partially flush root count and ref count updates to DB, and see that
        // correct values are computed by the backend.
        ////////////////////////////////////////////////////////////////

        // Can't use `init_backend` here because we init differently.
        let mut backend = StorageBackend::new(cache_size, D::default());
        for n in [&n41, &n31, &n32] {
            n.cache_into_backend(&mut backend);
            backend.persist(&n.key);
        }
        backend.flush_all_changes_to_db();
        for n in [&n33, &n21, &n22, &n11] {
            n.cache_into_backend(&mut backend);
            backend.persist(&n.key);
        }
        // Persist `i` additional times.
        for (i, n) in nodes.iter().enumerate() {
            for _ in 0..i {
                backend.persist(&n.key);
            }
        }
        let root_map = backend.get_roots();
        for (i, n) in nodes.iter().enumerate() {
            assert_eq!(backend.get(&n.key).unwrap().ref_count, n.ref_count);
            assert_eq!(backend.get_root_count(&n.key), (i + 1) as u32);
            assert_eq!(root_map.get(&n.key).cloned(), Some((i + 1) as u32));
        }
        assert_eq!(root_map.len(), nodes.len());
        // Unpersist `i+1` times, reducing all root counts to zero.
        for (i, n) in nodes.iter().enumerate() {
            for _ in 0..=i {
                backend.unpersist(&n.key);
            }
            assert_eq!(backend.get_root_count(&n.key), 0);
        }
        assert_eq!(backend.get_roots(), HashMap::new());
    }

    /// Test that `pre_fetch` via get fill the cache in traversal order, taking
    /// into account cache size limitations.
    #[test]
    fn pre_fetch_inmemorydb() {
        test_pre_fetch::<InMemoryDB>();
    }
    #[cfg(feature = "sqlite")]
    #[test]
    fn pre_fetch_sqldb() {
        test_pre_fetch::<crate::db::SqlDB>();
    }
    #[cfg(feature = "parity-db")]
    #[test]
    fn pre_fetch_paritydb() {
        test_pre_fetch::<crate::db::ParityDb>();
    }
    fn test_pre_fetch<D: DB>() {
        // Arranging the nodes in layers, the variables names here are
        // `n<layer><column>` for the`column`th node in layer `layer`.
        let n41 = RawNode::new(&[4, 1], 3, vec![]);
        let n31 = RawNode::new(&[3, 1], 1, vec![&n41]);
        let n32 = RawNode::new(&[3, 2], 2, vec![&n41]);
        let n33 = RawNode::new(&[3, 3], 1, vec![&n41]);
        let n21 = RawNode::new(&[2, 1], 1, vec![&n31, &n32]);
        let n22 = RawNode::new(&[2, 2], 1, vec![&n32, &n33]);
        let n11 = RawNode::new(&[1, 1], 0, vec![&n21, &n22]);

        let test = |cache_size: usize| {
            let mut backend = StorageBackend::new(cache_size, D::default());
            for n in [&n41, &n31, &n32, &n33, &n21, &n22, &n11] {
                n.cache_into_backend(&mut backend);
            }
            backend.flush_all_changes_to_db();
            backend.read_cache.clear();
            let max_depth = None;
            let truncate = false;
            backend.pre_fetch(&n11.key, max_depth, truncate);
            backend.get(&n11.key);
            let lru_keys: std::vec::Vec<_> =
                backend.read_cache.iter().map(|(k, _)| k.clone()).collect();
            let mut expected_keys: std::vec::Vec<_> = [&n11, &n21, &n22, &n31, &n32, &n33, &n41]
                .map(|n| n.key.clone())
                .into_iter()
                .collect();
            expected_keys.truncate(cache_size);
            assert_eq!(lru_keys, expected_keys);
        };
        test(1);
        test(3);
        test(7);
    }

    #[test]
    fn gc_inmemorydb() {
        test_gc::<InMemoryDB>();
    }
    #[cfg(feature = "sqlite")]
    #[test]
    fn gc_sqldb() {
        test_gc::<crate::db::SqlDB>();
    }
    #[cfg(feature = "parity-db")]
    #[test]
    fn gc_paritydb() {
        test_gc::<crate::db::ParityDb>();
    }
    fn test_gc<D: DB>() {
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
        let o_nodes = [&o31, &o32, &o21, &o11];

        let mk_backend = || {
            let cache_size = 100;
            StorageBackend::new(cache_size, D::default())
        };

        ////////////////////////////////////////////////////////////////
        // Cache all nodes into backend in memory, but don't flush, and then
        // uncache: all nodes should automatically be cleaned up by uncache, no
        // GC needed.

        let backend = &mut mk_backend();
        for n in n_nodes.iter().chain(o_nodes.iter()) {
            n.cache_into_backend(backend);
        }
        for n in n_nodes.iter().chain(o_nodes.iter()) {
            backend.uncache(&n.key);
        }

        assert_eq!(backend.database.size(), 0);
        assert_eq!(backend.read_cache.len(), 0);
        assert_eq!(backend.write_cache.len(), 0);

        ////////////////////////////////////////////////////////////////
        // Insert all nodes into DB, then GC: everything gets cleaned up.

        let backend = &mut mk_backend();
        for n in n_nodes.iter().chain(o_nodes.iter()) {
            n.cache_into_backend(backend);
        }
        backend.flush_all_changes_to_db();
        for n in n_nodes.iter().chain(o_nodes.iter()) {
            backend.uncache(&n.key);
        }
        assert_eq!(backend.database.size(), n_nodes.len() + o_nodes.len());

        backend.gc();
        assert_eq!(backend.database.size(), 0);
        assert_eq!(backend.read_cache.len(), 0);
        assert_eq!(backend.write_cache.len(), 0);

        ////////////////////////////////////////////////////////////////
        // Insert n nodes into DB and uncache them, then cache o nodes into
        // memory, then GC: only the o nodes and the n nodes reachable from them
        // remain.

        let backend = &mut mk_backend();
        for n in n_nodes {
            n.cache_into_backend(backend);
        }
        backend.flush_all_changes_to_db();
        for n in n_nodes {
            backend.uncache(&n.key);
        }
        for n in o_nodes {
            n.cache_into_backend(backend);
        }

        backend.gc();
        let reachable_n_nodes = [&n21, &n31, &n32, &n41, &n42, &n43, &n44];
        let unreachable_n_nodes = [&n11, &n22, &n33];

        for n in unreachable_n_nodes {
            assert!(backend.get(&n.key).is_none());
        }
        for n in o_nodes.iter().chain(reachable_n_nodes.iter()) {
            assert!(backend.get(&n.key).is_some());
        }
        for n in reachable_n_nodes {
            assert!(backend.database.get_node(&n.key).is_some());
        }

        ////////////////////////////////////////////////////////////////
        // Insert n nodes into DB, including marking some roots, and uncache
        // them. Cache o nodes into memory, mark some as roots, and uncache
        // them. Additionally mark and unmark some n nodes as roots in memory. Gc and see
        // what remains is what we expect.

        let backend = &mut mk_backend();
        for n in n_nodes {
            n.cache_into_backend(backend);
        }
        backend.persist(&n33.key);
        backend.persist(&n44.key);
        backend.flush_all_changes_to_db();
        for n in n_nodes {
            backend.uncache(&n.key);
        }

        for n in o_nodes {
            n.cache_into_backend(backend);
        }
        backend.persist(&o21.key);
        for n in o_nodes {
            backend.uncache(&n.key);
        }

        backend.unpersist(&n33.key); // Now a root in DB, but not a root in mem.
        backend.persist(&n31.key); // A root only in mem.
        backend.persist(&n44.key); // A root in DB and a double root in mem.

        backend.gc();

        let reachable_n_nodes = [&n31, &n41, &n42, &n44];
        let reachable_o_nodes = [&o21, &o31, &o32];
        let unreachable_n_nodes = [&n11, &n21, &n22, &n32, &n33, &n43];
        let unreachable_o_nodes = [&o11];
        for n in unreachable_n_nodes.iter().chain(unreachable_o_nodes.iter()) {
            assert!(backend.get(&n.key).is_none());
        }
        for n in reachable_o_nodes.iter().chain(reachable_n_nodes.iter()) {
            assert!(backend.get(&n.key).is_some());
        }
        for n in reachable_n_nodes {
            assert!(backend.database.get_node(&n.key).is_some());
        }
    }

    #[test]
    fn backend_stats() {
        let n1: RawNode = RawNode::new(&[1], 0, vec![]);
        let n2 = RawNode::new(&[2], 0, vec![]);
        let cache_size = 16;
        let db = InMemoryDB::default();
        let mut backend = StorageBackend::new(cache_size, db);

        let stats = backend.get_stats();
        assert_eq!(stats.get_cache_hits, 0);
        assert_eq!(stats.get_cache_misses, 0);

        n1.cache_into_backend(&mut backend);
        let _ = backend.get(&n1.key);
        let stats = backend.get_stats();
        assert_eq!(stats.get_cache_hits, 1);
        assert_eq!(stats.get_cache_misses, 0);

        let _ = backend.get(&n2.key);
        let stats = backend.get_stats();
        assert_eq!(stats.get_cache_hits, 1);
        assert_eq!(stats.get_cache_misses, 1);

        n2.insert_into_db(&mut backend.database);
        let _ = backend.get(&n2.key);
        let stats = backend.get_stats();
        assert_eq!(stats.get_cache_hits, 1);
        assert_eq!(stats.get_cache_misses, 2);

        let _ = backend.get(&n2.key);
        let stats = backend.get_stats();
        assert_eq!(stats.get_cache_hits, 2);
        assert_eq!(stats.get_cache_misses, 2);

        let _ = backend.get(&n1.key);
        let stats = backend.get_stats();
        assert_eq!(stats.get_cache_hits, 3);
        assert_eq!(stats.get_cache_misses, 2);
    }
}
