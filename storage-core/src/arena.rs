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
#![allow(clippy::derived_hash_with_manual_eq)]
//! An [`Arena`] for storing Merkle-ized data structures in
//! memory, persisting them to disk, and reloading them from disk.
//!
//! Arena objects are content-addressed by [`ArenaHash`] hashes, and managed via
//! [`Sp`] smart pointers that track in-memory references. See [`StorageBackend`]
//! for the persistence internals, and assumptions about the interaction between
//! the arena and back-end.
use crate::storable::{Loader, child_from};
use crate::storage::{DEFAULT_CACHE_SIZE, default_storage};
use crate::{
    DefaultDB, DefaultHasher,
    backend::{OnDiskObject, StorageBackend},
    db::DB,
};
use crate::{Storable, WellBehavedHasher};
use base_crypto::hash::PERSISTENT_HASH_BYTES;
#[allow(deprecated)]
use crypto::digest::{Digest, OutputSizeUser, crypto_common::generic_array::GenericArray};
use derive_where::derive_where;
use hex::ToHex;
use parking_lot::{ReentrantMutex as SyncMutex, ReentrantMutexGuard as MutexGuard};
use rand::Rng;
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
 
#[cfg(feature = "test-utilities")]
/// A tracking on time spent in reconstruction of data types loading from the backend.
///
/// Tracks a map of type names to a pair for number of times reconstructed, and the total duration
/// of the reconstruction.
pub static TCONSTRUCT: std::sync::Mutex<Option<HashMap<&'static str, (usize, std::time::Duration)>>> = std::sync::Mutex::new(None);

pub(crate) fn hash<'a, H: WellBehavedHasher>(
    root_binary_repr: &[u8],
    child_hashes: impl Iterator<Item = &'a ArenaHash<H>>,
) -> ArenaHash<H> {
    let mut hasher = H::default();
    hasher.update((root_binary_repr.len() as u32).to_le_bytes());
    hasher.update(root_binary_repr);

    for c in child_hashes {
        hasher.update(c.0.clone())
    }

    ArenaHash(hasher.finalize())
}

/// A wrapped `ArenaKey` which includes a tag indicating the content's data type.
/// The tag and key are left intentionally opaque to the end user to reduce the
/// possibility of mishandling the embedded tag.
#[derive_where(Debug, Clone, PartialEq, Eq, Ord, PartialOrd)]
#[derive(Serializable)]
#[phantom(T, H)]
pub struct TypedArenaKey<T: ?Sized, H: WellBehavedHasher> {
    /// the inner key
    pub key: ArenaKey<H>,
    _phantom: PhantomData<T>,
}

impl<T, H: WellBehavedHasher> TypedArenaKey<T, H> {
    /// Returns the referenced children that are *not* directly embedded in this node.
    pub fn refs(&self) -> Vec<&ArenaHash<H>> {
        self.key.refs()
    }
}

impl<T, H: WellBehavedHasher> From<TypedArenaKey<T, H>> for ArenaKey<H> {
    fn from(val: TypedArenaKey<T, H>) -> Self {
        val.key
    }
}

impl<T, H: WellBehavedHasher> From<ArenaKey<H>> for TypedArenaKey<T, H> {
    fn from(val: ArenaKey<H>) -> Self {
        TypedArenaKey {
            key: val,
            _phantom: PhantomData,
        }
    }
}

impl<T: Tagged, H: WellBehavedHasher> Tagged for TypedArenaKey<T, H> {
    fn tag() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Owned(format!("storage-key({})", T::tag()))
    }
    fn tag_unique_factor() -> String {
        "storage-key".into()
    }
}

// newtype is a hack to get the allow lint to work.
#[allow(deprecated)]
type HashArray<H> = GenericArray<u8, <H as OutputSizeUser>::OutputSize>;

/// The key used in the `HashMap` in the Arena. Parameterised on the hash function
/// being used by the arena.
#[derive_where(Clone, PartialEq, Eq, Ord, PartialOrd, Default)]
pub struct ArenaHash<H: Digest = DefaultHasher>(pub HashArray<H>);

impl<H: Digest> Tagged for ArenaHash<H> {
    fn tag() -> std::borrow::Cow<'static, str> {
        "storage-hash".into()
    }
    fn tag_unique_factor() -> String {
        "storage-hash".into()
    }
}

impl<D: DB> Storable<D> for ArenaHash<D::Hasher> {
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
        #[allow(deprecated)]
        let mut array = GenericArray::<u8, <D::Hasher as OutputSizeUser>::OutputSize>::default();
        reader.read_exact(&mut array)?;
        Ok(Self(array))
    }
}

// impl<H: Digest + 'static> WellBehaved for ArenaHash<H> {}

impl<H: Digest> Debug for ArenaHash<H> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.encode_hex::<String>())
    }
}

