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

//! Curve selection for Midnight. This may change over time, but we are likely
//! to keep:
//!
//! * A primary prime field [`Fr`].
//! * Embedded elliptic curve points [`EmbeddedGroupAffine`].
//! * An Embedded prime field [`EmbeddedFr`].

use crate::macros::{fr_display, wrap_display, wrap_field_arith, wrap_group_arith};
use base_crypto::fab::{Aligned, Alignment, AlignmentAtom, AlignmentSegment};
use fake::{Dummy, Faker};
use ff::{Field, FromUniformBytes, PrimeField};
use group::Group;
use group::GroupEncoding;
use midnight_circuits::ecc::curves::CircuitCurve;
#[cfg(feature = "proptest")]
use proptest::prelude::Arbitrary;
use rand::Rng;
use rand::distributions::Standard;
use rand::prelude::Distribution;
use serde::{Deserialize, Deserializer, Serialize, Serializer, ser::Error as SerError};
use serialize::ScaleBigInt;
use serialize::Tagged;
use serialize::tag_enforcement_test;
use serialize::{Deserializable, Serializable};
#[cfg(feature = "proptest")]
use serialize::{NoStrategy, randomised_serialization_test, simple_arbitrary};
use std::cmp::Ordering;
use std::hash::Hasher;
use std::io::{self, Read, Write};
#[cfg(feature = "proptest")]
use std::marker::PhantomData;
use std::mem::size_of;
use std::ops::Mul;
use storage::{Storable, arena::ArenaKey, db::DB, storable::Loader};
use zeroize::DefaultIsZeroes;
/// The outer, main curve
pub mod outer {
    /// The base prime field, used to represent curve points
    pub type Base = midnight_curves::Fp;
    /// The scalar prime field, used in circuit
    pub type Scalar = midnight_curves::Fq;
    /// The affine representation of a curve point
    pub type Affine = midnight_curves::G1Affine;
    /// The size of the outer curve point in bytes
    pub const POINT_BYTES: usize = Affine::compressed_size();
}

/// The embedded / cycle curve, used in-circuit mainly
pub mod embedded {
    /// The base prime field, used to represent curve points; the scalar of [`outer`](super::outer)
    pub type Base = midnight_curves::Fq;
    /// The scalar prime field, used in embedded proofs
    pub type Scalar = midnight_curves::Fr;
    /// The affine representation of a curve point over the extended curve
    /// (which contains the relevant cryptographic subgroup).
    pub type AffineExtended = midnight_curves::JubjubExtended;
    /// The affine representation of a curve point of the relevant cryptographic subgroup.
    pub type Affine = midnight_curves::JubjubSubgroup;
}

// Since field elements are large, often sparse, and very common, we handle them specially
// for Borsh serialization: We begin with one byte indicating how many bytes are
// required to little-endian encode the field element, and then that many bytes
// of little endian encoding.
//
// For this encoding to be unique, it is an error for the last byte to be zero.
// (Zero itself is represented as the zero length)
//
// In pseudocode:
//
// b = ceil(f.log2() / 8)
// little_endian(f as u<b>)
macro_rules! field_serialize {
    ($name:ident, $wrapped:ty, $to_bytes:ident) => {
        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
                let mut vec = Vec::new();
                <$name as Serializable>::serialize(self, &mut vec).map_err(S::Error::custom)?;
                ser.serialize_bytes(&vec)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
                let bytes = serde_bytes::ByteBuf::deserialize(de)?;
                <$name as Deserializable>::deserialize(&mut &bytes[..], 0)
                    .map_err(serde::de::Error::custom)
            }
        }

        impl From<$name> for ScaleBigInt {
            fn from(val: $name) -> ScaleBigInt {
                let mut res = ScaleBigInt::default();
                let repr = val.0.to_repr();
                res.0[..repr.len()].copy_from_slice(&repr);
                res
            }
        }

        impl TryFrom<ScaleBigInt> for $name {
            type Error = std::io::Error;
            fn try_from(val: ScaleBigInt) -> std::io::Result<$name> {
                if val.0[FR_BYTES..].iter().any(|b| *b != 0) {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        concat!("out of range for ", stringify!($ty)),
                    ));
                }
                let cont_buf: [u8; FR_BYTES] = val.0[..FR_BYTES]
                    .try_into()
                    .expect("slice of known size must coerce to array");
                Ok($name(
                    <Option<_>>::from(<$wrapped>::from_repr(cont_buf.into())).ok_or_else(|| {
                        io::Error::new(
                            io::ErrorKind::InvalidData,
                            concat!("out of range for ", stringify!($name)),
                        )
                    })?,
                ))
            }
        }

        impl Serializable for $name {
            fn serialize(&self, writer: &mut impl Write) -> io::Result<()> {
                ScaleBigInt::from(*self).serialize(writer)
            }

            fn serialized_size(&self) -> usize {
                ScaleBigInt::from(*self).serialized_size()
            }
        }

        impl Deserializable for $name {
            fn deserialize(reader: &mut impl Read, recursion_depth: u32) -> io::Result<Self> {
                <$name>::try_from(ScaleBigInt::deserialize(reader, recursion_depth)?)
            }
        }
    };
}

