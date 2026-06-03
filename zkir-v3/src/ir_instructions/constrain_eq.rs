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

use midnight_circuits::instructions::AssertionInstructions;
use midnight_proofs::{circuit::Layouter, plonk};
use midnight_zk_stdlib::ZkStdLib;

use crate::{
    ir_instructions::F,
    ir_types::{CircuitValue, IrValue},
};

/// Constrains off-circuit the given inputs to be equal.
/// Equality constraint is supported on:
///   - `Native`
///   - `JubjubPoint`
///   - `Secp256k1Point`
///   - `Secp256k1Base`
///   - `Secp256k1Scalar`
///
/// # Errors
///
/// This function results in an error if the inputs are not equal or the types
/// are not supported.
pub fn constrain_eq_offcircuit(a: &IrValue, b: &IrValue) -> Result<(), anyhow::Error> {
    if a.get_type() != b.get_type() {
        return Err(anyhow::anyhow!(
            "Unsupported constrain_eq: {:?} == {:?}",
            a.get_type(),
            b.get_type()
        ));
    }

    if a != b {
        return Err(anyhow::anyhow!(
            "Equality constraint failed: {a:?} != {b:?}"
        ));
    }

    Ok(())
}

/// Constrains in-circuit the given inputs to be equal.
/// Equality constraint is supported on:
///   - `Native`
///   - `JubjubPoint`
///   - `Secp256k1Point`
///   - `Secp256k1Base`
///   - `Secp256k1Scalar`
///
/// # Errors
///
/// This function results in an error if the input types are not supported.
pub fn constrain_eq_incircuit(
    std_lib: &ZkStdLib,
    layouter: &mut impl Layouter<F>,
    a: &CircuitValue,
    b: &CircuitValue,
) -> Result<(), plonk::Error> {
    use CircuitValue::*;
    match (a, b) {
        (Native(x), Native(y)) => std_lib.assert_equal(layouter, x, y),
        (JubjubPoint(p), JubjubPoint(q)) => std_lib.jubjub().assert_equal(layouter, p, q),

        (Secp256k1Point(p), Secp256k1Point(q)) => {
            std_lib.secp256k1_curve().assert_equal(layouter, p, q)
        }
        (Secp256k1Base(s), Secp256k1Base(r)) => {
            (std_lib.secp256k1_curve().base_field_chip()).assert_equal(layouter, s, r)
        }
        (Secp256k1Scalar(s), Secp256k1Scalar(r)) => {
            (std_lib.secp256k1_curve().scalar_field_chip()).assert_equal(layouter, s, r)
        }

        _ => Err(plonk::Error::Synthesis(format!(
            "Unsupported constrain_eq: {:?} == {:?}",
            a.get_type(),
            b.get_type()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use group::Group;
    use group::ff::Field;
    use midnight_curves::{JubjubSubgroup, secp256k1};
    use rand_chacha::rand_core::OsRng;
    use transient_crypto::curve::Fr;

    use super::*;

    #[test]
    fn constrain_eq_offcircuit_behavior() {
        use IrValue::*;
        let x = Fr(F::random(OsRng));
        let p = JubjubSubgroup::random(OsRng);
        assert!(constrain_eq_offcircuit(&Native(x), &Native(x)).is_ok());
        assert!(constrain_eq_offcircuit(&JubjubPoint(p), &JubjubPoint(p)).is_ok());
        assert!(constrain_eq_offcircuit(&Native(x), &JubjubPoint(p)).is_err());

        let p = secp256k1::Secp256k1::random(OsRng);
        let s = secp256k1::Fp::random(OsRng);
        let r = secp256k1::Fq::random(OsRng);
        assert!(constrain_eq_offcircuit(&Secp256k1Point(p), &Secp256k1Point(p)).is_ok());
        assert!(constrain_eq_offcircuit(&Secp256k1Base(s), &Secp256k1Base(s)).is_ok());
        assert!(constrain_eq_offcircuit(&Secp256k1Scalar(r), &Secp256k1Scalar(r)).is_ok());
    }
}
