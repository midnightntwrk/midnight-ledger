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

//! Traits for defining new storage mechanisms

use crate as storage;
use crate::arena::{Arena, ArenaKey, Sp};
use crate::backend::StorageBackend;
use crate::db::{DB, DummyArbitrary, InMemoryDB};
use crate::merkle_patricia_trie::Annotation;
use crate::merkle_patricia_trie::MerklePatriciaTrie;
use crate::merkle_patricia_trie::Semigroup;
use crate::storable::Loader;
use crate::storable::SizeAnn;
use crate::{DefaultDB, Storable};
use base_crypto::time::Timestamp;
use crypto::digest::Digest;
use derive_where::derive_where;
use parking_lot::{Mutex, MutexGuard};
#[cfg(feature = "proptest")]
use proptest::arbitrary::Arbitrary;
#[cfg(feature = "proptest")]
use proptest::strategy::{BoxedStrategy, Strategy};
use rand::distributions::{Distribution, Standard};
#[cfg(feature = "proptest")]
use serialize::NoStrategy;
use serialize::{Deserializable, Serializable, Tagged, tag_enforcement_test};
use sha2::Sha256;
use std::any::{Any, TypeId};
use std::borrow::Borrow;
use std::fmt::{Debug, Formatter};
use std::hash::Hash;
use std::marker::PhantomData;
use std::ops::Deref;
use std::sync::{Arc, LazyLock};

/// Storage backed by an in-memory hash map, indexed by SHA256 hashes
pub type InMemoryStorage = Storage<InMemoryDB<Sha256>>;
/// The default size of the storage cache.
///
/// This size is in number of cache objects, not megabytes consumed! This value
/// is not well motivated, and we may want to change it later, or better yet,
/// refactor the back-end to track the memory size of the cache, instead of the
/// number of cached objects.
pub const DEFAULT_CACHE_SIZE: usize = 1024 * 1024;

/// A map from key hashes to values
#[derive(Storable)]
#[derive_where(Clone, Eq, PartialEq; V, A)]
#[storable(db = D, invariant = HashMap::invariant)]
pub struct HashMap<
    K: Serializable + Storable<D>,
    V: Storable<D>,
    D: DB = DefaultDB,
    A: Storable<D> + Annotation<(Sp<K, D>, Sp<V, D>)> = SizeAnn,
>(#[allow(clippy::type_complexity)] Map<ArenaKey<D::Hasher>, (Sp<K, D>, Sp<V, D>), D, A>);

impl<
    K: Serializable + Storable<D> + Tagged,
    V: Storable<D> + Tagged,
    D: DB,
    A: Storable<D> + Annotation<(Sp<K, D>, Sp<V, D>)> + Tagged,
> Tagged for HashMap<K, V, D, A>
{
    fn tag() -> std::borrow::Cow<'static, str> {
        format!("hash-map({},{},{})", K::tag(), V::tag(), A::tag()).into()
    }
    fn tag_unique_factor() -> String {
        <Map<ArenaKey<D::Hasher>, (Sp<K, D>, Sp<V, D>), D, A>>::tag_unique_factor()
    }
}
tag_enforcement_test!(HashMap<(), ()>);

impl<
    K: Serializable + Storable<D>,
    V: Storable<D> + PartialEq,
    D: DB,
    A: Storable<D> + Annotation<(Sp<K, D>, Sp<V, D>)>,
> Hash for HashMap<K, V, D, A>
{
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl<
    K: Serializable + Storable<D> + PartialOrd,
    V: Storable<D> + PartialOrd,
    D: DB,
    A: Storable<D> + PartialOrd + Annotation<(Sp<K, D>, Sp<V, D>)>,
> PartialOrd for HashMap<K, V, D, A>
{
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.0.partial_cmp(&other.0)
    }
}

impl<
    K: Serializable + Storable<D> + Ord,
    V: Storable<D> + Ord,
    D: DB,
    A: Storable<D> + Ord + Annotation<(Sp<K, D>, Sp<V, D>)>,
> Ord for HashMap<K, V, D, A>
{
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.cmp(&other.0)
    }
}

impl<
    K: Debug + Serializable + Storable<D>,
    V: Debug + Storable<D>,
    D: DB,
    A: Storable<D> + Annotation<(Sp<K, D>, Sp<V, D>)>,
> Debug for HashMap<K, V, D, A>
{
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_map()
            .entries(self.iter().map(|kv| (kv.0.clone(), kv.1.clone())))
            .finish()
    }
}

#[cfg(feature = "proptest")]
impl<
    K: Storable<D> + Debug + Serializable,
    V: Storable<D> + Debug,
    D: DB,
    A: Storable<D> + Annotation<(Sp<K, D>, Sp<V, D>)>,
> Arbitrary for HashMap<K, V, D, A>
where
    Standard: Distribution<V> + Distribution<K>,
{
    type Strategy = NoStrategy<HashMap<K, V, D, A>>;
    type Parameters = ();

    fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
        NoStrategy(PhantomData)
    }
}

impl<
    D: DB,
    K: Serializable + Storable<D>,
    V: Storable<D>,
    A: Storable<D> + Annotation<(Sp<K, D>, Sp<V, D>)>,
> Distribution<HashMap<K, V, D, A>> for Standard
where
    Standard: Distribution<V> + Distribution<K>,
{
    fn sample<R: rand::Rng + ?Sized>(&self, rng: &mut R) -> HashMap<K, V, D, A> {
        let mut map = HashMap::new();
        let size: usize = rng.gen_range(0..8);

        for _ in 0..size {
            map = map.insert(rng.r#gen(), rng.r#gen())
        }

        map
    }
}

impl<
    K: serde::Serialize + Serializable + Storable<D>,
    V: serde::Serialize + Storable<D>,
    D: DB,
    A: Storable<D> + Annotation<(Sp<K, D>, Sp<V, D>)>,
> serde::Serialize for HashMap<K, V, D, A>
{
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.collect_map(
            self.iter()
                .map(|kv| (kv.0.deref().clone(), kv.1.deref().clone())),
        )
    }
}

struct HashMapVisitor<K, V, D, A>(PhantomData<(K, V, D, A)>);

impl<
    'de,
    K: serde::Deserialize<'de> + Serializable + Storable<D>,
    V: serde::Deserialize<'de> + Storable<D>,
    D: DB,
    A: Storable<D> + Annotation<(Sp<K, D>, Sp<V, D>)>,
> serde::de::Visitor<'de> for HashMapVisitor<K, V, D, A>
{
    type Value = HashMap<K, V, D, A>;

    fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
        write!(formatter, "a hashmap")
    }

    fn visit_map<ACC: serde::de::MapAccess<'de>>(
        self,
        mut seq: ACC,
    ) -> Result<HashMap<K, V, D, A>, ACC::Error> {
        std::iter::from_fn(|| seq.next_entry::<K, V>().transpose()).collect()
    }
}

impl<
    'de,
    K: serde::Deserialize<'de> + Serializable + Storable<D1>,
    V: serde::Deserialize<'de> + Storable<D1>,
    D1: DB,
    A: Storable<D1> + Annotation<(Sp<K, D1>, Sp<V, D1>)>,
> serde::Deserialize<'de> for HashMap<K, V, D1, A>
{
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        de.deserialize_map(HashMapVisitor(PhantomData))
    }
}

impl<
    K: Serializable + Storable<D>,
    V: Storable<D>,
    D: DB,
    A: Storable<D> + Annotation<(Sp<K, D>, Sp<V, D>)>,
> Default for HashMap<K, V, D, A>
{
    fn default() -> Self {
        Self::new()
    }
}

impl<
    K: Serializable + Storable<D>,
    V: Storable<D>,
    D: DB,
    A: Storable<D> + Annotation<(Sp<K, D>, Sp<V, D>)>,
> HashMap<K, V, D, A>
{
    /// Creates an empty map
    pub fn new() -> Self {
        Self(Map::new())
    }

    fn gen_key(key: &K) -> ArenaKey<D::Hasher> {
        let mut hasher = D::Hasher::default();
        let mut bytes: std::vec::Vec<u8> = std::vec::Vec::new();
        K::serialize(key, &mut bytes).expect("HashMap key should be serializable");
        hasher.update(bytes);
        ArenaKey(hasher.finalize())
    }

    /// Insert object value in map, keyed with the hash of object key. Overwrites
    /// any preexisting object under the same key
    #[must_use]
    pub fn insert(&self, key: K, value: V) -> Self {
        HashMap(self.0.insert(
            Self::gen_key(&key),
            (
                self.0.mpt.0.arena.alloc(key),
                self.0.mpt.0.arena.alloc(value),
            ),
        ))
    }

    fn invariant(&self) -> Result<(), std::io::Error> {
        for (hash, v) in self.0.iter() {
            let key = &*v.0;
            let hash2 = Self::gen_key(key);
            if hash != hash2 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "hashmap key doesn't match serialized hash",
                ));
            }
        }
        Ok(())
    }

    /// Get object keyed by the hash of object key
    pub fn get(&self, key: &K) -> Option<Sp<V, D>> {
        self.0.get(&Self::gen_key(key)).map(|(_, v)| v.clone())
    }

    /// Remove object keyed by the hash of object key
    #[must_use]
    pub fn remove(&self, key: &K) -> Self {
        HashMap(self.0.remove(&Self::gen_key(key)))
    }

    /// Check if the map contains a key.
    pub fn contains_key(&self, key: &K) -> bool {
        self.0.contains_key(&Self::gen_key(key))
    }

    /// Consume internal pointers, returning only the leaves left dangling by this.
    /// Used for custom `Drop` implementations.
    pub fn into_inner_for_drop(self) -> impl Iterator<Item = (Option<K>, Option<V>)> {
        self.0.into_inner_for_drop().filter_map(|(k, v)| {
            let (k, v) = (Sp::into_inner(k), Sp::into_inner(v));
            if k.is_none() && v.is_none() {
                None
            } else {
                Some((k, v))
            }
        })
    }

    /// Iterate over the key value pairs in the hash map
    #[allow(clippy::type_complexity)]
    pub fn iter(&self) -> impl Iterator<Item = Sp<(Sp<K, D>, Sp<V, D>), D>> + use<K, V, D, A> {
        self.0.iter().map(|(_, v)| v)
    }

    /// Number of elements in the map
    pub fn size(&self) -> usize {
        self.0.size()
    }

    /// Returns true if empty
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Returns keys
    pub fn keys(&self) -> impl Iterator<Item = K> + use<K, V, D, A> {
        let mut res = std::vec::Vec::<K>::new();

        for (k, _) in self.iter().map(|x| (*x).clone()) {
            res.push((*k).clone());
        }

        res.into_iter()
    }

    /// Returns values
    pub fn values(&self) -> impl Iterator<Item = V> + use<K, V, D, A> {
        let mut res = std::vec::Vec::<V>::new();

        for (_, v) in self.iter().map(|x| (*x).clone()) {
            res.push((*v).clone());
        }

        res.into_iter()
    }

    /// Retrieve the annotation on the root of the trie
    pub fn ann(&self) -> A {
        self.0.ann()
    }
}

