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

//! Traits for defining new storage mechanisms

use crate::DefaultDB;
use crate::Storable;
use crate::arena::{Arena, ArenaHash, Sp};
use crate::backend::StorageBackend;
use crate::db::{DB, DummyArbitrary, InMemoryDB};
use derive_where::derive_where;
use parking_lot::{Mutex, MutexGuard};
#[cfg(feature = "proptest")]
use proptest::arbitrary::Arbitrary;
#[cfg(feature = "proptest")]
use proptest::strategy::{BoxedStrategy, Strategy};
use std::any::{Any, TypeId};
use std::marker::PhantomData;
use std::ops::Deref;
use std::sync::{Arc, LazyLock};

/// The default size of the storage cache.
///
/// This size is in number of cache objects, not megabytes consumed! This value
/// is not well motivated, and we may want to change it later, or better yet,
/// refactor the back-end to track the memory size of the cache, instead of the
/// number of cached objects.
///
/// That said, it *has* been set based on observation, to a point where the cache consumes under a
/// gigabyte during typical operation.
pub const DEFAULT_CACHE_SIZE: usize = 10_000;

#[derive(Clone, Debug)]
/// A factory for various storage objects
pub struct Storage<D: DB = DefaultDB> {
    /// The inner storage arena
    pub arena: Arena<D>,
}

impl<D: DB> Storage<D> {
    /// Create a new Storage type with given cache size and db.
    ///
    /// If the `cache_size` is zero, then the `StorageBackend` caches will be
    /// unbounded. Otherwise, the read cache will be strictly bounded by
    /// `cache_size`, and the write cache will be truncated to at most that size
    /// on `StorageBackend` flush operations.
    ///
    /// Note: the cache size is in *number* of objects, not number of megabytes
    /// of memory! See [`self::DEFAULT_CACHE_SIZE`] for a default choice.
    pub fn new(cache_size: usize, db: D) -> Self {
        let arena = Arena::<D>::new_from_backend(StorageBackend::new(cache_size, db));
        Self { arena }
    }

    /// Create a new Storage type from an existing Arena
    pub fn new_from_arena(arena: Arena<D>) -> Self {
        Self { arena }
    }
}

impl<D: DB> Deref for Storage<D> {
    type Target = Arena<D>;
    fn deref(&self) -> &Arena<D> {
        &self.arena
    }
}

impl<D: Default + DB> Default for Storage<D> {
    /// Create a new storage with the default cache size.
    fn default() -> Self {
        Self::new(DEFAULT_CACHE_SIZE, D::default())
    }
}

type StorageMap = std::collections::HashMap<TypeId, Arc<dyn Any + Sync + Send>>;

/// Mutable global default `Storage<D>` keyed on DB type `D`.
static STORAGES: LazyLock<Mutex<StorageMap>> =
    LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));

/// Return the shared storage object for DB type `D`, panicking if none is
/// available.
///
/// Use `try_get_default_storage` instead, if you want to be able to recover
/// from a missing default storage. But the intended use of default storage is
/// that you set it with `set_default_storage` during program initialization,
/// and then assume it's set from that point on, and so crashing if it's not set
/// is expected in normal usage, as it indicates an initialization bug.
///
/// # Implicit initialization of `InMemoryDB` backed storage
///
/// When `D = InMemoryDB`, if the default storage is not initialized, then
/// instead of crashing we initialize it implicitly using
/// `InMemoryDB::default`. This is to avoid needing to write boilerplate storage
/// initialization code in tests, and is not expected to be used in production,
/// where other, actually persistent `DB`s are used to back the storage.
///
/// # Foot-gun
///
/// The default storage is defined per process, so in particular all threads in
/// a process share the same default storage at each storage type.
///
/// Because `cargo test` runs tests as different threads in the same process,
/// any tests relying on the default storage may interfere with each other. For
/// most tests this probably doesn't matter, but for tests of the storage
/// itself, we need isolation.
///
/// See [`WrappedDB`] for creating disjoint default `Storage`s for the same DB
/// type, e.g. for test isolation.
pub fn default_storage<D: DB + Any>() -> Arc<Storage<D>> {
    match try_get_default_storage() {
        Some(arc) => arc,
        _ => {
            if TypeId::of::<D>() == TypeId::of::<InMemoryDB>() {
                // Implicit initialization, but only for InMemoryDB backed storage!
                set_default_storage(Storage::<D>::default).unwrap_or_else(|s| s)
            } else {
                panic!(
                    "default storage is not set! you probably need to call set_default_storage in your initialization code"
                )
            }
        }
    }
}

