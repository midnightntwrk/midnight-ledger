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

use group::ff::{Field, PrimeField};
use midnight_circuits::{
    instructions::{ArithInstructions, PublicInputInstructions, RangeCheckInstructions},
    types::{AssignedNative, AssignedNativePoint, Instantiable},
};
use midnight_curves::JubjubExtended;
use midnight_proofs::{circuit::Layouter, plonk::Error};
use midnight_zk_stdlib::ZkStdLib;
use num_bigint::BigUint;
use transient_crypto::curve::Fr;

use crate::{
    ir_instructions::F,
    ir_types::{CircuitValue, IrValue},
};

/// Encodes the given off-circuit value as a vector of IrValue::Native.
pub fn encode_offcircuit(value: &IrValue) -> Vec<IrValue> {
    let encoded = match value {
        IrValue::Native(x) => AssignedNative::<F>::as_public_input(&x.0),
        IrValue::JubjubPoint(p) => AssignedNativePoint::<JubjubExtended>::as_public_input(p),
        IrValue::JubjubScalar(s) => {
            // TODO: We do not use [AssignedScalarOfNativeCurve::as_public_input]
            // since that function is not compatible with what we want to do here
            // (1 JubjubScalar -> 1 Native).
            //
            // In future versions of `midnight-zk` we will adopt this very encoding,
            // but for now we need to do it manually here.
            debug_assert_eq!(F::NUM_BITS, 255);
            vec![F::from_bytes_le(&s.to_bytes()).unwrap()]
        }
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
        CircuitValue::JubjubPoint(p) => std_lib.jubjub().as_public_input(layouter, p),
        CircuitValue::JubjubScalar(s) => {
            // TODO: We do not simply black-box call [std_lib.jubjub().as_public_input]
            // since that function is not compatible with what we want to do here
            // (1 JubjubScalar -> 1 Native).
            //
            // In future versions of `midnight-zk` we will adopt this very encoding,
            // but for now we need to adapt it manually here.
            debug_assert_eq!(F::NUM_BITS, 255);
            match std_lib.jubjub().as_public_input(layouter, s)?.as_slice() {
                [x] => Ok(vec![x.clone()]),
                [x1, x2] => {
                    let one = BigUint::from(1u32);
                    std_lib.assert_lower_than_fixed(layouter, x1, &(&one << 248))?;
                    std_lib.assert_lower_than_fixed(layouter, x2, &(&one << 4))?;
                    Ok(vec![std_lib.linear_combination(
                        layouter,
                        &[(F::ONE, x1.clone()), (F::from(2).pow([248u64]), x2.clone())],
                        F::ZERO,
                    )?])
                }
                _ =>
                // This case is unreachable since the only entry point for assigned
                // Jubjub scalars that are longer than the canonical 252 bits is
                // `std_lib.jubjub().scalar_from_le_bytes`, but in ZKIRv3 this is only
                // called during `decode_incircuit` and always from the 32 bytes of an
                // `AssignedNative`. Such assigned Jubjub scalar would have fallen in
                // the previous match case of 2 limbs.
                {
                    Err(Error::Synthesis(
                        "Unexpected number of chunks from as_public_input".into(),
                    ))
                }
            }
        }
    }?;
    Ok(encoded.into_iter().map(CircuitValue::Native).collect())
}
