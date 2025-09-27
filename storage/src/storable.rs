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

//! A trait defining a `Storable` object, which can be assembled into a tree.

use crate::DefaultHasher;
use crate::arena::{ArenaKey, Sp, hash};
use crate::db::DB;
use base_crypto::signatures::{Signature, VerifyingKey};
use base_crypto::time::Timestamp;
use base_crypto::{
    cost_model::RunningCost,
    fab::{AlignedValue, Alignment, Value},
    hash::HashOutput,
};
use crypto::digest::Digest;
use derive_where::derive_where;
use macros::Storable;
#[cfg(feature = "proptest")]
use proptest::{
    prelude::*,
    strategy::{NewTree, ValueTree},
    test_runner::TestRunner,
};
use rand::prelude::Distribution;
use rand::{Rng, distributions::Standard};
use serialize::{Deserializable, Serializable, Tagged, tag_enforcement_test};
use sha2::Sha256;
use std::fmt::Debug;
use std::hash::Hash;
use std::marker::PhantomData;
use std::sync::Arc;

/// Super-trait containing all requirements for a Hasher
pub trait WellBehavedHasher: Digest + Send + Sync + Default + Debug + Clone + 'static {}

impl WellBehavedHasher for Sha256 {}

/// A loader for objects, for use in [`Storable::from_binary_repr`].
///
/// The intent is to instantiate this in different ways for deserializing
/// objects from the wire, vs deserializing them from the back-end.
pub trait Loader<D: DB> {
    /// Whether to check invariants for this loader.
    ///
    /// Essentially asks if data from this loader is from a trusted or untrusted source.
    const CHECK_INVARIANTS: bool;

    /// Get a smart pointer to the object with the given key.
    fn get<T: Storable<D>>(&self, key: &ChildNode<D::Hasher>) -> Result<Sp<T, D>, std::io::Error>;

    /// Allocate a new object in the arena.
    fn alloc<T: Storable<D>>(&self, obj: T) -> Sp<T, D>;

    /// Get the current recursion depth, for use with
    /// `Deserializable::deserialize`.
    fn get_recursion_depth(&self) -> u32;

    /// Does a check iff `CHECK_INVARIANTS` is true.
    fn do_check<T: Storable<D>>(&self, obj: T) -> std::io::Result<T> {
        if Self::CHECK_INVARIANTS {
            obj.check_invariant()?;
        }
        Ok(obj)
    }

    /// Convenience function that takes an iterator over `ArenaKey`s, and returns `Sp<T>` keyed by
    /// `iter.next()`.
    fn get_next<T: Storable<D>>(
        &self,
        iter: &mut impl Iterator<Item = ChildNode<D::Hasher>>,
    ) -> Result<Sp<T, D>, std::io::Error> {
        self.get(&iter.next().ok_or(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "iterator should not yield None".to_string(),
        ))?)
    }
}

#[derive(Debug, Clone, Storable, Serializable)]
#[derive_where(Hash, PartialEq, Eq, PartialOrd, Ord)]
#[storable(base)]
#[tag = "storage-child-node[v1]"]
#[phantom(H)]
/// A representataion of an individual child of a [Storable] object.
pub enum ChildNode<H: WellBehavedHasher = DefaultHasher> {
    /// A by-reference child, which can be looked up in the storage arena.
    Ref(ArenaKey<H>),
    /// A direct child, typically reserved for small children, represented as its raw data.
    Direct(DirectChildNode<H>),
}

impl<H: WellBehavedHasher> From<ArenaKey<H>> for ChildNode<H> {
    fn from(value: ArenaKey<H>) -> Self {
        ChildNode::Ref(value)
    }
}

impl<H: WellBehavedHasher> Distribution<ChildNode<H>> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> ChildNode<H> {
        ChildNode::Ref(rng.r#gen())
    }
}

impl<H: WellBehavedHasher> ChildNode<H> {
    /// Returns the hash of this child.
    pub fn hash(&self) -> &ArenaKey<H> {
        match self {
            ChildNode::Ref(h) => h,
            ChildNode::Direct(n) => &n.hash,
        }
    }