/// Return `Some(default storage)` if initialized, and `None` otherwise.
///
/// In normal usage, you should call `default_storage` instead, because an unset
/// default storage is an initialization bug.
pub fn try_get_default_storage<D: DB + Any>() -> Option<Arc<Storage<D>>> {
    let storages = STORAGES.lock();
    try_get_default_storage_locked(&storages)
}

// Factored out `try_get_default_storage` logic, for reuse where the lock is
// already held.
fn try_get_default_storage_locked<D: DB + Any>(
    storages: &MutexGuard<StorageMap>,
) -> Option<Arc<Storage<D>>> {
    storages.get(&TypeId::of::<Storage<D>>()).map(|arc| {
        arc.clone()
            .downcast::<Storage<D>>()
            .expect("impossible: we only insert Storage<D>")
    })
}

/// Attempts to set the shared storage object for a given DB type.
///
/// This function is similar to
/// <https://doc.rust-lang.org/std/sync/struct.OnceLock.html#method.set>, except
/// that it takes a closure instead of a value. The semantics are:
///
/// - if the default storage is already set for `D`, then return `Err(<existing
///   value>)`
///
/// - if the default storage is not already set for `D`, then set it by calling
///   `mk_value` and return `Ok(<value just set>)`
///
/// Note: It is NOT an error when this function returns `Err(...)`, it just
/// means `mk_value` wasn't actually called. Most callers shouldn't care about
/// this distinction, but returning the `Result` allows the distinction to be
/// tracked if it matters. Normal callers are expected to ignore the result if
/// they're setting the default storage in a context where their init code runs
/// in multiple threads, e.g.
///
/// ```ignore
/// let _idontcare = set_default_storage(|| ...);
/// ```
///
/// or call `unwrap` on the result if they expect to be the only caller (since
/// failure will indicate a bug). If the caller wants the resulting storage, and
/// doesn't care where it came from, then they should call
/// `Result::unwrap_or_else(|s| s)` on the result.
pub fn set_default_storage<D: DB + Any>(
    mk_value: impl FnOnce() -> Storage<D>,
) -> Result<Arc<Storage<D>>, Arc<Storage<D>>> {
    let mut storages = STORAGES.lock();
    match try_get_default_storage_locked(&storages) {
        Some(arc) => Err(arc),
        _ => {
            let storage = mk_value();
            let arc = Arc::new(storage);
            storages.insert(TypeId::of::<Storage<D>>(), arc.clone());
            Ok(arc)
        }
    }
}

/// Clears the shared storage object for a given DB type.
///
/// Since default storage is a global resource shared across all threads,
/// calling this function may cause other threads to crash when they
/// subsequently try to look up the default storage. We don't expect this
/// function to be used in production, but we provide it just case. Callers will
/// need to provide their own synchronization, to for example avoid a race where
/// other threads try to access the default storage between calls to this
/// function and `set_default_storage`.
///
/// # Note
///
/// This function is not "unsafe" in the formal Rust sense of causing undefined
/// behavior if called incorrectly. The `unsafe_` prefix is just to help avoid
/// someone calling it without understanding the consequences.
pub fn unsafe_drop_default_storage<D: DB + Any>() {
    STORAGES.lock().remove(&TypeId::of::<Storage<D>>());
}

/// A tagged newtype wrapper for `DB`s, to support creating disjoint [default
/// storage]([`default_storage`]) `DB`s of the same type, concurrently.
///
/// Disjoint default storage for the same DB type are needed, for example, when
/// writing tests that need to run in isolation.
///
/// See `self::tests::persist_to_disk` and
/// `self::tests::test_default_storage` for example usage.
#[derive(Clone)]
#[derive_where(Debug; D)]
pub struct WrappedDB<D: DB, T> {
    db: D,
    tag: PhantomData<T>,
}

impl<D: DB, T> WrappedDB<D, T> {
    /// Create a new `WrappedDB` from a `DB`.
    pub fn wrap(db: D) -> Self {
        Self {
            db,
            tag: PhantomData,
        }
    }
}

