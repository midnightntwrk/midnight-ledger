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

use midnight_circuits::{
    ecc::foreign::edwards_chip::AssignedForeignEdwardsPoint,
    field::foreign::params::MultiEmulationParams as MEP,
    types::{
        AssignedBit, AssignedByte, AssignedField, AssignedForeignPoint, AssignedNative,
        AssignedNativePoint, AssignedScalarOfNativeCurve, InnerValue,
    },
};
use midnight_curves::{Fr as JubjubFr, JubjubExtended, JubjubSubgroup, curve25519, k256, p256};
use midnight_proofs::{circuit::Value, plonk::Error};
#[cfg(feature = "proptest")]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use serialize::{Deserializable, Serializable, Tagged};
use transient_crypto::curve::{Fr, outer};

type F = outer::Scalar;

/// Number of bytes packed into a single native field element when encoding a
/// `Bytes(n)` value. 31 bytes (248 bits) fit safely below the BLS12-381 scalar
/// field modulus (~254 bits), so each chunk is canonical.
pub(crate) const BYTES_PER_FIELD_ELEMENT: usize = 31;

/// Maximum length `n` allowed for a `Bytes(n)` type (inclusive). Lengths are
/// bounded to keep encoded sizes and allocations sane; `2^24` bytes (16 MiB) is
/// far beyond any practical circuit input. This matches the bound Compact
/// imposes on byte-string lengths.
pub const MAX_BYTES_LEN: u32 = 1 << 24;

/// Type of IR values.
///
/// The serde (JSON) representation is a canonical string, e.g.
/// `"Scalar<BLS12-381>"` or `"Bytes<32>"`. It is implemented manually (see the
/// `Serialize`/`Deserialize` impls below) rather than through
/// `#[serde(rename)]`, so that the parametrized `Bytes<n>` form is supported
/// for every `n >= 1`.
#[cfg_attr(feature = "proptest", derive(Arbitrary))]
#[derive(Clone, Debug, PartialEq, Serializable)]
#[tag = "ir-type[v1]"]
pub enum IrType {
    /// Element of the BLS12-381 scalar field, a.k.a. the native field.
    /// This is also the base field of Jubjub.
    Native,

    /// Boolean (true or false).
    Bool,

    /// A single byte.
    Byte,

    /// A byte string of length `n` (`1 <= n <= `[`MAX_BYTES_LEN`]).
    ///
    /// Serializes as `"Bytes<n>"`; `Bytes(32)` is the former `Bytes32` type.
    /// `u32` (rather than `usize`) so the binary encoding is platform
    /// independent, matching the `u32` convention used for bit widths.
    Bytes(#[cfg_attr(feature = "proptest", proptest(strategy = "1u32..=1024u32"))] u32),

    /// Point of the Jubjub elliptic curve.
    JubjubPoint,

    /// Element of the scalar field of Jubjub.
    JubjubScalar,

    /// Point of the Secp256k1 elliptic curve, also known as K256.
    Secp256k1Point,

    /// Element of the base field of Secp256k1.
    Secp256k1Base,

    /// Element of the scalar field of Secp256k1.
    Secp256k1Scalar,

    /// Point of the Secp256r1 elliptic curve, also known as P256.
    Secp256r1Point,

    /// Element of the base field of Secp256r1.
    Secp256r1Base,

    /// Element of the scalar field of Secp256r1.
    Secp256r1Scalar,

    /// Point of the Curve25519 elliptic curve.
    Curve25519Point,

    /// Element of the base field of Curve25519.
    Curve25519Base,

    /// Element of the scalar field of Curve25519.
    Curve25519Scalar,
}

impl IrType {
    /// Number of raw `Fr` elements needed to represent a value of this type.
    pub fn encoded_len(&self) -> usize {
        match self {
            IrType::Native => 1,
            IrType::Bool => 1,
            IrType::Byte => 1,
            IrType::Bytes(n) => (*n as usize).div_ceil(BYTES_PER_FIELD_ELEMENT),
            IrType::JubjubPoint => 2,
            IrType::JubjubScalar => 1,

            IrType::Secp256k1Point => 5,
            IrType::Secp256k1Base => 2,
            IrType::Secp256k1Scalar => 2,

            IrType::Secp256r1Point => 5,
            IrType::Secp256r1Base => 2,
            IrType::Secp256r1Scalar => 2,

            // Edwards points encode as (x, y), with no additional
            // is-identity flag as needed for Weierstrass points.
            IrType::Curve25519Point => 4,
            IrType::Curve25519Base => 2,
            IrType::Curve25519Scalar => 2,
        }
    }