impl<K, V, D, A> FromIterator<(K, V)> for HashMap<K, V, D, A>
where
    K: Serializable + Storable<D>,
    V: Storable<D>,
    D: DB,
    A: Storable<D> + Annotation<(Sp<K, D>, Sp<V, D>)>,
{
    fn from_iter<T: IntoIterator<Item = (K, V)>>(iter: T) -> Self {
        iter.into_iter()
            .fold(HashMap::new(), |map, (k, v)| map.insert(k, v))
    }
}

impl<
    K: Serializable + Deserializable + Storable<D>,
    V: Storable<D>,
    D: DB,
    A: Storable<D> + Annotation<(Sp<K, D>, Sp<V, D>)> + Annotation<V>,
> From<Map<K, V, D, A>> for HashMap<K, V, D, A>
{
    fn from(value: Map<K, V, D, A>) -> Self {
        let mut hashmap = HashMap::new();

        for (k, v) in value.iter() {
            hashmap = hashmap.insert(k, v.deref().clone());
        }

        hashmap
    }
}

/// Iterator type
pub struct HashMapIntoIter<K, V, D: DB>
where
    K: 'static,
    V: 'static,
{
    #[allow(clippy::type_complexity)]
    inner: std::vec::IntoIter<(ArenaKey<D::Hasher>, (Sp<K, D>, Sp<V, D>))>,
}

impl<K, V, D> Iterator for HashMapIntoIter<K, V, D>
where
    K: Serializable + Storable<D> + Clone + 'static,
    V: Storable<D> + Clone + 'static,
    D: DB,
{
    type Item = (K, V);

    fn next(&mut self) -> Option<Self::Item> {
        self.inner
            .next()
            .map(|(_arena_key, (sp_key, sp_val))| ((*sp_key).clone(), (*sp_val).clone()))
    }
}

impl<K, V, D> IntoIterator for HashMap<K, V, D>
where
    K: Serializable + Storable<D> + Clone + 'static,
    V: Storable<D> + Clone + 'static,
    D: DB,
{
    type Item = (K, V);
    type IntoIter = HashMapIntoIter<K, V, D>;

    fn into_iter(self) -> Self::IntoIter {
        HashMapIntoIter {
            inner: self.0.into_iter(),
        }
    }
}

/// A set. Uses `HashMap` under the hood.
#[derive(Storable)]
#[derive_where(Clone, Eq, PartialEq, PartialOrd, Ord, Hash; V, A)]
#[storable(db = D)]
#[tag = "hash-set"]
pub struct HashSet<
    V: Storable<D> + Serializable,
    D: DB = DefaultDB,
    A: Storable<D> + Annotation<(Sp<V, D>, Sp<(), D>)> = SizeAnn,
>(pub HashMap<V, (), D, A>);
tag_enforcement_test!(HashSet<()>);

impl<
    V: serde::Serialize + Serializable + Storable<D>,
    D: DB,
    A: Storable<D> + Annotation<(Sp<V, D>, Sp<(), D>)>,
> serde::Serialize for HashSet<V, D, A>
{
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.collect_seq(self.iter().map(|v| (&**v).clone()))
    }
}

impl<V: Storable<D> + Serializable, D: DB, A: Storable<D> + Annotation<(Sp<V, D>, Sp<(), D>)>>
    HashSet<V, D, A>
{
    /// Creates an empty set
    pub fn new() -> Self {
        Self(HashMap::new())
    }

    /// Insert object value. Overwrites
    /// any preexisting object under the same value
    #[must_use]
    pub fn insert(&self, value: V) -> Self {
        HashSet(self.0.insert(value, ()))
    }

    /// Remove object
    #[must_use]
    pub fn remove(&self, value: &V) -> Self {
        HashSet(self.0.remove(value))
    }

    /// Check if the set contains a value.
    pub fn member(&self, value: &V) -> bool {
        self.0.contains_key(value)
    }

    /// Check if a `HashSet` is the subset of another `HashSet`.
    pub fn is_subset(&self, other: &HashSet<V, D, A>) -> bool {
        self.iter().all(|x| other.member(&x))
    }

    /// Union with another set
    pub fn union(&self, other: &HashSet<V, D, A>) -> HashSet<V, D, A>
    where
        V: Clone,
    {
        other
            .iter()
            .fold(self.clone(), |acc, x| acc.insert(x.deref().deref().clone()))
    }

    /// Iterate over the key value pairs in the hash set
    pub fn iter(&self) -> impl Iterator<Item = Arc<Sp<V, D>>> + '_
    where
        V: Clone,
    {
        self.0.iter().map(|v| Arc::new(v.0.clone()))
    }

    /// Number of elements in the set
    pub fn size(&self) -> usize {
        self.0.size()
    }

    /// Returns true if empty
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Retrieve the annotation on the root of the trie
    pub fn ann(&self) -> A {
        self.0.ann()
    }
}

impl<V: Storable<D> + Serializable, D: DB, A: Storable<D> + Annotation<(Sp<V, D>, Sp<(), D>)>>
    Default for HashSet<V, D, A>
{
    fn default() -> Self {
        Self::new()
    }
}

impl<
    V: Debug + Storable<D> + Serializable,
    D: DB,
    A: Storable<D> + Annotation<(Sp<V, D>, Sp<(), D>)>,
> Debug for HashSet<V, D, A>
{
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(feature = "proptest")]
impl<
    V: Storable<D> + Serializable + Debug,
    D: DB,
    A: Storable<D> + Annotation<(Sp<V, D>, Sp<(), D>)>,
> Arbitrary for HashSet<V, D, A>
where
    Standard: Distribution<V>,
{
    type Strategy = NoStrategy<HashSet<V, D, A>>;
    type Parameters = ();

    fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
        NoStrategy(PhantomData)
    }
}

impl<V: Storable<D> + Serializable, D: DB, A: Storable<D> + Annotation<(Sp<V, D>, Sp<(), D>)>>
    Distribution<HashSet<V, D, A>> for Standard
where
    Standard: Distribution<V>,
{
    fn sample<R: rand::Rng + ?Sized>(&self, rng: &mut R) -> HashSet<V, D, A> {
        let mut set = HashSet::new();
        let size: usize = rng.gen_range(0..8);

        for _ in 0..size {
            set = set.insert(rng.r#gen())
        }

        set
    }
}

impl<V, D, A> FromIterator<V> for HashSet<V, D, A>
where
    V: Storable<D> + Serializable,
    D: DB,
    A: Storable<D> + Annotation<(Sp<V, D>, Sp<(), D>)>,
{
    fn from_iter<T: IntoIterator<Item = V>>(iter: T) -> Self {
        iter.into_iter()
            .fold(HashSet::new(), |set, item| set.insert(item))
    }
}

/// An array built from a `MerklePatriciaTrie`
#[derive_where(Clone; V)]
#[derive(Storable)]
#[storable(db = D, invariant = Array::invariant)]
#[tag = "mpt-array[v1]"]
pub struct Array<V: Storable<D>, D: DB = DefaultDB>(
    // Array wraps MPT in an Sp to guarantee it only has one child
    #[storable(child)] Sp<MerklePatriciaTrie<V, D>, D>,
);
tag_enforcement_test!(Array<()>);

impl<V: Storable<D> + Debug, D: DB> Debug for Array<V, D> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

impl<V: Storable<D>, D: DB> From<&std::vec::Vec<V>> for Array<V, D> {
    fn from(value: &std::vec::Vec<V>) -> Self {
        value.clone().into()
    }
}

impl<const N: usize, V: Storable<D>, D: DB> From<[V; N]> for Array<V, D> {
    fn from(value: [V; N]) -> Self {
        Array::from_iter(value)
    }
}

impl<V: Storable<D>, D: DB> From<std::vec::Vec<V>> for Array<V, D> {
    fn from(value: std::vec::Vec<V>) -> Self {
        Array::from_iter(value)
    }
}

impl<V: Storable<D>, D: DB> From<&Array<V, D>> for std::vec::Vec<V> {
    fn from(value: &Array<V, D>) -> Self {
        value.iter().map(|x| (*x).clone()).collect()
    }
}

impl<V: Storable<D>, D: DB> From<Array<V, D>> for std::vec::Vec<V> {
    fn from(value: Array<V, D>) -> Self {
        (&value).into()
    }
}

impl<V: Storable<D>, D: DB> std::iter::FromIterator<V> for Array<V, D> {
    fn from_iter<I: IntoIterator<Item = V>>(iter: I) -> Self {
        let mut arr = Array::new();
        for item in iter {
            arr = arr.push(item);
        }
        arr
    }
}

impl<V: Storable<D>, D: DB> Distribution<Array<V, D>> for Standard
where
    Standard: Distribution<V>,
{
    fn sample<R: rand::Rng + ?Sized>(&self, rng: &mut R) -> Array<V, D> {
        let mut array = Array::new();
        let len = rng.r#gen::<u8>();
        for _ in 0..len {
            array = array.push(rng.r#gen())
        }
        array
    }
}

#[cfg(feature = "proptest")]
impl<V: Debug + Storable<D>, D: DB> Arbitrary for Array<V, D>
where
    Standard: Distribution<V>,
{
    type Strategy = NoStrategy<Array<V, D>>;
    type Parameters = ();

    fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
        NoStrategy(PhantomData)
    }
}

impl<V: Storable<D> + PartialEq, D: DB> PartialEq for Array<V, D> {
    fn eq(&self, other: &Self) -> bool {
        self.0.eq(&other.0)
    }
}

impl<V: Storable<D> + Eq, D: DB> Eq for Array<V, D> {}

impl<V: Storable<D> + PartialOrd, D: DB> PartialOrd for Array<V, D> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.0.partial_cmp(&other.0)
    }
}

