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
//! An [`Arena`] for storing Merkle-ized data structures in
//! memory, persisting them to disk, and reloading them from disk.
//!
//! Arena objects are content-addressed by [`ArenaKey`] hashes, and managed via
//! [`Sp`] smart pointers that track in-memory references. See [`StorageBackend`]
//! for the persistence internals, and assumptions about the interaction between
//! the arena and back-end.
use crate::storable::Loader;
use crate::storage::{DEFAULT_CACHE_SIZE, default_storage};
use crate::{DefaultDB, DefaultHasher, backend::StorageBackend, db::DB, storable::Storable};
use base_crypto::hash::PERSISTENT_HASH_BYTES;
use crypto::digest::{Digest, OutputSizeUser, crypto_common::generic_array::GenericArray};
use derive_where::derive_where;
use hex::ToHex;
use parking_lot::{ReentrantMutex as SyncMutex, ReentrantMutexGuard as MutexGuard};
use rand::distributions::{Distribution, Standard};
use serialize::{self, Deserializable, Serializable, Tagged};
use std::any::TypeId;
use std::cell::RefCell;
use std::fmt::Display;
use std::io;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::OnceLock;
use std::{
    any::Any,
    collections::{HashMap, HashSet},
    fmt::Debug,
    hash::Hash,
    io::Read,
    ops::Deref,
    sync::Arc,
};

pub(crate) fn hash<D: DB>(
    root_binary_repr: &std::vec::Vec<u8>,
    child_hashes: &std::vec::Vec<ArenaKey<D::Hasher>>,
) -> ArenaKey<D::Hasher> {
    let mut hasher = D::Hasher::default();
    hasher.update((root_binary_repr.len() as u32).to_le_bytes());
    hasher.update(root_binary_repr);

    for c in child_hashes {
        hasher.update(c.0.clone())
    }

    ArenaKey(hasher.finalize())
}

/// A wrapped `ArenaKey` which includes a tag indicating the content's data type.
/// The tag and key are left intentionally opaque to the end user to reduce the
/// possibility of mishandling the embedded tag.
#[derive_where(Debug, Clone, PartialEq, Eq, Ord, PartialOrd, Default)]
#[derive(Serializable)]
#[phantom(T, H)]
pub struct TypedArenaKey<T, H: Digest> {
    key: ArenaKey<H>,
    _phantom: PhantomData<T>,
}

impl<T, H: Digest> From<TypedArenaKey<T, H>> for ArenaKey<H> {
    fn from(val: TypedArenaKey<T, H>) -> Self {
        val.key
    }
}

impl<T, H: Digest> From<ArenaKey<H>> for TypedArenaKey<T, H> {
    fn from(val: ArenaKey<H>) -> Self {
        TypedArenaKey {
            key: val,
            _phantom: PhantomData,
        }
    }
}

impl<T: Tagged, H: Digest> Tagged for TypedArenaKey<T, H> {
    fn tag() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Owned(format!("storage-key({})", T::tag()))
    }
    fn tag_unique_factor() -> String {
        "storage-key".into()
    }
}

/// The key used in the `HashMap` in the Arena. Parameterised on the hash function
/// being used by the arena.
#[derive_where(Clone, PartialEq, Eq, Ord, PartialOrd, Default)]
pub struct ArenaKey<H: Digest = DefaultHasher>(
    pub GenericArray<u8, <H as OutputSizeUser>::OutputSize>,
);

impl<H: Digest> Tagged for ArenaKey<H> {
    fn tag() -> std::borrow::Cow<'static, str> {
        "storage-key".into()
    }
    fn tag_unique_factor() -> String {
        "storage-key".into()
    }
}

impl<D: DB> Storable<D> for ArenaKey<D::Hasher> {
    fn children(&self) -> std::vec::Vec<ArenaKey<<D as DB>::Hasher>> {
        std::vec::Vec::new()
    }

    fn to_binary_repr<W: std::io::Write>(&self, writer: &mut W) -> Result<(), std::io::Error>
    where
        Self: Sized,
    {
        writer.write_all(&self.0)?;
        Ok(())
    }

    fn from_binary_repr<R: std::io::Read>(
        reader: &mut R,
        _child_hashes: &mut impl Iterator<Item = ArenaKey<<D as DB>::Hasher>>,
        _loader: &impl Loader<D>,
    ) -> Result<Self, std::io::Error>
    where
        Self: Sized,
    {
        let mut array = GenericArray::<u8, <D::Hasher as OutputSizeUser>::OutputSize>::default();
        reader.read_exact(&mut array)?;
        Ok(Self(array))
    }
}

// impl<H: Digest + 'static> WellBehaved for ArenaKey<H> {}

impl<H: Digest> Debug for ArenaKey<H> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.encode_hex::<String>())
    }
}

impl<D: Digest> Hash for ArenaKey<D> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash::<H>(state)
    }

    fn hash_slice<H: std::hash::Hasher>(data: &[Self], state: &mut H)
    where
        Self: Sized,
    {
        GenericArray::<u8, <D as OutputSizeUser>::OutputSize>::hash_slice(
            data.iter()
                .map(|k| k.0.clone())
                .collect::<std::vec::Vec<GenericArray<u8, <D as OutputSizeUser>::OutputSize>>>()
                .as_slice(),
            state,
        )
    }
}

// Possible optimization: the key length is always equal to
// `<H as OutputSizeUser>::output_size()`, so we don't actually need
// to encode it in the serialization. This would reduce the keys from
// 36 to 32 bytes in the common case.
impl<H: Digest> Serializable for ArenaKey<H> {
    fn serialize(&self, writer: &mut impl std::io::Write) -> std::io::Result<()> {
        writer.write_all(&self.0[..])
    }

    fn serialized_size(&self) -> usize {
        <H as Digest>::output_size()
    }
}

impl<H: Digest> Deserializable for ArenaKey<H> {
    fn deserialize(
        reader: &mut impl std::io::Read,
        _recursive_depth: u32,
    ) -> std::io::Result<Self> {
        let mut res = vec![0u8; <H as Digest>::output_size()];
        reader.read_exact(&mut res[..])?;
        Ok(ArenaKey(GenericArray::clone_from_slice(&res)))
    }
}

impl<H: Digest> Distribution<ArenaKey<H>> for Standard {
    fn sample<R: rand::prelude::Rng + ?Sized>(&self, rng: &mut R) -> ArenaKey<H> {
        let mut bytes = GenericArray::default();
        rng.fill_bytes(&mut bytes);
        ArenaKey(bytes)
    }
}

impl<H: Digest> serde::Serialize for ArenaKey<H> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(&self.0[..])
    }
}

impl<'de, H: Digest> serde::Deserialize<'de> for ArenaKey<H> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ArenaKeyVisitor<H: Digest>(std::marker::PhantomData<H>);

        impl<'de, H: Digest> serde::de::Visitor<'de> for ArenaKeyVisitor<H> {
            type Value = ArenaKey<H>;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                write!(
                    formatter,
                    "a byte array of length {}",
                    <H as Digest>::output_size()
                )
            }

            fn visit_bytes<E: serde::de::Error>(self, v: &[u8]) -> Result<Self::Value, E> {
                if v.len() != <H as Digest>::output_size() {
                    return Err(E::invalid_length(v.len(), &self));
                }
                Ok(ArenaKey(GenericArray::clone_from_slice(v)))
            }

            fn visit_byte_buf<E: serde::de::Error>(self, v: Vec<u8>) -> Result<Self::Value, E> {
                self.visit_bytes(&v)
            }
        }

        deserializer.deserialize_bytes(ArenaKeyVisitor(std::marker::PhantomData))
    }
}

impl<H: Digest> ArenaKey<H> {
    /// Create an `ArenaKey` from bytes.
    ///
    /// Useful for `printf` debugging of tests.
    ///
    /// The bytes don't need to be as long as the key's internal byte array; the
    /// unspecified values will be filled in with zeros.
    pub(crate) fn _from_bytes(bs: &[u8]) -> Self {
        let mut bytes = GenericArray::default();
        for (i, b) in bs.iter().enumerate() {
            bytes[i] = *b;
        }
        ArenaKey(bytes)
    }
}

/// A tree of nodes stored in a map from hashes to values.
///
/// The `Arena` keeps an in-memory metadata map for the `Sp` wrapped objects it
/// manages. This metadata includes in-memory reference counts to these `Sp`
/// pointers, and should not be confused with the
/// [`crate::backend::OnDiskObject`] reference counts, which are concerned with
/// parent->child relationships in recursive Merkle-ized data structures.
///
/// # Pub access to `Arena` objects
///
/// There is no `pub` API for constructing arenas. Rather, library consumers are
/// expected to construct a [`crate::storage::Storage`] and access the arena via
/// the `.arena` field.
///
/// # Assumptions
///
/// The `backend` assumes there is a single family of `Arena`s, which are all
/// clones of the same initial arena, manipulating it. Specifically, the
/// assumption is that the same object will never be
/// [`StorageBackend::cache`]d more than once, which is guaranteed as long as
/// all the arenas share the same `metadata` structure, which is true if they
/// are clones.
///
/// # Developer note: Lock acquisition ordering
///
/// To avoid deadlocks, code that holds more than one `Arena` lock must always
/// acquire the locks in the same order that they're declared here. For example,
/// code that will hold the `metadata` and `backend` locks at the same time must
/// always acquire the `metadata` lock before attempting to acquire the
/// `backend` lock.
#[derive(Debug)]
#[derive_where(Clone)]
pub struct Arena<D: DB = DefaultDB> {
    metadata: Arc<SyncMutex<RefCell<MetaData<D>>>>,
    /// Cache of `Sp` data `Arc`s for sharing.
    ///
    /// Stored as weak references, so that data drops automatically when all
    /// referencing `Sp`s go out of scope or unload their data.
    ///
    /// # Invariant
    ///
    /// The code in this module that cleans up these weak pointers assumes that
    /// the `Arc`s for these weak pointers are only stored in `Sp` objects. If
    /// that's not true, e.g. if these `Arc`s were to leak out via pub APIs,
    /// then they may never be removed from the `sp_cache` after being dropped
    /// elsewhere, since the only time we attempt to clean up the `sp_cache` is
    /// in `Sp::drop`. That would cause a memory leak.
    ///
    /// # Invariant
    ///
    /// If there is a non-dangling pointer `p` in this `Sp` cache, then the
    /// root key for that pointer, i.e. `p.upgrade().unwrap().root` must be a
    /// key in `Self::metadata`. This allows us to use cache values in short
    /// cutting the construction of `Sp`s, without having to worry about calling
    /// `Self::track_locked`.
    sp_cache: Arc<SyncMutex<RefCell<SpCache<D>>>>,
    backend: Arc<SyncMutex<RefCell<StorageBackend<D>>>>,
}

impl<D: DB> Default for Arena<D> {
    fn default() -> Self {
        Self::new_from_backend(StorageBackend::<D>::new(DEFAULT_CACHE_SIZE, D::default()))
    }
}

/// The `metadata` is purely concerned with hash related metadata, so unlike for
/// the `sp_cache`, we don't care about ambiguity between hashes and types,
/// i.e. keying on the hash here is sufficient.
type MetaData<D> = HashMap<ArenaKey<<D as DB>::Hasher>, Node>;

/// An `ArenaKey` together with a type, to avoid collisions when keying typed
/// data by its hash: different types need not have disjoint hashes, and so we
/// need to include the type in the key to avoid collisions in some cases.
type DynTypedArenaKey<H> = (ArenaKey<H>, TypeId);

/// Keys are `hash x type_id` because the hash alone is ambiguous for
/// determining the typed value with this hash: the hash is determined only by
/// the binary serialization, which need not be disjoint across types.
type SpCache<D> =
    HashMap<DynTypedArenaKey<<D as DB>::Hasher>, std::sync::Weak<dyn Any + Sync + Send>>;

