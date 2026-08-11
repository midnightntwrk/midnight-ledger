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
///   - `Bool`
///   - `Byte`
///   - `JubjubPoint`
///   - `Secp256k1Point`
///   - `Secp256k1Base`
///   - `Secp256k1Scalar`
///   - `Secp256r1Point`
///   - `Secp256r1Base`
///   - `Secp256r1Scalar`
///   - `Curve25519Point`
///   - `Curve25519Base`
///   - `Curve25519Scalar`
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
///   - `Bool`
///   - `Byte`
///   - `JubjubPoint`
///   - `Secp256k1Point`
///   - `Secp256k1Base`
///   - `Secp256k1Scalar`
///   - `Secp256r1Point`
///   - `Secp256r1Base`
///   - `Secp256r1Scalar`
///   - `Curve25519Point`
///   - `Curve25519Base`
///   - `Curve25519Scalar`
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
        (Bool(a), Bool(b)) => Ok(Bool(std_lib.select(layouter, bit, a, b)?)),
        (Byte(a), Byte(b)) => Ok(Byte(std_lib.select(layouter, bit, a, b)?)),
        (JubjubPoint(p), JubjubPoint(q)) => {
            Ok(JubjubPoint(std_lib.jubjub().select(layouter, bit, p, q)?))
        }

        (Secp256k1Point(p), Secp256k1Point(q)) => Ok(Secp256k1Point(
            std_lib.secp256k1().select(layouter, bit, p, q)?,
        )),
        (Secp256k1Base(s), Secp256k1Base(r)) => Ok(Secp256k1Base(
            (std_lib.secp256k1().base_field_chip()).select(layouter, bit, s, r)?,
        )),
        (Secp256k1Scalar(s), Secp256k1Scalar(r)) => Ok(Secp256k1Scalar(
            (std_lib.secp256k1().scalar_field_chip()).select(layouter, bit, s, r)?,
        )),

        (Secp256r1Point(p), Secp256r1Point(q)) => Ok(Secp256r1Point(std_lib.p256().select(layouter, bit, p, q)?)),
        (Secp256r1Base(s), Secp256r1Base(r)) => Ok(Secp256r1Base(
            (std_lib.p256().base_field_chip()).select(layouter, bit, s, r)?,
        )),
        (Secp256r1Scalar(s), Secp256r1Scalar(r)) => Ok(Secp256r1Scalar(
            (std_lib.p256().scalar_field_chip()).select(layouter, bit, s, r)?,
        )),

        (Curve25519Point(p), Curve25519Point(q)) => Ok(Curve25519Point(
            std_lib.curve25519().select(layouter, bit, p, q)?,
        )),
        (Curve25519Base(s), Curve25519Base(r)) => Ok(Curve25519Base(
            (std_lib.curve25519().base_field_chip()).select(layouter, bit, s, r)?,
        )),
        (Curve25519Scalar(s), Curve25519Scalar(r)) => Ok(Curve25519Scalar(
            (std_lib.curve25519().scalar_field_chip()).select(layouter, bit, s, r)?,
        )),

        _ => Err(plonk::Error::Synthesis(format!(
            "Unsupported cond_select: {:?} ? {:?}",
            a.get_type(),
            b.get_type()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_offcircuit_behavior() {
        use IrValue::*;

        // `bit == true` returns `a`, `bit == false` returns `b`.
        assert_eq!(
            select_offcircuit(true, &Bool(true), &Bool(false)).unwrap(),
            Bool(true)
        );
        assert_eq!(
            select_offcircuit(false, &Bool(true), &Bool(false)).unwrap(),
            Bool(false)
        );

        assert_eq!(
            select_offcircuit(true, &Byte(1), &Byte(2)).unwrap(),
            Byte(1)
        );
        assert_eq!(
            select_offcircuit(false, &Byte(1), &Byte(2)).unwrap(),
            Byte(2)
        );

        // Mismatched operand types are rejected.
        assert!(select_offcircuit(true, &Bool(true), &Native(1.into())).is_err());
        assert!(select_offcircuit(true, &Byte(1), &Bool(true)).is_err());
    }
}