/// An element of our primary prime field.
#[derive(Default, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Storable)]
#[storable(base)]
pub struct Fr(pub outer::Scalar);
wrap_field_arith!(Fr);
fr_display!(Fr);
field_serialize!(Fr, outer::Scalar, to_bytes_le);
#[cfg(feature = "proptest")]
randomised_serialization_test!(Fr);
#[cfg(feature = "proptest")]
simple_arbitrary!(Fr);

impl DefaultIsZeroes for Fr {}

impl Tagged for Fr {
    fn tag() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("fr-bls")
    }
    fn tag_unique_factor() -> String {
        "fr-bls".into()
    }
}
tag_enforcement_test!(Fr);

impl std::hash::Hash for Fr {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write(&self.0.to_bytes_le()[..]);
    }
}

impl Distribution<Fr> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> Fr {
        Fr(outer::Scalar::random(rng))
    }
}

/// The number of bits required to represent [Fr].
pub const FR_BITS: usize = <outer::Scalar as PrimeField>::NUM_BITS as usize;
/// The number of bytes required to represent [Fr].
pub const FR_BYTES: usize = FR_BITS.div_ceil(8);
/// The number of bytes which can fit in an [Fr].
pub const FR_BYTES_STORED: usize = FR_BYTES - 1;

impl Dummy<Faker> for Fr {
    fn dummy_with_rng<R: Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        rng.r#gen()
    }
}

/// An element of our embedded prime field.
#[derive(Default, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Storable)]
#[storable(base)]
pub struct EmbeddedFr(pub embedded::Scalar);
wrap_field_arith!(EmbeddedFr);
fr_display!(EmbeddedFr);
field_serialize!(EmbeddedFr, embedded::Scalar, to_bytes);
#[cfg(feature = "proptest")]
randomised_serialization_test!(EmbeddedFr);
#[cfg(feature = "proptest")]
simple_arbitrary!(EmbeddedFr);

impl Tagged for EmbeddedFr {
    fn tag() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("embedded-fr[v1]")
    }
    fn tag_unique_factor() -> String {
        "embedded-fr[v1]".into()
    }
}
tag_enforcement_test!(EmbeddedFr);

impl DefaultIsZeroes for EmbeddedFr {}

impl Distribution<EmbeddedFr> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> EmbeddedFr {
        EmbeddedFr(embedded::Scalar::random(rng))
    }
}

impl Dummy<Faker> for EmbeddedFr {
    fn dummy_with_rng<R: Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        rng.r#gen()
    }
}

macro_rules! derive_via {
    ($base:ty, $via:ty, $($ty:ty),*) => {
        $(
        impl From<$ty> for $base {
            fn from(val: $ty) -> $base {
                (val as $via).into()
            }
        }
        )*
    }
}

macro_rules! derive_signed {
    ($base:ty, $($ty:ty, $via:ty),*) => {
        $(
        impl From<$ty> for $base {
            fn from(val: $ty) -> $base {
                if val < 0 {
                    -<$base>::from(val.unsigned_abs())
                } else {
                    (val as $via).into()
                }
            }
        }
        )*
    }
}

impl From<bool> for Fr {
    fn from(val: bool) -> Fr {
        Fr(outer::Scalar::from(u64::from(val)))
    }
}

derive_via!(Fr, u64, u8, u16, u32);
derive_signed!(Fr, i8, u8, i16, u16, i32, u32, i64, u64, i128, u128);
derive_via!(EmbeddedFr, u64, u8, u16, u32);
derive_signed!(EmbeddedFr, i8, u8, i16, u16, i32, u32, i64, u64, i128, u128);