#[allow(clippy::type_complexity)]
impl<D: DB> Arena<D> {
    #[allow(clippy::type_complexity)]
    fn lock_metadata(&self) -> MutexGuard<'_, RefCell<MetaData<D>>> {
        self.metadata.lock()
    }

    fn lock_backend(&self) -> MutexGuard<'_, RefCell<StorageBackend<D>>> {
        self.backend.lock()
    }

    fn lock_sp_cache(&self) -> MutexGuard<'_, RefCell<SpCache<D>>> {
        self.sp_cache.lock()
    }

    /// Create a new Arena.
    ///
    /// # Note
    ///
    /// This is not `pub` because library users are expected to access the arena
    /// via the `.arena` field of a [`crate::storage::Storage`] object.
    pub(crate) fn new_from_backend(backend: StorageBackend<D>) -> Self {
        Arena {
            backend: Arc::new(SyncMutex::new(RefCell::new(backend))),
            metadata: Arc::new(SyncMutex::new(RefCell::new(HashMap::new()))),
            sp_cache: Arc::new(SyncMutex::new(RefCell::new(HashMap::new()))),
        }
    }

    /// Apply a function to the back-end.
    ///
    /// This is the only `pub` way to access the back-end, since safe use of the
    /// back-end requires locking.
    pub fn with_backend<R>(&self, f: impl FnOnce(&mut StorageBackend<D>) -> R) -> R {
        f(&mut RefCell::borrow_mut(&self.lock_backend()))
    }

    fn alloc_locked<T: Storable<D>>(
        &self,
        metadata: &mut MutexGuard<'_, RefCell<MetaData<D>>>,
        value: T,
        children: std::vec::Vec<ArenaKey<D::Hasher>>,
    ) -> Sp<T, D> {
        let mut bytes: std::vec::Vec<u8> = std::vec::Vec::new();
        value
            .to_binary_repr(&mut bytes)
            .expect("Storable data should be able to be represented in binary");
        let root_hash = hash::<D>(&bytes, &children);
        self.new_sp_locked(metadata, value, root_hash, bytes, children)
    }

    /// Insert `value` into the arena, and cache its data in the back-end until
    /// all `Sp`s for this data are dropped.
    pub fn alloc<T: Storable<D>>(&self, value: T) -> Sp<T, D> {
        let children = value.children();
        assert!(
            children.len() <= 16,
            "In order to represent the arena as an MPT Storable values must have no more than 16 children (found: {} on type {})",
            children.len(),
            std::any::type_name::<T>(),
        );
        self.alloc_locked(&mut self.lock_metadata(), value, children)
    }

    /// Create a new `Sp`, taking care of tracking, caching, and ref counting.
    fn new_sp_locked<T: Storable<D>>(
        &self,
        metadata: &mut MutexGuard<'_, RefCell<MetaData<D>>>,
        value: T,
        key: ArenaKey<D::Hasher>,
        data: std::vec::Vec<u8>,
        children: std::vec::Vec<ArenaKey<D::Hasher>>,
    ) -> Sp<T, D> {
        self.track_locked(metadata, key.clone(), data, children);
        // Try to reuse any existing cached `Arc` for `value`, creating and
        // caching a new `Arc` if necessary.
        let arc = {
            let guard = &self.lock_sp_cache();
            match self.read_sp_cache_locked(guard, &key) {
                Some(arc) => arc,
                None => {
                    let arc = Arc::new(value);
                    self.write_sp_cache_locked(guard, key.clone(), arc.clone());
                    arc
                }
            }
        };
        Sp::eager(self.clone(), key, arc)
    }

    fn new_sp<T: Storable<D>>(
        &self,
        value: T,
        key: ArenaKey<D::Hasher>,
        data: std::vec::Vec<u8>,
        children: std::vec::Vec<ArenaKey<D::Hasher>>,
    ) -> Sp<T, D> {
        self.new_sp_locked(&mut self.lock_metadata(), value, key, data, children)
    }

    /// Invariant: any `key` that returns `Some` here must also be present in
    /// `Self::metadata`.
    ///
    /// Note: at some call sites callers need to hold the metadata lock when
    /// calling this function and until they've used the returned `Arc`, to
    /// avoid the above invariant being violated by another thread
    /// concurrently. A possible refactor would be to make this function take
    /// the metadata lock as well.
    fn read_sp_cache_locked<T: Sync + Send + Any>(
        &self,
        sp_cache: &MutexGuard<RefCell<SpCache<D>>>,
        key: &ArenaKey<D::Hasher>,
    ) -> Option<Arc<T>> {
        let type_id = TypeId::of::<T>();
        let cache_key = (key.clone(), type_id);
        let sp_cache = RefCell::borrow(sp_cache);
        sp_cache
            .get(&cache_key)
            .and_then(|weak| weak.upgrade())
            // The `downcast` is safe because we only insert `Arc`s of type `T`.
            .map(|arc| arc.clone().downcast::<T>().unwrap())
    }

    /// Invariant: any `key` written to this cache must already be present in
    /// `Self::metadata`.
    fn write_sp_cache_locked<T: Storable<D>>(
        &self,
        sp_cache: &MutexGuard<RefCell<SpCache<D>>>,
        key: ArenaKey<D::Hasher>,
        value: Arc<T>,
    ) {
        let type_id = TypeId::of::<T>();
        let cache_key = (key, type_id);
        // Upcast.
        let arc: Arc<dyn Any + Send + Sync> = value;
        RefCell::borrow_mut(sp_cache).insert(cache_key, Arc::downgrade(&arc));
    }

    /// Returns the number of unique elements stored in the Arena
    pub fn size(&self) -> usize {
        self.lock_metadata().borrow().len()
    }

    /// Try to build an eager Sp from the Sp cache, returning `None` if the
    /// needed object is not available.
    fn get_from_cache<T: Storable<D>>(&self, key: &ArenaKey<D::Hasher>) -> Option<Sp<T, D>> {
        // Hold the metadata lock so we can hold the Sp cache lock while calling
        // Sp::eager, which itself acquires the metadata lock. We need to be
        // careful to avoid a race where someone else clears our arc from the Sp
        // cache before we're able to call Sp::eager, which takes care of
        // updating the metadata.
        //
        // Mistakes have been made in the past:
        // https://github.com/midnightntwrk/midnight-ledger-prototype/pull/401
        let _metadata_lock = self.lock_metadata();
        let sp_cache_lock = self.lock_sp_cache();
        self.read_sp_cache_locked::<T>(&sp_cache_lock, key)
            .map(|arc| Sp::eager(self.clone(), key.clone(), arc))
    }

    /// Get a pointer into the arena.
    ///
    /// This attempts to load the value eagerly, but will fall back on any
    /// existing cached value if available, regardless of whether that value is
    /// fully forced or not. Will return an Err if the `protocol_version` in the key
    /// does not match that of the Arena.
    ///
    /// # Warning
    ///
    /// This function may perform unbounded recursion, to the depth of the
    /// deepest nesting of `Sp`s contained in the result, since it works by
    /// recursing down to the leaves and building DAG up from there. If this is
    /// not acceptable, then use [`Self::get_lazy`] instead, which has no
    /// unbounded recursion, and instead loads nested `Sp`s on demand.
    pub fn get<T: Storable<D>>(
        &self,
        key: &TypedArenaKey<T, D::Hasher>,
    ) -> Result<Sp<T, D>, std::io::Error> {
        self.get_unversioned(&key.key)
    }

    pub(crate) fn get_unversioned<T: Storable<D>>(
        &self,
        key: &ArenaKey<D::Hasher>,
    ) -> Result<Sp<T, D>, std::io::Error> {
        let max_depth = None;
        Sp::<T, D>::from_arena(self, key, max_depth)
    }

    /// Retrieves the child keys of a given key.
    pub fn children(
        &self,
        key: &ArenaKey<D::Hasher>,
    ) -> Result<Vec<ArenaKey<D::Hasher>>, io::Error> {
        Ok(self
            .lock_backend()
            .borrow_mut()
            .get(key)
            .ok_or(io::Error::new(
                io::ErrorKind::NotFound,
                format!("BackendLoader::get(): key {key:?} not in storage arena. Are you sure you persisted this key or one of its ancestors?"),
            ))?
            .children.clone())
    }

    /// Get a pointer into the arena.
    ///
    /// This attempts to load the value lazily, but will fall back on any
    /// existing cached value if available, regardless of whether that value is
    /// fully forced or not. Will return an Err if the `protocol_version` in the key
    /// does not match that of the Arena.
    pub fn get_lazy<T: Storable<D> + Tagged>(
        &self,
        key: &TypedArenaKey<T, D::Hasher>,
    ) -> Result<Sp<T, D>, std::io::Error> {
        self.get_lazy_unversioned(&key.key)
    }

    pub(crate) fn get_lazy_unversioned<T: Storable<D>>(
        &self,
        key: &ArenaKey<D::Hasher>,
    ) -> Result<Sp<T, D>, std::io::Error> {
        let max_depth = Some(0);
        Sp::<T, D>::from_arena(self, key, max_depth)
    }

    /// Here "tracked" means that the node is in the metadata map, and has been
    /// `cache`d into the back-end. It's up to `Sp::drop` to
    /// `StorageBackend::uncache` tracked objects when the last `Sp` pointing to
    /// them goes out of scope.
    ///
    /// This is a no-op if `key` is already tracked.
    fn track_locked(
        &self,
        metadata: &MutexGuard<'_, RefCell<MetaData<D>>>,
        key: ArenaKey<D::Hasher>,
        data: std::vec::Vec<u8>,
        children: std::vec::Vec<ArenaKey<D::Hasher>>,
    ) {
        if !RefCell::borrow(metadata).contains_key(&key) {
            RefCell::borrow_mut(&self.lock_backend()).cache(key.clone(), data, children);
            RefCell::borrow_mut(metadata).insert(key, Node::new());
        }
    }

    /// Removes an object from the in-memory arena, remaining in back-end
    /// database if persisted or referenced.
    fn remove_locked(
        &self,
        metadata: &mut MutexGuard<'_, RefCell<MetaData<D>>>,
        key: &ArenaKey<D::Hasher>,
    ) {
        RefCell::borrow_mut(metadata).remove(key);
        RefCell::borrow_mut(&self.lock_backend()).uncache(key);
    }

    fn decrement_ref_locked(
        &self,
        metadata: &mut MutexGuard<'_, RefCell<MetaData<D>>>,
        key: &ArenaKey<D::Hasher>,
    ) {
        let mut remove = None;

        if let Some(v) = RefCell::borrow_mut(metadata).get_mut(key) {
            v.ref_count -= 1;
            if v.ref_count == 0 {
                remove = Some(key);
            }
        }

        if let Some(key) = remove {
            self.remove_locked(metadata, key);
        }
    }

    fn decrement_ref(&self, key: &ArenaKey<D::Hasher>) {
        self.decrement_ref_locked(&mut self.lock_metadata(), key);
    }

    fn increment_ref_locked(
        &self,
        metadata: &mut MutexGuard<'_, RefCell<MetaData<D>>>,
        key: &ArenaKey<D::Hasher>,
    ) {
        RefCell::borrow_mut(metadata)
            .get_mut(key)
            .expect("attempted to increment non-existant ref")
            .ref_count += 1;
    }

    fn increment_ref(&self, key: &ArenaKey<D::Hasher>) {
        self.increment_ref_locked(&mut self.lock_metadata(), key)
    }

    /// Deserializes an SP.
    ///
    /// # Note
    ///
    /// This is a boundary for user controlled input, namely the serialization
    /// in `reader`. So we need to be careful here to gracefully handle
    /// malformed (or even maliciously formed?) input. In contrast, the
    /// `StorageBackend` assumes its inputs are sanitized, and panics when
    /// receiving malformed inputs (e.g. parents with pointers to non-existent
    /// children).
    #[inline(always)]
    pub fn deserialize_sp<T: Storable<D>, R: Read>(
        &self,
        reader: &mut R,
        recursive_depth: u32,
    ) -> Result<Sp<T, D>, std::io::Error> {
        let nodes: TopoSortedNodes = Deserializable::deserialize(reader, recursive_depth)?;
        let mut existing_nodes: Vec<IntermediateRepr<D>> = Vec::with_capacity(nodes.nodes.len());
        fn idx_existing_nodes<D: DB>(
            n: &[IntermediateRepr<D>],
            i: u64,
        ) -> std::io::Result<&IntermediateRepr<D>> {
            if i < n.len() as u64 {
                Ok(&n[i as usize])
            } else {
                Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "error deserializing storage graph: child node index {i} out of range of processed nodes {}",
                        n.len()
                    ),
                ))
            }
        }

        let mut result = Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no nodes",
        ));

        for node in nodes.nodes.iter() {
            let children = node
                .child_indices
                .iter()
                .map(|i| {
                    idx_existing_nodes(&existing_nodes, *i)
                        .map(|n| hash::<D>(&n.binary_repr, &n.children))
                })
                .collect::<Result<Vec<_>, _>>()?;
            let root = hash::<D>(&node.data, &children);
            let ir: IntermediateRepr<D> = IntermediateRepr {
                binary_repr: node.data.clone(),
                children,
                db_type: PhantomData,
            };
            existing_nodes.push(ir);
            result = Ok(root);
        }

        let key = result?;
        let res: Sp<T, D> = IrLoader {
            arena: self,
            all: &existing_nodes
                .into_iter()
                .map(|node| (hash::<D>(&node.binary_repr, &node.children), node))
                .collect(),
            recursion_depth: recursive_depth,
            visited: Rc::new(RefCell::new(HashSet::new())),
        }
        .get(&key)?;
        if nodes == res.serialize_to_node_list() {
            Ok(res)
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "deserialized storage graph not in normal form",
            ))
        }
    }
}

/// A `Loader` that loads by deserializing binary data from the back-end, with an
/// optional depth bound that allows for lazy loading of nested `Sp`s.
///
/// Note that the `max_depth` is only a limit on recursion, but there is no
/// guarantee that exactly this many levels will be loaded in the result: this
/// is because we load from the `Arena::sp_cache` when possible, and have no
/// control over what we find there.
pub struct BackendLoader<'a, D: DB> {
    arena: &'a Arena<D>,
    max_depth: Option<usize>,
    recursion_depth: u32,
}

#[cfg(feature = "proptest")]
impl<'a, D: DB> BackendLoader<'a, D> {
    /// Construct a new `BackendLoader`
    pub fn new(arena: &'a Arena<D>, max_depth: Option<usize>) -> Self {
        BackendLoader {
            arena,
            max_depth,
            recursion_depth: 0,
        }
    }
}