impl<V: Storable<D> + Ord, D: DB> Ord for Array<V, D> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.cmp(&other.0)
    }
}

impl<V: Storable<D>, D: DB> Hash for Array<V, D> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.deref().hash(state);
    }
}

impl<V: Storable<D>, D: DB> Default for Array<V, D> {
    fn default() -> Self {
        Self::new()
    }
}

impl<V: Storable<D>, D: DB> Array<V, D> {
    // Convert array index into u4 nibbles for use as mpt path.
    //
    // Drops leading zero nibbles, so that small arrays will have short paths.
    fn index_to_nibbles(i: usize) -> Vec<u8> {
        let nibbles = to_nibbles(&BigEndianU64(i as u64));
        nibbles.into_iter().skip_while(|x| *x == 0).collect()
    }

    fn nibbles_to_index(raw_nibbles: &[u8]) -> Result<u64, std::io::Error> {
        if raw_nibbles.first() == Some(&0) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "nibbles in array should not have leading zeroes",
            ));
        }
        let mut nibbles = [0u8; 16];
        if raw_nibbles.len() > 16 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "too long key for index in array",
            ));
        }
        nibbles[(16 - raw_nibbles.len())..].copy_from_slice(raw_nibbles);
        let val: BigEndianU64 = from_nibbles(&nibbles)?;
        Ok(val.0)
    }

    fn invariant(&self) -> Result<(), std::io::Error> {
        let len = self.len() as u64;
        self.0
            .iter()
            .map(|(k, _)| Self::nibbles_to_index(&k))
            .try_for_each(|n| {
                if n? >= len {
                    Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "index out of range for array on deserialization",
                    ))
                } else {
                    Ok(())
                }
            })
    }

    /// Construct an empty new array
    pub fn new() -> Self {
        Array(Sp::new(MerklePatriciaTrie::new()))
    }

    /// Generates a new [Array] from a value slice
    pub fn new_from_slice(values: &[V]) -> Self {
        let mut array = Array::<V, D>::new();
        for v in values.iter() {
            array = array.push(v.clone());
        }
        array
    }

    /// Number of elements in Array.
    ///
    /// The elements are stored at indices `0..len()`.
    pub fn len(&self) -> usize {
        self.0.deref().clone().size()
    }

    /// Get element at `index`. Returns `None` if `index` is out of bounds.
    pub fn get(&self, index: usize) -> Option<&V> {
        self.0.lookup(&Self::index_to_nibbles(index))
    }

    /// Insert element at index.
    ///
    /// Must be an existing index, or returns `None`. Use `push` if you want to
    /// grow the array.
    #[must_use]
    pub fn insert(&self, index: usize, value: V) -> Option<Self> {
        if index >= self.len() {
            return None; // Index out of bounds
        }
        Some(Array(Sp::new(
            self.0.insert(&Self::index_to_nibbles(index), value),
        )))
    }

    /// Appends an element to the end of the array, growing the length by 1.
    #[must_use]
    pub fn push(&self, value: V) -> Self {
        let index = self.len();
        Self(Sp::new(
            self.0.insert(&Self::index_to_nibbles(index), value),
        ))
    }

    /// Consume internal pointers, returning only the leaves left dangling by this.
    /// Used for custom `Drop` implementations.
    pub fn into_inner_for_drop(self) -> impl Iterator<Item = V> {
        Sp::into_inner(self.0)
            .into_iter()
            .flat_map(MerklePatriciaTrie::into_inner_for_drop)
    }

    /// Iterate over the elements in the array as `Sp<V>`s
    pub fn iter(&self) -> ArrayIter<'_, V, D> {
        ArrayIter::new(self)
    }

    /// Iterate over the elements in the array as `&V` references
    pub fn iter_deref(&self) -> impl Iterator<Item = &V> {
        (0..self.len()).filter_map(|i| self.get(i))
    }

    /// Returns true if empty
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl<V: Storable<D> + serde::Serialize, D: DB> serde::Serialize for Array<V, D> {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.collect_seq(self.iter().map(|v| v.deref().clone()))
    }
}

struct ArrayVisitor<V, D>(PhantomData<(V, D)>);

impl<'de, V: Storable<D> + serde::Deserialize<'de>, D: DB> serde::de::Visitor<'de>
    for ArrayVisitor<V, D>
{
    type Value = Array<V, D>;

    fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
        write!(formatter, "an array")
    }

    fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Array<V, D>, A::Error> {
        Ok(Array::<V, D>::from(
            &std::iter::from_fn(|| seq.next_element::<V>().transpose())
                .collect::<Result<std::vec::Vec<V>, A::Error>>()?,
        ))
    }
}

impl<'de, V: Storable<D1> + serde::Deserialize<'de>, D1: DB> serde::Deserialize<'de>
    for Array<V, D1>
{
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        de.deserialize_seq(ArrayVisitor(PhantomData))
    }
}

/// An iterator over `in_memory::Array`
pub struct ArrayIter<'a, V: Storable<D>, D: DB> {
    array: &'a Array<V, D>,
    next_index: usize,
}

impl<'a, V: Storable<D>, D: DB> ArrayIter<'a, V, D> {
    fn new(array: &'a Array<V, D>) -> Self {
        ArrayIter {
            array,
            next_index: 0,
        }
    }
}

impl<V: Storable<D>, D: DB> Iterator for ArrayIter<'_, V, D> {
    type Item = Sp<V, D>;

    fn next(&mut self) -> Option<Self::Item> {
        let result = self
            .array
            .0
            .lookup_sp(&Array::<V, D>::index_to_nibbles(self.next_index));
        self.next_index += 1;
        return result;
    }
}

#[derive(Storable)]
#[derive_where(Clone, Eq, PartialEq, PartialOrd, Ord, Hash; V)]
#[storable(db = D)]
#[tag = "multi-set[v1]"]
/// A set with quantity. Often known as a bag.
pub struct MultiSet<V: Serializable + Storable<D>, D: DB> {
    elements: HashMap<V, u32, D>,
}
tag_enforcement_test!(MultiSet<(), DefaultDB>);

impl<V: Serializable + Storable<D>, D: DB> MultiSet<V, D> {
    /// Create a new, empty `MultiSet`
    pub fn new() -> Self {
        MultiSet {
            elements: HashMap::new(),
        }
    }

    /// Insert an element with a quantity of one or, if the element is already in the set, increase its quantity by one
    #[must_use]
    pub fn insert(&self, element: V) -> Self {
        // Add an `entry` fn for HashMap
        let current_count = self.elements.get(&element).map(|x| *x.deref()).unwrap_or(0);
        MultiSet {
            elements: self.elements.insert(element, current_count + 1),
        }
    }

    /// Decrement the count of an element, removing it if its count becomes 0
    #[must_use]
    pub fn remove(&self, element: &V) -> Self {
        self.remove_n(element, 1)
    }

    /// Decrement the count of an element by `n`, removing it if its count becomes 0
    #[must_use]
    pub fn remove_n(&self, element: &V, n: u32) -> Self {
        let current_count = self.elements.get(&element).map(|x| *x.deref()).unwrap_or(0);
        let result = u32::checked_sub(current_count, n).unwrap_or(0);
        if result == 0 {
            MultiSet {
                elements: self.elements.remove(&element),
            }
        } else {
            MultiSet {
                elements: self.elements.insert(element.clone(), result),
            }
        }
    }

    /// How many of a given element are in the structure? Returns 0 when the element isn't present
    pub fn count(&self, element: &V) -> u32 {
        match self.elements.get(element) {
            Some(i) => *i.deref(),
            None => 0,
        }
    }

    /// Check if a `MutliSet` is the subset of another `MutliSet`.
    pub fn has_subset(&self, other: &MultiSet<V, D>) -> bool {
        for element_x_other_count in other.elements.iter() {
            let self_count = self.count(element_x_other_count.0.deref());
            if self_count < *element_x_other_count.1.deref() {
                return false;
            }
        }
        true
    }

    /// Check if the set contains a value.
    pub fn member(&self, element: &V) -> bool {
        self.elements.contains_key(element)
    }
}

/// A one-element collection.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serializable)]
pub struct Identity<V>(pub V);

