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

use crate::arena::{ArenaKey, Sp};
use crate::db::DB;
use base_crypto::signatures::{Signature, VerifyingKey};
use base_crypto::time::Timestamp;
use base_crypto::{
    cost_model::RunningCost,
    fab::{AlignedValue, Alignment, Value},
    hash::HashOutput,
};
use crypto::digest::Digest;
#[cfg(feature = "proptest")]
use proptest::{
    prelude::*,
    strategy::{NewTree, ValueTree},
    test_runner::TestRunner,
};
use serialize::{Deserializable, Serializable, Tagged, tag_enforcement_test};
use sha2::Sha256;
use std::fmt::Debug;
use std::marker::PhantomData;

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
    fn get<T: Storable<D>>(&self, key: &ArenaKey<D::Hasher>) -> Result<Sp<T, D>, std::io::Error>;

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
        iter: &mut impl Iterator<Item = ArenaKey<D::Hasher>>,
    ) -> Result<Sp<T, D>, std::io::Error> {
        self.get(&iter.next().ok_or(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "iterator should not yield None".to_string(),
        ))?)
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
    fn children(&self) -> std::vec::Vec<ArenaKey<D::Hasher>>;

    /// Serializes self, omitting any children.
    fn to_binary_repr<W: std::io::Write>(&self, writer: &mut W) -> Result<(), std::io::Error>
    where
        Self: Sized;

    /// Instantiates self, given hashes of any children, and loader that loads
    /// children given their hash.
    fn from_binary_repr<R: std::io::Read>(
        reader: &mut R,
        child_hashes: &mut impl Iterator<Item = ArenaKey<D::Hasher>>,
        loader: &impl Loader<D>,
    ) -> Result<Self, std::io::Error>
    where
        Self: Sized;

    /// An invariant to check on deserialization. Should be invoked from within `from_binary_repr`.
    fn check_invariant(&self) -> Result<(), std::io::Error> {
        Ok(())
    }
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
            fn children(&self) -> std::vec::Vec<ArenaKey<D::Hasher>> {
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
                _child_hashes: &mut impl Iterator<Item = ArenaKey<D::Hasher>>,
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
    fn children(&self) -> std::vec::Vec<ArenaKey<<D as DB>::Hasher>> {
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
        _child_hashes: &mut impl Iterator<Item = ArenaKey<<D as DB>::Hasher>>,
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
    fn children(&self) -> std::vec::Vec<ArenaKey<<D as DB>::Hasher>> {
        self.iter().map(|v| Sp::hash(v).clone().into()).collect()
    }

    fn to_binary_repr<W: std::io::Write>(&self, writer: &mut W) -> Result<(), std::io::Error>
    where
        Self: Sized,
    {
        u8::serialize(&(self.len() as u8), writer)
    }

    fn from_binary_repr<R: std::io::Read>(
        reader: &mut R,
        child_hashes: &mut impl Iterator<Item = ArenaKey<<D as DB>::Hasher>>,
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
    fn children(&self) -> std::vec::Vec<ArenaKey<D::Hasher>> {
        self.clone().map_or(vec![], |sp| vec![sp.root.clone()])
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
        child_hashes: &mut impl Iterator<Item = ArenaKey<D::Hasher>>,
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
            fn children(&self) -> std::vec::Vec<ArenaKey<D1::Hasher>> {
                vec![self.$aidx.hash().clone().into() $(, self.$asidx.hash().clone().into())*]
            }

            /// Serializes self, omitting any children
            fn to_binary_repr<W: std::io::Write>(&self, _writer: &mut W) -> Result<(), std::io::Error> {
                Ok(())
            }

            fn from_binary_repr<R: std::io::Read>(
                _reader: &mut R,
                child_hashes: &mut impl Iterator<Item = ArenaKey<D1::Hasher>>,
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