impl<D: DB> Loader<D> for BackendLoader<'_, D> {
    const CHECK_INVARIANTS: bool = false;

    fn get<T: Storable<D>>(
        &self,
        key: &ArenaKey<<D as DB>::Hasher>,
    ) -> Result<Sp<T, D>, std::io::Error> {
        // Build from existing cached value if possible.

        // Avoid race: keep the metadata locked until we call `Sp::eager` //
        // below, so that no one can sneak in and remove `key` from the
        // metadata in the mean time.
        let metadata_lock = self.arena.lock_metadata();
        let maybe_arc = self
            .arena
            .read_sp_cache_locked(&self.arena.lock_sp_cache(), key);
        if let Some(arc) = maybe_arc {
            return Ok(Sp::eager(self.arena.clone(), key.clone(), arc));
        }
        drop(metadata_lock);

        // Otherwise, deserialize new sp from backend.

        let obj = self
            .arena
            .lock_backend()
            .borrow_mut()
            .get(key)
            .ok_or(io::Error::new(
                io::ErrorKind::NotFound,
                format!("BackendLoader::get(): key {key:?} not in storage arena. Are you sure you persisted this key or one of its ancestors?"),
            ))?
            .clone();
        //  Build lazy value if at max_depth.
        if self.max_depth == Some(0) {
            // We need to hold this lock until `Sp::lazy` below is done, since
            // that will call `Arena::increment_ref` internally, which assumes
            // that the `track_locked` call here has been superseded by a
            // possible `Sp::drop`.
            let metadata_lock = self.arena.lock_metadata();
            self.arena
                .track_locked(&metadata_lock, key.clone(), obj.data, obj.children);
            return Ok(Sp::lazy(self.arena.clone(), key.clone()));
        }
        // If not at max depth, then deserialize recursively.
        let loader = BackendLoader {
            arena: self.arena,
            max_depth: self.max_depth.map(|max_depth| max_depth - 1),
            recursion_depth: self.recursion_depth + 1,
        };
        let value = T::from_binary_repr::<&[u8]>(
            &mut &obj.data.clone()[..],
            &mut obj.children.clone().into_iter(),
            &loader,
        )?;
        Ok(self
            .arena
            .new_sp(value, key.clone(), obj.data, obj.children))
    }

    fn alloc<T: Storable<D>>(&self, obj: T) -> Sp<T, D> {
        self.arena.alloc(obj)
    }

    fn get_recursion_depth(&self) -> u32 {
        self.recursion_depth
    }
}

/// A `Loader` that uses `IntermediateRepr` objects to get the binary data for
/// deserialization.
///
/// This is used to deserialize `Sp` objects from a stream of binary data,
/// e.g. for serialized objects sent over the wire. This loader is a boundary
/// between user controlled input and trusted internal data, and so always
/// forces a full deserialization, avoiding laziness.
///
/// This loader does *not* use the sp-cache to avoid deserializing objects that
/// are already in the arena, and always does a full deserialization from
/// scratch. Any `Sp` returned by `IrLoader::get` will still be deduplicated and
/// present in the `Sp` cache, but we guarantee that all of its keys were
/// manually deserialized at least once.
///
/// This loader always returns *strict* `Sp`s, because it always doe a full
/// deserialization, and some `Loader` consumers (in particular
/// `Node::from_binary_repr`) have optional sanity checks that are only enabled
/// for strict `Sp`s.
pub(crate) struct IrLoader<'a, D: DB> {
    arena: &'a Arena<D>,
    all: &'a HashMap<ArenaKey<D::Hasher>, IntermediateRepr<D>>,
    recursion_depth: u32,
    /// The keys we've already deserialized once.
    visited: Rc<RefCell<HashSet<DynTypedArenaKey<D::Hasher>>>>,
}

#[cfg(test)]
impl<'a, D: DB> IrLoader<'a, D> {
    pub(crate) fn new(
        arena: &'a Arena<D>,
        all: &'a HashMap<ArenaKey<D::Hasher>, IntermediateRepr<D>>,
    ) -> IrLoader<'a, D> {
        IrLoader {
            arena,
            all,
            recursion_depth: 0,
            visited: Rc::new(RefCell::new(HashSet::new())),
        }
    }
}

impl<D: DB> Loader<D> for IrLoader<'_, D> {
    const CHECK_INVARIANTS: bool = true;

    /// Always forces deserialization of each key the first time we see it, so
    /// that `Sp` deserialization does not depend on the `Arena` state before
    /// this `IrLoader` was constructed.
    ///
    /// This loader always returns eager `Sp`s.
    fn get<T: Storable<D>>(
        &self,
        key: &ArenaKey<<D as DB>::Hasher>,
    ) -> Result<Sp<T, D>, std::io::Error> {
        // We need to use typed keys to avoid conflating identical keys at
        // different types T.
        //
        // Mistakes have been made:
        // https://shielded.atlassian.net/browse/PM-16347
        let typed_key = (key.clone(), TypeId::of::<T>());

        // If we've visited this key before, then try to get it from the cache.
        if self.visited.borrow().contains(&typed_key) {
            // In a sane world we could assume that the Sp cache contained any
            // values we've already deserialized, but in theory someone could
            // implement a malicious/stupid `Storable::from_binary_repr` that
            // calls `Loader::get` and then `drop`s the result, before calling
            // `Loader::get` again on the same key. So, instead, we just keep
            // our own cache for the duration of this `IrLoader`.
            if let Some(sp) = self.arena.get_from_cache::<T>(key) {
                assert!(!sp.is_lazy(), "BUG: IrLoader MUST return strict sps");
                return Ok(sp);
            }
        }

        // Otherwise, deserialize Sp from the IRs.
        let ir = self.all.get(key).ok_or(io::Error::new(
            io::ErrorKind::NotFound,
            "IR not found in `all` map",
        ))?;
        if self.recursion_depth > serialize::RECURSION_LIMIT {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Reached recursion limit".to_string(),
            ));
        }
        let loader = IrLoader {
            arena: self.arena,
            all: self.all,
            recursion_depth: self.recursion_depth + 1,
            visited: self.visited.clone(),
        };
        let sp = self.arena.alloc(T::from_binary_repr(
            &mut ir.binary_repr.clone().as_slice(),
            &mut ir.children.clone().into_iter(),
            &loader,
        )?);
        assert!(!sp.is_lazy(), "BUG: IrLoader MUST return strict sps");
        self.visited.borrow_mut().insert(typed_key);
        Ok(sp)
    }

    fn alloc<T: Storable<D>>(&self, obj: T) -> Sp<T, D> {
        self.arena.alloc(obj)
    }

    fn get_recursion_depth(&self) -> u32 {
        self.recursion_depth
    }
}

/// An intermediate raw binary representation of an arena object.
#[derive(Debug)]
pub struct IntermediateRepr<D: DB> {
    binary_repr: std::vec::Vec<u8>,
    children: std::vec::Vec<ArenaKey<D::Hasher>>,
    db_type: PhantomData<D>,
}

impl<D: DB> IntermediateRepr<D> {
    /// Constructs an intermediate repr from a `Storable` reference.
    #[cfg(test)]
    pub fn from_storable<S: Storable<D>>(s: &S) -> Self {
        let mut binary_repr: std::vec::Vec<u8> = vec![];
        s.to_binary_repr(&mut binary_repr).unwrap();
        IntermediateRepr {
            binary_repr,
            children: s.children(),
            db_type: PhantomData,
        }
    }
}

/// Metadata for objects stored in the Arena
#[derive(Debug, Clone)]
struct Node {
    /// Number of `Sp` pointers to the key for this node. When this goes to
    /// zero, we call `StorageBackend::uncache` on the corresponding key,
    /// knowing that no existing `Sp` has the corresponding key.
    ///
    /// No relation to `crate::backend::OnDiskObject::ref_count`! That other ref
    /// count is concerned with parent-child relationships in the Merkle DAG.
    ///
    /// Note: since the back-end is untyped, whereas `Sp`s are typed, this
    /// `Self::ref_count` can account for `Sp`s of *distinct* types, when those
    /// differently typed `Sp`s have the same hash (easy way to get such hash
    /// collisions: enums with no children and no data, whose hashes are just
    /// the hash of their discriminant tags). So, in particular, knowing that
    /// the last `sp: Sp<T>` for a specific type `T` has gone out of scope,
    /// doesn't tell us that the back-end data for `sp.root` can be uncached,
    /// since some other `Sp<U>` with the same hash could still be referencing
    /// that back-end data.
    ref_count: u32,
}

impl Node {
    fn new() -> Self {
        Node { ref_count: 0 }
    }
}

/// A typed pointer to a value stored in the `Arena`.
///
/// An `Sp<T>` can be lazily initialized, in which case its internal `T` value
/// won't be loaded until access is attempted.
pub struct Sp<T: ?Sized + 'static, D: DB = DefaultDB> {
    /// Cached Pointer data
    ///
    /// The `Arc` is to allow sharing of the data with other `Sp`s. The
    /// `OnceLock` is to support lazy loading.
    ///
    /// The implementation attempts, via use of the `Arena::sp_cache`, to
    /// enforce that there will never be two `Sp`s with the same key with
    /// distinct `Arc` payloads. I.e., if `x, y: Sp<T>` and `x.root == y.root`,
    /// then it should always be true that if `x.data.get().is_some()` and
    /// `y.data.get().is_some()` then `Arc::ptr_eq(x.data.get().unwrap(),
    /// y.data.get().unwrap())` is true.
    data: OnceLock<Arc<T>>,
    /// The arena this Sp points into
    pub(crate) arena: Arena<D>,
    /// The persistent hash of data.
    pub(crate) root: ArenaKey<D::Hasher>,
}

impl<T: Display, D: DB> Display for Sp<T, D> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.data.get() {
            Some(arc) => arc.fmt(f),
            None => write!(f, "<Lazy Sp>"),
        }
    }
}

impl<T: Debug, D: DB> Debug for Sp<T, D> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.data.get() {
            Some(arc) => arc.fmt(f),
            None => write!(f, "<Lazy Sp>"),
        }
    }
}

impl<T: Tagged, D: DB> Tagged for Sp<T, D> {
    fn tag() -> std::borrow::Cow<'static, str> {
        T::tag()
    }
    fn tag_unique_factor() -> String {
        T::tag_unique_factor()
    }
}

impl<T, D: DB> Hash for Sp<T, D> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.root.hash(state);
    }
}

impl<T: Storable<D>, D: DB> Sp<T, D> {
    /// Allocates a new Sp against the default storage
    pub fn new(value: T) -> Self {
        default_storage().arena.alloc(value)
    }
}

/// Constructors for `Sp` that take care of `Arena` ref counting.
impl<T: ?Sized + 'static, D: DB> Sp<T, D> {
    /// Create a new `Sp` that is eagerly initialized with the provided data
    /// `value`.
    ///
    /// Note: this function assumes that `root` is already in `metadata`, and
    /// will panic if not. See `Sp::lazy` for more details.
    ///
    /// Note: this function assumes that the `arc` argument is already in the
    /// arena data cache if it should be. We don't try to handle that logic
    /// here, since it's not uniform:
    ///
    /// - when creating a non-derived `Sp`s in `Arena::new_sp_locked`, we need to
    ///   check the arena cache, and otherwise create and cache a new `Sp`s.
    ///
    /// - when forcing a lazy `Sp`s in `Sp::force_as_arc`, we need to look in the
    ///   arena cache, and then fall back on deserialization.
    fn eager(arena: Arena<D>, root: ArenaKey<D::Hasher>, arc: Arc<T>) -> Self {
        let sp = Sp::lazy(arena, root);
        let _ = sp.data.set(arc);
        sp
    }

    /// Create a new `Sp` with an uninitialized data payload.
    ///
    /// Note: this function assumes that `root` is already in `metadata`, and
    /// will panic if not. If you're creating a new `Sp` for a key x type that's
    /// new to the cache, then you should call `Arena::track_locked` to register
    /// the `Sp` before creating it. Note that `track_locked` is a no-op for
    /// already registered root keys, so there is no harm in calling it if
    /// you're not sure.
    fn lazy(arena: Arena<D>, root: ArenaKey<D::Hasher>) -> Self {
        // This `increment_ref` will panic if `root` is not in `metadata`.
        arena.increment_ref(&root);
        let data = OnceLock::new();
        Sp { data, arena, root }
    }
}

impl<T: Storable<D>, D: DB> Sp<T, D> {
    /// Get Sp based on value from arena cache, falling back on deserializing
    /// object from back-end if necessary.
    ///
    /// Deserialize object for `key`, recursively deserializing any children,
    /// and updating `already_deserialized` to include all child values
    /// recursively deserialized along the way. Note that the return value
    /// itself is not included in `already_deserialized`, because the function
    /// only exists as a helper for `Arena::get` and `Sp::force_as_arc`, which throws away the
    /// `already_deserialized` map before returning.
    ///
    /// If `max_depth == Some(depth)`, then `Sp::data` values down to that depth
    /// only will be initialized, where the top level is depth zero. So, for
    /// example, using `max_depth == Some(0)` will result in a lazy,
    /// uninitialized `Sp`, and `max_depth == Some(1)` will result in an `Sp`
    /// with its `data` value initialized, but with all children set to lazy,
    /// uninitialized `Sp`s.
    ///
    /// Note: the `max_depth` is only advisory, since if we already have a value
    /// cached, we'll just return that, independent of how deep it has been forced.
    fn from_arena(
        arena: &Arena<D>,
        key: &ArenaKey<D::Hasher>,
        max_depth: Option<usize>,
    ) -> Result<Sp<T, D>, std::io::Error> {
        let loader = BackendLoader {
            arena,
            max_depth,
            recursion_depth: 0,
        };
        loader.get(key)
    }
}

impl<T: Storable<D>, D: DB> Deref for Sp<T, D> {
    type Target = T;

    /// Access the inner data, forcing initialization if necessary.
    fn deref(&self) -> &Self::Target {
        self.force_as_arc()
    }
}