    /// Returns the referenced children that are *not* directly embedded in this node.
    pub fn refs(&self) -> Vec<&ArenaKey<H>> {
        let mut res = Vec::with_capacity(32);
        let mut frontier = Vec::with_capacity(32);
        frontier.push(self);
        while let Some(node) = frontier.pop() {
            match node {
                ChildNode::Ref(n) => res.push(n),
                ChildNode::Direct(d) => frontier.extend(d.children.iter()),
            }
        }
        res
    }
}

#[derive(Debug, Clone)]
#[derive_where(PartialOrd, Ord, Hash)]
/// The raw data of a child object
pub struct DirectChildNode<H: WellBehavedHasher> {
    /// The data label of this node
    pub data: Arc<Vec<u8>>,
    /// The child nodes
    pub children: Arc<Vec<ChildNode<H>>>,
    pub(crate) hash: ArenaKey<H>,
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
    pub(crate) fn new(data: Vec<u8>, children: Vec<ChildNode<H>>) -> Self {
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
        "(vec(u8),vec(storage-child-node[v1]))".to_owned()
    }
}

impl<H: WellBehavedHasher> Deserializable for DirectChildNode<H> {
    fn deserialize(reader: &mut impl std::io::Read, recursion_depth: u32) -> std::io::Result<Self> {
        let data: Vec<u8> = Deserializable::deserialize(reader, recursion_depth + 1)?;
        let children: Vec<ChildNode<H>> = Deserializable::deserialize(reader, recursion_depth + 1)?;
        Ok(DirectChildNode::new(data, children))
    }
}

/// A `Storable` object.
///
/// Some methods have `where Self: Sized` to mark them as "explicitly non
/// dispatchable", so that `Storable` can be object safe.
///
/// Assertions:
/// * To maintain an injective relationship between `Arena` and a Merkle Patricia trie a `Storable`
///   object can have no more than 16 children.
pub trait Storable<D: DB>: Clone + Sync + Send + 'static {
    /// Provides an iterator over hashes of child `Sp`s, if any. These hashes
    /// will be passed back into `from_binary_repr` when deserializing.
    fn children(&self) -> std::vec::Vec<ChildNode<D::Hasher>>;

    /// Serializes self, omitting any children.
    fn to_binary_repr<W: std::io::Write>(&self, writer: &mut W) -> Result<(), std::io::Error>
    where
        Self: Sized;

    /// Instantiates self, given hashes of any children, and loader that loads
    /// children given their hash.
    fn from_binary_repr<R: std::io::Read>(
        reader: &mut R,
        child_nodes: &mut impl Iterator<Item = ChildNode<D::Hasher>>,
        loader: &impl Loader<D>,
    ) -> Result<Self, std::io::Error>
    where
        Self: Sized;

    /// An invariant to check on deserialization. Should be invoked from within `from_binary_repr`.
    fn check_invariant(&self) -> Result<(), std::io::Error> {
        Ok(())
    }

    /// Represents self as a `ChildNode`
    fn as_child(&self) -> ChildNode<D::Hasher> {
        let children = self.children();
        assert!(
            children.len() <= 16,
            "In order to represent the arena as an MPT Storable values must have no more than 16 children (found: {} on type {})",
            children.len(),
            std::any::type_name::<Self>(),
        );
        let mut data: std::vec::Vec<u8> = std::vec::Vec::new();
        self.to_binary_repr(&mut data)
            .expect("Storable data should be able to be represented in binary");
        child_from(&data, &children)
    }
}

pub(crate) fn child_from<H: WellBehavedHasher>(
    data: &[u8],
    children: &[ChildNode<H>],
) -> ChildNode<H> {
    if is_in_small_object_limit(data, children) {
        ChildNode::Direct(DirectChildNode::new(data.to_vec(), children.to_vec()))
    } else {
        ChildNode::Ref(hash(&data, children.iter().map(ChildNode::hash)))
    }
}

fn is_in_small_object_limit<H: WellBehavedHasher>(data: &[u8], children: &[ChildNode<H>]) -> bool {
    const SMALL_OBJECT_LIMIT: usize = 1024;
    let mut size = 2 + data.len();
    for child in children.iter() {
        size += child.serialized_size();
        if size > SMALL_OBJECT_LIMIT {
            return false;
        }
    }
    size <= SMALL_OBJECT_LIMIT
}

/// Helper function, producing an error when an unrecognized discriminant is
/// encountered in implementing `Storable::from_binary_repr`.
fn bad_discriminant_error<A>() -> Result<A, std::io::Error> {
    Err(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        "Unrecognised discriminant",
    ))
}