impl<D: Digest> Hash for ArenaHash<D> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash::<H>(state)
    }

    fn hash_slice<H: std::hash::Hasher>(data: &[Self], state: &mut H)
    where
        Self: Sized,
    {
        #[allow(deprecated)]
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
impl<H: Digest> Serializable for ArenaHash<H> {
    fn serialize(&self, writer: &mut impl std::io::Write) -> std::io::Result<()> {
        writer.write_all(&self.0[..])
    }

    fn serialized_size(&self) -> usize {
        <H as Digest>::output_size()
    }
}

impl<H: Digest> Deserializable for ArenaHash<H> {
    fn deserialize(
        reader: &mut impl std::io::Read,
        _recursive_depth: u32,
    ) -> std::io::Result<Self> {
        let mut res = vec![0u8; <H as Digest>::output_size()];
        reader.read_exact(&mut res[..])?;
        #[allow(deprecated)]
        Ok(ArenaHash(GenericArray::clone_from_slice(&res)))
    }
}

impl<H: Digest> Distribution<ArenaHash<H>> for Standard {
    fn sample<R: rand::prelude::Rng + ?Sized>(&self, rng: &mut R) -> ArenaHash<H> {
        #[allow(deprecated)]
        let mut bytes = GenericArray::default();
        rng.fill_bytes(&mut bytes);
        ArenaHash(bytes)
    }
}

impl<H: Digest> serde::Serialize for ArenaHash<H> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(&self.0[..])
    }
}

impl<'de, H: Digest> serde::Deserialize<'de> for ArenaHash<H> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ArenaHashVisitor<H: Digest>(std::marker::PhantomData<H>);

        impl<'de, H: Digest> serde::de::Visitor<'de> for ArenaHashVisitor<H> {
            type Value = ArenaHash<H>;

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
                #[allow(deprecated)]
                Ok(ArenaHash(GenericArray::clone_from_slice(v)))
            }

            fn visit_byte_buf<E: serde::de::Error>(self, v: Vec<u8>) -> Result<Self::Value, E> {
                self.visit_bytes(&v)
            }
        }

        deserializer.deserialize_bytes(ArenaHashVisitor(std::marker::PhantomData))
    }
}

impl<H: Digest> ArenaHash<H> {
    /// Create an `ArenaHash` from bytes.
    ///
    /// Useful for `printf` debugging of tests.
    ///
    /// The bytes don't need to be as long as the key's internal byte array; the
    /// unspecified values will be filled in with zeros.
    pub(crate) fn _from_bytes(bs: &[u8]) -> Self {
        #[allow(deprecated)]
        let mut bytes = GenericArray::default();
        for (i, b) in bs.iter().enumerate() {
            bytes[i] = *b;
        }
        ArenaHash(bytes)
    }
}

#[derive(Debug, Clone, Storable, Serializable)]
#[derive_where(Hash, PartialEq, Eq, PartialOrd, Ord)]
#[storable(base)]
#[tag = "storage-key[v2]"]
#[phantom(H)]
/// A representataion of an individual child of a [Storable] object.
pub enum ArenaKey<H: WellBehavedHasher = DefaultHasher> {
    /// A by-reference child, which can be looked up in the storage arena.
    Ref(ArenaHash<H>),
    /// A direct child, typically reserved for small children, represented as its raw data.
    Direct(DirectChildNode<H>),
}

impl<H: WellBehavedHasher> From<ArenaHash<H>> for ArenaKey<H> {
    fn from(value: ArenaHash<H>) -> Self {
        ArenaKey::Ref(value)
    }
}

impl<H: WellBehavedHasher> Distribution<ArenaKey<H>> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> ArenaKey<H> {
        ArenaKey::Ref(rng.r#gen())
    }
}

impl<H: WellBehavedHasher> ArenaKey<H> {
    /// Returns the hash of this child.
    pub fn hash(&self) -> &ArenaHash<H> {
        match self {
            ArenaKey::Ref(h) => h,
            ArenaKey::Direct(n) => &n.hash,
        }
    }

    /// Returns the referenced children that are *not* directly embedded in this node.
    pub fn refs(&self) -> Vec<&ArenaHash<H>> {
        let mut res = Vec::with_capacity(32);
        let mut frontier = Vec::with_capacity(32);
        frontier.push(self);
        while let Some(node) = frontier.pop() {
            match node {
                ArenaKey::Ref(n) => res.push(n),
                ArenaKey::Direct(d) => frontier.extend(d.children.iter()),
            }
        }
        res
    }

    /// Returns Some(key) if Ref, None otherwise
    #[cfg(test)]
    pub fn into_ref(&self) -> Option<&ArenaHash<H>> {
        match self {
            ArenaKey::Ref(key) => Some(key),
            ArenaKey::Direct(..) => None,
        }
    }
}

#[derive(Debug, Clone)]
#[derive_where(PartialOrd, Ord, Hash)]
/// The raw data of a child object
pub struct DirectChildNode<H: WellBehavedHasher> {
    /// The data label of this node
    pub data: Arc<Vec<u8>>,
    /// The child nodes
    pub children: Arc<Vec<ArenaKey<H>>>,
    pub(crate) hash: ArenaHash<H>,
    pub(crate) serialized_size: usize,
}

impl<H: WellBehavedHasher> PartialEq for DirectChildNode<H> {
    fn eq(&self, other: &Self) -> bool {
        self.hash == other.hash
    }
}
impl<H: WellBehavedHasher> Eq for DirectChildNode<H> {}

impl<H: WellBehavedHasher> DirectChildNode<H> {
    /// Create a new direct child object from its parts
    pub(crate) fn new(data: Vec<u8>, children: Vec<ArenaKey<H>>) -> Self {
        let hash = crate::arena::hash(&data, children.iter().map(|c| c.hash()));
        let serialized_size = data.serialized_size() + children.serialized_size();
        DirectChildNode {
            data: Arc::new(data),
            children: Arc::new(children),
            hash,
            serialized_size,
        }
    }
}