impl<V: Storable<D>, D: DB> Storable<D> for Identity<V> {
    fn children(&self) -> std::vec::Vec<ArenaKey<<D as DB>::Hasher>> {
        self.0.children()
    }
    fn from_binary_repr<R: std::io::Read>(
        reader: &mut R,
        child_hashes: &mut impl Iterator<Item = ArenaKey<<D as DB>::Hasher>>,
        loader: &impl Loader<D>,
    ) -> Result<Self, std::io::Error>
    where
        Self: Sized,
    {
        V::from_binary_repr(reader, child_hashes, loader).map(Identity)
    }
    fn to_binary_repr<W: std::io::Write>(&self, writer: &mut W) -> Result<(), std::io::Error>
    where
        Self: Sized,
    {
        self.0.to_binary_repr(writer)
    }
}

impl<V: Tagged> Tagged for Identity<V> {
    fn tag() -> std::borrow::Cow<'static, str> {
        V::tag()
    }
    fn tag_unique_factor() -> String {
        V::tag_unique_factor()
    }
}

impl<V> Distribution<Identity<V>> for Standard
where
    Standard: Distribution<V>,
{
    fn sample<R: rand::Rng + ?Sized>(&self, rng: &mut R) -> Identity<V> {
        let v = <Standard as Distribution<V>>::sample(self, rng);
        Identity(v)
    }
}

impl<V> From<V> for Identity<V> {
    fn from(v: V) -> Self {
        Identity(v)
    }
}

impl<T: Storable<D> + Serializable, D: DB, A: Storable<D> + Annotation<(Sp<T, D>, Sp<(), D>)>>
    Semigroup for HashSet<T, D, A>
{
    fn append(&self, other: &Self) -> Self {
        self.union(&other)
    }
}

/// An abstract container of items
pub trait Container<D: DB> {
    /// The contained type
    type Item: Storable<D> + Clone + PartialEq + Eq + PartialOrd + Ord + Hash;
    /// Gets an iterator over the `Container`'s items
    fn iter_items(self) -> impl Iterator<Item = Self::Item>;
    /// Wrap a single item in a `Container``
    fn once(_: Self::Item) -> Self;
}

impl<T: Storable<D> + Clone + PartialEq + Eq + PartialOrd + Ord + Hash, D: DB> Container<D>
    for Identity<T>
{
    type Item = T;
    fn iter_items(self) -> impl Iterator<Item = Self::Item> {
        std::iter::once(self.0)
    }

    fn once(item: Self::Item) -> Self {
        Self(item)
    }
}

impl<
    T: Serializable + Storable<D> + Clone + PartialEq + Eq + PartialOrd + Ord + Hash,
    D: DB,
    A: Storable<D> + Annotation<(Sp<T, D>, Sp<(), D>)>,
> Container<D> for HashSet<T, D, A>
{
    type Item = T;

    fn iter_items(self) -> impl Iterator<Item = Self::Item> {
        self.0.keys()
    }

    fn once(item: Self::Item) -> Self {
        Self::new().insert(item)
    }
}

#[derive(Debug)]
struct BigEndianU64(u64);

impl Serializable for BigEndianU64 {
    fn serialize(&self, writer: &mut impl std::io::Write) -> std::io::Result<()> {
        writer.write_all(&self.0.to_be_bytes())
    }
    fn serialized_size(&self) -> usize {
        8
    }
}

impl Deserializable for BigEndianU64 {
    fn deserialize(
        reader: &mut impl std::io::Read,
        _recursion_depth: u32,
    ) -> std::io::Result<Self> {
        let mut buf = [0u8; 8];
        reader.read_exact(&mut buf[..])?;
        Ok(BigEndianU64(u64::from_be_bytes(buf)))
    }
}

/// A mapping from `Timestamp`s to values
///
/// `Timestamp`s are big-endian encoded to allow for efficient predecessor
/// searching and pruning.
#[derive(Storable)]
#[derive_where(Clone, Eq, PartialEq, PartialOrd, Ord, Hash; C)]
#[storable(db = D)]
pub struct TimeFilterMap<C: Serializable + Storable<D>, D: DB>
where
    C: Container<D> + Serializable + Storable<D>,
    <C as Container<D>>::Item: Serializable + Storable<D>,
{
    time_map: Map<BigEndianU64, C, D>,
    set: MultiSet<<C as Container<D>>::Item, D>,
}
impl<C: Serializable + Storable<D>, D: DB> Tagged for TimeFilterMap<C, D>
where
    C: Container<D> + Serializable + Storable<D> + Tagged,
    <C as Container<D>>::Item: Serializable + Storable<D> + Tagged,
{
    fn tag() -> std::borrow::Cow<'static, str> {
        format!("time-filter-map[v1]({})", C::tag()).into()
    }
    fn tag_unique_factor() -> String {
        format!(
            "({},{})",
            C::tag(),
            <MultiSet<<C as Container<D>>::Item, D>>::tag()
        )
    }
}
tag_enforcement_test!(TimeFilterMap<Identity<()>, DefaultDB>);

impl<V: Container<D> + Debug + Storable<D> + Serializable, D: DB> Debug for TimeFilterMap<V, D>
where
    <V as Container<D>>::Item: Serializable,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.time_map.fmt(f)
    }
}

impl<C: Container<D> + Debug + Storable<D> + Serializable, D: DB> TimeFilterMap<C, D>
where
    <C as Container<D>>::Item: Serializable,
{
    /// Create a new `TimeFilterMap`
    pub fn new() -> Self {
        TimeFilterMap {
            time_map: Map::new(),
            set: MultiSet::new(),
        }
    }

    /// Return either a value precisely at the provided `Timestamp`, or the value at the next-earliest `Timestamp`, if one exists
    pub fn get(&self, ts: Timestamp) -> Option<&C> {
        let ts = BigEndianU64(ts.to_secs());
        match self.time_map.get(&ts) {
            Some(res) => Some(res),
            None => self.time_map.find_predecessor(&ts).map(|(_, v)| v),
        }
    }

    /// Insert a value at the given `Timestamp`. If an entry at the `Timestamp` already exists, its value is replaced.
    ///
    /// Note: Despite the value being a `Container` item, this method does _not_ append to existing entries, it replaces them.
    #[must_use]
    pub fn insert(&self, ts: Timestamp, v: <C as Container<D>>::Item) -> Self {
        let mut res = self.clone();
        if let Some(x) = self.time_map.get(&BigEndianU64(ts.to_secs())) {
            for val in x.clone().iter_items() {
                res.set = res.set.remove(&val);
            }
        }

        res.time_map = res
            .time_map
            .insert(BigEndianU64(ts.to_secs()), C::once(v.clone()));
        res.set = res.set.insert(v);
        res
    }

    /// Insert a value at the given `Timestamp`. If an entry at the `Timestamp` already exists, this value is appended to it.
    #[must_use]
    pub fn upsert_one(&self, ts: Timestamp, v: <C as Container<D>>::Item) -> Self
    where
        C: Semigroup + Default,
    {
        self.upsert(ts, &C::once(v))
    }

    /// Insert or update all values into a `Container` into our `TimeFilterMap`
    #[must_use]
    pub fn upsert(&self, ts: Timestamp, v: &C) -> Self
    where
        C: Semigroup + Default,
    {
        let xs = self
            .time_map
            .get(&BigEndianU64(ts.to_secs()))
            .cloned()
            .unwrap_or_default();
        let mut res = self.clone();
        for new_val in v.clone().iter_items() {
            res.set = res.set.insert(new_val.clone());
        }
        res.time_map = res
            .time_map
            .insert(BigEndianU64(ts.to_secs()), xs.append(&v));

        res
    }

    /// Check if the `TimeFilterMap` contains a value
    pub fn contains(&self, v: &<C as Container<D>>::Item) -> bool {
        self.set.member(v)
    }

    /// Check if `TimeFilterMap` contains all values in a `Container`
    pub fn contains_all(&self, v: C) -> bool {
        v.iter_items().all(|val| self.set.member(&val))
    }

    /// Removes all entries with keys before the `cutoff_timestamp`
    #[must_use]
    pub fn filter(&self, cutoff_timestamp: Timestamp) -> Self
    where
        MerklePatriciaTrie<C, D>: 'static + Clone,
    {
        let cutoff_key = to_nibbles(&BigEndianU64(cutoff_timestamp.to_secs()));

        let mut res = self.clone();
        let (new_mpt, removed_items_for_set) = self.time_map.mpt.prune(&cutoff_key);
        res.time_map.mpt = Sp::new(new_mpt);

        for items in removed_items_for_set {
            for item in items.deref().clone().iter_items() {
                res.set = res.set.remove(&item);
            }
        }
        res
    }
}

/// A persistently stored map, guaranteeing O(1) clones and log-time
/// modifications.
#[derive_where(PartialEq, Eq, PartialOrd, Ord; V, A)]
#[derive_where(Hash, Clone)]
pub struct Map<K, V: Storable<D>, D: DB = DefaultDB, A: Storable<D> + Annotation<V> = SizeAnn> {
    pub(crate) mpt: Sp<MerklePatriciaTrie<V, D, A>, D>,
    key_type: PhantomData<K>,
}

impl<K: Tagged, V: Storable<D> + Tagged, D: DB, A: Storable<D> + Annotation<V> + Tagged> Tagged
    for Map<K, V, D, A>
{
    fn tag() -> std::borrow::Cow<'static, str> {
        format!("mpt-map({},{},{})", K::tag(), V::tag(), A::tag()).into()
    }
    fn tag_unique_factor() -> String {
        <MerklePatriciaTrie<V, D, A>>::tag_unique_factor()
    }
}
tag_enforcement_test!(Map<(), ()>);

impl<
    K: Sync + Send + 'static + Deserializable,
    V: Storable<D>,
    D: DB,
    A: Storable<D> + Annotation<V>,