impl<T: ?Sized, D: DB> Clone for Sp<T, D> {
    fn clone(&self) -> Self {
        self.arena.increment_ref(&self.root);
        Sp {
            root: self.root.clone(),
            arena: self.arena.clone(),
            data: self.data.clone(),
        }
    }
}

impl<T, D: DB> Sp<T, D> {
    /// Return true iff this `Sp` is lazy/unforced, i.e. its data has not yet
    /// been loaded.
    ///
    /// If the `Sp` is lazy, it can be forced by dereferencing it.
    pub fn is_lazy(&self) -> bool {
        self.data.get().is_none()
    }

    /// Return hash of self and all children, cached from `&lt;T as Storable&gt;::hash()`.
    ///
    /// This is the root key of `self`, as a content-addressed Merkle node.
    pub fn hash(&self) -> TypedArenaKey<T, D::Hasher> {
        TypedArenaKey {
            key: self.root.clone(),
            _phantom: PhantomData,
        }
    }

    /// Notify the storage back-end to increment the persist count on this object.
    ///
    /// See `[StorageBackend::persist]`.
    pub fn persist(&self) {
        self.arena
            .with_backend(|backend| backend.persist(&self.root))
    }

    /// Notify the storage back-end to decrement the persist count on this
    /// object.
    ///
    /// See `[StorageBackend::unpersist]`.
    pub fn unpersist(&self) {
        self.arena
            .with_backend(|backend| backend.unpersist(&self.root))
    }

    /// Returns the content of this `Sp`, if this `Sp` is initialized, and is
    /// the only reference to its data.  When the `Sp` is initialized, this
    /// behaves like [`Arc::into_inner`].
    pub fn into_inner(this: Sp<T, D>) -> Option<T> {
        // Note that we don't want to call `self.force_as_arc()` here, since
        // that could force an uninitialized `Sp` unnecessarily.
        let data: Option<Arc<T>> = this.data.get().cloned();
        // The `Sp` gets dropped, decrementing the ref count, but if initialized
        // the content survives, either in another `Arc`, or in the return
        // value.
        drop(this);
        data.and_then(|arc| Arc::into_inner(arc))
    }
}

impl<T: ?Sized + 'static, D: DB> Sp<T, D> {
    /// Replace the `self.data` with an uninitialized lazy value.
    ///
    /// Note: if there are multiple outstanding refs to the data in this `Sp`,
    /// e.g. because a clone of this `Sp` is owned by some larger data
    /// structure, then the underlying data won't actually be dropped until all
    /// such `Sp`s go out of scope or have `unload` called on them.
    pub fn unload(&mut self) {
        // Return our data to the uninitialized state, dropping the `Arc` in
        // `data`, if any.
        self.data.take();
        self.gc_weak_pointer();
    }

    /// Remove our weak pointer from the `sp_cache` if it's dangling.
    fn gc_weak_pointer(&mut self) {
        let sp_cache_guard = self.arena.lock_sp_cache();
        let mut sp_cache = sp_cache_guard.borrow_mut();
        let key = (self.root.clone(), TypeId::of::<T>());
        // NOTE: Here, we rely on the `Arc` reference count to perform the cleanup if and only if
        // the underlying `Arc` is no longer allocated. This relies on the `Sp`s internal `Arc`
        // not leaking, as this ensures this check will be made during each `Arc` drop, including
        // the final one.
        //
        // Previously, this used `weak.upgrade().is_none()` to determine this, but this can lead to
        // a rare race condition between threads A and B that both hold the same Sp, and are
        // deallocating it simultaneously:
        //
        // - A drops its reference
        // - A acquires the critical section lock
        // - A calls `upgrade`, obtaining an `Arc`
        // - B drops its reference
        // - B waits for the critical section lock
        // - A calls `is_none`, dropping the final `Arc` and triggering `T::drop` in the critical
        //   section
        //
        // Because T::drop can drop further `Sp`s, this can cause the same thread to re-enter the
        // critical section, and attempt to double-borrow the sp cache mutably.
        //
        // Because `strong_count` can not bring a value of `T` into the critical section, it avoids
        // this loop.
        if sp_cache
            .get(&key)
            .is_some_and(|weak| weak.strong_count() == 0)
        {
            sp_cache.remove(&key);
        }
    }
}

impl<T: Storable<D>, D: DB> Sp<T, D> {
    /// Return the inner value as an `Arc` ref, initializing the `OnceLock` if
    /// this is an uninitialized lazy `Sp`.
    fn force_as_arc(&self) -> &Arc<T> {
        // Initialize `OnceLock` if necessary.
        if self.data.get().is_none() {
            // We store the `maybe_arc` in a separate variable, instead in a
            // temporary directly in the `match` scrutinee, to avoid holding a
            // lock on the `sp_cache` when we attempt to lock the `metadata` in
            // the match body, implicitly, via the call to `from_arena`, since
            // that would violate the lock acquisition ordering.
            let maybe_arc = self
                .arena
                .read_sp_cache_locked::<T>(&self.arena.lock_sp_cache(), &self.root);
            let arc: Arc<T> = match maybe_arc {
                Some(arc) => arc,
                None => {
                    let max_depth = Some(1);
                    // All we really want is the inner `Arc` here, but the
                    // easiest way to get that is to just create the lazy `Sp`
                    // for that `Arc`, i.e. what `self` will become when
                    // `force_as_arc` is done!
                    let sp: Sp<T, _> = Sp::from_arena(&self.arena, &self.root, max_depth)
                        .expect("root should be in the arena");
                    let arc = sp
                        .data
                        .get()
                        .expect("result of Sp::from_arena should be initialized");
                    arc.clone()
                }
            };
            // We don't care if this succeeds: failure just means
            // someone else set the same value in another thread.
            let _ = self.data.set(arc);
        }
        self.data.get().unwrap()
    }

    /// Topologically sort a sub-graph of storage into a sequence of nodes.
    ///
    /// This will force and load all nodes in this sub-graph.
    pub fn serialize_to_node_list(&self) -> TopoSortedNodes {
        self.serialize_to_node_list_bounded(u64::MAX)
            .expect("unbounded serialization must succeed")
    }

    /// Topologically sort a sub-graph of storage into a sequence of nodes.
    ///
    /// This will force and load all nodes in this sub-graph.
    ///
    /// The size limit stops serialization if a specified serialized size limit is overstepped.
    ///
    /// Only returns `None` if a size limit is provided and overstepped
    pub fn serialize_to_node_list_bounded(
        &self,
        mut raw_size_limit: u64,
    ) -> Option<TopoSortedNodes> {
        let arena = self.arena.clone();
        let root = self.hash().key;
        // Topological sort using Kahn's algorithm.
        // However, we need to know the incoming vertices of a given node, so to start, we just walk
        // the graph to get a better representation:
        //
        // node hash -> incoming
        // node hash -> OnDiskObjecct
        let mut incoming_vertices: HashMap<_, usize> = HashMap::new();
        let mut disk_objects = HashMap::new();
        let mut frontier = vec![root.clone()];
        while let Some(key) = frontier.pop() {
            if disk_objects.contains_key(&key) {
                continue;
            }
            let node = arena
                .lock_backend()
                .borrow_mut()
                .get(&key)
                .expect("Arena should contain current serialization target")
                .clone();
            for child in node.children.iter() {
                *incoming_vertices.entry(child.clone()).or_default() += 1;
                frontier.push(child.clone());
            }
            raw_size_limit = raw_size_limit
                .checked_sub(PERSISTENT_HASH_BYTES as u64 + node.data.len() as u64)?;
            disk_objects.insert(key, node);
        }
        // now we can use Kahn's algorithm as specified
        let mut list_indices = HashMap::new();
        // Note that only the root should have no incoming edges to start
        let mut empty_incoming_nodes = vec![root.clone()];
        while let Some(node) = empty_incoming_nodes.pop() {
            if list_indices.contains_key(&node) {
                continue;
            }
            let disk = disk_objects.get(&node).expect("node must be present");
            list_indices.insert(node.clone(), list_indices.len() as u64);
            for child in disk.children.iter() {
                let incoming = incoming_vertices
                    .get_mut(&child)
                    .expect("node must be present");
                *incoming -= 1;
                if *incoming == 0 {
                    empty_incoming_nodes.push(child.clone());
                }
            }
        }
        let len = list_indices.len();
        let mut list = TopoSortedNodes {
            nodes: vec![TopoSortedNode::default(); len],
        };
        for (hash, idx) in list_indices.iter() {
            let disk = disk_objects.remove(hash).expect("node must be present");
            // We flip the index ordering, as it a) makes deserialization easier, and b) makes leaf
            // nodes have smaller indexes, which is usually more sensible.
            list.nodes[len - 1 - *idx as usize] = TopoSortedNode {
                child_indices: disk
                    .children
                    .iter()
                    .map(|child| len as u64 - 1 - list_indices[child])
                    .collect(),
                data: disk.data,
            };
        }
        Some(list)
    }
}

impl<T: ?Sized + 'static, D: DB> Drop for Sp<T, D> {
    fn drop(&mut self) {
        // It's important that we unload() before calling decrement_ref(),
        // because unload() is responsible for cleaning up the sp_cache, and
        // decrement_ref() is responsible for cleaning up the metadata, and the
        // invariant is that any Arc in the sp_cache must have a corresponding
        // entry in the metadata.
        self.unload();
        self.arena.decrement_ref(&self.root);
    }
}

impl<T, D: DB> PartialEq for Sp<T, D> {
    /// An O(1) implementation of equality for `Sp<T>`.
    ///
    /// # Warning
    ///
    /// It's possible this implementation is inconsistent with the
    /// implementation for the underlying type `T`, if any, because:
    ///
    /// - our equality is reflexive, but equality on `T` may not be, i.e. `T`
    ///   might not implement `Eq`.
    ///
    /// - our equality is maximally fine grained, but equality on the underlying
    ///   type `T` could equate two values with different hashes.
    fn eq(&self, other: &Self) -> bool {
        self.root == other.root
    }
}

/// See warning on [`Sp::eq`] above.
impl<T, D: DB> Eq for Sp<T, D> {}

/// See warning on [`Sp::eq`] above.
impl<T: PartialOrd + Storable<D>, D: DB> PartialOrd for Sp<T, D> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        if self.root == other.root {
            return Some(std::cmp::Ordering::Equal);
        }
        self.force_as_arc().partial_cmp(other.force_as_arc())
    }
}

/// See warning on [`Sp::eq`] above.
impl<T: Ord + Storable<D>, D: DB> Ord for Sp<T, D> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        if self.root == other.root {
            return std::cmp::Ordering::Equal;
        }
        self.force_as_arc().cmp(other.force_as_arc())
    }
}

/// A topologically sorted sub-graph of the storage graph.
///
/// Stored as a sequence of nodes, each referencing their children as indices in this sequence.
/// The final entry in this is the root of the sub-graph (which is assumed to have only one root).
/// Each node in the graph should have its children *preceding* it in the graph, allowing the graph
/// to be restored from this representation in a single in-order iteration.
#[derive(Clone, PartialEq, Eq, Debug, Serializable)]
pub struct TopoSortedNodes {
    /// The individual sorted nodes
    pub nodes: Vec<TopoSortedNode>,
}

/// An individual node in `TopoSortedNodes`.
///
/// It represents a node in the storage DAG, with its data, and with child references as indices
/// into its parent `TopoSortedNodes` nodes vector.
#[derive(Clone, PartialEq, Eq, Debug, Default, Serializable)]
pub struct TopoSortedNode {
    /// The indices of the children of this node
    pub child_indices: Vec<u64>,
    /// The data of this node
    pub data: Vec<u8>,
}

impl<D: DB, T: Storable<D>> Storable<D> for Sp<T, D> {
    fn children(&self) -> std::vec::Vec<ArenaKey<D::Hasher>> {
        self.deref().children()
    }

    fn from_binary_repr<R: std::io::Read>(
        reader: &mut R,
        child_hashes: &mut impl Iterator<Item = ArenaKey<D::Hasher>>,
        loader: &impl Loader<D>,
    ) -> Result<Self, std::io::Error> {
        T::from_binary_repr(reader, child_hashes, loader).map(|sp| loader.alloc(sp))
    }

    fn to_binary_repr<W: std::io::Write>(&self, writer: &mut W) -> Result<(), std::io::Error> {
        self.deref().to_binary_repr(writer)
    }

    fn check_invariant(&self) -> Result<(), std::io::Error> {
        T::check_invariant(&self)
    }
}

impl<T: Storable<D>, D: DB> Serializable for Sp<T, D> {
    #[allow(clippy::type_complexity)]
    fn serialize(&self, writer: &mut impl std::io::Write) -> std::io::Result<()> {
        self.serialize_to_node_list().serialize(writer)
    }

    fn serialized_size(&self) -> usize {
        self.serialize_to_node_list().serialized_size()
    }
}

impl<T: Storable<D>, D: DB> Deserializable for Sp<T, D> {
    fn deserialize(
        reader: &mut impl std::io::Read,
        recursive_depth: u32,
    ) -> Result<Self, std::io::Error> {
        default_storage()
            .arena
            .clone()
            .deserialize_sp(reader, recursive_depth)
    }
}

/// Define bin-tree type for use in tests.
#[cfg(any(test, feature = "stress-test"))]
pub(crate) mod bin_tree {
    use super::*;
    use crate as storage;
    use macros::Storable;