impl<H: WellBehavedHasher> Serializable for DirectChildNode<H> {
    fn serialize(&self, writer: &mut impl std::io::Write) -> std::io::Result<()> {
        self.data.serialize(writer)?;
        self.children.serialize(writer)
    }
    fn serialized_size(&self) -> usize {
        self.serialized_size
    }
}

impl<H: WellBehavedHasher> Tagged for DirectChildNode<H> {
    fn tag() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("storage-direct-child-node[v1]")
    }
    fn tag_unique_factor() -> String {
        "(vec(u8),vec(storage-key[v2]))".to_owned()
    }
}

impl<H: WellBehavedHasher> Deserializable for DirectChildNode<H> {
    fn deserialize(reader: &mut impl std::io::Read, recursion_depth: u32) -> std::io::Result<Self> {
        let data: Vec<u8> = Deserializable::deserialize(reader, recursion_depth + 1)?;
        let children: Vec<ArenaKey<H>> = Deserializable::deserialize(reader, recursion_depth + 1)?;
        Ok(DirectChildNode::new(data, children))
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
type MetaData<D> = HashMap<ArenaHash<<D as DB>::Hasher>, Node>;

/// An `ArenaHash` together with a type, to avoid collisions when keying typed
/// data by its hash: different types need not have disjoint hashes, and so we
/// need to include the type in the key to avoid collisions in some cases.
type DynTypedArenaHash<H> = (ArenaHash<H>, TypeId);

/// Keys are `hash x type_id` because the hash alone is ambiguous for
/// determining the typed value with this hash: the hash is determined only by
/// the binary serialization, which need not be disjoint across types.
type SpCache<D> =
    HashMap<DynTypedArenaHash<<D as DB>::Hasher>, std::sync::Weak<dyn Any + Sync + Send>>;

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
        let mut data: std::vec::Vec<u8> = std::vec::Vec::new();
        value
            .to_binary_repr(&mut data)
            .expect("Storable data should be able to be represented in binary");
        let child_repr = child_from(&data, &children);
        let root_hash = child_repr.hash().clone();
        if let ArenaKey::Ref(_) = &child_repr {
            self.new_sp_locked(
                &mut self.lock_metadata(),
                value,
                root_hash.clone(),
                data,
                children,
                child_repr,
            )
        } else {
            Sp {
                arena: self.clone(),
                data: OnceLock::from(Arc::new(value)),
                child_repr,
                root: root_hash.clone(),
            }
        }
    }

    /// Create a new `Sp`, taking care of tracking, caching, and ref counting.
    fn new_sp_locked<T: Storable<D>>(
        &self,
        metadata: &mut MutexGuard<'_, RefCell<MetaData<D>>>,
        value: T,
        key: ArenaHash<D::Hasher>,
        data: std::vec::Vec<u8>,
        children: std::vec::Vec<ArenaKey<D::Hasher>>,
        child_repr: ArenaKey<D::Hasher>,
    ) -> Sp<T, D> {
        self.track_locked(metadata, key.clone(), data, children, &child_repr);
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
        Sp::eager(self.clone(), key, arc, child_repr)
    }

    fn new_sp<T: Storable<D>>(
        &self,
        value: T,
        key: ArenaHash<D::Hasher>,
        data: std::vec::Vec<u8>,
        children: std::vec::Vec<ArenaKey<D::Hasher>>,
    ) -> Sp<T, D> {
        let child_node = child_from(&data, &children);
        self.new_sp_locked(
            &mut self.lock_metadata(),
            value,
            key,
            data,
            children,
            child_node,
        )
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
        key: &ArenaHash<D::Hasher>,
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
        key: ArenaHash<D::Hasher>,
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
    fn get_from_cache<T: Storable<D>>(&self, key: &ArenaHash<D::Hasher>) -> Option<Sp<T, D>> {
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
            .map(|arc| {
                let child_repr = arc.as_child();
                Sp::eager(self.clone(), key.clone(), arc, child_repr)
            })
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
        key: &ArenaHash<D::Hasher>,
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
        key: ArenaHash<D::Hasher>,
        data: std::vec::Vec<u8>,
        children: std::vec::Vec<ArenaKey<D::Hasher>>,
        child_repr: &ArenaKey<D::Hasher>,
    ) {
        if !RefCell::borrow(metadata).contains_key(&key) {
            RefCell::borrow_mut(metadata).insert(key.clone(), Node::new());
            if let ArenaKey::Ref(_) = child_repr {
                RefCell::borrow_mut(&self.lock_backend()).cache(key, data, children);
            }
        }
    }

    /// Removes an object from the in-memory arena, remaining in back-end
    /// database if persisted or referenced.
    fn remove_locked(
        &self,
        metadata: &mut MutexGuard<'_, RefCell<MetaData<D>>>,
        key: &ArenaHash<D::Hasher>,
    ) {
        RefCell::borrow_mut(metadata).remove(key);
        RefCell::borrow_mut(&self.lock_backend()).uncache(key);
    }

    fn decrement_ref_locked(
        &self,
        metadata: &mut MutexGuard<'_, RefCell<MetaData<D>>>,
        key: &ArenaHash<D::Hasher>,
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

    fn decrement_ref(&self, key: &ArenaHash<D::Hasher>) {
        self.decrement_ref_locked(&mut self.lock_metadata(), key);
    }

    fn increment_ref_locked(
        &self,
        metadata: &mut MutexGuard<'_, RefCell<MetaData<D>>>,
        key: &ArenaHash<D::Hasher>,
    ) {
        RefCell::borrow_mut(metadata)
            .get_mut(key)
            .expect("attempted to increment non-existant ref")
            .ref_count += 1;
    }

    fn increment_ref(&self, key: &ArenaHash<D::Hasher>) {
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
                        .map(|n| hash::<D::Hasher>(&n.binary_repr, n.children.iter()))
                })
                .collect::<Result<Vec<_>, _>>()?;
            let root = hash::<D::Hasher>(&node.data, children.iter());
            let ir: IntermediateRepr<D> = IntermediateRepr {
                binary_repr: node.data.clone(),
                children,
                db_type: PhantomData,
            };
            existing_nodes.push(ir);
            result = Ok(root);
        }