> Storable<D> for Map<K, V, D, A>
{
    /// Rather than in-lining the wrapped MPT it is a child such that we know the public Map has
    /// only a single child element (rather than up to 16)
    fn children(&self) -> std::vec::Vec<ArenaKey<D::Hasher>> {
        vec![Sp::hash(&self.mpt).into()]
    }

    fn to_binary_repr<W: std::io::Write>(&self, _writer: &mut W) -> Result<(), std::io::Error>
    where
        Self: Sized,
    {
        Ok(())
    }

    fn from_binary_repr<R: std::io::Read>(
        _reader: &mut R,
        child_hashes: &mut impl Iterator<Item = ArenaKey<D::Hasher>>,
        loader: &impl Loader<D>,
    ) -> Result<Self, std::io::Error>
    where
        Self: Sized,
    {
        let res = Self {
            mpt: loader.get_next(child_hashes)?,
            key_type: PhantomData,
        };
        loader.do_check(res)
    }

    fn check_invariant(&self) -> Result<(), std::io::Error> {
        self.mpt
            .iter()
            .try_for_each(|(k, _)| from_nibbles::<K>(&k).and(Ok(())))
    }
}

impl<
    K: Sync + Send + 'static + Deserializable,
    V: Storable<D>,
    D: DB,
    A: Storable<D> + Annotation<V>,
> Serializable for Map<K, V, D, A>
{
    fn serialize(&self, writer: &mut impl std::io::Write) -> std::io::Result<()> {
        Sp::new(self.clone()).serialize(writer)
    }
    fn serialized_size(&self) -> usize {
        Sp::new(self.clone()).serialized_size()
    }
}

impl<
    K: Sync + Send + 'static + Deserializable,
    V: Storable<D>,
    D: DB,
    A: Storable<D> + Annotation<V>,
> Deserializable for Map<K, V, D, A>
{
    fn deserialize(reader: &mut impl std::io::Read, recursion_depth: u32) -> std::io::Result<Self> {
        <Sp<Map<K, V, D, A>, D> as Deserializable>::deserialize(reader, recursion_depth)
            .map(|s| (*s).clone())
    }
}

impl<K, V, D, A> FromIterator<(K, V)> for Map<K, V, D, A>
where
    K: Serializable + Deserializable,
    V: Storable<D>,
    D: DB,
    A: Storable<D> + Annotation<V>,
{
    fn from_iter<T: IntoIterator<Item = (K, V)>>(iter: T) -> Self {
        iter.into_iter()
            .fold(Map::new(), |map, (k, v)| map.insert(k, v))
    }
}

#[cfg(feature = "proptest")]
impl<
    K: Serializable + Deserializable + Debug,
    V: Storable<D> + Debug,
    D: DB,
    A: Storable<D> + Annotation<V>,
> Arbitrary for Map<K, V, D, A>
where
    Standard: Distribution<V> + Distribution<K>,
{
    type Strategy = NoStrategy<Map<K, V, D, A>>;
    type Parameters = ();

    fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
        NoStrategy(PhantomData)
    }
}

impl<K: Serializable + Deserializable, V: Storable<D>, D: DB, A: Storable<D> + Annotation<V>>
    Distribution<Map<K, V, D, A>> for Standard
where
    Standard: Distribution<V> + Distribution<K>,
{
    fn sample<R: rand::Rng + ?Sized>(&self, rng: &mut R) -> Map<K, V, D, A> {
        let mut map = Map::new();
        let size: usize = rng.gen_range(0..8);

        for _ in 0..size {
            map = map.insert(rng.r#gen(), rng.r#gen());
        }
        map
    }
}

impl<T: serde::Serialize + Storable<D>, D: DB> serde::Serialize for Sp<T, D> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        <T as serde::Serialize>::serialize(&self, serializer)
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

impl<
    K: serde::Serialize + Serializable + Deserializable,
    V: Storable<D> + serde::Serialize,
    D: DB,
    A: Storable<D> + Annotation<V>,
> serde::Serialize for Map<K, V, D, A>
{
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.collect_map(self.iter().map(|kv| (kv.0, kv.1.deref().clone())))
    }
}

struct MapVisitor<K, V, D, A>(PhantomData<(K, V, D, A)>);

impl<
    'de,
    K: serde::Deserialize<'de> + Serializable + Deserializable,
    V: Storable<D> + serde::Deserialize<'de>,
    D: DB,
    A: Storable<D> + Annotation<V>,
> serde::de::Visitor<'de> for MapVisitor<K, V, D, A>
{
    type Value = Map<K, V, D, A>;

    fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
        write!(formatter, "a map")
    }

    fn visit_map<ACC: serde::de::MapAccess<'de>>(
        self,
        mut seq: ACC,
    ) -> Result<Map<K, V, D, A>, ACC::Error> {
        std::iter::from_fn(|| seq.next_entry::<K, V>().transpose()).collect()
    }
}

impl<
    'de,
    K: serde::Deserialize<'de> + Serializable + Deserializable,
    V: serde::Deserialize<'de> + Storable<D1>,
    D1: DB,
    A: Storable<D1> + Annotation<V>,
> serde::Deserialize<'de> for Map<K, V, D1, A>
{
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        de.deserialize_map(MapVisitor(PhantomData))
    }
}

fn to_nibbles<T: Serializable>(value: &T) -> std::vec::Vec<u8> {
    let mut bytes = std::vec::Vec::new();
    T::serialize(value, &mut bytes).unwrap();
    let mut nibbles = std::vec::Vec::new();
    for b in bytes {
        nibbles.push((b & 0xf0) >> 4);
        nibbles.push(b & 0x0f);
    }

    nibbles
}

fn from_nibbles<T: Deserializable>(value: &[u8]) -> std::io::Result<T> {
    if value.iter().any(|v| *v >= 16) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "nibble out of range",
        ));
    }
    let bytes = value
        .chunks(2)
        .map(|nibbles_pair| {
            if nibbles_pair.len() != 2 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "nibble array must have even length",
                ));
            }
            Ok((nibbles_pair[0] << 4) | nibbles_pair[1])
        })
        .collect::<Result<std::vec::Vec<u8>, std::io::Error>>()?;
    T::deserialize(&mut &bytes[..], 0)
}

impl<K: Serializable + Deserializable, V: Storable<D>, D: DB, A: Storable<D> + Annotation<V>>
    Default for Map<K, V, D, A>
{
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Serializable + Deserializable, V: Storable<D>, D: DB, A: Storable<D> + Annotation<V>>
    Map<K, V, D, A>
{
    /// Returns an empty map.
    pub fn new() -> Self {
        Self {
            mpt: Sp::new(MerklePatriciaTrie::new()),
            key_type: PhantomData,
        }
    }

    /// Insert a key-value pair into the map. Must be `O(log(|self|))`.
    #[must_use]
    pub fn insert(&self, key: K, value: V) -> Self {
        Map {
            mpt: Sp::new(self.mpt.insert(&to_nibbles(&key), value)),
            key_type: self.key_type,
        }
    }

    /// Remove a key from the map. Must be `O(log(|self|))`
    #[must_use]
    pub fn remove(&self, key: &K) -> Self {
        Map {
            mpt: Sp::new(self.mpt.remove(&to_nibbles(&key))),
            key_type: self.key_type,
        }
    }

    /// Consume internal pointers, returning only the leaves left dangling by this.
    /// Used for custom `Drop` implementations.
    pub fn into_inner_for_drop(self) -> impl Iterator<Item = V> {
        Sp::into_inner(self.mpt)
            .into_iter()
            .flat_map(MerklePatriciaTrie::into_inner_for_drop)
    }

    /// Iterate over the key-value pairs in the map in a deterministic, but unspecified order.
    pub fn iter(&self) -> impl Iterator<Item = (K, Sp<V, D>)> + use<K, V, D, A> {
        self.mpt.iter().filter_map(|(p, v)| {
            // The path should always decode as nibbles if the map is well
            // formed, but at the moment a ill-formed maps can be created by
            // deserialization.
            let key = from_nibbles::<K>(&p).ok()?;
            Some((key, v))
        })
    }

    /// Iterator over the keys in the map in a deterministic, but unspecified order.
    pub fn keys(&self) -> impl Iterator<Item = K> + use<K, V, D, A> {
        self.iter().map(|(k, _)| k)
    }

    /// Check if the map contains a key. Must be `O(log(|self|))`.
    pub fn contains_key<Q>(&self, key: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: Ord + Serializable,
    {
        self.mpt.lookup(&to_nibbles(key)).is_some()
    }

    /// Retrieve the value stored at a key, if applicable. Must be `O(log(|self|))`.
    pub fn get<Q>(&self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: Serializable,
    {
        self.mpt.lookup(&to_nibbles(&key))
    }

    /// Lookup as Sp instead of raw `V` value.
    pub fn lookup_sp<Q>(&self, key: &Q) -> Option<Sp<V, D>>
    where
        Q: Serializable,
    {
        self.mpt.lookup_sp(&to_nibbles(key))
    }

    /// Check if the map is empty. Must be O(1).
    pub fn is_empty(&self) -> bool {
        self.mpt.is_empty()
    }

    /// Retrieve the number of key-value pairs in the map. Must be O(1).
    pub fn size(&self) -> usize {
        self.mpt.deref().clone().size()
    }

    fn from_mpt(&self, mpt: MerklePatriciaTrie<V, D, A>) -> Self {
        Map {
            mpt: Sp::new(mpt),
            key_type: self.key_type,
        }
    }

    /// Retrieve the annotation on the root of the trie
    pub fn ann(&self) -> A {
        self.mpt.ann()
    }
}

impl<V: Storable<D>, D: DB, A: Storable<D> + Annotation<V>> Map<BigEndianU64, V, D, A> {
    /// Find the nearest predecessor to a given `target_path`
    pub fn find_predecessor<'a>(
        &'a self,
        target_path: &BigEndianU64,
    ) -> Option<(std::vec::Vec<u8>, &'a V)> {
        let target_nibbles = to_nibbles(&target_path);
        self.mpt.find_predecessor(target_nibbles.as_slice())
    }

    /// Prunes all paths which are lexicographically less than or equal to `target_path`.
    /// Returns the updated tree, and a vector of the removed leaves.
    ///
    /// # Panics
    ///
    /// If any values in `target_path` are not `u4` nibbles, i.e. larger than
    /// 15.
    pub fn prune(&self, target_path: &[u8]) -> (Self, std::vec::Vec<Sp<V, D>>) {
        let (mpt, removed) = self.mpt.prune(target_path);
        (self.from_mpt(mpt), removed)
    }
}

enum Decodable<T> {
    Yes(T),
    No,
}

impl<T, E> From<Result<T, E>> for Decodable<T> {
    fn from(value: Result<T, E>) -> Self {
        match value {
            Ok(v) => Decodable::Yes(v),
            Err(_) => Decodable::No,
        }
    }
}

impl<T: Debug> Debug for Decodable<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Decodable::Yes(v) => v.fmt(f),
            Decodable::No => write!(f, "!decode error!"),
        }
    }
}

