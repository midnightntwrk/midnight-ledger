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

use midnight_circuits::instructions::EccInstructions;
use midnight_circuits::{ecc::curves::CircuitCurve, types::AssignedNative};
use midnight_curves::JubjubExtended;
use midnight_proofs::{circuit::Layouter, plonk};
use midnight_zk_stdlib::ZkStdLib;
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
    }
}
