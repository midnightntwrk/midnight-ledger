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

use midnight_circuits::field::foreign::params::MultiEmulationParams as MEP;
use midnight_circuits::types::{AssignedField, AssignedNative};
use midnight_circuits::types::{
    AssignedForeignPoint, AssignedNativePoint, AssignedScalarOfNativeCurve, Instantiable,
};
use midnight_curves::{Fr as JubjubFr, JubjubExtended, k256};
use midnight_proofs::circuit::Value;
use midnight_proofs::{circuit::Layouter, plonk};
use midnight_zk_stdlib::ZkStdLib;
use transient_crypto::curve::Fr;

use crate::ir_instructions::assign::assign_incircuit;
use crate::ir_instructions::constrain_eq::constrain_eq_incircuit;
use crate::ir_instructions::encode::encode_incircuit;
use crate::{
    ir_instructions::F,
    ir_types::{CircuitValue, IrType, IrValue},
};
use anyhow::anyhow;

/// Decodes the given Fr values as an IrValue value of the given type.
///
/// # Errors
///
/// This function returns an error if the provided raw values cannot be
/// decoded as the given type.
pub fn decode_offcircuit(encoded: &[Fr], val_t: &IrType) -> Result<IrValue, anyhow::Error> {
    let encoded: Vec<F> = encoded.iter().map(|f| f.0).collect();
    match val_t {
        IrType::Native => AssignedNative::from_public_input(&encoded)
            .map(Fr)
            .map(IrValue::Native),

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

        IrType::Secp256k1Base => {
            AssignedField::<F, k256::Fp, MEP>::from_public_input(&encoded)
                .map(IrValue::Secp256k1Base)
        }

        IrType::Secp256k1Scalar => {
            AssignedField::<F, k256::Fq, MEP>::from_public_input(&encoded)
                .map(IrValue::Secp256k1Scalar)
        }
    }
    .ok_or_else(|| anyhow!("Failed to decode {encoded:?} as {val_t:?}"))
}

/// Decodes the given in-circuit `Native` values as CircuitValue value of the
/// given type.
///
/// # Errors
///
/// This function returns an error if the provided raw values cannot be
/// decoded as the given type.
pub fn decode_incircuit(
    std_lib: &ZkStdLib,
    layouter: &mut impl Layouter<F>,
    encoded: &[AssignedNative<F>],
    val_t: &IrType,
) -> Result<CircuitValue, plonk::Error> {
    // We witness the decoded value, encode it in-circuit and constraint the output of such
    // encoding to be equal to the `encoded` inputs to this function.
    // This guarantees that `decode` is the inverse of `encode` on the image of `encode`,
    // and that `decode` leads to an unsatisfiable circuit if given a vector of scalars that
    // is not in the image of `encode`.

    let encoded_val: Value<Vec<F>> = Value::from_iter(encoded.iter().map(|x| x.value().copied()));
    let decoded_val = encoded_val
        .map_with_result(|v| {
            decode_offcircuit(&v.iter().map(|f| Fr(*f)).collect::<Vec<_>>(), val_t)
        })
        .map_err(|e| plonk::Error::Synthesis(format!("{e:?}")))?;
    let decoded = assign_incircuit(std_lib, layouter, val_t, &[decoded_val])?[0].clone();
    let encoding_of_decoded = encode_incircuit(std_lib, layouter, &decoded)?;

    if encoding_of_decoded.len() != encoded.len() {
        return Err(plonk::Error::Synthesis(format!(
            "Cannot decode {} elements as {val_t:?}",
            encoded.len()
        )));
    }

    for (x, expected) in encoding_of_decoded.iter().zip(encoded) {
        constrain_eq_incircuit(
            std_lib,
            layouter,
            x,
            &CircuitValue::Native(expected.clone()),
        )?;
    }

    Ok(decoded)
}

/// Converts a native field element to a Jubjub scalar by reducing modulo
/// the Jubjub scalar field order if necessary.
pub fn native_to_jubjub_scalar(native: &Fr) -> JubjubFr {
    let mut bytes = [0u8; 64];
    bytes[..32].copy_from_slice(&native.0.to_bytes_le());
    JubjubFr::from_bytes_wide(&bytes)
}