impl From<u64> for Fr {
    fn from(val: u64) -> Fr {
        Fr(outer::Scalar::from(val))
    }
}

impl From<u128> for Fr {
    fn from(val: u128) -> Fr {
        Fr(outer::Scalar::from_u128(val))
    }
}

impl Aligned for Fr {
    fn alignment() -> Alignment {
        Alignment::singleton(AlignmentAtom::Field)
    }
}

impl Aligned for EmbeddedFr {
    fn alignment() -> Alignment {
        Alignment::singleton(AlignmentAtom::Field)
    }
}

impl Aligned for EmbeddedGroupAffine {
    fn alignment() -> Alignment {
        Alignment(vec![
            AlignmentSegment::Atom(AlignmentAtom::Field),
            AlignmentSegment::Atom(AlignmentAtom::Field),
        ])
    }
}

impl From<bool> for EmbeddedFr {
    fn from(val: bool) -> EmbeddedFr {
        EmbeddedFr(embedded::Scalar::from(u64::from(val)))
    }
}

impl From<u64> for EmbeddedFr {
    fn from(val: u64) -> EmbeddedFr {
        EmbeddedFr(embedded::Scalar::from(val))
    }
}

impl From<u128> for EmbeddedFr {
    fn from(val: u128) -> EmbeddedFr {
        EmbeddedFr(embedded::Scalar::from_u128(val))
    }
}

impl TryFrom<EmbeddedFr> for Fr {
    type Error = ();
    fn try_from(val: EmbeddedFr) -> Result<Fr, Self::Error> {
        Fr::from_le_bytes(&val.as_le_bytes()).ok_or(())
    }
}

impl TryFrom<Fr> for EmbeddedFr {
    type Error = ();
    fn try_from(val: Fr) -> Result<EmbeddedFr, Self::Error> {
        EmbeddedFr::from_le_bytes(&val.as_le_bytes()).ok_or(())
    }
}

impl Fr {
    /// Interpret a little-endiang byte-string as an [Fr].
    pub fn from_le_bytes(bytes: &[u8]) -> Option<Self> {
        let mut repr = [0u8; FR_BYTES];
        if bytes.len() <= repr.len() {
            repr[..bytes.len()].copy_from_slice(bytes)
        } else {
            return None;
        }
        outer::Scalar::from_repr(repr).map(Fr).into()
    }

    /// Initialize an [Fr] from arbitrary 64 bytes (little-endian)
    /// ensuring the result falls into the space by taking modulo.
    pub fn from_uniform_bytes(bytes: &[u8; 64]) -> Self {
        Fr(outer::Scalar::from_uniform_bytes(&bytes))
    }

    /// Output an [Fr] as a little-endian bytes-string
    ///
    /// # Examples
    ///
    /// ```
    /// use midnight_transient_crypto::curve::Fr;
    /// assert_eq!(Fr::from(42), Fr::from_le_bytes(&Fr::from(42).as_le_bytes()).unwrap())
    /// ```
    pub fn as_le_bytes(&self) -> Vec<u8> {
        self.0.to_bytes_le().to_vec()
    }
}

impl EmbeddedFr {
    /// Interpret a little-endiang byte-string as an [`EmbeddedFr`].
    pub fn from_le_bytes(bytes: &[u8]) -> Option<Self> {
        let mut repr = [0u8; FR_BYTES];
        if bytes.len() <= repr.len() {
            repr[..bytes.len()].copy_from_slice(bytes)
        } else {
            return None;
        }
        embedded::Scalar::from_repr(repr).map(EmbeddedFr).into()
    }

    /// Output an [`EmbeddedFr`] as a little-endian bytes-string
    pub fn as_le_bytes(&self) -> Vec<u8> {
        self.0.to_bytes().to_vec()
    }
}

/// An element in the embedded elliptic curve.
#[derive(Default, Copy, Clone, Storable)]
#[storable(base)]
pub struct EmbeddedGroupAffine(pub embedded::Affine);

wrap_group_arith!(EmbeddedGroupAffine, EmbeddedFr);
wrap_display!(EmbeddedGroupAffine);
#[cfg(feature = "proptest")]
randomised_serialization_test!(EmbeddedGroupAffine);
#[cfg(feature = "proptest")]
simple_arbitrary!(EmbeddedGroupAffine);

