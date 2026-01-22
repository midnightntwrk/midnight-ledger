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

use midnight_circuits::types::AssignedNative;
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
    #[serde(rename = "Scalar<BLS12-381>")]
    Native,
}

/// Off-circuit IR value carrying actual data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IrValue {
    Native(Fr),
}

impl IrValue {
    pub(crate) fn get_type(&self) -> IrType {
        match self {
            IrValue::Native(_) => IrType::Native,
        }
    }
}

/// In-circuit IR value, this is a placeholder for an [IrValue], a circuit
/// variable that does not necessarily carry actual data (it will carry data
/// during the proving process, but not during the circuit compilation)
#[derive(Clone, Debug)]
pub enum CircuitValue {
    Native(AssignedNative<F>),
}

impl CircuitValue {
    pub fn value(&self) -> Value<IrValue> {
        match self {
            CircuitValue::Native(x) => x.value().cloned().map(|x| IrValue::Native(Fr(x))),
        }
    }

    pub fn get_type(&self) -> IrType {
        match self {
            CircuitValue::Native(_) => IrType::Native,
        }
    }
}

/// Implements both `From<T> for Enum` (wrap) and `TryFrom<Enum> for T` (unwrap)
/// for the specified enum variants.
macro_rules! impl_enum_from_try_from {
    ($enum:ident, $error:ty { $($variant:ident => $t:ty),* $(,)? }) => {
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
                    }
                }
            }
        )*
    };
}

// Derives implementations, for every basic type T:
//  - From<T> for IrValue
//  - TryFrom<IrValue> for T
impl_enum_from_try_from!(IrValue, anyhow::Error {
    Native => Fr,
});

// Derives implementations, for every basic type T:
//  - From<T> for CircuitValue
//  - TryFrom<CircuitValue> for T
impl_enum_from_try_from!(CircuitValue, Error {
    Native => AssignedNative<F>,
});
