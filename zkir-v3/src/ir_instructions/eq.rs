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

use midnight_circuits::instructions::EqualityInstructions;
use midnight_circuits::types::AssignedBit;
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
pub fn test_eq_offcircuit(a: &IrValue, b: &IrValue) -> Result<bool, anyhow::Error> {
    use IrValue::*;
    match (a, b) {
        (Native(x), Native(y)) => Ok(x == y),
        (JubjubPoint(p), JubjubPoint(q)) => Ok(p == q),
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
) -> Result<AssignedBit<F>, plonk::Error> {
    use CircuitValue::*;
    match (a, b) {
        (Native(x), Native(y)) => std_lib.is_equal(layouter, x, y),
        (JubjubPoint(p), JubjubPoint(q)) => std_lib.jubjub().is_equal(layouter, p, q),
        _ => Err(plonk::Error::Synthesis(format!(
            "Unsupported test_eq: {:?} == {:?}",
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
    fn test_eq_offcircuit_behavior() {
        use IrValue::*;
        let x = Fr(F::random(OsRng));
        let p = JubjubSubgroup::random(OsRng);
        assert!(test_eq_offcircuit(&Native(x), &Native(x)).unwrap());
        assert!(test_eq_offcircuit(&JubjubPoint(p), &JubjubPoint(p)).unwrap());
        assert!(test_eq_offcircuit(&Native(x), &JubjubPoint(p)).is_err());
    }
}