    /// Canonical string representation used in the serde (JSON) encoding.
    fn to_type_string(&self) -> String {
        match self {
            IrType::Native => "Scalar<BLS12-381>".to_string(),
            IrType::Bool => "Bool".to_string(),
            IrType::Byte => "Byte".to_string(),
            IrType::Bytes(n) => format!("Bytes<{n}>"),
            IrType::JubjubPoint => "Point<Jubjub>".to_string(),
            IrType::JubjubScalar => "Scalar<Jubjub>".to_string(),
            IrType::Secp256k1Point => "Point<Secp256k1>".to_string(),
            IrType::Secp256k1Base => "Base<Secp256k1>".to_string(),
            IrType::Secp256k1Scalar => "Scalar<Secp256k1>".to_string(),
            IrType::Secp256r1Point => "Point<Secp256r1>".to_string(),
            IrType::Secp256r1Base => "Base<Secp256r1>".to_string(),
            IrType::Secp256r1Scalar => "Scalar<Secp256r1>".to_string(),
            IrType::Curve25519Point => "Point<Curve25519>".to_string(),
            IrType::Curve25519Base => "Base<Curve25519>".to_string(),
            IrType::Curve25519Scalar => "Scalar<Curve25519>".to_string(),
        }
    }

    /// Parses the canonical string representation. Returns `None` for unknown
    /// or non-canonical strings, including `Bytes<0>`, non-canonical integer
    /// forms (e.g. leading zeros), and lengths outside `1..=`[`MAX_BYTES_LEN`].
    fn from_type_string(s: &str) -> Option<Self> {
        Some(match s {
            "Scalar<BLS12-381>" => IrType::Native,
            "Bool" => IrType::Bool,
            "Byte" => IrType::Byte,
            "Point<Jubjub>" => IrType::JubjubPoint,
            "Scalar<Jubjub>" => IrType::JubjubScalar,
            "Point<Secp256k1>" => IrType::Secp256k1Point,
            "Base<Secp256k1>" => IrType::Secp256k1Base,
            "Scalar<Secp256k1>" => IrType::Secp256k1Scalar,
            "Point<Secp256r1>" => IrType::Secp256r1Point,
            "Base<Secp256r1>" => IrType::Secp256r1Base,
            "Scalar<Secp256r1>" => IrType::Secp256r1Scalar,
            "Point<Curve25519>" => IrType::Curve25519Point,
            "Base<Curve25519>" => IrType::Curve25519Base,
            "Scalar<Curve25519>" => IrType::Curve25519Scalar,
            other => {
                let inner = other.strip_prefix("Bytes<")?.strip_suffix('>')?;
                // Reject non-canonical integers: empty, non-digits, or a
                // leading zero (which also rules out "0" itself).
                let canonical = !inner.is_empty()
                    && inner.bytes().all(|b| b.is_ascii_digit())
                    && !inner.starts_with('0');
                if !canonical {
                    return None;
                }
                let n: u32 = inner.parse().ok()?;
                if n > MAX_BYTES_LEN {
                    return None;
                }
                IrType::Bytes(n)
            }
        })
    }
}

impl Serialize for IrType {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_type_string())
    }
}

impl<'de> Deserialize<'de> for IrType {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = <String as Deserialize>::deserialize(deserializer)?;
        IrType::from_type_string(&s)
            .ok_or_else(|| de::Error::custom(format!("invalid IR type: {s:?}")))
    }
}

/// Off-circuit IR value carrying actual data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IrValue {
    /// BLS12-381 scalar field element.
    Native(Fr),

    /// Boolean.
    Bool(bool),

    /// A single byte.
    Byte(u8),

    /// A byte string of length `n`.
    Bytes(Vec<u8>),

    /// Jubjub point.
    JubjubPoint(JubjubSubgroup),

    /// Jubjub scalar field value.
    JubjubScalar(JubjubFr),

    /// Secp256k1 point.
    Secp256k1Point(k256::K256),

    /// Secp256k1 base field value.
    Secp256k1Base(k256::Fp),

    /// Secp256k1 scalar field value.
    Secp256k1Scalar(k256::Fq),

    /// Secp256r1 point.
    Secp256r1Point(p256::P256),

    /// Secp256r1 base field value.
    Secp256r1Base(p256::Fp),

    /// Secp256r1 scalar field value.
    Secp256r1Scalar(p256::Fq),

    /// Curve25519 point.
    Curve25519Point(curve25519::Curve25519Subgroup),

    /// Curve25519 base field value.
    Curve25519Base(curve25519::Fp),

    /// Curve25519 scalar field value.
    Curve25519Scalar(curve25519::Scalar),
}