/// Implements `Storable` for a type with no children
macro_rules! base_storable {
    ($val:ty) => {
        impl<D: DB> Storable<D> for $val {
            fn children(&self) -> std::vec::Vec<ChildNode<D::Hasher>> {
                std::vec::Vec::new()
            }

            /// Serializes self, omitting any children
            fn to_binary_repr<W: std::io::Write>(
                &self,
                writer: &mut W,
            ) -> Result<(), std::io::Error> {
                <$val as Serializable>::serialize(self, writer)
            }

            fn from_binary_repr<R: std::io::Read>(
                reader: &mut R,
                _child_hashes: &mut impl Iterator<Item = ChildNode<D::Hasher>>,
                loader: &impl Loader<D>,
            ) -> Result<Self, std::io::Error> {
                <$val as Deserializable>::deserialize(reader, loader.get_recursion_depth())
            }
        }
    };
}

/// A wrapper type representing size for storage objects
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq, PartialOrd, Ord, Serializable)]
#[tag = "size-annotation"]
pub struct SizeAnn(pub u64);
tag_enforcement_test!(SizeAnn);

base_storable!(());
base_storable!(bool);
base_storable!(u8);
base_storable!(u16);
base_storable!(u32);
base_storable!(u64);
base_storable!(u128);
base_storable!(i8);
base_storable!(i16);
base_storable!(i32);
base_storable!(i64);
base_storable!(i128);
base_storable!(HashOutput);
base_storable!(Value);
base_storable!(Alignment);
base_storable!(AlignedValue);
base_storable!(Signature);
base_storable!(VerifyingKey);
base_storable!(Timestamp);
base_storable!(RunningCost);
base_storable!(String);
base_storable!(SizeAnn);

impl<T: Send + Sync + 'static, D: DB> Storable<D> for PhantomData<T> {
    fn children(&self) -> std::vec::Vec<ChildNode<<D as DB>::Hasher>> {
        vec![]
    }
    fn to_binary_repr<W: std::io::Write>(&self, _writer: &mut W) -> Result<(), std::io::Error>
    where
        Self: Sized,
    {
        Ok(())
    }
    fn from_binary_repr<R: std::io::Read>(
        _reader: &mut R,
        _child_hashes: &mut impl Iterator<Item = ChildNode<<D as DB>::Hasher>>,
        _loader: &impl Loader<D>,
    ) -> Result<Self, std::io::Error>
    where
        Self: Sized,
    {
        Ok(PhantomData)
    }
}

#[cfg(test)]
// Storable for Vec is inherently unsafe as a Vec can be arbitrarily long whereas `Storable`
// requires that a node has no more than 16 children. However, it is useful for testing.
impl<T: Storable<D>, D: DB> Storable<D> for std::vec::Vec<Sp<T, D>> {
    fn children(&self) -> std::vec::Vec<ChildNode<<D as DB>::Hasher>> {
        self.iter().map(|v| Sp::as_child(v)).collect()
    }

    fn to_binary_repr<W: std::io::Write>(&self, writer: &mut W) -> Result<(), std::io::Error>
    where
        Self: Sized,
    {
        u8::serialize(&(self.len() as u8), writer)
    }

    fn from_binary_repr<R: std::io::Read>(
        reader: &mut R,
        child_hashes: &mut impl Iterator<Item = ChildNode<<D as DB>::Hasher>>,
        loader: &impl Loader<D>,
    ) -> Result<Self, std::io::Error>
    where
        Self: Sized,
    {
        let len = u8::deserialize(reader, loader.get_recursion_depth())?;

        let mut value = std::vec::Vec::new();

        for _ in 0..len {
            value.push(loader.get_next(child_hashes)?)
        }

        Ok(value)
    }
}

impl<T: Storable<D>, D: DB> Storable<D> for Option<Sp<T, D>> {
    fn children(&self) -> std::vec::Vec<ChildNode<D::Hasher>> {
        self.clone().map_or(vec![], |sp| vec![sp.as_child()])
    }

    /// Serializes self, omitting any children
    fn to_binary_repr<W: std::io::Write>(&self, writer: &mut W) -> Result<(), std::io::Error> {
        match self {
            Some(_) => <u8 as Serializable>::serialize(&0, writer),
            None => <u8 as Serializable>::serialize(&1, writer),
        }
    }

