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

#[cfg(test)]
use std::ops::Add;

use midnight_circuits::instructions::{ArithInstructions, EccInstructions};
use midnight_proofs::{circuit::Layouter, plonk};
use midnight_zk_stdlib::ZkStdLib;

use crate::{
    ir_instructions::F,
    ir_types::{CircuitValue, IrValue},
};

/// Adds off-circuit the given inputs.
/// Addition is supported on:
///   - `Native`
///   - `JubjubPoint`
///   - `Secp256k1Point`
///   - `Secp256k1Base`
///   - `Secp256k1Scalar`
///
/// # Errors
///
/// This function results in an error if the input types are not supported.
pub fn add_offcircuit(x: &IrValue, y: &IrValue) -> Result<IrValue, anyhow::Error> {
    use IrValue::*;
    match (x, y) {
        (Native(a), Native(b)) => Ok(Native(*a + *b)),
        (JubjubPoint(p), JubjubPoint(q)) => Ok(JubjubPoint(p + q)),

        (Secp256k1Point(p), Secp256k1Point(q)) => Ok(Secp256k1Point(p + q)),
        (Secp256k1Base(s), Secp256k1Base(r)) => Ok(Secp256k1Base(s + r)),
        (Secp256k1Scalar(s), Secp256k1Scalar(r)) => Ok(Secp256k1Scalar(s + r)),

        _ => Err(anyhow::anyhow!(
            "Unsupported addition: {:?} + {:?}",
            x.get_type(),
            y.get_type()
        )),
    }
}

/// Adds in-circuit the given inputs.
/// Addition is supported on:
///   - `Native`
///   - `JubjubPoint`
///   - `Secp256k1Point`
///   - `Secp256k1Base`
///   - `Secp256k1Scalar`
///
/// # Errors
///
/// This function results in an error if the input types are not supported.
pub fn add_incircuit(
    std_lib: &ZkStdLib,
    layouter: &mut impl Layouter<F>,
    x: &CircuitValue,
    y: &CircuitValue,
) -> Result<CircuitValue, plonk::Error> {
    use CircuitValue::*;
    match (x, y) {
        (Native(a), Native(b)) => {
            let r = std_lib.add(layouter, a, b)?;
            Ok(Native(r))
        }
        (JubjubPoint(p), JubjubPoint(q)) => {
            let r = std_lib.jubjub().add(layouter, p, q)?;
            Ok(JubjubPoint(r))
        }

        (Secp256k1Point(p), Secp256k1Point(q)) => {
            let r = std_lib.secp256k1_curve().add(layouter, p, q)?;
            Ok(Secp256k1Point(r))
        }
        (Secp256k1Base(a), Secp256k1Base(b)) => {
            let r = (std_lib.secp256k1_curve().base_field_chip()).add(layouter, a, b)?;
            Ok(Secp256k1Base(r))
        }
        (Secp256k1Scalar(a), Secp256k1Scalar(b)) => {
            let r = (std_lib.secp256k1_curve().scalar_field_chip()).add(layouter, a, b)?;
            Ok(Secp256k1Scalar(r))
        }

        _ => Err(plonk::Error::Synthesis(format!(
            "Unsupported addition: {:?} + {:?}",
            x.get_type(),
            y.get_type()
        ))),
    }
}

#[cfg(test)]
impl Add for IrValue {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        add_offcircuit(&self, &rhs).unwrap()
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
    fn test_add() {
        use IrValue::*;

        let [x, y] = core::array::from_fn(|_| Fr(F::random(OsRng)));
        let [p, q] = core::array::from_fn(|_| JubjubSubgroup::random(OsRng));

        assert_eq!(Native(x) + Native(y), Native(x + y));
        assert_eq!(JubjubPoint(p) + JubjubPoint(q), JubjubPoint(p + q));

        // Negative test: adding incompatible types should fail
        let result = add_offcircuit(&Native(x), &JubjubPoint(p));
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "Unsupported addition: Native + JubjubPoint"
        );

        let [p, q] = core::array::from_fn(|_| secp256k1::Secp256k1::random(OsRng));
        let [x, y] = core::array::from_fn(|_| secp256k1::Fp::random(OsRng));
        let [r, s] = core::array::from_fn(|_| secp256k1::Fq::random(OsRng));
        assert_eq!(Secp256k1Point(p) + Secp256k1Point(q), Secp256k1Point(p + q));
        assert_eq!(Secp256k1Base(x) + Secp256k1Base(y), Secp256k1Base(x + y));
        assert_eq!(
            Secp256k1Scalar(r) + Secp256k1Scalar(s),
            Secp256k1Scalar(r + s)
        );
    }
}