impl IrValue {
    pub(crate) fn get_type(&self) -> IrType {
        match self {
            IrValue::Native(_) => IrType::Native,
            IrValue::Bool(_) => IrType::Bool,
            IrValue::Byte(_) => IrType::Byte,
            IrValue::Bytes(bs) => IrType::Bytes(bs.len() as u32),
            IrValue::JubjubPoint(_) => IrType::JubjubPoint,
            IrValue::JubjubScalar(_) => IrType::JubjubScalar,

            IrValue::Secp256k1Point(_) => IrType::Secp256k1Point,
            IrValue::Secp256k1Base(_) => IrType::Secp256k1Base,
            IrValue::Secp256k1Scalar(_) => IrType::Secp256k1Scalar,

            IrValue::Secp256r1Point(_) => IrType::Secp256r1Point,
            IrValue::Secp256r1Base(_) => IrType::Secp256r1Base,
            IrValue::Secp256r1Scalar(_) => IrType::Secp256r1Scalar,

            IrValue::Curve25519Point(_) => IrType::Curve25519Point,
            IrValue::Curve25519Base(_) => IrType::Curve25519Base,
            IrValue::Curve25519Scalar(_) => IrType::Curve25519Scalar,
        }
    }

    pub(crate) fn default(val_t: &IrType) -> Self {
        match val_t {
            IrType::Native => IrValue::Native(Fr::default()),
            IrType::Bool => IrValue::Bool(bool::default()),
            IrType::Byte => IrValue::Byte(u8::default()),
            IrType::Bytes(n) => IrValue::Bytes(vec![u8::default(); *n as usize]),
            IrType::JubjubPoint => IrValue::JubjubPoint(JubjubSubgroup::default()),
            IrType::JubjubScalar => IrValue::JubjubScalar(JubjubFr::default()),

            IrType::Secp256k1Point => IrValue::Secp256k1Point(k256::K256::default()),
            IrType::Secp256k1Base => IrValue::Secp256k1Base(k256::Fp::default()),
            IrType::Secp256k1Scalar => IrValue::Secp256k1Scalar(k256::Fq::default()),

            IrType::Secp256r1Point => IrValue::Secp256r1Point(p256::P256::default()),
            IrType::Secp256r1Base => IrValue::Secp256r1Base(p256::Fp::default()),
            IrType::Secp256r1Scalar => IrValue::Secp256r1Scalar(p256::Fq::default()),

            IrType::Curve25519Point => {
                IrValue::Curve25519Point(curve25519::Curve25519Subgroup::default())
            }
            IrType::Curve25519Base => IrValue::Curve25519Base(curve25519::Fp::default()),
            IrType::Curve25519Scalar => IrValue::Curve25519Scalar(curve25519::Scalar::default()),
        }
    }
}

/// In-circuit IR value, this is a placeholder for an [IrValue], a circuit
/// variable that does not necessarily carry actual data (it will carry data
/// during the proving process, but not during the circuit compilation)
#[derive(Clone, Debug)]
#[allow(clippy::large_enum_variant)]
pub enum CircuitValue {
    Native(AssignedNative<F>),
    Bool(AssignedBit<F>),
    Byte(AssignedByte<F>),
    Bytes(Vec<AssignedByte<F>>),
    JubjubPoint(AssignedNativePoint<JubjubExtended>),
    JubjubScalar(AssignedScalarOfNativeCurve<JubjubExtended>),

    Secp256k1Point(AssignedForeignPoint<F, k256::K256, MEP>),
    Secp256k1Base(AssignedField<F, k256::Fp, MEP>),
    Secp256k1Scalar(AssignedField<F, k256::Fq, MEP>),

    Secp256r1Point(AssignedForeignPoint<F, p256::P256, MEP>),
    Secp256r1Base(AssignedField<F, p256::Fp, MEP>),
    Secp256r1Scalar(AssignedField<F, p256::Fq, MEP>),