impl Distribution<EmbeddedGroupAffine> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> EmbeddedGroupAffine {
        EmbeddedGroupAffine(embedded::Affine::random(rng))
    }
}

impl Mul<Fr> for EmbeddedGroupAffine {
    type Output = EmbeddedGroupAffine;

    fn mul(self, mut rhs: Fr) -> EmbeddedGroupAffine {
        let embedded_m1 = EmbeddedFr::from(0u64) - EmbeddedFr::from(1u64);
        let embedded_modulus = Fr::from_le_bytes(&embedded_m1.as_le_bytes())
            .expect("embedded modulus should fit in scalar field")
            + Fr::from(1);
        while rhs > embedded_modulus {
            rhs = rhs - embedded_modulus;
        }
        self * EmbeddedFr::try_from(rhs).expect("after reducing, rhs should fit in embedded scalar")
    }
}

impl From<embedded::Affine> for EmbeddedGroupAffine {
    fn from(g: embedded::Affine) -> Self {
        Self(g)
    }
}

impl Dummy<Faker> for EmbeddedGroupAffine {
    fn dummy_with_rng<R: Rng + ?Sized>(f: &Faker, rng: &mut R) -> Self {
        EmbeddedGroupAffine::generator() * EmbeddedFr::dummy_with_rng(f, rng)
    }
}

impl serde::Serialize for EmbeddedGroupAffine {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        let mut vec = Vec::new();
        <EmbeddedGroupAffine as Serializable>::serialize(self, &mut vec)
            .map_err(<S::Error as serde::ser::Error>::custom)?;
        ser.serialize_bytes(&vec)
    }
}

impl Serializable for EmbeddedGroupAffine {
    fn serialize(&self, writer: &mut impl Write) -> std::io::Result<()> {
        writer.write_all(self.0.to_bytes().as_ref())
    }

    fn serialized_size(&self) -> usize {
        size_of::<<embedded::Affine as GroupEncoding>::Repr>()
    }
}

impl Deserializable for EmbeddedGroupAffine {
    fn deserialize(reader: &mut impl Read, _recursion_depth: u32) -> std::io::Result<Self> {
        let mut data = <<embedded::AffineExtended as GroupEncoding>::Repr>::default();
        reader.read_exact(data.as_mut())?;
        <Option<_>>::from(embedded::Affine::from_bytes(&data).map(EmbeddedGroupAffine)).ok_or_else(
            || {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "invalid group element encoding",
                )
            },
        )
    }
}

impl Tagged for EmbeddedGroupAffine {
    fn tag() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("embedded-group-affine[v1]")
    }
    fn tag_unique_factor() -> String {
        "embedded-group-affine[v1]".into()
    }
}
tag_enforcement_test!(EmbeddedGroupAffine);

impl EmbeddedGroupAffine {
    /// Creates a new elliptic curve element from it's affine coordinates. It *is*
    /// checked for validity.
    pub fn new(x: Fr, y: Fr) -> Option<Self> {
        embedded::AffineExtended::from_xy(x.0, y.0).map(|p| EmbeddedGroupAffine(p.into_subgroup()))
    }

    /// Retrieves the curve point's affine `x` coordinate.
    /// Or `None` if this is the identity
    pub fn x(&self) -> Option<Fr> {
        Into::<embedded::AffineExtended>::into(self.0)
            .coordinates()
            .map(|c| Fr(c.0))
    }

    /// Retrieves the curve point's affine `y` coordinate.
    /// Or `None` if this is the identity
    pub fn y(&self) -> Option<Fr> {
        Into::<embedded::AffineExtended>::into(self.0)
            .coordinates()
            .map(|c| Fr(c.1))
    }

    /// Returns the primary generator of the embedded curve.
    pub fn generator() -> Self {
        EmbeddedGroupAffine(embedded::Affine::generator())
    }

    /// Returns the identity element for curve addition.
    pub fn identity() -> Self {
        EmbeddedGroupAffine(embedded::Affine::identity())
    }

    /// Returns if the curve point is the point at infinity.
    pub fn is_infinity(&self) -> bool {
        false
    }

    /// Whether or not this embedded curve has an infinity point in affine representation.
    pub const HAS_INFINITY: bool = true;
}

impl PartialOrd for EmbeddedGroupAffine {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl std::hash::Hash for EmbeddedGroupAffine {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.x().hash(state);
        self.y().hash(state);
    }
}

