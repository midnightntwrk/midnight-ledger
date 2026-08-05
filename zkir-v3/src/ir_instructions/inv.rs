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

use group::ff::Field;
use midnight_circuits::instructions::ArithInstructions;
use midnight_proofs::{circuit::Layouter, plonk};
use midnight_zk_stdlib::ZkStdLib;
use transient_crypto::curve::Fr;

use crate::{
    ir_instructions::F,
    ir_types::{CircuitValue, IrValue},
};

/// Inverts off-circuit the given input.
/// Inversion is supported on:
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
/// This function results in an error if the input type is not supported,
/// or if the given value is zero.
pub fn inv_offcircuit(x: &IrValue) -> Result<IrValue, anyhow::Error> {
    use IrValue::*;
    let zero_err = || anyhow::anyhow!("cannot invert zero of type {:?}", x.get_type());
    match x {
        Native(a) => Option::from(a.0.invert())
            .ok_or_else(zero_err)
            .map(Fr)
            .map(Native),

        Secp256k1Base(s) => Option::from(s.invert())
            .ok_or_else(zero_err)
            .map(Secp256k1Base),

        Secp256k1Scalar(s) => Option::from(s.invert())
            .ok_or_else(zero_err)
            .map(Secp256k1Scalar),

        Secp256r1Base(s) => Option::from(s.invert()).ok_or_else(zero_err).map(Secp256r1Base),

        Secp256r1Scalar(s) => Option::from(s.invert())
            .ok_or_else(zero_err)
            .map(Secp256r1Scalar),

        Curve25519Base(s) => Option::from(s.invert())
            .ok_or_else(zero_err)
            .map(Curve25519Base),

        // Nb. dalek's inherent `Scalar::invert` (which is not a `CtOption`)
        // would shadow `Field::invert` here, hence the qualified call.
        Curve25519Scalar(s) => Option::from(Field::invert(s))
            .ok_or_else(zero_err)
            .map(Curve25519Scalar),

        _ => Err(anyhow::anyhow!(
            "Unsupported inversion of {:?}",
            x.get_type(),
        )),
    }
}

/// Inverts in-circuit the given input.
/// Inversion is supported on:
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
/// This function results in an error if the input type is not supported,
/// or if the given value is zero.
pub fn inv_incircuit(
    std_lib: &ZkStdLib,
    layouter: &mut impl Layouter<F>,
    x: &CircuitValue,
) -> Result<CircuitValue, plonk::Error> {
    use CircuitValue::*;
    match x {
        Native(a) => {
            let r = std_lib.inv(layouter, a)?;
            Ok(Native(r))
        }
        Secp256k1Base(a) => {
            let r = (std_lib.secp256k1().base_field_chip()).inv(layouter, a)?;
            Ok(Secp256k1Base(r))
        }
        Secp256k1Scalar(a) => {
            let r = (std_lib.secp256k1().scalar_field_chip()).inv(layouter, a)?;
            Ok(Secp256k1Scalar(r))
        }

        Secp256r1Base(a) => {
            let r = (std_lib.p256().base_field_chip()).inv(layouter, a)?;
            Ok(Secp256r1Base(r))
        }
        Secp256r1Scalar(a) => {
            let r = (std_lib.p256().scalar_field_chip()).inv(layouter, a)?;
            Ok(Secp256r1Scalar(r))
        }

        Curve25519Base(a) => {
            let r = (std_lib.curve25519().base_field_chip()).inv(layouter, a)?;
            Ok(Curve25519Base(r))
        }
        Curve25519Scalar(a) => {
            let r = (std_lib.curve25519().scalar_field_chip()).inv(layouter, a)?;
            Ok(Curve25519Scalar(r))
        }

        _ => Err(plonk::Error::Synthesis(format!(
            "Unsupported inversion of {:?}",
            x.get_type(),
        ))),
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
    fn test_inv() {
        use IrValue::*;

        let x = Fr(F::random(OsRng));
        assert_eq!(
            inv_offcircuit(&Native(x)).unwrap(),
            Native(Fr(x.0.invert().unwrap()))
        );

        let x = k256::Fp::random(OsRng);
        assert_eq!(
            inv_offcircuit(&Secp256k1Base(x)).unwrap(),
            Secp256k1Base(x.invert().unwrap())
        );

        let x = k256::Fq::random(OsRng);
        assert_eq!(
            inv_offcircuit(&Secp256k1Scalar(x)).unwrap(),
            Secp256k1Scalar(x.invert().unwrap())
        );

        let x = p256::Fp::random(OsRng);
        assert_eq!(
            inv_offcircuit(&Secp256r1Base(x)).unwrap(),
            Secp256r1Base(x.invert().unwrap())
        );

        let x = p256::Fq::random(OsRng);
        assert_eq!(
            inv_offcircuit(&Secp256r1Scalar(x)).unwrap(),
            Secp256r1Scalar(x.invert().unwrap())
        );

        let x = curve25519::Fp::random(OsRng);
        assert_eq!(
            inv_offcircuit(&Curve25519Base(x)).unwrap(),
            Curve25519Base(x.invert().unwrap())
        );

        let x = <curve25519::Scalar as Field>::random(OsRng);
        assert_eq!(
            inv_offcircuit(&Curve25519Scalar(x)).unwrap(),
            Curve25519Scalar(Field::invert(&x).unwrap())
        );
    }
}
