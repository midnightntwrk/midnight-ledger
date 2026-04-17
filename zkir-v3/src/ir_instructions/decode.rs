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

use midnight_circuits::instructions::{DecompositionInstructions, EccInstructions};
use midnight_circuits::types::AssignedScalarOfNativeCurve;
use midnight_circuits::{ecc::curves::CircuitCurve, types::AssignedNative};
use midnight_curves::{Fr as JubjubFr, JubjubExtended};
use midnight_proofs::{circuit::Layouter, plonk};
use midnight_zk_stdlib::ZkStdLib;
use num_bigint::BigUint;
use num_traits::Num;
use transient_crypto::curve::Fr;

use crate::{
    ir_instructions::F,
    ir_types::{CircuitValue, IrType, IrValue},
};

/// Decodes the given Fr values as an IrValue value of the given type.
///
/// # Errors
///
/// This function returns an error if the provided raw values cannot be
/// decoded as the given type.
pub fn decode_offcircuit(encoded: &[Fr], val_t: &IrType) -> Result<IrValue, anyhow::Error> {
    match val_t {
        IrType::Native => match encoded {
            [x] => Ok(IrValue::Native(*x)),
            _ => Err(anyhow::Error::msg(
                "Expected exactly one value for Native decoding",
            )),
        },
        IrType::JubjubPoint => match encoded {
            [x, y] => {
                let p = JubjubExtended::from_xy(x.0, y.0).ok_or_else(|| {
                    anyhow::Error::msg("Failed to decode Jubjub point from coordinates")
                })?;
                Ok(IrValue::JubjubPoint(p.into_subgroup()))
            }
            _ => Err(anyhow::Error::msg(
                "Expected exactly two values for JubjubPoint decoding",
            )),
        },

        IrType::JubjubScalar => match encoded {
            [x] => Ok(IrValue::JubjubScalar(native_to_jubjub_scalar(x))),
            _ => Err(anyhow::Error::msg(
                "Expected exactly one value for JubjubScalar decoding",
            )),
        },
    }
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
    match val_t {
        IrType::Native => match encoded {
            [x] => Ok(CircuitValue::Native(x.clone())),
            _ => Err(plonk::Error::Synthesis(
                "Expected exactly one value for Native decoding".into(),
            )),
        },
        IrType::JubjubPoint => match encoded {
            [x, y] => {
                let p = std_lib.jubjub().point_from_coordinates(layouter, x, y)?;
                Ok(CircuitValue::JubjubPoint(p))
            }
            _ => Err(plonk::Error::Synthesis(
                "Expected exactly two values for JubjubPoint decoding".into(),
            )),
        },
        IrType::JubjubScalar => match encoded {
            [x] => {
                // Until we can use a further release of midnight-zk (currently v1.0.0),
                // we must make sure that all ZKIR assigned Jubjub scalars have an internal
                // representation of at most 252 bits (so that they are encoded into a
                // single native field element).
                // To this end, we manually reduce the received encoded value modulo
                // the Jubjub order.
                let jubjub_order = {
                    let p_str = "e7db4ea6533afa906673b0101343b00a6682093ccc81082d0970e5ed6f72cb7";
                    let p = BigUint::from_str_radix(p_str, 16).unwrap();
                    std_lib.biguint().assign_fixed_biguint(layouter, p)?
                };
                let r = {
                    let x_bytes = std_lib.assigned_to_le_bytes(layouter, x, None)?;
                    let x_big = std_lib.biguint().from_le_bytes(layouter, &x_bytes)?;
                    let (_q, r) = std_lib.biguint().div_rem(layouter, &x_big, &jubjub_order)?;
                    r
                };
                // We will drop the most significant bits, so we make sure they are 0.
                let r_bits = std_lib.biguint().to_le_bits(layouter, &r)?;
                for b in r_bits[252..].iter() {
                    std_lib.assert_false(layouter, b)?;
                }
                // SAFETY: AssignedScalarOfNativeCurve<C> is a newtype over
                // Vec<AssignedBit<C::Base>>, so the transmute is sound.
                // TODO: We are NOT proud of this, revisit when the API allows it.
                let s: AssignedScalarOfNativeCurve<JubjubExtended> =
                    unsafe { std::mem::transmute(r_bits[..252].to_vec()) };

                Ok(CircuitValue::JubjubScalar(s))
            }
            _ => Err(plonk::Error::Synthesis(
                "Expected exactly one value for JubjubScalar decoding".into(),
            )),
        },
    }
}

/// Converts a native field element to a Jubjub scalar by reducing modulo
/// the Jubjub scalar field order if necessary.
pub fn native_to_jubjub_scalar(native: &Fr) -> JubjubFr {
    let mut bytes = [0u8; 64];
    bytes[..32].copy_from_slice(&native.0.to_bytes_le());
    JubjubFr::from_bytes_wide(&bytes)
}