impl PartialEq for EmbeddedGroupAffine {
    fn eq(&self, other: &EmbeddedGroupAffine) -> bool {
        (self.x(), self.y()) == (other.x(), other.y())
    }
}

impl Eq for EmbeddedGroupAffine {}

impl Ord for EmbeddedGroupAffine {
    fn cmp(&self, other: &Self) -> Ordering {
        let a: Option<(embedded::Base, embedded::Base)> =
            Into::<embedded::AffineExtended>::into(self.0).coordinates();
        let b: Option<(embedded::Base, embedded::Base)> =
            Into::<embedded::AffineExtended>::into(other.0).coordinates();
        a.cmp(&b)
    }
}

impl AsRef<outer::Scalar> for Fr {
    fn as_ref(&self) -> &outer::Scalar {
        &self.0
    }
}

impl AsRef<embedded::Scalar> for EmbeddedFr {
    fn as_ref(&self) -> &embedded::Scalar {
        &self.0
    }
}

impl AsRef<embedded::Affine> for EmbeddedGroupAffine {
    fn as_ref(&self) -> &embedded::Affine {
        &self.0
    }
}

macro_rules! impl_smaller_ints {
    ($($ty:ty),* => $via:ty) => {
        $(
            impl TryFrom<Fr> for $ty {
                type Error = ();
                fn try_from(f: Fr) -> Result<$ty, ()> {
                    <$via>::try_from(f)?.try_into().map_err(|_| ())
                }
            }
        )*
    }
}

impl TryFrom<Fr> for u128 {
    type Error = ();
    fn try_from(f: Fr) -> Result<u128, ()> {
        let repr = f.0.to_repr();
        let limbs = repr.as_ref();
        if limbs[16..].iter().any(|limb| limb != &0) {
            Err(())
        } else {
            Ok(limbs[..16].iter().enumerate().fold(0, |acc, (i, byte)| {
                acc + ((*byte as u128) << (8 * i as u128))
            }))
        }
    }
}

impl_smaller_ints!(u8, u16, u32, u64 => u128);

impl TryFrom<Fr> for i128 {
    type Error = ();
    fn try_from(f: Fr) -> Result<i128, ()> {
        let positive_attempt = u128::try_from(f)
            .ok()
            .and_then(|uint| i128::try_from(uint).ok());
        if let Some(pos) = positive_attempt {
            return Ok(pos);
        }
        let negative_attempt = u128::try_from(-f)
            .ok()
            .and_then(|uint| i128::checked_sub_unsigned(0i128, uint));
        if let Some(neg) = negative_attempt {
            return Ok(neg);
        }
        Err(())
    }
}

impl_smaller_ints!(i8, i16, i32, i64 => i128);

impl TryFrom<Fr> for bool {
    type Error = ();
    fn try_from(f: Fr) -> Result<bool, ()> {
        match u64::try_from(f)? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fr_le_bytes() {
        let val = 0x1234u16;
        let val_le = val.to_le_bytes();
        assert_eq!(val_le, [0x34, 0x12]);
        assert_eq!(Fr::from_le_bytes(&val_le).unwrap(), val.into());
        let restored = Fr::from(val).as_le_bytes();
        assert_eq!(&restored[..2], &val_le);
        assert_eq!(&restored[2..], &[0u8; 30]);
    }

    #[test]
    fn test_identity_point() {
        let id = EmbeddedGroupAffine::identity();
        assert_eq!((id.x(), id.y()), (Some(0.into()), Some(1.into())));
        assert!(!id.is_infinity());
    }

    #[test]
    fn test_identity() {
        let id = EmbeddedGroupAffine::identity();
        assert_eq!(id * Fr::from(42), id);
    }

    #[test]
    fn embedded_fr_within_fr() {
        let embedded_m1 = EmbeddedFr::from(0) - EmbeddedFr::from(1);
        assert!(Fr::try_from(embedded_m1).is_ok());
        let outer_m1 = Fr::from(0) - Fr::from(1);
        assert!(EmbeddedFr::try_from(outer_m1).is_err());
    }

    #[test]
    fn test_embedded_group_ser() {
        let elem = EmbeddedGroupAffine::identity();
        let mut writer = Vec::new();
        Serializable::serialize(&elem, &mut writer).unwrap();
        assert_eq!(
            elem,
            <EmbeddedGroupAffine as Deserializable>::deserialize(&mut writer.as_slice(), 0)
                .unwrap()
        );
    }
}