    #[derive(Storable, Debug)]
    #[derive_where(Clone, PartialEq, Eq)]
    #[tag = "test-bin-tree"]
    #[storable(db = D)]
    pub(crate) struct BinTree<D: DB> {
        value: u64,
        pub(crate) left: Option<Sp<BinTree<D>, D>>,
        pub(crate) right: Option<Sp<BinTree<D>, D>>,
    }

    impl<D: DB> BinTree<D> {
        pub(crate) fn new(
            value: u64,
            left: Option<Sp<BinTree<D>, D>>,
            right: Option<Sp<BinTree<D>, D>>,
        ) -> BinTree<D> {
            BinTree { value, left, right }
        }

        /// Return sum of all node values.
        ///
        /// The point is that this forces the whole tree to be loaded.
        #[cfg(all(
            feature = "stress-test",
            any(feature = "parity-db", feature = "sqlite")
        ))]
        pub(crate) fn sum(&self) -> u64 {
            self.value
                + self.left.as_ref().map(|l| l.sum()).unwrap_or(0)
                + self.right.as_ref().map(|r| r.sum()).unwrap_or(0)
        }
    }

    /// Here `counting_tree(n)` computes a `BinTree` of height `n` with
    /// left-to-right BFS node values `[1, 2, .., 2^n - 1]`.
    ///
    /// For example, `counting_tree(3)` computes the tree
    ///
    /// ```text
    ///      1
    ///     / \
    ///    /   \
    ///   2     3
    ///  / \   / \
    /// 4   5 6   7
    /// ```
    #[cfg(any(
        test,
        all(
            feature = "stress-test",
            any(feature = "parity-db", feature = "sqlite")
        )
    ))]
    pub(crate) fn counting_tree<D: DB>(arena: &Arena<D>, height: usize) -> Sp<BinTree<D>, D> {
        fn go<D: DB>(arena: &Arena<D>, value: u64, height: usize) -> Sp<BinTree<D>, D> {
            assert!(height > 0);
            let (left, right) = {
                if height == 1 {
                    (None, None)
                } else {
                    (
                        Some(go(arena, 2 * value, height - 1)),
                        Some(go(arena, 2 * value + 1, height - 1)),
                    )
                }
            };
            arena.alloc(BinTree::new(value, left, right))
        }
        go(arena, 1, height)
    }
}

/// Helper functions for testing arena in other crates, specifically the `/examples`
/// for this crate.
pub mod test_helpers {
    use super::*;

    /// Get the root count of key.
    pub fn get_root_count<D: DB>(arena: &Arena<D>, key: &ArenaKey<D::Hasher>) -> u32 {
        arena.lock_backend().borrow().get_root_count(key)
    }

    /// Read the `sp_cache`.
    ///
    /// # Safety
    ///
    /// The `Arc` returned here *must* be dropped before the `Sp` itself is to ensure proper cache
    /// cleanup. A failure to do so could lead to the de-allocation of the value *not* leading to
    /// the value being removed from the cache, as `Sp`'s rely on the `Arc` reference count to
    /// determine if this cleanup should be performed. This requires the last drop to be an `Sp`
    /// drop, not just an `Arc` drop.
    pub fn read_sp_cache<D: DB, T: Storable<D>>(
        arena: &Arena<D>,
        key: &ArenaKey<D::Hasher>,
    ) -> Option<Arc<T>> {
        arena.read_sp_cache_locked::<T>(&arena.lock_sp_cache(), key)
    }
}

/// Stress tests.
#[cfg(feature = "stress-test")]
pub mod stress_tests {
    use super::*;
    use crate::{self as storage, Storage, arena::Sp, storage::Array};

    fn new_arena() -> Arena<DefaultDB> {
        let storage = Storage::<DefaultDB>::new(16, DefaultDB::default());
        storage.arena
    }

    /// Test that we can allocate and drop a deeply nested `Sp` without blowing
    /// up the stack via implicit recursion.
    pub fn drop_deeply_nested_data() {
        use super::bin_tree::BinTree;

        let arena = new_arena();
        let mut bt = BinTree::new(0, None, None);
        let depth = 100_000;
        for i in 1..depth {
            bt = BinTree::new(i, Some(arena.alloc(bt)), None);
        }
    }

    /// Test that we can serialize a deeply nested `Sp` without blowing up the
    /// stack via recursion.
    pub fn serialize_deeply_nested_data() {
        use super::bin_tree::BinTree;

        let arena = new_arena();
        let mut bt = BinTree::new(0, None, None);
        let depth = 100_000;
        for i in 1..depth {
            bt = BinTree::new(i, Some(arena.alloc(bt)), None);
        }

        let mut buf = std::vec::Vec::new();
        Sp::serialize(&arena.alloc(bt), &mut buf).unwrap();
    }

    /// Similar to `tests::test_sp_nesting`, but with a more complex structure,
    /// where the `Sp` is nested inside an `Array`.
    pub fn array_nesting() {
        use macros::Storable;
        #[derive(Debug, Clone, PartialOrd, Ord, PartialEq, Eq, Hash, Storable)]
        struct Nesty(Array<Nesty>);
        impl Drop for Nesty {
            fn drop(&mut self) {
                if self.0.is_empty() {
                    return;
                }
                let take = |ptr: &mut Nesty| {
                    let mut tmp = Array::new();
                    std::mem::swap(&mut tmp, &mut ptr.0);
                    tmp
                };
                let mut frontier = vec![take(self)];
                while let Some(curr) = frontier.pop() {
                    let items = curr.iter().collect::<std::vec::Vec<_>>();
                    drop(curr);
                    frontier.extend(
                        items
                            .into_iter()
                            .flat_map(Sp::into_inner)
                            .map(|mut n| take(&mut n)),
                    );
                }
            }
        }
        let mut nest = Nesty(Array::new());
        for i in 0..16_000 {
            nest = Nesty(vec![nest].into());
            if i % 100 == 0 {
                dbg!(i);
            }
        }
        drop(nest);
        // Did we survive the drop?
        println!("drop(nest) returned!");
    }

    /// See `thrash_the_cache_variations_inner` for details.
    #[cfg(feature = "sqlite")]
    pub fn thrash_the_cache_variations_sqldb(args: &[String]) {
        use crate::db::SqlDB;
        fn mk_open_db() -> impl Fn() -> SqlDB {
            let path = tempfile::NamedTempFile::new().unwrap().into_temp_path();
            move || SqlDB::exclusive_file(&path)
        }
        thrash_the_cache_variations(args, mk_open_db);
    }

    /// See `thrash_the_cache_variations_inner` for details.
    #[cfg(feature = "parity-db")]
    pub fn thrash_the_cache_variations_paritydb(args: &[String]) {
        use crate::db::ParityDb;
        fn mk_open_db() -> impl Fn() -> ParityDb {
            let path = tempfile::TempDir::new().unwrap().keep();
            move || ParityDb::open(&path)
        }
        thrash_the_cache_variations(args, mk_open_db);
    }

    #[cfg(any(feature = "parity-db", feature = "sqlite"))]
    fn thrash_the_cache_variations<D: DB, O: Fn() -> D>(
        args: &[String],
        mk_open_db: impl Fn() -> O,
    ) {
        let msg = "thrash_the_cache_variations(p: f64, include_cyclic: bool)";
        if args.len() != 2 {
            panic!("{msg}: wrong number of args");
        }
        let p = args[0]
            .parse::<f64>()
            .unwrap_or_else(|e| panic!("{msg}: couldn't parse p={}: {e}", args[0]));
        let include_cyclic = args[1]
            .parse()
            .unwrap_or_else(|e| panic!("{msg}: couldn't parse include_cyclic={}: {e}", args[1]));
        thrash_the_cache_variations_inner(p, include_cyclic, mk_open_db);
    }

    /// Run various cache thrashing combinations with fixed `p`.
    ///
    /// Here `mk_open_db` creates a fresh `open_db` function every time it's
    /// called.
    #[cfg(any(feature = "parity-db", feature = "sqlite"))]
    fn thrash_the_cache_variations_inner<D: DB, O: Fn() -> D>(
        p: f64,
        include_cyclic: bool,
        mk_open_db: impl Fn() -> O,
    ) {
        let num_lookups = 100_000;
        thrash_the_cache(1000, p, num_lookups, false, mk_open_db());
        if include_cyclic {
            thrash_the_cache(1000, p, num_lookups, true, mk_open_db());
        }
        thrash_the_cache(10_000, p, num_lookups, false, mk_open_db());
        if include_cyclic {
            thrash_the_cache(10_000, p, num_lookups, true, mk_open_db());
        }
    }

    /// Test thrashing the cache, where the cache is smaller than the number of
    /// items actively held in memory.
    ///
    /// Parameters:
    ///
    /// - `num_values`: the number of unique values to insert into the arena.
    ///
    /// - `p`: the cache size as a proportion of `num_values`.
    ///
    /// - `num_lookups`: number of values to look up in the arena by hash key.
    ///
    /// - `is_cyclic`: whether to lookup values in a cyclic pattern, or
    ///   randomly. For random lookups, the probability of a cache hit is `p`
    ///   for each lookup. For cyclic lookups, the cache hit rate is 0 if the
    ///   `p` is less than 1.0, and 1 otherwise.
    ///
    /// - `open_db`: opens a new connection to the *same* db every time it's
    ///   called.
    ///
    /// Parity-db beats SQLite here, but by how much varies a lot:
    ///
    /// - for p = 0.5, SQLite takes about 4 times as long
    /// - for p = 0.8, SQLite takes 1.5 times to 2.5 times as long, doing better for
    ///   larger `num_values`.
    #[cfg(any(feature = "parity-db", feature = "sqlite"))]
    fn thrash_the_cache<D: DB>(
        num_values: usize,
        p: f64,
        num_lookups: usize,
        is_cyclic: bool,
        open_db: impl Fn() -> D,
    ) {
        use crate::storage::Storage;
        use rand::Rng;
        use std::io::{Write, stdout};
        use std::{collections::HashMap, time::Instant};

        assert!(p > 0.0 && p <= 1.0, "Cache proportion must be in (0,1]");

        let db = open_db();
        let cache_size = (num_values as f64 * p) as usize;
        let storage = Storage::new(cache_size, db);
        let arena = storage.arena;
        let mut key_map = HashMap::new();
        let mut rng = rand::thread_rng();

        let prefix = format!(
            "thrash_the_cache(num_values={}, p={}, num_lookups={}, is_cyclic={})",
            num_values, p, num_lookups, is_cyclic
        );

        // Insert numbers and store their root keys
        let start_time = Instant::now();
        println!("{prefix} inserting data:");
        for x in 0..num_values {
            if x % (num_values / 100) == 0 {
                print!(".");
                stdout().flush().unwrap();
            }
            let sp = arena.alloc(x as u64);
            sp.persist();
            key_map.insert(x, sp.hash());
        }
        let elapsed = start_time.elapsed();
        println!("{:.2?}", elapsed);

        // Flush changes and create a fresh cache.
        let start_time = Instant::now();
        print!("{prefix} flushing to disk: ");
        arena.with_backend(|b| b.flush_all_changes_to_db());
        drop(arena);
        let db = open_db();
        let storage = Storage::new(cache_size, db);
        let arena = storage.arena;
        let elapsed = start_time.elapsed();
        println!("{:.2?}", elapsed);

        // Warm up the cache, i.e. fetch all values once.
        println!("{prefix} warming up the cache:");
        let start_time = Instant::now();
        for x in 0..num_values {
            if x % (num_values / 100) == 0 {
                print!(".");
                stdout().flush().unwrap();
            }
            let hash = key_map.get(&x).unwrap();
            arena.get::<u64>(hash).unwrap();
        }
        let elapsed = start_time.elapsed();
        println!("{:.2?}", elapsed);

        // Compute values to lookup.
        let xs: std::vec::Vec<_> = if is_cyclic {
            (0..num_values).cycle().take(num_lookups).collect()
        } else {
            (0..num_lookups)
                .map(|_| rng.gen_range(0..num_values))
                .collect()
        };

        // Repeatedly lookup values via their hash, num_lookups times.
        println!("{prefix} fetching data:");
        let start_time = Instant::now();
        for (i, x) in xs.into_iter().enumerate() {
            if i % (num_lookups / 100) == 0 {
                print!(".");
                stdout().flush().unwrap();
            }
            let hash = key_map.get(&x).unwrap();
            arena.get::<u64>(hash).unwrap();
        }
        let elapsed = start_time.elapsed();
        println!("{:.2?}", elapsed);
        println!();
    }