    fn from_binary_repr<R: std::io::Read>(
        reader: &mut R,
        child_hashes: &mut impl Iterator<Item = ChildNode<D::Hasher>>,
        loader: &impl Loader<D>,
    ) -> Result<Self, std::io::Error> {
        let dis = <u8 as Deserializable>::deserialize(reader, 0)?;
        match dis {
            0 => {
                let sp = loader.get_next::<T>(child_hashes)?;
                Ok(Some(sp))
            }
            1 => Ok(None),
            _ => bad_discriminant_error(),
        }
    }
}

macro_rules! tuple_storable {
    (($a:tt, $aidx: tt) $(, ($as:tt, $asidx:tt))*) => {
        impl<$a: Storable<D1>,$($as: Storable<D1>,)* D1: DB> Storable<D1> for (Sp<$a, D1>, $(Sp<$as, D1>,)*) {
            fn children(&self) -> std::vec::Vec<ChildNode<D1::Hasher>> {
                vec![self.$aidx.as_child() $(, self.$asidx.as_child())*]
            }

            /// Serializes self, omitting any children
            fn to_binary_repr<W: std::io::Write>(&self, _writer: &mut W) -> Result<(), std::io::Error> {
                Ok(())
            }

            fn from_binary_repr<R: std::io::Read>(
                _reader: &mut R,
                child_hashes: &mut impl Iterator<Item = ChildNode<D1::Hasher>>,
                loader: &impl Loader<D1>,
            ) -> Result<Self, std::io::Error> {
                Ok((loader.get_next::<$a>(child_hashes)?, $(loader.get_next::<$as>(child_hashes)?, )*))
            }
        }
    }
}

tuple_storable!((A, 0));
tuple_storable!((A, 0), (B, 1));
tuple_storable!((A, 0), (B, 1), (C, 2));
tuple_storable!((A, 0), (B, 1), (C, 2), (D, 3));
tuple_storable!((A, 0), (B, 1), (C, 2), (D, 3), (E, 4));
tuple_storable!((A, 0), (B, 1), (C, 2), (D, 3), (E, 4), (F, 5));
tuple_storable!((A, 0), (B, 1), (C, 2), (D, 3), (E, 4), (F, 5), (G, 6));
tuple_storable!(
    (A, 0),
    (B, 1),
    (C, 2),
    (D, 3),
    (E, 4),
    (F, 5),
    (G, 6),
    (H, 7)
);
tuple_storable!(
    (A, 0),
    (B, 1),
    (C, 2),
    (D, 3),
    (E, 4),
    (F, 5),
    (G, 6),
    (H, 7),
    (I, 8)
);
tuple_storable!(
    (A, 0),
    (B, 1),
    (C, 2),
    (D, 3),
    (E, 4),
    (F, 5),
    (G, 6),
    (H, 7),
    (I, 8),
    (J, 9)
);
tuple_storable!(
    (A, 0),
    (B, 1),
    (C, 2),
    (D, 3),
    (E, 4),
    (F, 5),
    (G, 6),
    (H, 7),
    (I, 8),
    (J, 9),
    (K, 10)
);
tuple_storable!(
    (A, 0),
    (B, 1),
    (C, 2),
    (D, 3),
    (E, 4),
    (F, 5),
    (G, 6),
    (H, 7),
    (I, 8),
    (J, 9),
    (K, 10),
    (L, 11)
);
tuple_storable!(
    (A, 0),
    (B, 1),
    (C, 2),
    (D, 3),
    (E, 4),
    (F, 5),
    (G, 6),
    (H, 7),
    (I, 8),
    (J, 9),
    (K, 10),
    (L, 11),
    (M, 12)
);
tuple_storable!(
    (A, 0),
    (B, 1),
    (C, 2),
    (D, 3),
    (E, 4),
    (F, 5),
    (G, 6),
    (H, 7),
    (I, 8),
    (J, 9),
    (K, 10),
    (L, 11),
    (M, 12),
    (N, 13)
);
tuple_storable!(
    (A, 0),
    (B, 1),
    (C, 2),
    (D, 3),
    (E, 4),
    (F, 5),
    (G, 6),
    (H, 7),
    (I, 8),
    (J, 9),
    (K, 10),
    (L, 11),
    (M, 12),
    (N, 13),
    (O, 14)
);
tuple_storable!(
    (A, 0),
    (B, 1),
    (C, 2),
    (D, 3),
    (E, 4),
    (F, 5),
    (G, 6),
    (H, 7),
    (I, 8),
    (J, 9),
    (K, 10),
    (L, 11),
    (M, 12),
    (N, 13),
    (O, 14),
    (P, 15)
);

