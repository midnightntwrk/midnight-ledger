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
use std::ops::Mul;

use midnight_circuits::instructions::ArithInstructions;
use midnight_proofs::{circuit::Layouter, plonk};
use midnight_zk_stdlib::ZkStdLib;

use crate::{
    ir_instructions::F,
    ir_types::{CircuitValue, IrValue},
};

/// Multiplies off-circuit the given inputs.
/// Multiplication is supported on:
///   - `Native`
///   - `Secp256k1Base`
///   - `Secp256k1Scalar`
///   - `Secp256r1Base`
///   - `Secp256r1Scalar`
///   - `Curve25519Base`
///   - `Curve25519Scalar`
///
/// # Errors
///
/// This function results in an error if the input types are not supported.
pub fn mul_offcircuit(x: &IrValue, y: &IrValue) -> Result<IrValue, anyhow::Error> {
    use IrValue::*;
    match (x, y) {
        (Native(a), Native(b)) => Ok(Native(*a * *b)),
        (Secp256k1Base(s), Secp256k1Base(r)) => Ok(Secp256k1Base(*s * *r)),
        (Secp256k1Scalar(s), Secp256k1Scalar(r)) => Ok(Secp256k1Scalar(*s * *r)),

        (Secp256r1Base(s), Secp256r1Base(r)) => Ok(Secp256r1Base(*s * *r)),
        (Secp256r1Scalar(s), Secp256r1Scalar(r)) => Ok(Secp256r1Scalar(*s * *r)),

        (Curve25519Base(s), Curve25519Base(r)) => Ok(Curve25519Base(*s * *r)),
        (Curve25519Scalar(s), Curve25519Scalar(r)) => Ok(Curve25519Scalar(*s * *r)),

        _ => Err(anyhow::anyhow!(
            "Unsupported multiplication: {:?} x {:?}",
            x.get_type(),
            y.get_type()
        )),
    }
}

/// Multiplies in-circuit the given inputs.
/// Multiplication is supported on:
///   - `Native`
///   - `Secp256k1Base`
///   - `Secp256k1Scalar`
///   - `Secp256r1Base`
///   - `Secp256r1Scalar`
///   - `Curve25519Base`
///   - `Curve25519Scalar`
///
/// # Errors
///
/// This function results in an error if the input types are not supported.
pub fn mul_incircuit(
    std_lib: &ZkStdLib,
    layouter: &mut impl Layouter<F>,
    x: &CircuitValue,
    y: &CircuitValue,
) -> Result<CircuitValue, plonk::Error> {
    use CircuitValue::*;
    match (x, y) {
        (Native(a), Native(b)) => {
            let r = std_lib.mul(layouter, a, b, None)?;
            Ok(Native(r))
        }
        (Secp256k1Base(a), Secp256k1Base(b)) => {
            let r = (std_lib.secp256k1().base_field_chip()).mul(layouter, a, b, None)?;
            Ok(Secp256k1Base(r))
        }
        (Secp256k1Scalar(a), Secp256k1Scalar(b)) => {
            let r = (std_lib.secp256k1().scalar_field_chip()).mul(layouter, a, b, None)?;
            Ok(Secp256k1Scalar(r))
        }

        (Secp256r1Base(a), Secp256r1Base(b)) => {
            let r = (std_lib.p256().base_field_chip()).mul(layouter, a, b, None)?;
            Ok(Secp256r1Base(r))
        }
        (Secp256r1Scalar(a), Secp256r1Scalar(b)) => {
            let r = (std_lib.p256().scalar_field_chip()).mul(layouter, a, b, None)?;
            Ok(Secp256r1Scalar(r))
        }

        (Curve25519Base(a), Curve25519Base(b)) => {
            let r = (std_lib.curve25519().base_field_chip()).mul(layouter, a, b, None)?;
            Ok(Curve25519Base(r))
        }
        (Curve25519Scalar(a), Curve25519Scalar(b)) => {
            let r = (std_lib.curve25519().scalar_field_chip()).mul(layouter, a, b, None)?;
            Ok(Curve25519Scalar(r))
        }

        _ => Err(plonk::Error::Synthesis(format!(
            "Unsupported multiplication: {:?} x {:?}",
            x.get_type(),
            y.get_type()
        ))),
    }
}

#[cfg(test)]
impl Mul for IrValue {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self {
        mul_offcircuit(&self, &rhs).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use group::ff::Field;
    use midnight_curves::{curve25519, k256, p256};
    use rand_chacha::rand_core::OsRng;
    use transient_crypto::curve::Fr;

    use super::*;

    #[test]
    fn test_mul() {
        use IrValue::*;

        let [x, y] = core::array::from_fn(|_| Fr(F::random(OsRng)));

        assert_eq!(Native(x) * Native(y), Native(x * y));

        let [x, y] = core::array::from_fn(|_| k256::Fp::random(OsRng));
        let [r, s] = core::array::from_fn(|_| k256::Fq::random(OsRng));

        assert_eq!(Secp256k1Base(x) * Secp256k1Base(y), Secp256k1Base(x * y));
        assert_eq!(
            Secp256k1Scalar(r) * Secp256k1Scalar(s),
            Secp256k1Scalar(r * s)
        );

        // Negative test: multiplying incompatible types should fail
        let result = mul_offcircuit(&Secp256k1Base(x), &Secp256k1Scalar(r));
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "Unsupported multiplication: Secp256k1Base x Secp256k1Scalar"
        );

        let [x, y] = core::array::from_fn(|_| p256::Fp::random(OsRng));
        let [r, s] = core::array::from_fn(|_| p256::Fq::random(OsRng));

        assert_eq!(Secp256r1Base(x) * Secp256r1Base(y), Secp256r1Base(x * y));
        assert_eq!(Secp256r1Scalar(r) * Secp256r1Scalar(s), Secp256r1Scalar(r * s));

        // Negative test: multiplying incompatible types should fail
        let result = mul_offcircuit(&Secp256r1Base(x), &Secp256r1Scalar(r));
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "Unsupported multiplication: Secp256r1Base x Secp256r1Scalar"
        );

        let [x, y] = core::array::from_fn(|_| curve25519::Fp::random(OsRng));
        let [r, s] = core::array::from_fn(|_| <curve25519::Scalar as Field>::random(OsRng));

        assert_eq!(Curve25519Base(x) * Curve25519Base(y), Curve25519Base(x * y));
        assert_eq!(
            Curve25519Scalar(r) * Curve25519Scalar(s),
            Curve25519Scalar(r * s)
        );
    }
}
