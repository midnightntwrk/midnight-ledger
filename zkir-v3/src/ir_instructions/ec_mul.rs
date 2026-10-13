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

use midnight_circuits::instructions::EccInstructions;
use midnight_proofs::{circuit::Layouter, plonk};
use midnight_zk_stdlib::ZkStdLib;

use crate::{
    ir_instructions::F,
    ir_types::{CircuitValue, IrValue},
};

/// Multiplication (off-circuit) of an elliptic curve point by a scalar.
/// Supported on:
///   - `JubjubPoint x JubjubScalar`
///   - `Secp256k1Point x Secp256k1Scalar`
///   - `Secp256r1Point x Secp256r1Scalar`
///   - `Curve25519Point x Curve25519Scalar`
///
/// # Errors
///
/// This function results in an error if the input types are not supported.
pub fn ec_mul_offcircuit(point: &IrValue, scalar: &IrValue) -> Result<IrValue, anyhow::Error> {
    use IrValue::*;
    match (point, scalar) {
        (JubjubPoint(p), JubjubScalar(s)) => Ok(JubjubPoint(p * s)),
        (Secp256k1Point(p), Secp256k1Scalar(s)) => Ok(Secp256k1Point(*p * s)),
        (Secp256r1Point(p), Secp256r1Scalar(s)) => Ok(Secp256r1Point(*p * s)),
        (Curve25519Point(p), Curve25519Scalar(s)) => Ok(Curve25519Point(*p * s)),
        _ => Err(anyhow::anyhow!(
            "Unsupported EC multiplication: {:?} x {:?}",
            point.get_type(),
            scalar.get_type()
        )),
    }
}

/// Multiplication (in-circuit) of an elliptic curve point by a scalar.
/// Supported on:
///   - `JubjubPoint x JubjubScalar`
///   - `Secp256k1Point x Secp256k1Scalar`
///   - `Secp256r1Point x Secp256r1Scalar`
///   - `Curve25519Point x Curve25519Scalar`
///
/// # Errors
///
/// This function results in an error if the input types are not supported.
pub fn ec_mul_incircuit(
    std_lib: &ZkStdLib,
    layouter: &mut impl Layouter<F>,
    point: &CircuitValue,
    scalar: &CircuitValue,
) -> Result<CircuitValue, plonk::Error> {
    use CircuitValue::*;
    match (point, scalar) {
        (JubjubPoint(p), JubjubScalar(s)) => {
            let r = std_lib.jubjub().mul(layouter, s, p)?;
            Ok(JubjubPoint(r))
        }
        (Secp256k1Point(p), Secp256k1Scalar(s)) => {
            let r = std_lib.secp256k1().msm(
                layouter,
                std::slice::from_ref(s),
                std::slice::from_ref(p),
            )?;
            Ok(Secp256k1Point(r))
        }
        (Secp256r1Point(p), Secp256r1Scalar(s)) => {
            let r = std_lib
                .p256()
                .msm(layouter, std::slice::from_ref(s), std::slice::from_ref(p))?;
            Ok(Secp256r1Point(r))
        }
        (Curve25519Point(p), Curve25519Scalar(s)) => {
            let r = std_lib.curve25519().msm(
                layouter,
                std::slice::from_ref(s),
                std::slice::from_ref(p),
            )?;
            Ok(Curve25519Point(r))
        }
        _ => Err(plonk::Error::Synthesis(format!(
            "Unsupported EC multiplication: {:?} x {:?}",
            point.get_type(),
            scalar.get_type()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use group::Group;
    use group::ff::Field;
    use midnight_curves::{Fr as JubjubFr, JubjubSubgroup, curve25519, k256, p256};
    use rand_chacha::rand_core::OsRng;

    use super::*;

    #[test]
    fn test_ec_mul() {
        use IrValue::*;

        let p = JubjubSubgroup::random(OsRng);
        let s = JubjubFr::random(OsRng);

        assert_eq!(
            ec_mul_offcircuit(&JubjubPoint(p), &JubjubScalar(s)).unwrap(),
            JubjubPoint(p * s)
        );

        let q = k256::K256::random(OsRng);
        let r = k256::Fq::random(OsRng);

        assert_eq!(
            ec_mul_offcircuit(&Secp256k1Point(q), &Secp256k1Scalar(r)).unwrap(),
            Secp256k1Point(q * r)
        );

        // Negative test: multiplying incompatible types should fail
        let result = ec_mul_offcircuit(&JubjubPoint(p), &Secp256k1Scalar(r));
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "Unsupported EC multiplication: JubjubPoint x Secp256k1Scalar"
        );

        let q = p256::P256::random(OsRng);
        let r = p256::Fq::random(OsRng);

        assert_eq!(
            ec_mul_offcircuit(&Secp256r1Point(q), &Secp256r1Scalar(r)).unwrap(),
            Secp256r1Point(q * r)
        );

        // Negative test: multiplying by a scalar of another curve should fail
        let result = ec_mul_offcircuit(&Secp256r1Point(q), &JubjubScalar(s));
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "Unsupported EC multiplication: Secp256r1Point x JubjubScalar"
        );

        let q = curve25519::Curve25519Subgroup::random(OsRng);
        let r = <curve25519::Scalar as Field>::random(OsRng);

        assert_eq!(
            ec_mul_offcircuit(&Curve25519Point(q), &Curve25519Scalar(r)).unwrap(),
            Curve25519Point(q * r)
        );

        // Negative test: multiplying by a scalar of another curve should fail
        let result = ec_mul_offcircuit(&Curve25519Point(q), &JubjubScalar(s));
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "Unsupported EC multiplication: Curve25519Point x JubjubScalar"
        );
    }
}
