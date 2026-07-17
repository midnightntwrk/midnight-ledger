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
    field::foreign::params::MultiEmulationParams as MEP,
    types::{
        AssignedBit, AssignedByte, AssignedField, AssignedForeignPoint, AssignedNative,
        AssignedNativePoint, AssignedScalarOfNativeCurve, InnerValue,
    },
};
use midnight_curves::{Fr as JubjubFr, JubjubExtended, JubjubSubgroup, k256};
use midnight_proofs::{circuit::Value, plonk::Error};
#[cfg(feature = "proptest")]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};
use serialize::{Deserializable, Serializable, Tagged};
use transient_crypto::curve::{Fr, outer};

type F = outer::Scalar;

/// Type of IR values
#[cfg_attr(feature = "proptest", derive(Arbitrary))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Serializable)]
#[tag = "ir-type[v1]"]
pub enum IrType {
    /// Element of the BLS12-381 scalar field, a.k.a. the native field.
    /// This is also the base field of Jubjub.
    #[serde(rename = "Scalar<BLS12-381>")]
    Native,

    /// Boolean (true or false).
    Bool,

    /// 32 bytes.
    #[serde(rename = "Bytes<32>")]
    Bytes32,

    /// Point of the Jubjub elliptic curve.
    #[serde(rename = "Point<Jubjub>")]
    JubjubPoint,

    /// Element of the scalar field of Jubjub.
    #[serde(rename = "Scalar<Jubjub>")]
    JubjubScalar,

    /// Point of the Secp256k1 elliptic curve, also known as K256.
    #[serde(rename = "Point<Secp256k1>")]
    Secp256k1Point,

    /// Element of the base field of Secp256k1.
    #[serde(rename = "Base<Secp256k1>")]
    Secp256k1Base,

    /// Element of the scalar field of Secp256k1.
    #[serde(rename = "Scalar<Secp256k1>")]
    Secp256k1Scalar,
}

impl IrType {
    /// Number of raw `Fr` elements needed to represent a value of this type.
    pub fn encoded_len(&self) -> usize {
        match self {
            IrType::Native => 1,
            IrType::Bool => 1,
            IrType::Bytes32 => 2,
            IrType::JubjubPoint => 2,
            IrType::JubjubScalar => 1,

            IrType::Secp256k1Point => 5,
            IrType::Secp256k1Base => 2,
            IrType::Secp256k1Scalar => 2,
        }
    }
}

/// Off-circuit IR value carrying actual data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IrValue {
    /// BLS12-381 scalar field element.
    Native(Fr),

    /// Boolean.
    Bool(bool),

    /// 32 Bytes.
    Bytes32([u8; 32]),

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
}

impl IrValue {
    pub(crate) fn get_type(&self) -> IrType {
        match self {
            IrValue::Native(_) => IrType::Native,
            IrValue::Bool(_) => IrType::Bool,
            IrValue::Bytes32(_) => IrType::Bytes32,
            IrValue::JubjubPoint(_) => IrType::JubjubPoint,
            IrValue::JubjubScalar(_) => IrType::JubjubScalar,

            IrValue::Secp256k1Point(_) => IrType::Secp256k1Point,
            IrValue::Secp256k1Base(_) => IrType::Secp256k1Base,
            IrValue::Secp256k1Scalar(_) => IrType::Secp256k1Scalar,
        }
    }

    pub(crate) fn default(val_t: &IrType) -> Self {
        match val_t {
            IrType::Native => IrValue::Native(Fr::default()),
            IrType::Bool => IrValue::Bool(bool::default()),
            IrType::Bytes32 => IrValue::Bytes32([u8::default(); 32]),
            IrType::JubjubPoint => IrValue::JubjubPoint(JubjubSubgroup::default()),
            IrType::JubjubScalar => IrValue::JubjubScalar(JubjubFr::default()),

            IrType::Secp256k1Point => IrValue::Secp256k1Point(k256::K256::default()),
            IrType::Secp256k1Base => IrValue::Secp256k1Base(k256::Fp::default()),
            IrType::Secp256k1Scalar => IrValue::Secp256k1Scalar(k256::Fq::default()),
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
    Bytes32([AssignedByte<F>; 32]),
    JubjubPoint(AssignedNativePoint<JubjubExtended>),
    JubjubScalar(AssignedScalarOfNativeCurve<JubjubExtended>),

    Secp256k1Point(AssignedForeignPoint<F, k256::K256, MEP>),
    Secp256k1Base(AssignedField<F, k256::Fp, MEP>),
    Secp256k1Scalar(AssignedField<F, k256::Fq, MEP>),
}

impl CircuitValue {
    pub fn value(&self) -> Value<IrValue> {
        match self {
            CircuitValue::Native(x) => x.value().cloned().map(|x| IrValue::Native(Fr(x))),
            CircuitValue::Bool(b) => b.value().map(IrValue::Bool),
            CircuitValue::Bytes32(bs) => Value::<Vec<u8>>::from_iter(bs.iter().map(|b| b.value()))
                .map(|b| b.try_into().unwrap())
                .map(IrValue::Bytes32),
            CircuitValue::JubjubPoint(p) => p.value().map(IrValue::JubjubPoint),
            CircuitValue::JubjubScalar(s) => s.value().map(IrValue::JubjubScalar),

            CircuitValue::Secp256k1Point(p) => p.value().map(IrValue::Secp256k1Point),
            CircuitValue::Secp256k1Scalar(s) => s.value().map(IrValue::Secp256k1Scalar),
            CircuitValue::Secp256k1Base(s) => s.value().map(IrValue::Secp256k1Base),
        }
    }

    pub fn get_type(&self) -> IrType {
        match self {
            CircuitValue::Native(_) => IrType::Native,
            CircuitValue::Bool(_) => IrType::Bool,
            CircuitValue::Bytes32(_) => IrType::Bytes32,
            CircuitValue::JubjubPoint(_) => IrType::JubjubPoint,
            CircuitValue::JubjubScalar(_) => IrType::JubjubScalar,

            CircuitValue::Secp256k1Point(_) => IrType::Secp256k1Point,
            CircuitValue::Secp256k1Base(_) => IrType::Secp256k1Base,
            CircuitValue::Secp256k1Scalar(_) => IrType::Secp256k1Scalar,
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
    Bytes32 => [u8; 32],
    JubjubPoint => JubjubSubgroup,
    JubjubScalar => JubjubFr,

    Secp256k1Point => k256::K256,
    Secp256k1Base => k256::Fp,
    Secp256k1Scalar => k256::Fq,
);

// Derives implementations, for every basic type T:
//  - From<T> for CircuitValue
//  - TryFrom<CircuitValue> for T
impl_enum_from_try_from!(CircuitValue, Error, Error::Synthesis;
    Native => AssignedNative<F>,
    Bool => AssignedBit<F>,
    Bytes32 => [AssignedByte<F>; 32],
    JubjubPoint => AssignedNativePoint<JubjubExtended>,
    JubjubScalar => AssignedScalarOfNativeCurve<JubjubExtended>,

    Secp256k1Point => AssignedForeignPoint<F, k256::K256, MEP>,
    Secp256k1Base => AssignedField<F, k256::Fp, MEP>,
    Secp256k1Scalar => AssignedField<F, k256::Fq, MEP>,
);
