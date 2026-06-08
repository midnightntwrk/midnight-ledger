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

use midnight_circuits::instructions::ControlFlowInstructions;
use midnight_circuits::types::AssignedBit;
use midnight_proofs::{circuit::Layouter, plonk};
use midnight_zk_stdlib::ZkStdLib;

use crate::{
    ir_instructions::F,
    ir_types::{CircuitValue, IrValue},
};

/// Conditionally selects off-circuit between two values.
/// If `bit` is true, returns `a`; otherwise returns `b`.
///
/// Supported on:
///   - `Native`
///   - `JubjubPoint`
///
/// # Errors
///
/// Returns an error if `a` and `b` do not have the same type.
pub fn select_offcircuit(bit: bool, a: &IrValue, b: &IrValue) -> Result<IrValue, anyhow::Error> {
    if a.get_type() != b.get_type() {
        return Err(anyhow::anyhow!(
            "Unsupported cond_select: {:?} ? {:?}",
            a.get_type(),
            b.get_type()
        ));
    }
    Ok(if bit { a.clone() } else { b.clone() })
}

/// Conditionally selects in-circuit between two values.
/// If `bit` is true, returns `a`; otherwise returns `b`.
///
/// Supported on:
///   - `Native`
///   - `JubjubPoint`
///
/// # Errors
///
/// This function results in an error if the input types are not supported.
pub fn select_incircuit(
    std_lib: &ZkStdLib,
    layouter: &mut impl Layouter<F>,
    bit: &AssignedBit<F>,
    a: &CircuitValue,
    b: &CircuitValue,
) -> Result<CircuitValue, plonk::Error> {
    use CircuitValue::*;
    match (a, b) {
        (Native(x), Native(y)) => Ok(Native(std_lib.select(layouter, bit, x, y)?)),
        (JubjubPoint(p), JubjubPoint(q)) => {
            Ok(JubjubPoint(std_lib.jubjub().select(layouter, bit, p, q)?))
        }
        _ => Err(plonk::Error::Synthesis(format!(
            "Unsupported cond_select: {:?} ? {:?}",
            a.get_type(),
            b.get_type()
        ))),
    }
}