    /// See `load_large_tree_inner` for details.
    ///
    /// Example time with height = 20:
    ///
    /// ```text
    /// $ cargo run --all-features --release --bin stress -p midnight-storage -- arena::stress_tests::load_large_tree_sqldb 20
    /// load_large_tree: 0.40/0.40: init
    /// load_large_tree: 20.43/20.84: create tree
    /// load_large_tree: 12.20/33.03: persist tree to disk
    /// load_large_tree: 2.10/35.13: drop
    /// load_large_tree: 0.69/35.81: init
    /// load_large_tree: 27.89/63.70: lazy load and traverse tree, no prefetch
    /// load_large_tree: 2.47/66.17: drop
    /// load_large_tree: 0.00/66.18: init
    /// load_large_tree: 8.40/74.57: lazy load and traverse tree, with prefetch
    /// load_large_tree: 2.46/77.03: drop
    /// load_large_tree: 0.00/77.04: init
    /// load_large_tree: 26.19/103.23: eager load and traverse tree, no prefetch
    /// load_large_tree: 2.47/105.69: drop
    /// load_large_tree: 0.00/105.70: init
    /// load_large_tree: 8.01/113.71: eager load and traverse tree, with prefetch
    /// load_large_tree: 2.48/116.19: drop
    ///   97.10s user 18.82s system 99% cpu 1:56.56 total
    /// ```
    ///
    /// Note that tree creation takes 20 s here, vs 4 s for parity-db, even tho
    /// naively, creation should have no interaction with the db: turns out the
    /// hidden db interaction on creation happens in `StorageBackend::cache`,
    /// which checks the db to see if the to-be-cached key is already in the db
    /// or not, which is used for ref-counting. If I comment that check out,
    /// which is irrelevant for this test, then creation time drops to 3.5 s, larger than an 80 %
    /// improvement.
    #[cfg(feature = "sqlite")]
    pub fn load_large_tree_sqldb(args: &[String]) {
        use crate::db::SqlDB;
        let path = tempfile::NamedTempFile::new().unwrap().into_temp_path();
        let open_db = || SqlDB::<crate::DefaultHasher>::exclusive_file(&path);
        load_large_tree(args, open_db);
    }

    /// See `load_large_tree_inner` for details.
    ///
    /// Example time with height = 20:
    ///
    /// ```text
    /// $ cargo run --all-features --release --bin stress -p midnight-storage -- arena::stress_tests::load_large_tree_paritydb 20
    /// load_large_tree: 0.06/0.06: init
    /// load_large_tree: 3.83/3.89: create tree
    /// load_large_tree: 6.02/9.90: persist tree to disk
    /// load_large_tree: 19.16/29.06: drop
    /// load_large_tree: 0.92/29.99: init
    /// load_large_tree: 7.83/37.81: lazy load and traverse tree, no prefetch
    /// load_large_tree: 2.50/40.31: drop
    /// load_large_tree: 0.02/40.33: init
    /// load_large_tree: 8.31/48.64: lazy load and traverse tree, with prefetch
    /// load_large_tree: 2.52/51.16: drop
    /// load_large_tree: 0.02/51.18: init
    /// load_large_tree: 7.03/58.21: eager load and traverse tree, no prefetch
    /// load_large_tree: 2.58/60.79: drop
    /// load_large_tree: 0.02/60.81: init
    /// load_large_tree: 7.30/68.11: eager load and traverse tree, with prefetch
    /// load_large_tree: 2.47/70.59: drop
    ///   66.03s user 1.54s system 94% cpu 1:11.23 total
    /// ```
    ///
    /// Note that the biggest time delta is in the first `drop`. I think this is
    /// because parity-db does many operations asynchronously, returning to the
    /// caller immediately after work is passed off to a background thread),
    /// which then needs to be finished before the db can be dropped.
    #[cfg(feature = "parity-db")]
    pub fn load_large_tree_paritydb(args: &[String]) {
        use crate::db::ParityDb;
        let path = tempfile::TempDir::new().unwrap().keep();
        let open_db = || ParityDb::<crate::DefaultHasher>::open(&path);
        load_large_tree(args, open_db);
    }

    #[cfg(any(feature = "parity-db", feature = "sqlite"))]
    fn load_large_tree<D: DB>(args: &[String], open_db: impl Fn() -> D) {
        let msg = "load_large_tree(height: usize)";
        if args.len() != 1 {
            panic!("{msg}: wrong number of args");
        }
        let height = args[0]
            .parse()
            .unwrap_or_else(|e| panic!("{msg}: couldn't parse height={}: {e}", args[0]));
        load_large_tree_inner(height, open_db);
    }

    /// Create and persist a large tree, then load it various ways and traverse
    /// it.
    ///
    /// Here `open_db` must open a new connection to the *same* db every
    /// time. The test flushes to the db, and then reopens it.
    ///
    /// The tree will have `2^height - 1` nodes.
    #[cfg(any(feature = "parity-db", feature = "sqlite"))]
    fn load_large_tree_inner<D: DB>(height: usize, open_db: impl Fn() -> D) {
        use crate::storage::Storage;
        use bin_tree::*;

        let cache_size = 1 << height;
        // Value sum of tree: 1 + 2 + 3 + 4 + ... + 2^height - 1
        let sum = (1 << (height - 1)) * ((1 << height) - 1);
        let mut timer = crate::test::Timer::new("load_large_tree");

        // Build and persist tree.
        //
        // Compute key in a block, to ensure everything else gets dropped.
        let key = {
            let db = open_db();
            let storage = Storage::new(cache_size, db);
            let arena = storage.arena;
            timer.delta("init");

            let bt = counting_tree(&arena, height);
            timer.delta("create tree");

            bt.persist();
            arena.with_backend(|b| b.flush_all_changes_to_db());
            timer.delta("persist tree to disk");

            bt.hash()
        };
        timer.delta("drop");

        // Lazy load and traverse tree, no prefetch.
        {
            let db = open_db();
            let storage = Storage::new(cache_size, db);
            let arena = storage.arena;
            timer.delta("init");

            let bt = arena.get_lazy::<BinTree<D>>(&key).unwrap();
            assert_eq!(bt.sum(), sum);
            timer.delta("lazy load and traverse tree, no prefetch");
        }
        timer.delta("drop");

        // Lazy load and traverse tree, with prefetch.
        {
            let db = open_db();
            let storage = Storage::new(cache_size, db);
            let arena = storage.arena;
            timer.delta("init");

            let max_depth = Some(height);
            let truncate = false;
            arena.with_backend(|b| b.pre_fetch(&key.clone().into(), max_depth, truncate));
            let bt = arena.get_lazy::<BinTree<D>>(&key).unwrap();
            assert_eq!(bt.sum(), sum);
            timer.delta("lazy load and traverse tree, with prefetch");
        }
        timer.delta("drop");

        // Eager load and traverse tree, no prefetch.
        {
            let db = open_db();
            let storage = Storage::new(cache_size, db);
            let arena = storage.arena;
            timer.delta("init");

            let bt = arena.get::<BinTree<D>>(&key).unwrap();
            assert_eq!(bt.sum(), sum);
            timer.delta("eager load and traverse tree, no prefetch");
        }
        timer.delta("drop");

        // Eager load and traverse tree, with prefetch.
        {
            let db = open_db();
            let storage = Storage::new(cache_size, db);
            let arena = storage.arena;
            timer.delta("init");

            let max_depth = Some(height);
            let truncate = false;
            arena.with_backend(|b| b.pre_fetch(&key.clone().into(), max_depth, truncate));
            let bt = arena.get::<BinTree<D>>(&key).unwrap();
            assert_eq!(bt.sum(), sum);
            timer.delta("eager load and traverse tree, with prefetch");
        }
        timer.delta("drop");
    }

    /// Performance when reading and writing random data into a map and flushing
    /// it in a tight loop
    pub fn read_write_map_loop<D: DB>(args: &[String]) {
        let msg = "read_write_map_loop(num_operations: usize, flush_interval: usize)";
        if args.len() != 2 {
            panic!("{msg}: wrong number of args");
        }
        let num_operations = args[0]
            .parse()
            .unwrap_or_else(|e| panic!("{msg}: couldn't parse num_operations={}: {e}", args[0]));
        let flush_interval = args[1]
            .parse()
            .unwrap_or_else(|e| panic!("{msg}: couldn't parse flush_interval={}: {e}", args[1]));
        read_write_map_loop_inner::<D>(num_operations, flush_interval);
    }