#[macro_export]
#[cfg(feature = "proptest")]
/// Proptests for asserting `Storable` properties
macro_rules! randomised_storable_test {
    ($type:ty) => {
        #[cfg(test)]
        ::paste::paste! {
            /// Test that `to_binary_repr` followed by `from_binary_repr` is the identity
            /// for argument value.
            #[allow(non_snake_case)]
            #[test]
            fn [<proptest_storable_round_trip_ $type>]() where $type: proptest::prelude::Arbitrary {
                let mut runner = proptest::test_runner::TestRunner::default();

                runner.run(&<$type as proptest::prelude::Arbitrary>::arbitrary(), |v| {
                    let sp_v = default_storage::<DefaultDB>().arena.alloc(v.clone());
                    assert_eq!(&(*sp_v), &v);

                    let mut buf = std::vec::Vec::new();
                    Storable::<DefaultDB>::to_binary_repr(&v, &mut buf).unwrap();
                    let max_depth = None;
                    let arena = &default_storage().arena.clone();
                    let loader = BackendLoader::new(&arena, max_depth);
                    let v2 = <$type as Storable::<DefaultDB>>::from_binary_repr(&mut buf.as_slice(), &mut sp_v.children().into_iter(), &loader).unwrap();
                    assert_eq!(v, v2);

                    Ok(())
                }).unwrap();
            }
        }
    };
}

#[cfg(feature = "proptest")]
/// A proptest Tree for generating values of `Sp<T>`
pub struct SpTree<T: Storable<D>, TT: ValueTree<Value = T>, D: DB>(TT, PhantomData<D>);

#[cfg(feature = "proptest")]
impl<T: Storable<D> + Debug, TT: ValueTree<Value = T>, D: DB> ValueTree for SpTree<T, TT, D> {
    type Value = Sp<T, D>;

    fn current(&self) -> Self::Value {
        crate::storage::default_storage()
            .arena
            .alloc(self.0.current())
    }

    fn simplify(&mut self) -> bool {
        self.0.simplify()
    }

    fn complicate(&mut self) -> bool {
        self.0.complicate()
    }
}

#[cfg(feature = "proptest")]
#[derive(Debug)]
/// A proptest testing strategy for values of `Sp<T>`
pub struct SpStrategy<T: Storable<D>, S: Strategy<Value = T>, D: DB>(S, PhantomData<D>);

#[cfg(feature = "proptest")]
impl<T: Storable<D> + Debug, S: Strategy<Value = T>, D: DB> Strategy for SpStrategy<T, S, D> {
    type Tree = SpTree<T, S::Tree, D>;
    type Value = Sp<T, D>;

    fn new_tree(&self, runner: &mut TestRunner) -> NewTree<Self> {
        self.0.new_tree(runner).map(|t| SpTree(t, PhantomData))
    }
}

#[cfg(feature = "proptest")]
impl<T: Arbitrary + Storable<D>, D: DB> Arbitrary for Sp<T, D> {
    type Parameters = T::Parameters;
    type Strategy = SpStrategy<T, T::Strategy, D>;

    fn arbitrary_with(args: Self::Parameters) -> Self::Strategy {
        SpStrategy(T::arbitrary_with(args), PhantomData)
    }
}

#[cfg(all(feature = "proptest", test))]
mod proptests {
    use super::Storable;
    use crate::arena::{BackendLoader, Sp};
    use crate::{DefaultDB, storage::default_storage};
    use serialize::randomised_serialization_test;

    randomised_storable_test!(u32);
    type SimpleSpOption = Option<Sp<u8>>;
    randomised_storable_test!(SimpleSpOption);
    randomised_serialization_test!(SimpleSpOption);
    type SimpleSpTuple = (Sp<u8>, Sp<u8>);
    randomised_storable_test!(SimpleSpTuple);
    randomised_serialization_test!(SimpleSpTuple);
}