impl<D: Default + DB, T> Default for WrappedDB<D, T> {
    fn default() -> Self {
        Self {
            db: Default::default(),
            tag: Default::default(),
        }
    }
}

/// A pass-thru implementation of `DB`.
///
/// # Foot-gun
///
/// If the `DB` trait ever grows another method with a default implementation,
/// we'll need to be sure to add the pass-thru here, to preserve any possibly
/// overriding implementations provided by the wrapped db.
impl<D: DB, T: Sync + Send + 'static> DB for WrappedDB<D, T> {
    type Hasher = D::Hasher;

    fn get_node(
        &self,
        key: &ArenaHash<Self::Hasher>,
    ) -> Option<crate::backend::OnDiskObject<Self::Hasher>> {
        self.db.get_node(key)
    }

    fn get_unreachable_keys(&self) -> std::vec::Vec<ArenaHash<Self::Hasher>> {
        self.db.get_unreachable_keys()
    }

    fn insert_node(
        &mut self,
        key: ArenaHash<Self::Hasher>,
        object: crate::backend::OnDiskObject<Self::Hasher>,
    ) {
        self.db.insert_node(key, object)
    }

    fn delete_node(&mut self, key: &ArenaHash<Self::Hasher>) {
        self.db.delete_node(key)
    }

    fn get_root_count(&self, key: &ArenaHash<Self::Hasher>) -> u32 {
        self.db.get_root_count(key)
    }

    fn set_root_count(&mut self, key: ArenaHash<Self::Hasher>, count: u32) {
        self.db.set_root_count(key, count)
    }

    fn get_roots(&self) -> std::collections::HashMap<ArenaHash<Self::Hasher>, u32> {
        self.db.get_roots()
    }

    fn size(&self) -> usize {
        self.db.size()
    }

    fn batch_update<I>(&mut self, iter: I)
    where
        I: Iterator<Item = (ArenaHash<Self::Hasher>, crate::db::Update<Self::Hasher>)>,
    {
        self.db.batch_update(iter)
    }

    fn batch_get_nodes<I>(
        &self,
        keys: I,
    ) -> std::vec::Vec<(
        ArenaHash<Self::Hasher>,
        Option<crate::backend::OnDiskObject<Self::Hasher>>,
    )>
    where
        I: Iterator<Item = ArenaHash<Self::Hasher>>,
    {
        self.db.batch_get_nodes(keys)
    }

    fn bfs_get_nodes<C>(
        &self,
        key: &ArenaHash<Self::Hasher>,
        cache_get: C,
        truncate: bool,
        max_depth: Option<usize>,
        max_count: Option<usize>,
    ) -> std::vec::Vec<(
        ArenaHash<Self::Hasher>,
        crate::backend::OnDiskObject<Self::Hasher>,
    )>
    where
        C: Fn(&ArenaHash<Self::Hasher>) -> Option<crate::backend::OnDiskObject<Self::Hasher>>,
    {
        self.db
            .bfs_get_nodes(key, cache_get, truncate, max_depth, max_count)
    }
}

#[cfg(feature = "proptest")]
/// A pass-thru implementation for `Arbitrary`.
impl<D: DB + Arbitrary, T> Arbitrary for WrappedDB<D, T> {
    type Parameters = D::Parameters;
    type Strategy = BoxedStrategy<Self>;

    fn arbitrary_with(params: Self::Parameters) -> Self::Strategy {
        D::arbitrary_with(params)
            .prop_map(|db| WrappedDB {
                db,
                tag: PhantomData,
            })
            .boxed()
    }
}

impl<D: DB + DummyArbitrary, T> DummyArbitrary for WrappedDB<D, T> {}

impl<T: serde::Serialize + Storable<D>, D: DB> serde::Serialize for Sp<T, D> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        <T as serde::Serialize>::serialize(self, serializer)
    }
}

impl<'a, T: serde::Deserialize<'a> + Storable<D>, D: DB> serde::Deserialize<'a> for Sp<T, D> {
    fn deserialize<D2>(deserializer: D2) -> Result<Self, D2::Error>
    where
        D2: serde::Deserializer<'a>,
    {
        T::deserialize(deserializer).map(Sp::new)
    }
}