    /// Performance when reading and writing random data into a map and flushing
    /// it in a tight loop.
    ///
    /// Parameters:
    ///
    /// - `num_operations`: total number of reads and writes to perform.
    ///
    /// - `flush_interval`: how many operations to do between each flush.
    ///
    /// # Summary of performance of `SqlDB` for various SQLite configurations
    ///
    /// For the 1000000 operation, 1000 flush interval `read_write_map_loop`
    /// stress test, we see the following total db flush times:
    ///
    /// - original settings: 2860 s
    ///
    /// - with synchronous = 0, but original journal mode: 466 s
    ///
    /// - with synchronous = 0, and WAL journal: 442 s
    ///
    /// I.e. speedup factor is 2860/442 ~ 6.5 times.
    ///
    /// The time spent on in-memory storage::Map updates in this stress test
    /// don't depend on the db settings (of course), and are about 175 s, so with
    /// the DB optimizations we have a ratio of ~ 2.5 times for in-memory updates vs
    /// disk writes for map inserts, which seems pretty good from the point of
    /// view of db traffic, but may indicate there's room to improve the
    /// implementation of the in-memory part.
    ///
    /// Of the db flush time, it seems about `7%` is devoted to preparing the data
    /// to be flushed, and the other `93%` is the time our `db::sql::SqlDB` takes to
    /// do the actual flushing.
    fn read_write_map_loop_inner<D: DB>(num_operations: usize, flush_interval: usize) {
        use crate::storage::{Map, Storage, WrappedDB, set_default_storage};
        use rand::{Rng, seq::SliceRandom as _};
        use serde_json::json;
        use std::io::{Write, stdout};
        use std::time::Instant;

        // Create a unique tag for our WrappedDB
        struct Tag;
        type DB<D> = WrappedDB<D, Tag>;

        let storage = set_default_storage(Storage::<DB<D>>::default).unwrap();

        let mut rng = rand::thread_rng();
        let mut map = Map::<u128, u128, DB<D>>::new();
        let mut keys = vec![];
        let mut total_write_time = std::time::Duration::new(0, 0);
        let mut total_read_time = std::time::Duration::new(0, 0);
        let mut total_flush_time = std::time::Duration::new(0, 0);
        let mut reads = 0;
        let mut writes = 0;
        let mut flushes = 0;
        let prefix = format!(
            "read_write_map_loop(num_operations={num_operations}, flush_interval={flush_interval})"
        );

        let mut time_series: std::vec::Vec<serde_json::Value> = vec![];

        println!("{prefix} running: ");
        let start = Instant::now();
        for i in 1..=num_operations {
            // Print progress
            if i % (num_operations / 100) == 0 {
                print!(".");
                stdout().flush().unwrap();
            }

            // Alternate between reading and writing.
            if i % 2 == 0 {
                // Choose a random key to read from the keys inserted so far.
                let key = keys.choose(&mut rng).unwrap();
                let read_start = Instant::now();
                let _ = map.get(key);
                total_read_time += read_start.elapsed();
                reads += 1;
            } else {
                // Generate a random key to write.
                let key = rng.r#gen::<u128>();
                keys.push(key);
                let value = rng.r#gen::<u128>();
                let write_start = Instant::now();
                map = map.insert(key, value);
                total_write_time += write_start.elapsed();
                writes += 1;
            }

            // Periodic flush
            if i % flush_interval == 0 {
                // debug
                let cache_size = storage.arena.with_backend(|b| b.get_write_cache_len());
                let cache_bytes = storage
                    .arena
                    .with_backend(|b| b.get_write_cache_obj_bytes());

                let flush_start = Instant::now();
                storage
                    .arena
                    .with_backend(|backend| backend.flush_all_changes_to_db());
                let flush_time = flush_start.elapsed();
                total_flush_time += flush_time;
                flushes += 1;

                // debug
                let map_size = map.size();
                let map_size_ratio = flush_time / (map_size as u32);
                let cache_size_ratio = flush_time / (cache_size as u32);
                let cache_bytes_ratio = flush_time / (cache_bytes as u32);
                println!(
                    "ft: {:0.2?}; ms: {}; ft/ms: {:0.2?}; cs: {}; ft/cs: {:0.2?}; cb: {}; ft/cb: {:0.2?}; cb/cs: {}",
                    flush_time,
                    map_size,
                    map_size_ratio,
                    cache_size,
                    cache_size_ratio,
                    cache_bytes,
                    cache_bytes_ratio,
                    cache_bytes / cache_size,
                );
                time_series.push(json!({
                    "i": i,
                    "flush_time": flush_time.as_secs_f32(),
                    "map_size": map_size,
                    "cache_size": cache_size,
                    "cache_bytes": cache_bytes,
                }));
            }
        }
        println!();

        // Print statistics
        let total_time = start.elapsed();
        println!("{prefix} results:");
        println!("- total time: {:.2?}", total_time);
        println!("- operations performed: {num_operations}");
        println!(
            "  - reads:  {} (avg {:.2?} per op)",
            reads,
            total_read_time / reads as u32
        );
        println!(
            "  - writes: {} (avg {:.2?} per op)",
            writes,
            total_write_time / writes as u32
        );
        println!(
            "  - flushes: {} (avg {:.2?} per flush)",
            flushes,
            total_flush_time / std::cmp::max(flushes, 1) as u32
        );
        println!(
            "  - ops/second: {:.0}",
            num_operations as f64 / total_time.as_secs_f64()
        );

        // Save statistics in json file.
        let write_json = || -> std::io::Result<()> {
            let now = std::time::UNIX_EPOCH.elapsed().unwrap().as_secs();
            std::fs::create_dir_all("tmp")?;
            let file_path =
                format!("tmp/read_write_map_loop.{num_operations}_{flush_interval}.{now:?}.json");
            let mut file = std::fs::File::create(&file_path)?;
            let header = json!({
                "total_time": total_time.as_secs_f32(),
                "num_operations": num_operations,
                "flush_interval": flush_interval,
                "reads": reads,
                "total_read_time": total_read_time.as_secs_f32(),
                "writes": writes,
                "total_write_time": total_write_time.as_secs_f32(),
                "flushes": flushes,
                "total_flush_time": total_flush_time.as_secs_f32(),
            });
            let json = json!({
                "header": header,
                "data": time_series,
            });
            writeln!(file, "{}", serde_json::to_string_pretty(&json)?)?;
            let canon_path = std::path::Path::new(&file_path).canonicalize()?;
            println!("- JSON stats: {}", canon_path.display());
            Ok(())
        };
        write_json().unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate as storage;
    use crate::{DefaultHasher, storage::Array};
    use macros::Storable;

    fn new_arena() -> Arena<DefaultDB> {
        Arena::<DefaultDB>::new_from_backend(StorageBackend::<DefaultDB>::new(
            16,
            DefaultDB::default(),
        ))
    }

    #[test]
    fn alloc() {
        let val: u8 = 2;
        let map = new_arena();
        let alloced = map.alloc::<u8>(val);
        assert_eq!(*alloced, val);
    }

    #[test]
    fn dedup() {
        let val: u8 = 2;
        let map = new_arena();
        let _malloced_a = map.alloc::<u8>(val);
        let _malloced_b = map.alloc::<u8>(val);
        assert_eq!(map.size(), 1)
    }

    #[test]
    fn drop_node() {
        let map = new_arena();
        let _malloc_a = map.alloc::<u8>(6);
        {
            let _malloc_b = map.alloc::<u8>(8);
            assert_eq!(map.size(), 2);
        }
        assert_eq!(map.size(), 1);
    }

    #[test]
    fn clone_increment_refcount() {
        let map = new_arena();
        let malloc_a = map.alloc::<u8>(6);
        let malloc_b = malloc_a.clone();
        let ref_count = map
            .lock_metadata()
            .borrow()
            .get(&malloc_a.root)
            .unwrap()
            .ref_count;
        assert_eq!(malloc_a, malloc_b);
        assert_eq!(ref_count, 2);
    }

    #[test]
    fn init_many() {
        for _ in 0..10_000 {
            Array::<()>::new();
        }
    }

    // Test that `into_inner` returns the inner value when it should (last ref),
    // and doesn't when it shouldn't (not last ref).
    #[test]
    fn into_inner() {
        let arena = new_arena();
        let sp1 = arena.alloc(42u32);
        let sp2 = sp1.clone();
        assert!(Sp::into_inner(sp1).is_none());
        assert!(Sp::into_inner(sp2).is_some());
    }

    // Test that using `into_inner` in custom `drop` avoids implicit recursion
    // blowing up the stack.
    #[test]
    fn test_sp_nesting() {
        let arena = new_arena();
        #[derive(Clone, PartialOrd, Ord, PartialEq, Eq)]
        struct Nesty(Option<Sp<Nesty>>);
        impl Storable<DefaultDB> for Nesty {
            fn children(&self) -> std::vec::Vec<ArenaKey<<DefaultDB as DB>::Hasher>> {
                self.0.children()
            }
            fn to_binary_repr<W: std::io::Write>(
                &self,
                writer: &mut W,
            ) -> Result<(), std::io::Error> {
                self.0.to_binary_repr(writer)
            }
            fn from_binary_repr<R: std::io::Read>(
                reader: &mut R,
                child_hashes: &mut impl Iterator<Item = ArenaKey<DefaultHasher>>,
                loader: &impl Loader<DefaultDB>,
            ) -> Result<Self, std::io::Error> {
                Ok(Nesty(
                    <Option<Sp<Nesty>> as Storable<DefaultDB>>::from_binary_repr(
                        reader,
                        child_hashes,
                        loader,
                    )?,
                ))
            }
        }
        impl Drop for Nesty {
            fn drop(&mut self) {
                if self.0.is_none() {
                    return;
                }
                let mut frontier = std::mem::take(&mut self.0)
                    .into_iter()
                    .collect::<std::vec::Vec<_>>();
                while let Some(nest) = frontier.pop() {
                    frontier
                        .extend(Sp::into_inner(nest).and_then(|mut n| std::mem::take(&mut n.0)));
                }
            }
        }
        let mut nest = Nesty(None);
        for _ in 0..100_000 {
            nest = Nesty(Some(arena.alloc(nest)));
        }
        drop(nest);
    }

    #[cfg(feature = "stress-test")]
    #[test]
    fn array_nesting() {
        crate::stress_test::runner::StressTest::new()
            .with_max_memory(1 << 30)
            .run("arena::stress_tests::array_nesting");
    }

    // Test that weak refs in `Arena::sp_cache` are cleaned up when the last
    // strong ref to an `Sp` is dropped, but not before that, meaning that `Sp`s for the same
    // root hash share the same `Arc`, i.e. the one in the cache.
    #[test]
    fn sp_cache_sp_drop() {
        let arena = &new_arena();

        // Allocate an `Sp` in the arena
        let sp1 = arena.alloc(42u32);
        let root_key = sp1.root.clone();
        let type_id = TypeId::of::<u32>();
        let cache_key = (root_key.clone(), type_id);

        // Ensure the `Arc` is in the `sp_cache`, and equal to the one in the
        // `Sp`
        {
            let sp_cache = arena.lock_sp_cache();
            let sp_cache = sp_cache.borrow();
            assert!(sp_cache.get(&cache_key).is_some());
            let weak_ref = sp_cache.get(&cache_key).unwrap();
            assert!(weak_ref.upgrade().is_some());
            let dyn_arc = weak_ref.upgrade().unwrap();
            let arc = dyn_arc.downcast::<u32>().unwrap();
            assert!(Arc::ptr_eq(&arc, sp1.data.get().unwrap()));
        }

        // Clone the `Sp` to increase the strong reference count
        let sp2 = sp1.clone();

        // The `Arc` should be the same for both `Sp`s
        assert!(Arc::ptr_eq(
            sp1.data.get().unwrap(),
            sp2.data.get().unwrap()
        ));
        // Strong count should be 2 now
        assert_eq!(Arc::strong_count(sp1.data.get().unwrap()), 2);

        // Drop one `Sp`
        drop(sp2);

        // Strong count should decrease
        assert_eq!(Arc::strong_count(sp1.data.get().unwrap()), 1);

        // The `Arc` should still be in `sp_cache`
        {
            let sp_cache = arena.lock_sp_cache();
            let sp_cache = sp_cache.borrow();
            assert!(sp_cache.get(&cache_key).is_some());
            let weak_ref = sp_cache.get(&cache_key).unwrap();
            assert!(weak_ref.upgrade().is_some());
            let dyn_arc = weak_ref.upgrade().unwrap();
            let arc = dyn_arc.downcast::<u32>().unwrap();
            assert!(Arc::ptr_eq(&arc, sp1.data.get().unwrap()));
        }

        // Drop the last strong reference
        drop(sp1);

        // Now the `Arc` should be dropped, and the weak reference should be cleaned up
        {
            let sp_cache = arena.lock_sp_cache();
            let sp_cache = sp_cache.borrow();
            assert!(
                sp_cache.get(&cache_key).is_none(),
                "the weak reference should be gone"
            );
        }
    }

    // Test that `Sp::unload` removes the weak reference from the `sp_cache`,
    // but only when the last strong reference is dropped.
    #[test]
    fn sp_cache_sp_unload() {
        let arena = &new_arena();
        let mut sp1 = arena.alloc(42u32);
        let mut sp2 = sp1.clone();
        let cache_key = (sp1.root.clone(), TypeId::of::<u32>());

        // Verify the weak reference exists in the cache
        {
            let sp_cache = arena.lock_sp_cache();
            let sp_cache = sp_cache.borrow();
            let weak_ref = sp_cache.get(&cache_key).unwrap();
            assert!(
                weak_ref.upgrade().is_some(),
                "weak reference should be valid before unload"
            );
        }

        // Unload sp1
        sp1.unload();

        // Verify the weak reference is still in the cache after sp1.unload()
        {
            let sp_cache = arena.lock_sp_cache();
            let sp_cache = sp_cache.borrow();
            let weak_ref = sp_cache.get(&cache_key).unwrap();
            assert!(
                weak_ref.upgrade().is_some(),
                "weak reference should still be valid after unloading sp1"
            );
        }

        // Unload sp2
        sp2.unload();

        // Now the weak reference should be cleaned up
        {
            let sp_cache = arena.lock_sp_cache();
            let sp_cache = sp_cache.borrow();
            assert!(
                sp_cache.get(&cache_key).is_none(),
                "the weak reference should be gone after unloading sp2"
            );
        }
    }

    // Test that attempting to load the same value into the arena twice,
    // independently, using `Arena::alloc`, results in the underlying `Arc`
    // being shared.
    #[test]
    fn sp_cache_alloc_same_data_twice() {
        let arena = &new_arena();
        let sp1 = arena.alloc(42u32);
        let sp2 = arena.alloc(42u32);
        let data1 = sp1.data.get().unwrap();
        let data2 = sp2.data.get().unwrap();
        assert!(
            Arc::ptr_eq(data1, data2),
            "underlying Arc should be shared when allocating the same data"
        );
    }

    // Test that lazy loading a large datastructure works correctly:
    //
    // - only load nodes when requested.
    //
    // - reuse duplicated nodes
    #[test]
    fn lazy_load_large_data_structure() {
        use super::bin_tree::*;
        let arena = &new_arena();

        type BinTree = super::bin_tree::BinTree<DefaultDB>;

        // Build a tree, unload and walk the left fringe, and check that only
        // the left fringe is forced, while also checking that printing doesn't
        // force any lazy sps, by comparing the Debug fmt of the tree with an
        // expected value.
        {
            let mut bt = BinTree::new(0, None, None);
            let depth = 5;
            for i in 1..depth {
                bt = BinTree::new(i, Some(arena.alloc(bt.clone())), Some(arena.alloc(bt)));
            }
            let mut bt = arena.alloc(bt);
            bt.unload();
            let mut p = Some(&bt);
            for _ in 0..depth {
                p = p.unwrap().left.as_ref();
            }
            // Obtain golden value.
            /* println!("{:?}", bt); */
            let golden = "BinTree { value: 4, left: Some(BinTree { value: 3, left: Some(BinTree { value: 2, left: Some(BinTree { value: 1, left: Some(BinTree { value: 0, left: None, right: None }), right: Some(<Lazy Sp>) }), right: Some(<Lazy Sp>) }), right: Some(<Lazy Sp>) }), right: Some(<Lazy Sp>) }";
            let actual = format!("{:?}", bt);
            assert_eq!(actual, golden);
        }

        // Build a large tree (would not fit in memory without sharing of
        // duplicate nodes) where all nodes at the same depth are equal. Unload
        // the tree, construct two lazy pointers into the root, and check that
        // walking down the right fringe of the tree gives the same Arcs as
        // walking down the left fringe.
        {
            // Build the tree.

            let mut bt1 = BinTree::new(0, None, None);
            let depth = 100;
            for i in 1..depth {
                bt1 = BinTree::new(i, Some(arena.alloc(bt1.clone())), Some(arena.alloc(bt1)));
            }
            let mut bt1 = arena.alloc(bt1);

            // Unload and get second pointer.

            let key = bt1.root.clone();
            bt1.unload();
            let bt2 = arena.get_lazy::<BinTree>(&key.into()).unwrap();

            // Walk down the left and right fringes in lock step, checking that
            // nothing is forced prematurely, and that Arcs are shared as
            // expected.

            let mut p1 = Some(&bt1);
            let mut p2 = Some(&bt2);
            for _ in 0..depth {
                assert!(p1.unwrap().data.get().is_none());
                assert!(p2.unwrap().data.get().is_none());
                assert!(Arc::ptr_eq(
                    p1.unwrap().force_as_arc(),
                    p2.unwrap().force_as_arc(),
                ));
                p1 = p1.unwrap().left.as_ref();
                p2 = p2.unwrap().right.as_ref();
            }
        }

        // Construct full tree with no shared nodes, unload, walk a random path,
        // and check that no other nodes were forced.
        {
            // Load a full tree into memory.

            let depth = 13;
            let mut bt = counting_tree(arena, depth);
            // Check that we have a full tree in memory
            assert_eq!(arena.lock_sp_cache().borrow().len(), (1 << depth) - 1);

            // Unload the tree and lazy load a random path.

            bt.unload();
            let mut p = Some(&bt);
            // https://xkcd.com/221/
            let random: u64 = 0x616a7011af5e1b64;
            for i in 0..depth {
                if (random >> i) & 1 == 0 {
                    assert!(p.unwrap().data.get().is_none());
                    p = p.unwrap().left.as_ref();
                } else {
                    assert!(p.unwrap().data.get().is_none());
                    p = p.unwrap().right.as_ref();
                }
            }
            // Check that we only have the forced path in memory.
            assert_eq!(arena.lock_sp_cache().borrow().len(), depth);
        }
    }

    #[cfg(feature = "stress-test")]
    #[test]
    // Remove this "should_panic" once implicit recursion in Sp drop is fixed.
    #[should_panic = "stress test failed: \nthread 'main' has overflowed its stack"]
    fn drop_deeply_nested_data() {
        crate::stress_test::runner::StressTest::new()
            // Must capture, so we can match the output with `should_panic`.
            .with_nocapture(false)
            .run("arena::stress_tests::drop_deeply_nested_data");
    }

    #[cfg(feature = "stress-test")]
    #[test]
    // Remove this "should_panic" once implicit recursion in Sp drop is fixed.
    #[should_panic = "stress test failed: \nthread 'main' has overflowed its stack"]
    fn serialize_deeply_nested_data() {
        crate::stress_test::runner::StressTest::new()
            // Must capture, so we can match the output with `should_panic`.
            .with_nocapture(false)
            .run("arena::stress_tests::serialize_deeply_nested_data");
    }

    #[cfg(all(feature = "stress-test", feature = "sqlite"))]
    #[test]
    fn thrash_the_cache_sqldb() {
        thrash_the_cache("sqldb");
    }
    #[cfg(all(feature = "stress-test", feature = "parity-db"))]
    #[test]
    fn thrash_the_cache_paritydb() {
        thrash_the_cache("paritydb");
    }
    #[cfg(all(
        feature = "stress-test",
        any(feature = "sqlite", feature = "parity-db")
    ))]
    /// Here `db_name` should be `paritydb` or `sqldb`.
    fn thrash_the_cache(db_name: &str) {
        let test_name = &format!("arena::stress_tests::thrash_the_cache_variations_{db_name}");
        let time_limit = 10 * 60;
        // Thrash the cache with p=0.1.
        //
        // Here we should see a small variation between cyclic and non-cyclic,
        // since the cache is very small.
        crate::stress_test::runner::StressTest::new()
            .with_max_runtime(time_limit)
            .run_with_args(test_name, &["0.1", "true"]);
        // Thrash the cache with p=0.5.
        //
        // Don't include cyclic, since it will be the same as in the last test.
        crate::stress_test::runner::StressTest::new()
            .with_max_runtime(time_limit)
            .run_with_args(test_name, &["0.5", "false"]);
        // Thrash the cache with p=0.8.
        //
        // Don't include cyclic, since it will be the same as in the last test.
        crate::stress_test::runner::StressTest::new()
            .with_max_runtime(time_limit)
            .run_with_args(test_name, &["0.8", "false"]);
        // Thrash the cache with p=1.0.
        //
        // With cache as large as the data, cyclic vs non-cyclic should be
        // irrelevant, and this should be the fastest.
        crate::stress_test::runner::StressTest::new()
            .with_max_runtime(time_limit)
            .run_with_args(test_name, &["1.0", "true"]);
    }

    #[cfg(all(feature = "stress-test", feature = "sqlite"))]
    #[test]
    fn load_large_tree_sqldb() {
        crate::stress_test::runner::StressTest::new()
            .with_max_runtime(5 * 60)
            .with_max_memory(2 << 30)
            .run_with_args("arena::stress_tests::load_large_tree_sqldb", &["15"]);
    }

    #[cfg(all(feature = "stress-test", feature = "parity-db"))]
    #[test]
    fn load_large_tree_paritydb() {
        crate::stress_test::runner::StressTest::new()
            .with_max_runtime(5 * 60)
            .with_max_memory(2 << 30)
            .run_with_args("arena::stress_tests::load_large_tree_paritydb", &["15"]);
    }

    #[cfg(all(feature = "stress-test", feature = "sqlite"))]
    #[test]
    fn read_write_map_loop_sqldb() {
        crate::stress_test::runner::StressTest::new()
            .with_max_runtime(60)
            .run_with_args(
                "arena::stress_tests::read_write_map_loop_sqldb",
                &["10000", "1000"],
            );
    }
    #[cfg(all(feature = "stress-test", feature = "parity-db"))]
    #[test]
    fn read_write_map_loop_paritydb() {
        crate::stress_test::runner::StressTest::new()
            .with_max_runtime(60)
            .run_with_args(
                "arena::stress_tests::read_write_map_loop_paritydb",
                &["10000", "1000"],
            );
    }

    // Stress test concurrent arena access, to trigger deadlocks from
    // inconsistent-ordering in mutex acquisition via `Arena::alloc`,
    // `Sp::unload`, `Arena::get_lazy`, `Arena::get`, and `Sp::force_as_arc`
    // (via `Sp::deref()`). For the `Sp` data, we use nested chains of
    // `Option<Sp<_>>`.
    #[test]
    fn concurrent_arena_access() {
        use std::thread;

        type Ty = Sp<Option<Sp<Option<Sp<Option<Sp<Option<Sp<u32>>>>>>>>>;

        let mut threads = std::vec::Vec::new();
        let num_threads = 20;
        for i in 0..num_threads {
            let arena = default_storage().arena.clone();
            threads.push(thread::spawn(move || {
                let mk_sp = |value: u32| -> Ty {
                    let sp = arena.alloc(value);
                    let sp = arena.alloc(Some(sp));
                    let sp = arena.alloc(Some(sp));
                    let sp = arena.alloc(Some(sp));
                    let sp = arena.alloc(Some(sp));
                    sp.clone()
                };
                let mut common_sp = mk_sp(0);
                let mut sp_unique = mk_sp((i + 1) as u32);
                let force_sp = |sp: &Ty| -> u32 {
                    let sp = sp.deref().as_ref().unwrap();
                    let sp = sp.deref().as_ref().unwrap();
                    let sp = sp.deref().as_ref().unwrap();
                    let sp = sp.deref().as_ref().unwrap();
                    *sp.deref()
                };
                for _ in 0..100 {
                    common_sp.unload();
                    sp_unique.unload();
                    let common_val = force_sp(&common_sp);
                    let unique_val = force_sp(&sp_unique);
                    assert_eq!(common_val, 0);
                    assert_eq!(unique_val, (i + 1) as u32);
                    assert_eq!(
                        arena.get(&common_sp.root.clone().into()).unwrap(),
                        common_sp
                    );
                    assert_eq!(
                        arena.get_lazy(&sp_unique.root.clone().into()).unwrap(),
                        sp_unique
                    );
                }
            }));
        }
        // On @ntc2's laptop, without this sleep, the test finishes in less than
        // 2 seconds when it doesn't deadlock. So if we're not done after 10
        // seconds, assume deadlock.
        thread::sleep(std::time::Duration::from_secs(10));
        for t in threads {
            assert!(
                t.is_finished(),
                "deadlock: the threads should finish in about 2 seconds"
            );
        }
    }

    // Test serialization of both eager and lazy sps.
    #[test]
    fn serialize_sp() {
        let arena = &new_arena();

        // Build an Sp with children, of type
        // Sp<Option<Sp<Option<Sp<Option<Sp<Option<Sp<u32>>>>>>>>>.
        let sp = arena.alloc(42u32);
        let sp = arena.alloc(Some(sp));
        let sp = arena.alloc(Some(sp));
        let sp = arena.alloc(Some(sp));
        let mut sp = arena.alloc(Some(sp));

        // Test eager sp.
        let eager_size = Sp::serialized_size(&sp);
        let mut eager_serialization = vec![];
        Sp::serialize(&sp, &mut eager_serialization).unwrap();
        assert_eq!(eager_serialization.len(), eager_size);

        // Test lazy sp. Unload before each serialization operation, since the
        // operations may force the sp.
        sp.unload();
        let lazy_size = Sp::serialized_size(&sp);
        sp.unload();
        let mut lazy_serialization = vec![];
        Sp::serialize(&sp, &mut lazy_serialization).unwrap();
        assert_eq!(lazy_serialization.len(), lazy_size);
    }

    /// Test that serialization and deserialization of a dag with many edges
    /// pointing to the same nodes runs quickly. Here "quickly" means "takes
    /// time proportional to the size of the deduplicated dag, not the size of
    /// the naive fully unfolded/duplicated dag". Concretely, we create a binary
    /// tree where every interior node has equal children, so that such a height
    /// `n` tree has `n` unique nodes, but `2^n - 1` nodes when fully unfolded.
    #[test]
    fn serialize_highly_duplicated_dag() {
        use std::thread;
        use std::time::Duration;

        #[derive(Storable, Clone, PartialEq, Eq, Debug)]
        #[tag = "test-bin-tree"]
        struct BinTree {
            value: u32,
            left: Option<Sp<BinTree>>,
            right: Option<Sp<BinTree>>,
        }

        // Create a tall tree where each interior node has equal children.

        let arena = &new_arena();
        let mut bt = BinTree {
            value: 0,
            left: None,
            right: None,
        };
        let height = 30;
        for i in 1..height {
            bt = BinTree {
                value: i,
                left: Some(arena.alloc(bt.clone())),
                right: Some(arena.alloc(bt)),
            };
        }
        let sp = arena.alloc(bt);

        // Serialize the tree in another thread, panicking if that takes too
        // long.

        println!("serializing ...");
        let handle = std::thread::spawn(move || {
            let mut serialized = vec![];
            Sp::serialize(&sp, &mut serialized).unwrap();
            serialized
        });
        // Sleep at most 5 seconds.
        for _ in 0..50 {
            thread::sleep(Duration::from_millis(100));
            if handle.is_finished() {
                break;
            }
        }
        if !handle.is_finished() {
            panic!("serialize_highly_duplicated_dag: serialization took too long!");
        }
        let serialized = handle.join().unwrap();

        // Deserialize the tree in another thread, panicking if that takes too
        // long.

        println!("deserializing ...");
        let handle = std::thread::spawn(move || {
            let recursive_depth = 0;
            Sp::<BinTree>::deserialize(&mut serialized.as_slice(), recursive_depth).unwrap();
        });
        // Sleep at most 5 seconds.
        for _ in 0..50 {
            thread::sleep(Duration::from_millis(100));
            if handle.is_finished() {
                break;
            }
        }
        if !handle.is_finished() {
            panic!("serialize_highly_duplicated_dag: deserialization took too long!");
        }
        handle.join().unwrap();
    }

    /// Test that we can deserialize data that contains the same key multiple
    /// times at distinct types.
    ///
    /// Although our underlying Merkle dags are un-typed, our `Sp`s are typed, and so
    /// two `Sp`s with different types can have the same key, if their underlying
    /// binary representation as a Merkle node are the same. This test builds a
    /// structure containing two `Sp`s with different types but the same keys, and
    /// checks that it can be round tripped through serialization.
    ///
    /// This test was created to illustrate bug
    /// https://shielded.atlassian.net/browse/PM-16347, where deserialization
    /// was crashing because it was conflating `Sp`s with the same key but
    /// different types.
    #[test]
    fn deserialize_same_key_at_two_different_types() {
        #[derive(Clone, Storable)]
        #[tag = "test-pair"]
        struct Pair {
            // It's essential that the child Sps not be inlined, otherwise we'll
            // have nothing to test!
            #[storable(child)]
            x: Sp<u32>,
            #[storable(child)]
            y: Sp<u64>,
        }

        let arena = &new_arena();

        // Create two Sps with same hash but different types, and ensure they're
        // not inlined when contained in a Pair.
        let x = arena.alloc(0u32);
        let y = arena.alloc(0u64);
        assert_eq!(x.hash().key, y.hash().key);
        assert_ne!(x.type_id(), y.type_id());
        let sp = arena.alloc(Pair { x, y });
        assert_eq!(
            sp.children().len(),
            2,
            "children were inlined, need to fix `Pairg as Storable` impl"
        );

        // Round trip serialization of Pair.
        let mut bytes: Vec<u8> = vec![];
        Sp::serialize(&sp, &mut bytes).unwrap();
        drop(sp);
        let _ = Sp::<Pair, _>::deserialize(&mut bytes.as_slice(), 0).unwrap();
    }

    /// Attempt to `get` an unknown key from the arena and see that we don't
    /// panic. Once upon a time we did.
    #[test]
    fn get_unknown_key() {
        let arena = new_arena();
        let sp = arena.alloc(42u32);
        //let key = VersionedArenaKey::<DefaultHasher>::default();
        let key = sp.hash();
        assert!(arena.get::<u32>(&key).is_ok());
        let arena = new_arena();
        assert!(arena.get::<u32>(&key).is_err());
    }

    /// Test intensive concurrent manipulation of `Sp`s for the same key.
    ///
    /// When originally written, this test exercised a race between removing a
    /// key from the metadata when dropping an Sp, and removing its Arc from the
    /// `sp_cache`. In between, another thread could read the Arc from the `sp_cache`
    /// and assume the key was still in the metadata, an invariant violation that
    /// caused `increment_ref_locked` to panic.
    #[test]
    fn metadata_sp_cache_race() {
        use std::thread;
        let arena = new_arena();

        // Create a persistent key that we can get from both threads.

        let sp = arena.alloc(42u32);
        let key = sp.hash();
        sp.persist();
        drop(sp);

        // Get and drop key repeatedly in current and separate threads.

        let arena1 = arena.clone();
        let key1 = key.clone();
        let t1 = thread::spawn(move || {
            for _ in 0..1000 {
                let sp = arena1.get::<u32>(&key1).unwrap();
                drop(sp);
            }
        });
        for i in 0..1000 {
            // Alternate between get and get_lazy
            if i % 2 == 0 {
                let sp = arena.get_lazy::<u32>(&key).unwrap();
                drop(sp);
            } else {
                let sp = arena.get::<u32>(&key).unwrap();
                drop(sp);
            }
        }
        t1.join().unwrap();
    }

    /// Test that `Sp::is_lazy` correctly classifies laziness.
    #[test]
    fn sp_is_lazy() {
        let arena = new_arena();
        let mut sp = arena.alloc(42u32);

        assert!(!sp.is_lazy());
        sp.unload();
        assert!(sp.is_lazy());
        let _ = sp.deref();
        assert!(!sp.is_lazy());

        let key = sp.hash();
        sp.persist();
        drop(sp);

        let sp = arena.get_lazy::<u32>(&key).unwrap();
        assert!(sp.is_lazy());

        let sp = arena.get::<u32>(&key).unwrap();
        assert!(!sp.is_lazy());
    }
}