        let mut key_to_child_repr: HashMap<ArenaHash<<D as DB>::Hasher>, ArenaKey<D::Hasher>> =
            std::collections::HashMap::new();
        for node in nodes.nodes.iter() {
            let children = node
                .child_indices
                .iter()
                .map(|i| {
                    idx_existing_nodes(&existing_nodes, *i)
                        .map(|n| hash::<D::Hasher>(&n.binary_repr, n.children.iter()))
                })
                .collect::<Result<Vec<_>, _>>()?;
            let root = hash::<D::Hasher>(&node.data, children.iter());
            let children = children
                .iter()
                .map(|h| {
                    key_to_child_repr
                        .get(h)
                        .ok_or(std::io::Error::other("child not in key_to_child_repr"))
                })
                .map(|r| r.cloned())
                .collect::<Result<Vec<_>, _>>()?;
            key_to_child_repr.insert(root, child_from(&node.data, &children));
        }

        let key = result?;
        let res: Sp<T, D> = IrLoader {
            arena: self,
            all: &existing_nodes
                .into_iter()
                .map(|node| {
                    (
                        hash::<D::Hasher>(&node.binary_repr, node.children.iter()),
                        node,
                    )
                })
                .collect(),
            recursion_depth: recursive_depth,
            visited: Rc::new(RefCell::new(HashSet::new())),
            key_to_child_repr,
        }
        .get(&ArenaKey::Ref(key))?;
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

#[cfg(feature = "test-utilities")]
struct ConstructTracker(&'static str, std::time::Instant);

#[cfg(feature = "test-utilities")]
impl Drop for ConstructTracker {
    fn drop(&mut self) {
        let dt = self.1.elapsed();
        let mut construct_map = TCONSTRUCT.lock().unwrap();
        let (nconstruct, tconstruct) = construct_map.get_or_insert_default().entry(self.0).or_default();
        *nconstruct += 1;
        *tconstruct += dt;
    }
}

impl<D: DB> Loader<D> for BackendLoader<'_, D> {
    const CHECK_INVARIANTS: bool = false;

    fn get<T: Storable<D>>(
        &self,
        child: &ArenaKey<<D as DB>::Hasher>,
    ) -> Result<Sp<T, D>, std::io::Error> {
        #[cfg(feature = "test-utilities")]
        let _tracker = ConstructTracker(std::any::type_name::<T>(), std::time::Instant::now());
        let key = match child {
            ArenaKey::Direct(direct_node) => {
                let value = T::from_binary_repr(
                    &mut &direct_node.data[..],
                    &mut direct_node.children.iter().cloned(),
                    self,
                )?;
                return Ok(Sp::new(value));
            }
            ArenaKey::Ref(key) => key,
        };
        // Build from existing cached value if possible.

        // Avoid race: keep the metadata locked until we call `Sp::eager` //
        // below, so that no one can sneak in and remove `key` from the
        // metadata in the mean time.
        let metadata_lock = self.arena.lock_metadata();
        let maybe_arc = self
            .arena
            .read_sp_cache_locked::<T>(&self.arena.lock_sp_cache(), key);
        if let Some(arc) = maybe_arc {
            let child_repr = arc.as_child();
            return Ok(Sp::eager(self.arena.clone(), key.clone(), arc, child_repr));
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
            let child_repr = child_from(&obj.data, &obj.children);
            self.arena.track_locked(
                &metadata_lock,
                key.clone(),
                obj.data,
                obj.children,
                &child_repr,
            );
            return Ok(Sp::lazy(self.arena.clone(), key.clone(), child_repr));
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
    all: &'a HashMap<ArenaHash<D::Hasher>, IntermediateRepr<D>>,
    recursion_depth: u32,
    /// The keys we've already deserialized once.
    visited: Rc<RefCell<HashSet<DynTypedArenaHash<D::Hasher>>>>,
    key_to_child_repr: HashMap<ArenaHash<D::Hasher>, ArenaKey<D::Hasher>>,
}

#[cfg(test)]
impl<'a, D: DB> IrLoader<'a, D> {
    pub(crate) fn new(
        arena: &'a Arena<D>,
        all: &'a HashMap<ArenaHash<D::Hasher>, IntermediateRepr<D>>,
        key_to_child_repr: HashMap<ArenaHash<D::Hasher>, ArenaKey<D::Hasher>>,
    ) -> IrLoader<'a, D> {
        IrLoader {
            arena,
            all,
            recursion_depth: 0,
            visited: Rc::new(RefCell::new(HashSet::new())),
            key_to_child_repr,
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
        child: &ArenaKey<<D as DB>::Hasher>,
    ) -> Result<Sp<T, D>, std::io::Error> {
        let key = match child {
            ArenaKey::Direct(child) => {
                let value = T::from_binary_repr(
                    &mut &child.data[..],
                    &mut child.children.iter().cloned(),
                    self,
                )?;
                return Ok(self.arena.alloc(value));
            }
            ArenaKey::Ref(key) => key,
        };
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
            return Err(std::io::Error::other("Reached recursion limit".to_string()));
        }
        let loader = IrLoader {
            arena: self.arena,
            all: self.all,
            recursion_depth: self.recursion_depth + 1,
            visited: self.visited.clone(),
            key_to_child_repr: self.key_to_child_repr.clone(),
        };
        let sp = self.arena.alloc(T::from_binary_repr(
            &mut ir.binary_repr.clone().as_slice(),
            &mut ir.children.clone().into_iter().map(|k| {
                self.key_to_child_repr
                    .get(&k)
                    .expect("should be able to convert child ArenaHash to ArenaKey")
                    .clone()
            }),
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
    children: std::vec::Vec<ArenaHash<D::Hasher>>,
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
            children: s.children().into_iter().map(|n| n.hash().clone()).collect(),
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
    ref_count: u64,
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
    /// This Sp represented as a child node (for easy access)
    pub child_repr: ArenaKey<D::Hasher>,
    /// The arena this Sp points into
    pub arena: Arena<D>,
    /// The persistent hash of data.
    pub root: ArenaHash<D::Hasher>,
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
    fn eager(
        arena: Arena<D>,
        root: ArenaHash<D::Hasher>,
        arc: Arc<T>,
        child_repr: ArenaKey<D::Hasher>,
    ) -> Self {
        let sp = Sp::lazy(arena, root, child_repr);
        let _ = sp.data.set(arc);
        sp
    }

    /// Converts this Sp into one that is tracked directly, making it possible
    /// to lookup by its hash.
    ///
    /// This forces the Sp to be considered a reference when used as a child of
    /// other Sps, and places it into internal caches, and eventually the
    /// database if persisted or a parent is persisted.
    pub fn into_tracked(&self) -> Self {
        match &self.child_repr {
            ArenaKey::Direct(dcn) => {
                let child_repr = ArenaKey::Ref(self.root.clone());
                self.arena.track_locked(
                    &self.arena.lock_metadata(),
                    self.root.clone(),
                    (*dcn.data).clone(),
                    (*dcn.children).clone(),
                    &child_repr,
                );
                Sp {
                    data: self.data.clone(),
                    child_repr,
                    arena: self.arena.clone(),
                    root: self.root.clone(),
                }
            }
            ArenaKey::Ref(_) => self.clone(),
        }
    }

    /// Create a new `Sp` with an uninitialized data payload.
    ///
    /// Note: this function assumes that `root` is already in `metadata`, and
    /// will panic if not. If you're creating a new `Sp` for a key x type that's
    /// new to the cache, then you should call `Arena::track_locked` to register
    /// the `Sp` before creating it. Note that `track_locked` is a no-op for
    /// already registered root keys, so there is no harm in calling it if
    /// you're not sure.
    fn lazy(arena: Arena<D>, root: ArenaHash<D::Hasher>, child_repr: ArenaKey<D::Hasher>) -> Self {
        // This `increment_ref` will panic if the child ref is not in `metadata`.
        if let ArenaKey::Ref(hash) = &child_repr {
            arena.increment_ref(hash);
        }
        let data = OnceLock::new();
        Sp {
            data,
            arena,
            root,
            child_repr,
        }
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
        if let ArenaKey::Ref(_) = self.child_repr {
            self.arena.increment_ref(&self.root);
        }
        Sp {
            root: self.root.clone(),
            child_repr: self.child_repr.clone(),
            arena: self.arena.clone(),
            data: self.data.clone(),
        }
    }
}

impl<D: DB> Sp<dyn Any + Send + Sync, D> {
    /// Downcasts this dynamically typed pointer to a concrete type, if possible.
    pub fn downcast<T: Any + Send + Sync>(&self) -> Option<Sp<T, D>> {
        if let ArenaKey::Ref(_) = self.child_repr {
            self.arena.increment_ref(&self.root);
        }
        let data: OnceLock<Arc<T>> = match self.data.get() {
            Some(arc) => {
                let concrete_arc: Arc<T> = arc.clone().downcast().ok()?;
                concrete_arc.into()
            }
            None => OnceLock::new(),
        };
        Some(Sp {
            root: self.root.clone(),
            child_repr: self.child_repr.clone(),
            arena: self.arena.clone(),
            data,
        })
    }

    /// Downcasts this dynamically typed pointer to a concrete type, but pushes through the cast
    /// regardless of the underlying type.
    ///
    /// This will effectively unload the Sp, and construct a new lazy Sp with the same backing
    /// data. There is no way of knowing if this will succeed, as the lazy loading will defer
    /// failure to a context where a failure panics.
    pub fn force_downcast<T: Any + Send + Sync>(&self) -> Sp<T, D> {
        if let ArenaKey::Ref(_) = self.child_repr {
            self.arena.increment_ref(&self.root);
        }
        let data: OnceLock<Arc<T>> = match self.data.get().map(|arc| arc.clone().downcast::<T>()) {
            Some(Ok(concrete_arc)) => concrete_arc.into(),
            None | Some(Err(_)) => OnceLock::new(),
        };
        Sp {
            root: self.root.clone(),
            child_repr: self.child_repr.clone(),
            arena: self.arena.clone(),
            data,
        }
    }
}

impl<T: Any + Send + Sync, D: DB> Sp<T, D> {
    /// Casts this pointer into a dynamically typed `Any` pointer.
    pub fn upcast(&self) -> Sp<dyn Any + Send + Sync, D> {
        if let ArenaKey::Ref(_) = self.child_repr {
            self.arena.increment_ref(&self.root);
        }
        let data: OnceLock<Arc<dyn Any + Send + Sync>> = match self.data.get() {
            Some(arc) => {
                let dyn_arc: Arc<dyn Any + Send + Sync> = arc.clone();
                dyn_arc.into()
            }
            None => OnceLock::new(),
        };
        Sp {
            root: self.root.clone(),
            child_repr: self.child_repr.clone(),
            arena: self.arena.clone(),
            data,
        }
    }
}

impl<T: ?Sized, D: DB> Sp<T, D> {
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
    pub fn hash(&self) -> ArenaHash<D::Hasher> {
        self.root.clone()
    }

    /// Returns the [`TypedArenaKey`] representation of this Sp, useful as a
    /// reference to persist.
    pub fn as_typed_key(&self) -> TypedArenaKey<T, D::Hasher> {
        TypedArenaKey {
            key: self.as_child(),
            _phantom: PhantomData,
        }
    }

    /// Returns the [`ArenaKey`] representation of this Sp, being either a ref
    /// to `[Sp::hash]`, or the direct encoding for small children.
    pub fn as_child(&self) -> ArenaKey<D::Hasher> {
        self.child_repr.clone()
    }
}

impl<T: Storable<D>, D: DB> Sp<T, D> {
    /// Notify the storage back-end to increment the persist count on this object.
    ///
    /// See `[StorageBackend::persist]`.
    pub fn persist(&mut self) {
        // Promote self to Ref if not already
        if let ArenaKey::Direct(..) = self.child_repr {
            let mut data: std::vec::Vec<u8> = std::vec::Vec::new();
            let value = self
                .data
                .get()
                .expect("A Direct node must contain it's data");
            value
                .to_binary_repr(&mut data)
                .expect("Storable data should be able to be represented in binary");
            let child_repr = ArenaKey::Ref(self.root.clone());
            let new_sp = self.arena.new_sp_locked(
                &mut self.arena.lock_metadata(),
                value.as_ref().clone(),
                self.root.clone(),
                data,
                self.children(),
                child_repr,
            );
            *self = new_sp;
        }
        self.arena.with_backend(|backend| {
            self.child_repr
                .refs()
                .into_iter()
                .for_each(|ref_| backend.persist(ref_))
        });
    }

    /// Notify the storage back-end to decrement the persist count on this
    /// object.
    ///
    /// See `[StorageBackend::unpersist]`.
    pub fn unpersist(&self) {
        self.arena.with_backend(|backend| {
            self.child_repr
                .refs()
                .into_iter()
                .for_each(|ref_| backend.unpersist(ref_))
        });
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
                    let sp: Sp<T, _> =
                        match Sp::from_arena(&self.arena, &self.as_child(), max_depth) {
                            Ok(v) => v,
                            Err(e) => panic!(
                                "root should be in the arena (T={}): {e:?}",
                                std::any::type_name::<T>()
                            ),
                        };
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
        let root = self.child_repr.clone();
        // Topological sort using Kahn's algorithm.
        // However, we need to know the incoming vertices of a given node, so to start, we just walk
        // the graph to get a better representation:
        //
        // node hash -> incoming
        // node hash -> OnDiskObjecct
        let mut incoming_vertices: HashMap<_, usize> = HashMap::new();
        let mut disk_objects = HashMap::new();
        let mut frontier = vec![root.clone()];
        while let Some(child) = frontier.pop() {
            if disk_objects.contains_key(child.hash()) {
                continue;
            }
            let node = match child {
                ArenaKey::Ref(ref key) => arena
                    .lock_backend()
                    .borrow_mut()
                    .get(key)
                    .expect("Arena should contain current serialization target")
                    .clone(),
                ArenaKey::Direct(ref d) => OnDiskObject {
                    data: d.data.as_ref().clone(),
                    ref_count: 0,
                    children: d.children.as_ref().clone(),
                },
            };
            for child in node.children.iter() {
                *incoming_vertices.entry(child.clone()).or_default() += 1;
                frontier.push(child.clone());
            }
            raw_size_limit = raw_size_limit
                .checked_sub(PERSISTENT_HASH_BYTES as u64 + node.data.len() as u64)?;
            disk_objects.insert(child.hash().clone(), node);
        }
        // now we can use Kahn's algorithm as specified
        let mut list_indices = HashMap::new();
        // Note that only the root should have no incoming edges to start
        let mut empty_incoming_nodes = vec![root.clone()];
        while let Some(node) = empty_incoming_nodes.pop() {
            if list_indices.contains_key(&node) {
                continue;
            }
            let disk = disk_objects.get(node.hash()).expect("node must be present");
            list_indices.insert(node.clone(), list_indices.len() as u64);
            for child in disk.children.iter() {
                let incoming = incoming_vertices
                    .get_mut(child)
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
        for (child_node, idx) in list_indices.iter() {
            let disk = disk_objects
                .remove(child_node.hash())
                .expect("node must be present");
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
        // We only need to do this on refs, because others aren't actually
        // ref-counted. Additionally, note that if we have a Direct node here,
        // then the children contained within this Sp will do their own cleanup.
        self.unload();
        if let ArenaKey::Ref(hash) = &self.child_repr {
            // It's important that we unload() before calling decrement_ref(),
            // because unload() is responsible for cleaning up the sp_cache, and
            // decrement_ref() is responsible for cleaning up the metadata, and the
            // invariant is that any Arc in the sp_cache must have a corresponding
            // entry in the metadata.
            self.arena.decrement_ref(hash);
        }
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

#[derive_where(Clone)]
/// An opaque storable data structure. Any storable data can be read as an opaque object, but
/// cannot be practically mutated from there.
pub struct Opaque<D: DB> {
    data: Vec<u8>,
    children: Vec<Sp<dyn Any + Send + Sync, D>>,
}

impl<D: DB> Storable<D> for Opaque<D> {
    fn children(&self) -> std::vec::Vec<ArenaKey<<D as DB>::Hasher>> {
        self.children.iter().map(|child| child.as_child()).collect()
    }
    fn to_binary_repr<W: std::io::Write>(&self, writer: &mut W) -> Result<(), std::io::Error>
    where
        Self: Sized,
    {
        writer.write_all(&self.data)
    }
    fn from_binary_repr<R: std::io::Read>(
        reader: &mut R,
        child_nodes: &mut impl Iterator<Item = ArenaKey<<D as DB>::Hasher>>,
        loader: &impl Loader<D>,
    ) -> Result<Self, std::io::Error>
    where
        Self: Sized,
    {
        let mut data = Vec::new();
        reader.read_to_end(&mut data)?;
        let children = child_nodes
            .map(|hash| loader.get::<Opaque<_>>(&hash).map(|sp| sp.upcast()))
            .collect::<Result<_, _>>()?;
        Ok(Self { data, children })
    }
}

impl<D: DB> Storable<D> for Sp<dyn Any + Send + Sync, D> {
    fn children(&self) -> std::vec::Vec<ArenaKey<<D as DB>::Hasher>> {
        match &self.child_repr {
            ArenaKey::Direct(key) => key.children.deref().clone(),
            ArenaKey::Ref(hash) => self.arena.with_backend(|backend| {
                backend
                    .get(hash)
                    .expect("ref Sp must be in backend")
                    .children
                    .clone()
            }),
        }
    }
    fn from_binary_repr<R: std::io::Read>(
        reader: &mut R,
        child_nodes: &mut impl Iterator<Item = ArenaKey<<D as DB>::Hasher>>,
        loader: &impl Loader<D>,
    ) -> Result<Self, std::io::Error>
    where
        Self: Sized,
    {
        Opaque::from_binary_repr(reader, child_nodes, loader).map(|opaque| Sp::new(opaque).upcast())
    }
    fn to_binary_repr<W: std::io::Write>(&self, writer: &mut W) -> Result<(), std::io::Error>
    where
        Self: Sized,
    {
        match &self.child_repr {
            ArenaKey::Direct(key) => writer.write_all(&key.data),
            ArenaKey::Ref(hash) => self.arena.with_backend(|backend| {
                writer.write_all(&backend.get(hash).expect("ref Sp must be in backend").data)
            }),
        }
    }
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
        T::check_invariant(self)
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
pub mod bin_tree {
    use super::*;
    use crate::{self as storage, storable::SMALL_OBJECT_LIMIT};
    use macros::Storable;
    use std::fmt;

    #[derive(Storable)]
    #[derive_where(Clone, PartialEq, Eq)]
    #[tag = "test-bin-tree"]
    #[storable(db = D)]
    /// A binary tree used for stress-testing
    pub struct BinTree<D: DB> {
        value: u64,
        pub(crate) left: Option<Sp<BinTree<D>, D>>,
        pub(crate) right: Option<Sp<BinTree<D>, D>>,
        _data: [u8; SMALL_OBJECT_LIMIT], // used to ensure nodes are not in-lined
    }

    impl<D: DB> fmt::Debug for BinTree<D> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("BinTree")
                .field("value", &self.value)
                .field("left", &self.left)
                .field("right", &self.right)
                .finish()
        }
    }

    impl<D: DB> BinTree<D> {
        /// Create a new `BinTree`
        pub fn new(
            value: u64,
            left: Option<Sp<BinTree<D>, D>>,
            right: Option<Sp<BinTree<D>, D>>,
        ) -> BinTree<D> {
            BinTree {
                value,
                left,
                right,
                _data: [0; SMALL_OBJECT_LIMIT],
            }
        }

        /// Return sum of all node values.
        ///
        /// The point is that this forces the whole tree to be loaded.
        #[cfg(all(
            feature = "stress-test",
            any(feature = "parity-db", feature = "sqlite")
        ))]
        pub fn sum(&self) -> u64 {
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
    pub fn counting_tree<D: DB>(arena: &Arena<D>, height: usize) -> Sp<BinTree<D>, D> {
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
    pub fn get_root_count<D: DB>(arena: &Arena<D>, key: &ArenaHash<D::Hasher>) -> u32 {
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
        key: &ArenaHash<D::Hasher>,
    ) -> Option<Arc<T>> {
        arena.read_sp_cache_locked::<T>(&arena.lock_sp_cache(), key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate as storage;
    use crate::DefaultHasher;
    use crate::storable::SMALL_OBJECT_LIMIT;
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
        let val = [0; SMALL_OBJECT_LIMIT];
        let map = new_arena();
        let _malloced_a = map.alloc::<[u8; SMALL_OBJECT_LIMIT]>(val);
        let _malloced_b = map.alloc::<[u8; SMALL_OBJECT_LIMIT]>(val);
        assert_eq!(map.size(), 1)
    }

    #[test]
    fn drop_node() {
        let map = new_arena();
        let _malloc_a = map.alloc::<[u8; SMALL_OBJECT_LIMIT]>([0; SMALL_OBJECT_LIMIT]);
        {
            let _malloc_b = map.alloc::<[u8; SMALL_OBJECT_LIMIT]>([1; SMALL_OBJECT_LIMIT]);
            assert_eq!(map.size(), 2);
        }
        assert_eq!(map.size(), 1);
    }

    #[test]
    fn clone_increment_refcount() {
        let map = new_arena();
        let payload = [0; SMALL_OBJECT_LIMIT]; // must be larger than SMALL_OBJECT_LIMIT
        let malloc_a = map.alloc::<[u8; SMALL_OBJECT_LIMIT]>(payload);
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
        let sp1 = arena.alloc([42u8; SMALL_OBJECT_LIMIT]);
        let root_key = sp1.root.clone();
        let type_id = TypeId::of::<[u8; SMALL_OBJECT_LIMIT]>();
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
            let arc = dyn_arc.downcast::<[u8; SMALL_OBJECT_LIMIT]>().unwrap();
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
            let arc = dyn_arc.downcast::<[u8; SMALL_OBJECT_LIMIT]>().unwrap();
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
        let mut sp1 = arena.alloc([42u8; SMALL_OBJECT_LIMIT]);
        let mut sp2 = sp1.clone();
        let cache_key = (sp1.root.clone(), TypeId::of::<[u8; SMALL_OBJECT_LIMIT]>());

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
        let sp1 = arena.alloc([0u8; SMALL_OBJECT_LIMIT]);
        let sp2 = arena.alloc([0u8; SMALL_OBJECT_LIMIT]);
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
            let actual = format!("{:?}", bt);
            dbg!(&actual);
            assert!(actual.ends_with("right: Some(<Lazy Sp>) }), right: Some(<Lazy Sp>) }), right: Some(<Lazy Sp>) }), right: Some(<Lazy Sp>) }"));
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

            let key = bt1.as_typed_key();
            bt1.unload();
            let bt2 = arena.get_lazy::<BinTree>(&key).unwrap();

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
    #[should_panic = "has overflowed its stack"]
    fn drop_deeply_nested_data() {
        crate::stress_test::runner::StressTest::new()
            // Must capture, so we can match the output with `should_panic`.
            .with_nocapture(false)
            .run("arena::stress_tests::drop_deeply_nested_data");
    }

    #[cfg(feature = "stress-test")]
    #[test]
    // Remove this "should_panic" once implicit recursion in Sp drop is fixed.
    #[should_panic = "has overflowed its stack"]
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
                    assert_eq!(arena.get(&common_sp.as_typed_key()).unwrap(), common_sp);
                    assert_eq!(
                        arena.get_lazy(&sp_unique.as_typed_key()).unwrap(),
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
        assert_eq!(x.as_typed_key().key, y.as_typed_key().key);
        assert_ne!(x.type_id(), y.type_id());
        let sp = arena.alloc(Pair { x, y });
        assert_eq!(
            sp.children().len(),
            2,
            "children were inlined, need to fix `Pair as Storable` impl"
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
        let sp = arena.alloc([0; SMALL_OBJECT_LIMIT]);
        //let key = VersionedArenaHash::<DefaultHasher>::default();
        let key = sp.as_typed_key();
        assert!(arena.get::<[u8; SMALL_OBJECT_LIMIT]>(&key).is_ok());
        let arena = new_arena();
        assert!(arena.get::<[u8; SMALL_OBJECT_LIMIT]>(&key).is_err());
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

        let mut sp = arena.alloc(42u32);
        let key = sp.as_typed_key();
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
        let mut sp = arena.alloc([42u8; SMALL_OBJECT_LIMIT]);

        assert!(!sp.is_lazy());
        sp.unload();
        assert!(sp.is_lazy());
        let _ = sp.deref();
        assert!(!sp.is_lazy());

        let key = sp.as_typed_key();
        sp.persist();
        drop(sp);

        let sp = arena.get_lazy::<[u8; SMALL_OBJECT_LIMIT]>(&key).unwrap();
        assert!(sp.is_lazy());

        let sp = arena.get::<[u8; SMALL_OBJECT_LIMIT]>(&key).unwrap();
        assert!(!sp.is_lazy());
    }

    #[test]
    fn serialize_small_sp() {
        let arena = new_arena();
        let sp = arena.alloc(42u32);
        let mut bytes: Vec<u8> = vec![];
        Sp::serialize(&sp, &mut bytes).unwrap();
        let other_sp = Sp::deserialize(&mut bytes.as_slice(), 0).unwrap();
        assert_eq!(sp, other_sp);
    }
}