    Curve25519Point(AssignedForeignEdwardsPoint<F, curve25519::Curve25519, MEP>),
    Curve25519Base(AssignedField<F, curve25519::Fp, MEP>),
    Curve25519Scalar(AssignedField<F, curve25519::Scalar, MEP>),
}

impl CircuitValue {
    pub fn value(&self) -> Value<IrValue> {
        match self {
            CircuitValue::Native(x) => x.value().cloned().map(|x| IrValue::Native(Fr(x))),
            CircuitValue::Bool(b) => b.value().map(IrValue::Bool),
            CircuitValue::Byte(b) => b.value().map(IrValue::Byte),
            CircuitValue::Bytes(bs) => {
                Value::<Vec<u8>>::from_iter(bs.iter().map(|b| b.value())).map(IrValue::Bytes)
            }
            CircuitValue::JubjubPoint(p) => p.value().map(IrValue::JubjubPoint),
            CircuitValue::JubjubScalar(s) => s.value().map(IrValue::JubjubScalar),

            CircuitValue::Secp256k1Point(p) => p.value().map(IrValue::Secp256k1Point),
            CircuitValue::Secp256k1Scalar(s) => s.value().map(IrValue::Secp256k1Scalar),
            CircuitValue::Secp256k1Base(s) => s.value().map(IrValue::Secp256k1Base),

            CircuitValue::Secp256r1Point(p) => p.value().map(IrValue::Secp256r1Point),
            CircuitValue::Secp256r1Scalar(s) => s.value().map(IrValue::Secp256r1Scalar),
            CircuitValue::Secp256r1Base(s) => s.value().map(IrValue::Secp256r1Base),

            CircuitValue::Curve25519Point(p) => p.value().map(IrValue::Curve25519Point),
            CircuitValue::Curve25519Scalar(s) => s.value().map(IrValue::Curve25519Scalar),
            CircuitValue::Curve25519Base(s) => s.value().map(IrValue::Curve25519Base),
        }
    }

    pub fn get_type(&self) -> IrType {
        match self {
            CircuitValue::Native(_) => IrType::Native,
            CircuitValue::Bool(_) => IrType::Bool,
            CircuitValue::Byte(_) => IrType::Byte,
            CircuitValue::Bytes(bs) => IrType::Bytes(bs.len() as u32),
            CircuitValue::JubjubPoint(_) => IrType::JubjubPoint,
            CircuitValue::JubjubScalar(_) => IrType::JubjubScalar,

            CircuitValue::Secp256k1Point(_) => IrType::Secp256k1Point,
            CircuitValue::Secp256k1Base(_) => IrType::Secp256k1Base,
            CircuitValue::Secp256k1Scalar(_) => IrType::Secp256k1Scalar,

            CircuitValue::Secp256r1Point(_) => IrType::Secp256r1Point,
            CircuitValue::Secp256r1Base(_) => IrType::Secp256r1Base,
            CircuitValue::Secp256r1Scalar(_) => IrType::Secp256r1Scalar,

            CircuitValue::Curve25519Point(_) => IrType::Curve25519Point,
            CircuitValue::Curve25519Base(_) => IrType::Curve25519Base,
            CircuitValue::Curve25519Scalar(_) => IrType::Curve25519Scalar,
        }
    }
}

/// Implements both `From<T> for Enum` (wrap) and `TryFrom<Enum> for T` (unwrap)
/// for the specified enum variants.
macro_rules! impl_enum_from_try_from {
    ($enum:ident, $error:ty, $error_constructor:expr; $($variant:ident => $t:ty),* $(,)? ) => {
        $(
            // Wrap: From<T> -> Enum
            impl From<$t> for $enum {
                fn from(value: $t) -> Self {
                    $enum::$variant(value)
                }
            }

            // Unwrap: TryFrom<Enum> -> T
            impl std::convert::TryFrom<$enum> for $t {
                type Error = $error;

                fn try_from(value: $enum) -> Result<Self, Self::Error> {
                    match &value {
                        $enum::$variant(inner) => Ok(inner.clone()),
                        other => Err($error_constructor(
                            format!("cannot convert {:?} to {:?}",
                                     other.get_type(), stringify!($variant)),
                            )
                        ),
                    }
                }
            }
        )*
    };
}