impl<K: Deserializable + Debug, V: Storable<D> + Debug, D: DB, A: Storable<D> + Annotation<V>> Debug
    for Map<K, V, D, A>
{
    fn fmt(&self, formatter: &mut Formatter) -> std::fmt::Result {
        formatter
            .debug_map()
            .entries(
                self.mpt
                    .iter()
                    .map(|(k, v)| (Decodable::from(from_nibbles::<K>(&k)), v)),
            )
            .finish()
    }
}

// TODO: remove and use clones at IntoIter callsite
impl<
    K: Clone + Serializable + Deserializable,
    V: Clone + Storable<D>,
    D: DB,
    A: Storable<D> + Annotation<V>,
> IntoIterator for Map<K, V, D, A>
{
    type Item = (K, V);
    type IntoIter = std::vec::IntoIter<(K, V)>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
            .map(|(k, x)| (k, x.deref().clone()))
            .collect::<std::vec::Vec<_>>()
            .into_iter()
    }
}

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
        key: &ArenaKey<Self::Hasher>,
    ) -> Option<crate::backend::OnDiskObject<Self::Hasher>> {
        self.db.get_node(key)
    }

    fn get_unreachable_keys(&self) -> std::vec::Vec<ArenaKey<Self::Hasher>> {
        self.db.get_unreachable_keys()
    }

    fn insert_node(
        &mut self,
        key: ArenaKey<Self::Hasher>,
        object: crate::backend::OnDiskObject<Self::Hasher>,
    ) {
        self.db.insert_node(key, object)
    }

    fn delete_node(&mut self, key: &ArenaKey<Self::Hasher>) {
        self.db.delete_node(key)
    }

    fn get_root_count(&self, key: &ArenaKey<Self::Hasher>) -> u32 {
        self.db.get_root_count(key)
    }

    fn set_root_count(&mut self, key: ArenaKey<Self::Hasher>, count: u32) {
        self.db.set_root_count(key, count)
    }

    fn get_roots(&self) -> std::collections::HashMap<ArenaKey<Self::Hasher>, u32> {
        self.db.get_roots()
    }

    fn size(&self) -> usize {
        self.db.size()
    }

    fn batch_update<I>(&mut self, iter: I)
    where
        I: Iterator<Item = (ArenaKey<Self::Hasher>, crate::db::Update<Self::Hasher>)>,
    {
        self.db.batch_update(iter)
    }

    fn batch_get_nodes<I>(
        &self,
        keys: I,
    ) -> std::vec::Vec<(
        ArenaKey<Self::Hasher>,
        Option<crate::backend::OnDiskObject<Self::Hasher>>,
    )>
    where
        I: Iterator<Item = ArenaKey<Self::Hasher>>,
    {
        self.db.batch_get_nodes(keys)
    }

    fn bfs_get_nodes<C>(
        &self,
        key: &ArenaKey<Self::Hasher>,
        cache_get: C,
        truncate: bool,
        max_depth: Option<usize>,
        max_count: Option<usize>,
    ) -> std::vec::Vec<(
        ArenaKey<Self::Hasher>,
        crate::backend::OnDiskObject<Self::Hasher>,
    )>
    where
        C: Fn(&ArenaKey<Self::Hasher>) -> Option<crate::backend::OnDiskObject<Self::Hasher>>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iter_map() {
        let mut map = Map::<_, _>::new();
        map = map.insert(1, 4);
        map = map.insert(2, 5);
        map = map.insert(3, 6);
        for (k, v) in map.iter() {
            match (k, Sp::deref(&v)) {
                (1, 4) | (2, 5) | (3, 6) => {}
                _ => unreachable!(),
            }
        }
    }

    #[test]
    fn array_get() {
        let array: super::Array<_> = vec![0, 1, 2, 3].into();
        assert_eq!(array.get(0).cloned(), Some(0));
        assert_eq!(array.get(1).cloned(), Some(1));
        assert_eq!(array.get(2).cloned(), Some(2));
        assert_eq!(array.get(3).cloned(), Some(3));
        assert!(array.get(4).is_none());
        assert!(array.get(5).is_none());
        assert!(array.get(6).is_none());
        assert!(array.get(7).is_none());
    }

    #[test]
    fn array_push() {
        let mut array = super::Array::<u32>::new();
        assert_eq!(array.len(), 0);
        array = array.push(0);
        assert_eq!(array.len(), 1);
        array = array.push(1);
        assert_eq!(array.len(), 2);
        assert_eq!(array, vec![0, 1].into());
    }

    #[test]
    fn array_with_more_than_16_elements() {
        let _: super::Array<_> = (0..1024).collect();
    }

    #[test]
    fn array_index_to_nibbles_is_big_endian() {
        assert_eq!(Array::<u8>::index_to_nibbles(0), Vec::<u8>::new());
        assert_eq!(Array::<u8>::index_to_nibbles(1), vec![1]);
        assert_eq!(Array::<u8>::index_to_nibbles(15), vec![15]);
        assert_eq!(Array::<u8>::index_to_nibbles(16), vec![1, 0]);
        assert_eq!(Array::<u8>::index_to_nibbles(255), vec![15, 15]);
        assert_eq!(Array::<u8>::index_to_nibbles(256), vec![1, 0, 0]);
        assert_eq!(
            Array::<u8>::index_to_nibbles((1 << 12) - 1),
            vec![15, 15, 15]
        );
        assert_eq!(Array::<u8>::index_to_nibbles(1 << 12), vec![1, 0, 0, 0]);
        assert_eq!(
            Array::<u8>::index_to_nibbles((1 << 32) - 1),
            vec![15; 32 / 4]
        );
        let mut expected = vec![0; 32 / 4 + 1];
        expected[0] = 1;
        assert_eq!(Array::<u8>::index_to_nibbles(1 << 32), expected);
    }

    #[test]
    fn test_map_iterators() {
        let map = Map::<_, _>::new()
            .insert(40026u64, 12u64)
            .insert(12u64, 40026u64);
        let mut keys = map.keys().collect::<std::vec::Vec<_>>();
        keys.sort();
        assert_eq!(keys, vec![12u64, 40026u64]);
        let mut entries = map
            .iter()
            .map(|(k, v)| (k, *(v.deref())))
            .collect::<std::vec::Vec<_>>();
        entries.sort();
        assert_eq!(entries, vec![(12u64, 40026u64), (40026u64, 12u64)]);
    }

    #[test]
    fn test_hashmap() {
        let mut hashmap = HashMap::<_, _>::new()
            .insert(40026u64, 12u64)
            .insert(12u64, 40026u64);

        assert_eq!(hashmap.get(&40026u64).map(|sp| *(sp.deref())), Some(12u64));
        assert_eq!(hashmap.get(&12u64).map(|sp| *(sp.deref())), Some(40026u64));
        hashmap = hashmap.remove(&12u64);
        assert_eq!(hashmap.get(&12u64), None);
    }

    #[test]
    fn test_predecessor_when_target_is_branch_prefix_time_map() {
        let mut time_map = TimeFilterMap::<Identity<i32>, InMemoryDB>::new();

        time_map = time_map.insert(Timestamp::from_secs(1), 1);
        time_map = time_map.insert(Timestamp::from_secs(256), 256);
        time_map = time_map.insert(Timestamp::from_secs(512), 512);

        assert_eq!(
            time_map.get(Timestamp::from_secs(257)).map(|v| *v),
            Some(Identity(256))
        );
    }

    #[test]
    fn test_smoke_time_map() {
        let mut time_map = TimeFilterMap::<Identity<i32>, InMemoryDB>::new();
        time_map = time_map.insert(Timestamp::from_secs(1), 1);
        time_map = time_map.insert(Timestamp::from_secs(2), 2);

        assert_eq!(time_map.get(Timestamp::from_secs(0)).map(|v| *v), None);
        assert_eq!(
            time_map.get(Timestamp::from_secs(1)).map(|v| *v),
            Some(Identity(1))
        );
        assert_eq!(
            time_map.get(Timestamp::from_secs(2)).map(|v| *v),
            Some(Identity(2))
        );
        assert_eq!(
            time_map.get(Timestamp::from_secs(3)).map(|v| *v),
            Some(Identity(2))
        );
        assert_eq!(time_map.contains(&0), false);
        assert_eq!(time_map.contains(&1), true);
        assert_eq!(time_map.contains(&2), true);
        assert_eq!(time_map.contains(&3), false);
        // Drop all things before the first item
        time_map = time_map.filter(Timestamp::from_secs(2));
        // First item should be gone now
        assert_eq!(time_map.get(Timestamp::from_secs(1)).map(|v| *v), None);
        assert_eq!(
            time_map.get(Timestamp::from_secs(2)).map(|v| *v),
            Some(Identity(2))
        );
        assert_eq!(
            time_map.get(Timestamp::from_secs(3)).map(|v| *v),
            Some(Identity(2))
        );
        assert_eq!(time_map.contains(&0), false);
        assert_eq!(time_map.contains(&1), false);
        assert_eq!(time_map.contains(&2), true);
        assert_eq!(time_map.contains(&3), false);

        // Fails if to_nibbles bit emission order is reversed (as it was originally)
        time_map = time_map.insert(Timestamp::from_secs(16), 16);
        assert_eq!(
            time_map.get(Timestamp::from_secs(16)).map(|v| *v),
            Some(Identity(16))
        );
        assert_eq!(
            time_map.get(Timestamp::from_secs(17)).map(|v| *v),
            Some(Identity(16))
        );

        time_map = time_map.filter(Timestamp::from_secs(2));

        assert_eq!(time_map.get(Timestamp::from_secs(1)).map(|v| *v), None);
        assert_eq!(
            time_map.get(Timestamp::from_secs(2)).map(|v| *v),
            Some(Identity(2))
        );
        assert_eq!(
            time_map.get(Timestamp::from_secs(3)).map(|v| *v),
            Some(Identity(2))
        );
        assert_eq!(
            time_map.get(Timestamp::from_secs(17)).map(|v| *v),
            Some(Identity(16))
        );

        assert_eq!(time_map.contains(&0), false);
        assert_eq!(time_map.contains(&1), false);
        assert_eq!(time_map.contains(&2), true);
        assert_eq!(time_map.contains(&3), false);
        assert_eq!(time_map.contains(&16), true);

        // Fails if little-endian encoded during serialisation
        time_map = time_map.insert(Timestamp::from_secs(256), 256);

        assert_eq!(
            time_map.get(Timestamp::from_secs(256)).map(|v| *v),
            Some(Identity(256))
        );
        assert_eq!(
            time_map.get(Timestamp::from_secs(257)).map(|v| *v),
            Some(Identity(256))
        );

        time_map = time_map.filter(Timestamp::from_secs(2));

        assert_eq!(time_map.get(Timestamp::from_secs(1)).map(|v| *v), None);
        assert_eq!(
            time_map.get(Timestamp::from_secs(2)).map(|v| *v),
            Some(Identity(2))
        );
        assert_eq!(
            time_map.get(Timestamp::from_secs(3)).map(|v| *v),
            Some(Identity(2))
        );
        assert_eq!(
            time_map.get(Timestamp::from_secs(257)).map(|v| *v),
            Some(Identity(256))
        );

        assert_eq!(time_map.contains(&0), false);
        assert_eq!(time_map.contains(&1), false);
        assert_eq!(time_map.contains(&2), true);
        assert_eq!(time_map.contains(&3), false);
        assert_eq!(time_map.contains(&256), true);
    }

    #[test]
    fn test_get_empty_time_map() {
        let time_map = TimeFilterMap::<Identity<i32>, InMemoryDB>::new();
        assert_eq!(time_map.get(Timestamp::from_secs(100)), None);
    }

    #[test]
    fn test_insert_duplicate_value_allowed_time_map() {
        let mut time_map = TimeFilterMap::<Identity<i32>, InMemoryDB>::new();
        assert_eq!(0, time_map.set.count(&100));
        time_map = time_map.insert(Timestamp::from_secs(10), 100);
        assert_eq!(1, time_map.set.count(&100));
        time_map = time_map.insert(Timestamp::from_secs(20), 100);
        assert_eq!(2, time_map.set.count(&100));
    }

    #[test]
    fn test_insert_filter_clears_set_via_duplicate_logic_time_map() {
        let mut time_map = TimeFilterMap::<Identity<i32>, InMemoryDB>::new();
        time_map = time_map.insert(Timestamp::from_secs(10), 100);
        time_map = time_map.filter(Timestamp::from_secs(11));
        let _ = time_map.insert(Timestamp::from_secs(20), 100); // Should NOT panic
    }

    #[test]
    fn test_replace_existing_timestamp() {
        let mut time_map = TimeFilterMap::<Identity<i32>, InMemoryDB>::new();
        time_map = time_map.insert(Timestamp::from_secs(100), 1);
        assert_eq!(
            time_map.get(Timestamp::from_secs(100)).map(|v| *v),
            Some(Identity(1))
        );
        assert!(time_map.contains(&1));
        time_map = time_map.insert(Timestamp::from_secs(100), 2);
        assert!(!time_map.contains(&1));
        assert!(time_map.contains(&2));
        assert_eq!(
            time_map.get(Timestamp::from_secs(100)).map(|v| *v),
            Some(Identity(2))
        );
        assert_eq!(
            time_map.get(Timestamp::from_secs(101)).map(|v| *v),
            Some(Identity(2))
        );
    }

    #[test]
    fn test_filter_below_minimum_key_time_map() {
        let mut time_map = TimeFilterMap::<Identity<i32>, InMemoryDB>::new();
        time_map = time_map.insert(Timestamp::from_secs(10), 10);
        time_map = time_map.insert(Timestamp::from_secs(20), 20);
        time_map = time_map.filter(Timestamp::from_secs(9)); // Cutoff before any existing keys

        assert!(time_map.contains(&10));
        assert!(time_map.contains(&20));
        assert_eq!(
            time_map.get(Timestamp::from_secs(10)).map(|v| *v),
            Some(Identity(10))
        );
        assert_eq!(
            time_map.get(Timestamp::from_secs(20)).map(|v| *v),
            Some(Identity(20))
        );
    }

    #[test]
    fn test_filter_cutoff_above_maximum_key_time_map() {
        let mut time_map = TimeFilterMap::<Identity<i32>, InMemoryDB>::new();
        time_map = time_map.insert(Timestamp::from_secs(10), 10);
        time_map = time_map.insert(Timestamp::from_secs(20), 20);
        time_map = time_map.filter(Timestamp::from_secs(21)); // Cutoff after latest key

        assert!(!time_map.contains(&10));
        assert!(!time_map.contains(&20));
        assert_eq!(time_map.get(Timestamp::from_secs(10)), None);
        assert_eq!(time_map.get(Timestamp::from_secs(20)), None);
    }

    #[test]
    fn test_filter_exact_match_time_map() {
        let mut time_map = TimeFilterMap::<Identity<i32>, InMemoryDB>::new();
        time_map = time_map.insert(Timestamp::from_secs(10), 10);
        time_map = time_map.insert(Timestamp::from_secs(20), 20);
        time_map = time_map.insert(Timestamp::from_secs(30), 30);

        time_map = time_map.filter(Timestamp::from_secs(20)); // Prunes keys strictly < 20 (shouldn't remove 20)

        assert!(!time_map.contains(&10));
        assert!(time_map.contains(&20));
        assert!(time_map.contains(&30));
        assert_eq!(time_map.get(Timestamp::from_secs(10)), None);
        assert_eq!(
            time_map.get(Timestamp::from_secs(20)).map(|v| *v),
            Some(Identity(20))
        );
        assert_eq!(
            time_map.get(Timestamp::from_secs(21)).map(|v| *v),
            Some(Identity(20))
        );
        assert_eq!(
            time_map.get(Timestamp::from_secs(30)).map(|v| *v),
            Some(Identity(30))
        );
    }

    #[test]
    fn test_multiple_filters_time_map() {
        let mut time_map = TimeFilterMap::<Identity<i32>, InMemoryDB>::new();
        time_map = time_map.insert(Timestamp::from_secs(10), 10);
        time_map = time_map.insert(Timestamp::from_secs(20), 20);
        time_map = time_map.insert(Timestamp::from_secs(30), 30);
        time_map = time_map.insert(Timestamp::from_secs(40), 40);

        time_map = time_map.filter(Timestamp::from_secs(20)); // Removes 10
        assert!(!time_map.contains(&10));
        assert!(time_map.contains(&20));
        assert_eq!(
            time_map.get(Timestamp::from_secs(30)).map(|v| *v),
            Some(Identity(30))
        );
        assert_eq!(
            time_map.get(Timestamp::from_secs(31)).map(|v| *v),
            Some(Identity(30))
        );

        time_map = time_map.filter(Timestamp::from_secs(35)); // Removes 20, 30
        assert!(!time_map.contains(&20));
        assert!(!time_map.contains(&30));
        assert!(time_map.contains(&40));
        assert_eq!(time_map.get(Timestamp::from_secs(39)), None);
        assert_eq!(
            time_map.get(Timestamp::from_secs(40)).map(|v| *v),
            Some(Identity(40))
        );
        assert_eq!(
            time_map.get(Timestamp::from_secs(41)).map(|v| *v),
            Some(Identity(40))
        );

        time_map = time_map.filter(Timestamp::from_secs(41)); // Removes 40. The map is now empty.
        assert!(!time_map.contains(&40));
        assert_eq!(time_map.get(Timestamp::from_secs(40)), None);
    }

    #[test]
    fn test_zero_timestamp_time_map() {
        let mut time_map = TimeFilterMap::<Identity<i32>, InMemoryDB>::new();
        time_map = time_map.insert(Timestamp::from_secs(0), 0);
        time_map = time_map.insert(Timestamp::from_secs(10), 10);

        assert_eq!(
            time_map.get(Timestamp::from_secs(0)).map(|v| *v),
            Some(Identity(0))
        );
        assert_eq!(
            time_map.get(Timestamp::from_secs(5)).map(|v| *v),
            Some(Identity(0))
        );
        assert!(time_map.contains(&0));

        time_map = time_map.filter(Timestamp::from_secs(0));
        assert_eq!(
            time_map.get(Timestamp::from_secs(0)).map(|v| *v),
            Some(Identity(0))
        );
        assert!(time_map.contains(&0));

        time_map = time_map.filter(Timestamp::from_secs(1));
        assert_eq!(time_map.get(Timestamp::from_secs(0)), None);
        assert!(!time_map.contains(&0));
        assert_eq!(time_map.get(Timestamp::from_secs(5)), None);
        assert_eq!(
            time_map.get(Timestamp::from_secs(10)).map(|v| *v),
            Some(Identity(10))
        );
    }

    #[test]
    fn test_large_key_differences_time_map() {
        let mut time_map = TimeFilterMap::<Identity<i32>, InMemoryDB>::new();
        time_map = time_map.insert(Timestamp::from_secs(10), 10);
        time_map = time_map.insert(Timestamp::from_secs(1000000), 1000000);

        assert_eq!(time_map.get(Timestamp::from_secs(9)).map(|v| *v), None);
        assert_eq!(
            time_map.get(Timestamp::from_secs(10)).map(|v| *v),
            Some(Identity(10))
        );
        assert_eq!(
            time_map.get(Timestamp::from_secs(11)).map(|v| *v),
            Some(Identity(10))
        );
        assert_eq!(
            time_map.get(Timestamp::from_secs(999999)).map(|v| *v),
            Some(Identity(10))
        );
        assert_eq!(
            time_map.get(Timestamp::from_secs(1000000)).map(|v| *v),
            Some(Identity(1000000))
        );
        assert_eq!(
            time_map.get(Timestamp::from_secs(1000001)).map(|v| *v),
            Some(Identity(1000000))
        );
        assert!(time_map.contains(&10));
        assert!(time_map.contains(&1000000));

        time_map = time_map.filter(Timestamp::from_secs(10));
        assert_eq!(
            time_map.get(Timestamp::from_secs(10)).map(|v| *v),
            Some(Identity(10))
        );
        assert_eq!(
            time_map.get(Timestamp::from_secs(1000000)).map(|v| *v),
            Some(Identity(1000000))
        );
        assert!(time_map.contains(&10));
        assert!(time_map.contains(&1000000));

        time_map = time_map.filter(Timestamp::from_secs(1000000));
        assert_eq!(time_map.get(Timestamp::from_secs(10)), None);
        assert_eq!(
            time_map.get(Timestamp::from_secs(1000000)).map(|v| *v),
            Some(Identity(1000000))
        );
        assert!(!time_map.contains(&10));
        assert!(time_map.contains(&1000000));

        time_map = time_map.filter(Timestamp::from_secs(99999999));
        assert_eq!(time_map.get(Timestamp::from_secs(1000000)), None);
        assert!(!time_map.contains(&1000000));
    }

    /// Test default storage APIs, including using `WrappedDB` for isolation.
    #[test]
    fn test_default_storage() {
        // Create isolated storage types for `DefaultDB`.
        struct Tag1;
        type D1 = WrappedDB<DefaultDB, Tag1>;
        struct Tag2;
        type D2 = WrappedDB<DefaultDB, Tag2>;

        // Check that implicitly creating default storage of type InMemoryDB (if
        // necessary) works, by requesting it. Since in theory some other test
        // thread could have explicitly set the InMemoryDB default storage, this
        // test is not accurate. An accurate test could:
        //

        // - hold the STORAGES lock
        // - remove any existing InMemoryDB that was set by implicit usage in another test thread
        // - check that we get a new one implicitly
        // - reinsert the old one, if any
        // - drop the lock
        //
        // But the implicit InMemoryDB is just a hack for testing anyway, so
        // we'll just check that it's set and not worry about how :)
        {
            default_storage::<InMemoryDB>();
            assert!(try_get_default_storage::<InMemoryDB>().is_some());
        }

        // Check that default storages of other db types are not created
        // implicitly.
        assert!(try_get_default_storage::<D1>().is_none());
        let result = std::panic::catch_unwind(|| {
            default_storage::<D1>();
        });
        assert!(result.is_err());

        // Create a default storage of type D1.
        let b1 = set_default_storage::<D1>(Storage::<D1>::default).unwrap();
        let s1 = b1.arena.alloc(42u8);
        assert!(default_storage::<D1>().get::<u8>(&s1.hash()).is_ok());

        // Check that D1 and D2 have disjoint default storages, even tho they're
        // the same underlying database type.
        set_default_storage::<D2>(Storage::<D2>::default).unwrap();
        assert!(default_storage::<D2>().get::<u8>(&s1.hash()).is_err());

        // Drop the D1 default storage and see that we can create a new one.
        unsafe_drop_default_storage::<D1>();
        assert!(try_get_default_storage::<D1>().is_none());
        set_default_storage::<D1>(Storage::<D1>::default).unwrap();
        assert!(default_storage::<D1>().get::<u8>(&s1.hash()).is_err());

        // Check that dropping the default storage for D1 didn't affect existing
        // references.
        assert!(b1.get::<u8>(&s1.hash()).is_ok());
        assert!(default_storage::<D1>().get::<u8>(&s1.hash()).is_err());

        // Check that we can restore the original D1 default storage (unlikely
        // use case ...)
        let s = Arc::into_inner(b1).expect("we should have the only reference");
        unsafe_drop_default_storage::<D1>();
        set_default_storage::<D1>(|| s).unwrap();
        assert!(default_storage::<D1>().get::<u8>(&s1.hash()).is_ok());
    }

    #[cfg(feature = "sqlite")]
    #[test]
    fn persist_to_disk_sqldb() {
        use crate::{DefaultHasher, db::SqlDB};

        let path = tempfile::NamedTempFile::new().unwrap().into_temp_path();
        test_persist_to_disk::<SqlDB<DefaultHasher>>(|| SqlDB::exclusive_file(&path));
    }

    #[cfg(feature = "parity-db")]
    #[test]
    fn persist_to_disk_paritydb() {
        use crate::{DefaultHasher, db::ParityDb};

        let path = tempfile::TempDir::new().unwrap().keep();
        test_persist_to_disk::<ParityDb<DefaultHasher>>(|| ParityDb::open(&path));
    }

    /// Test that persisting objects to disk works:
    ///
    /// - create a first storage backed by a first db
    /// - create an object, persist it, and flush the db
    /// - create a second storage, backed by a second db, pointing to the same
    ///   file as the first db
    /// - reload the object from the second storage and check its correctness
    ///
    /// This incidentally includes a test of `WrappedDB` and
    /// `set_default_storage`.
    ///
    /// This test doesn't make sense for `InMemoryDB`, because that DB doesn't
    /// persist to disk.
    #[cfg(any(feature = "sqlite", feature = "parity-db"))]
    fn test_persist_to_disk<D: DB>(mk_db: impl Fn() -> D) {
        // Create a unique wrapper type for D, to avoid conflicts with
        // other tests running using D.
        struct Tag;
        type W<D> = WrappedDB<D, Tag>;

        // Compute key in a block so that everything else gets dropped. Need to
        // drop everything to avoid needing non-exclusive access to the DB.
        let key1 = {
            let db1: W<D> = WrappedDB::wrap(mk_db());
            let storage1 = Storage::new(DEFAULT_CACHE_SIZE, db1);
            let storage1 = set_default_storage(|| storage1).unwrap();
            let arena = &storage1.arena;
            let vals1 = vec![1u8, 1, 2, 3, 5];
            let array1: super::Array<_, W<D>> = vals1.into();
            let sp1 = arena.alloc(array1.clone());
            sp1.persist();
            storage1.with_backend(|backend| backend.flush_all_changes_to_db());
            sp1.hash()
        };
        unsafe_drop_default_storage::<W<D>>();
        std::thread::sleep(std::time::Duration::from_secs(1));

        let db2: W<D> = WrappedDB::wrap(mk_db());
        let storage2 = Storage::new(DEFAULT_CACHE_SIZE, db2);
        let storage2 = set_default_storage(|| storage2).unwrap();
        let array1 = storage2.arena.get::<super::Array<_, _>>(&key1).unwrap();
        let vals2 = vec![1u8, 1, 2, 3, 5];
        let array2: super::Array<_, W<D>> = vals2.into();
        assert_eq!(*array1, array2);
    }

    // Test that malformed map with odd-length nibbles no longer cause panics in
    // iteration.
    //
    // This test was created to demonstrate a crash that has since been fixed in PR#612.
    #[test]
    fn deserialization_malicious_map() {
        use crate::arena::Sp;
        use crate::merkle_patricia_trie::{MerklePatriciaTrie, Node};
        use serialize::{Deserializable, Serializable};

        // Create a malformed Extension node with odd-length nibbles.  This
        // bypasses normal validation by directly constructing the node.
        let leaf: Node<u32> = Node::Leaf {
            ann: SizeAnn(0),
            value: Sp::new(42u32),
        };
        let extension_node = Node::Extension {
            ann: SizeAnn(1),
            compressed_path: vec![1, 2, 3], // 3 nibbles = odd length!
            child: Sp::new(leaf),
        };
        let mpt = MerklePatriciaTrie(Sp::new(extension_node));
        let malformed_map = Map {
            mpt: Sp::new(mpt),
            key_type: std::marker::PhantomData::<u32>,
        };

        // Serialize the malformed map to get the attack vector.
        // This simulates what an attacker could send as serialized data.
        let mut serialized = std::vec::Vec::new();
        malformed_map.serialize(&mut serialized).unwrap();

        // Deserialize using public API
        let mut cursor = std::io::Cursor::new(&serialized);
        assert!(Map::<u32, u32>::deserialize(&mut cursor, 0).is_err());
    }
}
