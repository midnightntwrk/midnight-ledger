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
///   - `Bool`
///   - `Byte`
///   - `Bytes(n)`
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
///   - `Bool`
///   - `Byte`
///   - `Bytes(n)`
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
pub fn constrain_eq_incircuit(
    std_lib: &ZkStdLib,
    layouter: &mut impl Layouter<F>,
    a: &CircuitValue,
    b: &CircuitValue,
) -> Result<(), plonk::Error> {
    use CircuitValue::*;
    match (a, b) {
        (Native(x), Native(y)) => std_lib.assert_equal(layouter, x, y),

        (Bool(a), Bool(b)) => std_lib.assert_equal(layouter, a, b),

        (Byte(a), Byte(b)) => std_lib.assert_equal(layouter, a, b),

        (Bytes(xs), Bytes(ys)) if xs.len() == ys.len() => xs
            .iter()
            .zip(ys.iter())
            .try_for_each(|(x, y)| std_lib.assert_equal(layouter, x, y)),

        (JubjubPoint(p), JubjubPoint(q)) => std_lib.jubjub().assert_equal(layouter, p, q),

        (Secp256k1Point(p), Secp256k1Point(q)) => std_lib.secp256k1().assert_equal(layouter, p, q),
        (Secp256k1Base(s), Secp256k1Base(r)) => {
            (std_lib.secp256k1().base_field_chip()).assert_equal(layouter, s, r)
        }
        (Secp256k1Scalar(s), Secp256k1Scalar(r)) => {
            (std_lib.secp256k1().scalar_field_chip()).assert_equal(layouter, s, r)
        }

        (Secp256r1Point(p), Secp256r1Point(q)) => std_lib.p256().assert_equal(layouter, p, q),
        (Secp256r1Base(s), Secp256r1Base(r)) => {
            (std_lib.p256().base_field_chip()).assert_equal(layouter, s, r)
        }
        (Secp256r1Scalar(s), Secp256r1Scalar(r)) => {
            (std_lib.p256().scalar_field_chip()).assert_equal(layouter, s, r)
        }

        (Curve25519Point(p), Curve25519Point(q)) => {
            std_lib.curve25519().assert_equal(layouter, p, q)
        }
        (Curve25519Base(s), Curve25519Base(r)) => {
            (std_lib.curve25519().base_field_chip()).assert_equal(layouter, s, r)
        }
        (Curve25519Scalar(s), Curve25519Scalar(r)) => {
            (std_lib.curve25519().scalar_field_chip()).assert_equal(layouter, s, r)
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
    use midnight_curves::{JubjubSubgroup, curve25519, k256, p256};
    use rand::Rng;
    use rand_chacha::rand_core::OsRng;
    use transient_crypto::curve::Fr;

    use super::*;

    #[test]
    fn constrain_eq_offcircuit_behavior() {
        use IrValue::*;
        let x = Fr(F::random(OsRng));
        assert!(constrain_eq_offcircuit(&Native(x), &Native(x)).is_ok());

        assert!(constrain_eq_offcircuit(&Bool(true), &Bool(true)).is_ok());
        assert!(constrain_eq_offcircuit(&Bool(false), &Bool(false)).is_ok());
        assert!(constrain_eq_offcircuit(&Bool(true), &Bool(false)).is_err());
        assert!(constrain_eq_offcircuit(&Native(x), &Bool(true)).is_err());

        assert!(constrain_eq_offcircuit(&Byte(7), &Byte(7)).is_ok());
        assert!(constrain_eq_offcircuit(&Byte(7), &Byte(8)).is_err());
        assert!(constrain_eq_offcircuit(&Native(x), &Byte(7)).is_err());

        let bytes: Vec<u8> = (0..32).map(|_| rand::thread_rng().r#gen()).collect();
        assert!(constrain_eq_offcircuit(&Bytes(bytes.clone()), &Bytes(bytes.clone())).is_ok());
        assert!(constrain_eq_offcircuit(&Bytes(bytes), &Bytes(vec![0u8; 32])).is_err());

        let p = JubjubSubgroup::random(OsRng);
        assert!(constrain_eq_offcircuit(&JubjubPoint(p), &JubjubPoint(p)).is_ok());
        assert!(constrain_eq_offcircuit(&Native(x), &JubjubPoint(p)).is_err());

        let p = k256::K256::random(OsRng);
        let s = k256::Fp::random(OsRng);
        let r = k256::Fq::random(OsRng);
        assert!(constrain_eq_offcircuit(&Secp256k1Point(p), &Secp256k1Point(p)).is_ok());
        assert!(constrain_eq_offcircuit(&Secp256k1Base(s), &Secp256k1Base(s)).is_ok());
        assert!(constrain_eq_offcircuit(&Secp256k1Scalar(r), &Secp256k1Scalar(r)).is_ok());

        let p = p256::P256::random(OsRng);
        let s = p256::Fp::random(OsRng);
        let r = p256::Fq::random(OsRng);
        assert!(constrain_eq_offcircuit(&Secp256r1Point(p), &Secp256r1Point(p)).is_ok());
        assert!(constrain_eq_offcircuit(&Secp256r1Base(s), &Secp256r1Base(s)).is_ok());
        assert!(constrain_eq_offcircuit(&Secp256r1Scalar(r), &Secp256r1Scalar(r)).is_ok());
        assert!(constrain_eq_offcircuit(&Secp256r1Point(p), &Secp256r1Point(-p)).is_err());
        assert!(constrain_eq_offcircuit(&Secp256r1Base(s), &Secp256r1Scalar(r)).is_err());

        let p = curve25519::Curve25519Subgroup::random(OsRng);
        let s = curve25519::Fp::random(OsRng);
        let r = <curve25519::Scalar as Field>::random(OsRng);
        assert!(constrain_eq_offcircuit(&Curve25519Point(p), &Curve25519Point(p)).is_ok());
        assert!(constrain_eq_offcircuit(&Curve25519Base(s), &Curve25519Base(s)).is_ok());
        assert!(constrain_eq_offcircuit(&Curve25519Scalar(r), &Curve25519Scalar(r)).is_ok());
        assert!(constrain_eq_offcircuit(&Curve25519Point(p), &Curve25519Point(-p)).is_err());
        assert!(constrain_eq_offcircuit(&Curve25519Base(s), &Curve25519Scalar(r)).is_err());
    }
}