// Derives implementations, for every basic type T:
//  - From<T> for IrValue
//  - TryFrom<IrValue> for T
impl_enum_from_try_from!(IrValue, anyhow::Error, anyhow::Error::msg;
    Native => Fr,
    Bool => bool,
    Byte => u8,
    Bytes => Vec<u8>,
    JubjubPoint => JubjubSubgroup,
    JubjubScalar => JubjubFr,

    Secp256k1Point => k256::K256,
    Secp256k1Base => k256::Fp,
    Secp256k1Scalar => k256::Fq,

    Secp256r1Point => p256::P256,
    Secp256r1Base => p256::Fp,
    Secp256r1Scalar => p256::Fq,

    Curve25519Point => curve25519::Curve25519Subgroup,
    Curve25519Base => curve25519::Fp,
    Curve25519Scalar => curve25519::Scalar,
);

// Derives implementations, for every basic type T:
//  - From<T> for CircuitValue
//  - TryFrom<CircuitValue> for T
impl_enum_from_try_from!(CircuitValue, Error, Error::Synthesis;
    Native => AssignedNative<F>,
    Bool => AssignedBit<F>,
    Byte => AssignedByte<F>,
    Bytes => Vec<AssignedByte<F>>,
    JubjubPoint => AssignedNativePoint<JubjubExtended>,
    JubjubScalar => AssignedScalarOfNativeCurve<JubjubExtended>,

    Secp256k1Point => AssignedForeignPoint<F, k256::K256, MEP>,
    Secp256k1Base => AssignedField<F, k256::Fp, MEP>,
    Secp256k1Scalar => AssignedField<F, k256::Fq, MEP>,

    Secp256r1Point => AssignedForeignPoint<F, p256::P256, MEP>,
    Secp256r1Base => AssignedField<F, p256::Fp, MEP>,
    Secp256r1Scalar => AssignedField<F, p256::Fq, MEP>,

    Curve25519Point => AssignedForeignEdwardsPoint<F, curve25519::Curve25519, MEP>,
    Curve25519Base => AssignedField<F, curve25519::Fp, MEP>,
    Curve25519Scalar => AssignedField<F, curve25519::Scalar, MEP>,
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_type_serde_roundtrip() {
        // MAX_BYTES_LEN is the inclusive upper bound.
        for n in [1u32, 2, 31, 32, 48, 1000, MAX_BYTES_LEN] {
            let t = IrType::Bytes(n);
            let s = serde_json::to_string(&t).unwrap();
            assert_eq!(s, format!("\"Bytes<{n}>\""));
            let back: IrType = serde_json::from_str(&s).unwrap();
            assert_eq!(back, t);
        }
    }

    #[test]
    fn bytes_type_length_bound_enforced() {
        // The bound is inclusive: 2^24 is accepted, 2^24 + 1 is rejected.
        let at_bound = format!(r#""Bytes<{}>""#, MAX_BYTES_LEN);
        assert_eq!(
            serde_json::from_str::<IrType>(&at_bound).unwrap(),
            IrType::Bytes(MAX_BYTES_LEN)
        );
        let over_bound = format!(r#""Bytes<{}>""#, MAX_BYTES_LEN + 1);
        assert!(serde_json::from_str::<IrType>(&over_bound).is_err());
        // Values exceeding u32 are rejected as well.
        assert!(serde_json::from_str::<IrType>(r#""Bytes<99999999999>""#).is_err());
    }

    #[test]
    fn bytes_type_rejects_non_canonical() {
        // n = 0, leading zeros, whitespace, empty, and non-digits are all rejected.
        for s in [
            r#""Bytes<0>""#,
            r#""Bytes<00>""#,
            r#""Bytes<01>""#,
            r#""Bytes< 1>""#,
            r#""Bytes<1 >""#,
            r#""Bytes<>""#,
            r#""Bytes<-1>""#,
            r#""Bytes<1a>""#,
        ] {
            assert!(
                serde_json::from_str::<IrType>(s).is_err(),
                "should reject {s}"
            );
        }
    }

    #[test]
    fn legacy_bytes32_string_parses_as_bytes_32() {
        let t: IrType = serde_json::from_str(r#""Bytes<32>""#).unwrap();
        assert_eq!(t, IrType::Bytes(32));
        assert_eq!(serde_json::to_string(&t).unwrap(), r#""Bytes<32>""#);
    }

    #[test]
    fn other_types_serde_unchanged() {
        assert_eq!(
            serde_json::to_string(&IrType::Native).unwrap(),
            r#""Scalar<BLS12-381>""#
        );
        assert_eq!(serde_json::to_string(&IrType::Byte).unwrap(), r#""Byte""#);
    }
}
