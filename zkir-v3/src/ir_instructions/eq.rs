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

use midnight_circuits::instructions::{
    ArithInstructions, AssertionInstructions, ConversionInstructions, EccInstructions,
    EqualityInstructions,
};
use midnight_proofs::{circuit::Layouter, plonk};
use midnight_zk_stdlib::ZkStdLib;

use crate::{
    ir_instructions::F,
    ir_types::{CircuitValue, IrValue},
};

/// Tests off-circuit whether the given inputs are equal.
/// Equality testing is supported on:
///   - `Native`
///   - `JubjubPoint`
///
/// # Errors
///
/// This function results in an error if the input types are not supported.
pub fn test_eq_offcircuit(a: &IrValue, b: &IrValue) -> Result<IrValue, anyhow::Error> {
    use IrValue::*;
    match (a, b) {
        (Native(x), Native(y)) => Ok(IrValue::Native((x == y).into())),
        (JubjubPoint(p), JubjubPoint(q)) => Ok(IrValue::Native((p == q).into())),
        _ => Err(anyhow::anyhow!(
            "Unsupported test_eq: {:?} == {:?}",
            a.get_type(),
            b.get_type()
        )),
    }
}

/// Tests in-circuit whether the given inputs are equal.
/// Equality testing is supported on:
///   - `Native`
///   - `JubjubPoint`
///
/// # Errors
///
/// This function results in an error if the input types are not supported.
pub fn test_eq_incircuit(
    std_lib: &ZkStdLib,
    layouter: &mut impl Layouter<F>,
    a: &CircuitValue,
    b: &CircuitValue,
) -> Result<CircuitValue, plonk::Error> {
    use CircuitValue::*;
    match (a, b) {
        (Native(x), Native(y)) => {
            let bit = std_lib.is_equal(layouter, x, y)?;
            Ok(Native(std_lib.convert(layouter, &bit)?))
        }
        (JubjubPoint(p), JubjubPoint(q)) => {
            let jub = std_lib.jubjub();
            let bit_x =
                std_lib.is_equal(layouter, &jub.x_coordinate(p), &jub.x_coordinate(q))?;
            let bit_y =
                std_lib.is_equal(layouter, &jub.y_coordinate(p), &jub.y_coordinate(q))?;
            let nx = std_lib.convert(layouter, &bit_x)?;
            let ny = std_lib.convert(layouter, &bit_y)?;
            Ok(Native(std_lib.mul(layouter, &nx, &ny, None)?))
        }
        _ => Err(plonk::Error::Synthesis(format!(
            "Unsupported test_eq: {:?} == {:?}",
            a.get_type(),
            b.get_type()
        ))),
    }
}

/// Constrains off-circuit the given inputs to be equal.
/// Equality constraint is supported on:
///   - `Native`
///   - `JubjubPoint`
///
/// # Errors
///
/// This function results in an error if the inputs are not equal or the types
/// are not supported.
pub fn constrain_eq_offcircuit(a: &IrValue, b: &IrValue) -> Result<(), anyhow::Error> {
    use IrValue::*;
    match (a, b) {
        (Native(x), Native(y)) if x == y => Ok(()),
        (JubjubPoint(p), JubjubPoint(q)) if p == q => Ok(()),
        _ => Err(anyhow::anyhow!(
            "Equality constraint failed: {:?} != {:?}",
            a,
            b
        )),
    }
}

/// Constrains in-circuit the given inputs to be equal.
/// Equality constraint is supported on:
///   - `Native`
///   - `JubjubPoint`
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
        (JubjubPoint(p), JubjubPoint(q)) => {
            let jub = std_lib.jubjub();
            std_lib.assert_equal(
                layouter,
                &jub.x_coordinate(p),
                &jub.x_coordinate(q),
            )?;
            std_lib.assert_equal(
                layouter,
                &jub.y_coordinate(p),
                &jub.y_coordinate(q),
            )
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
    use midnight_curves::JubjubSubgroup;
    use rand_chacha::rand_core::OsRng;
    use transient_crypto::curve::Fr;

    use super::*;

    #[test]
    fn test_test_eq() {
        use IrValue::*;

        let [x, y] = core::array::from_fn(|_| Fr(F::random(OsRng)));
        let [p, q] = core::array::from_fn(|_| JubjubSubgroup::random(OsRng));

        assert_eq!(test_eq_offcircuit(&Native(x), &Native(x)).unwrap(), Native(true.into()));
        assert_eq!(test_eq_offcircuit(&Native(x), &Native(y)).unwrap(), Native(false.into()));
        assert_eq!(
            test_eq_offcircuit(&JubjubPoint(p), &JubjubPoint(p)).unwrap(),
            Native(true.into())
        );
        assert_eq!(
            test_eq_offcircuit(&JubjubPoint(p), &JubjubPoint(q)).unwrap(),
            Native(false.into())
        );

        // Negative test: comparing incompatible types should fail
        assert!(test_eq_offcircuit(&Native(x), &JubjubPoint(p)).is_err());
    }

    #[test]
    fn test_constrain_eq() {
        use IrValue::*;

        let x = Fr(F::random(OsRng));
        let y = Fr(F::random(OsRng));
        let p = JubjubSubgroup::random(OsRng);
        let q = JubjubSubgroup::random(OsRng);

        assert!(constrain_eq_offcircuit(&Native(x), &Native(x)).is_ok());
        assert!(constrain_eq_offcircuit(&Native(x), &Native(y)).is_err());
        assert!(constrain_eq_offcircuit(&JubjubPoint(p), &JubjubPoint(p)).is_ok());
        assert!(constrain_eq_offcircuit(&JubjubPoint(p), &JubjubPoint(q)).is_err());

        // Negative test: comparing incompatible types should fail
        assert!(constrain_eq_offcircuit(&Native(x), &JubjubPoint(p)).is_err());
    }
}
