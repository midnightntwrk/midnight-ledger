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
    instructions::{DecompositionInstructions, PublicInputInstructions, ZeroInstructions},
    types::{
        AssignedBigUint, AssignedField, AssignedForeignPoint, AssignedNative, AssignedNativePoint,
        AssignedScalarOfNativeCurve, Instantiable,
    },
};
use midnight_curves::{Fr as JubjubFr, JubjubExtended, k256};
use midnight_proofs::{circuit::Layouter, plonk::Error};
use midnight_zk_stdlib::ZkStdLib;
use num_bigint::BigUint;
use num_traits::Num;
use transient_crypto::curve::Fr;

use crate::{
    ir_instructions::F,
    ir_types::{CircuitValue, IrType, IrValue},
};
use anyhow::anyhow;

/// Encodes the given off-circuit value as a vector of IrValue::Native.
pub fn encode_offcircuit(value: &IrValue) -> Vec<IrValue> {
    let encoded = match value {
        IrValue::Native(x) => AssignedNative::<F>::as_public_input(&x.0),
        IrValue::Bytes32(bs) => {
            let mut low: [u8; 32] = *bs;
            low[31] = 0;

            let mut high = [0u8; 32];
            high[0] = bs[31];

            vec![
                F::from_bytes_le(&low).unwrap(),
                F::from_bytes_le(&high).unwrap(),
            ]
        }
        IrValue::JubjubPoint(p) => AssignedNativePoint::<JubjubExtended>::as_public_input(p),
        IrValue::JubjubScalar(s) => {
            let encoded = AssignedScalarOfNativeCurve::<JubjubExtended>::as_public_input(s);
            // In ZKIRv3, an assigned scalar can only originate from a circuit
            // input (PublicInput or PrivateInput), which yields canonical assigned
            // scalars (whose internal representation uses at most 252 bits).
            assert_eq!(encoded.len(), 1);
            encoded
        }

        IrValue::Secp256k1Point(p) => {
            AssignedForeignPoint::<F, k256::K256, MEP>::as_public_input(p)
        }
        IrValue::Secp256k1Base(s) => AssignedField::<F, k256::Fp, MEP>::as_public_input(s),
        IrValue::Secp256k1Scalar(s) => AssignedField::<F, k256::Fq, MEP>::as_public_input(s),
    };
    encoded
        .into_iter()
        .map(|s| IrValue::Native(Fr(s)))
        .collect()
}

/// Encodes the given in-circuit value as a vector of CircuitValue::Native.
pub fn encode_incircuit(
    std_lib: &ZkStdLib,
    layouter: &mut impl Layouter<F>,
    value: &CircuitValue,
) -> Result<Vec<CircuitValue>, Error> {
    let encoded = match value {
        CircuitValue::Native(x) => std_lib.as_public_input(layouter, x),
        CircuitValue::Bytes32(bs) => Ok(vec![
            std_lib.assigned_from_le_bytes(layouter, &bs[..31])?,
            bs[31].clone().into(),
        ]),
        CircuitValue::JubjubPoint(p) => std_lib.jubjub().as_public_input(layouter, p),
        CircuitValue::JubjubScalar(s) => {
            // Jubjub::Scalar::NUM_BITS is incorrectly set to 255 (instead of 252)
            // in midnight-curves v0.2.0. Consequently, Jubjub scalars may be encoded
            // unnecessarily as 2 native field values instead of one.
            // We return the first only and make sure the rest (supposedly one more)
            // are zero.
            let encoded = std_lib.jubjub().as_public_input(layouter, s)?;
            for x in encoded[1..].iter() {
                std_lib.assert_zero(layouter, x)?;
            }
            Ok(encoded[..1].to_vec())
        }

        CircuitValue::Secp256k1Point(p) => std_lib.secp256k1().as_public_input(layouter, p),
        CircuitValue::Secp256k1Base(s) => {
            (std_lib.secp256k1().base_field_chip()).as_public_input(layouter, s)
        }
        CircuitValue::Secp256k1Scalar(s) => {
            (std_lib.secp256k1().scalar_field_chip()).as_public_input(layouter, s)
        }
    }?;
    Ok(encoded.into_iter().map(CircuitValue::Native).collect())
}

/// Decodes the given Fr values as an IrValue of the given type.
///
/// # Errors
///
/// Returns an error if the provided raw values cannot be decoded as the given type.
pub fn decode_offcircuit(encoded: &[Fr], val_t: &IrType) -> Result<IrValue, anyhow::Error> {
    let encoded: Vec<F> = encoded.iter().map(|f| f.0).collect();
    match val_t {
        IrType::Native => AssignedNative::<F>::from_public_input(&encoded)
            .map(Fr)
            .map(IrValue::Native),

        IrType::Bytes32 => {
            if encoded.len() != 2 {
                None
            } else {
                let mut bytes = encoded[0].to_bytes_le();
                assert_eq!(bytes[31], 0);

                let high = encoded[1].to_bytes_le();
                for b in high.iter().skip(1) {
                    assert_eq!(*b, 0);
                }

                bytes[31] = high[0];
                Some(IrValue::Bytes32(bytes))
            }
        }

        IrType::JubjubPoint => AssignedNativePoint::<JubjubExtended>::from_public_input(&encoded)
            .map(IrValue::JubjubPoint),

        IrType::JubjubScalar => {
            AssignedScalarOfNativeCurve::<JubjubExtended>::from_public_input(&encoded)
                .map(IrValue::JubjubScalar)
        }

        IrType::Secp256k1Point => {
            AssignedForeignPoint::<F, k256::K256, MEP>::from_public_input(&encoded)
                .map(IrValue::Secp256k1Point)
        }

        IrType::Secp256k1Base => AssignedField::<F, k256::Fp, MEP>::from_public_input(&encoded)
            .map(IrValue::Secp256k1Base),

        IrType::Secp256k1Scalar => AssignedField::<F, k256::Fq, MEP>::from_public_input(&encoded)
            .map(IrValue::Secp256k1Scalar),
    }
    .ok_or_else(|| anyhow!("Failed to decode {encoded:?} as {val_t:?}"))
}

/// Converts a native field element to a Jubjub scalar by reducing modulo
/// the Jubjub scalar field order if necessary.
pub fn native_to_jubjub_scalar(native: &Fr) -> JubjubFr {
    let mut bytes = [0u8; 64];
    bytes[..32].copy_from_slice(&native.0.to_bytes_le());
    JubjubFr::from_bytes_wide(&bytes)
}

/// Reduces the given biguint modulo the Jubjub scalar field order and returns the
/// result as an `AssignedScalarOfNativeCurve<JubjubExtended>` of exactly 252 bits.
pub fn jubjub_scalar_from_biguint(
    std_lib: &ZkStdLib,
    layouter: &mut impl Layouter<F>,
    x: AssignedBigUint<F>,
) -> Result<AssignedScalarOfNativeCurve<JubjubExtended>, Error> {
    let jubjub_order = {
        let p_str = "e7db4ea6533afa906673b0101343b00a6682093ccc81082d0970e5ed6f72cb7";
        let p = BigUint::from_str_radix(p_str, 16).unwrap();
        std_lib.biguint().assign_fixed_biguint(layouter, p)?
    };
    let (_q, r) = std_lib.biguint().div_rem(layouter, &x, &jubjub_order)?;

    let r_le_bytes = std_lib.biguint().to_le_bytes(layouter, &r)?;
    std_lib
        .jubjub()
        .scalar_from_le_bytes(layouter, &r_le_bytes)
}
