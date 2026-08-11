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
use std::ops::Neg;

use midnight_circuits::instructions::{
    ArithInstructions, BinaryInstructions, EccInstructions as _,
};
use midnight_proofs::{circuit::Layouter, plonk};
use midnight_zk_stdlib::ZkStdLib;

use crate::{
    ir_instructions::F,
    ir_types::{CircuitValue, IrValue},
};

/// Negates off-circuit the given input.
/// Negation is supported on:
///   - `Native`
///   - `Bool`
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
/// This function results in an error if the input type is not supported.
pub fn neg_offcircuit(x: &IrValue) -> Result<IrValue, anyhow::Error> {
    use IrValue::*;
    match x {
        Native(a) => Ok(Native(-*a)),
        Bool(a) => Ok(Bool(!a)),
        JubjubPoint(p) => Ok(JubjubPoint(-p)),

        Secp256k1Point(p) => Ok(Secp256k1Point(-p)),
        Secp256k1Base(s) => Ok(Secp256k1Base(-s)),
        Secp256k1Scalar(s) => Ok(Secp256k1Scalar(-s)),

        Secp256r1Point(p) => Ok(Secp256r1Point(-p)),
        Secp256r1Base(s) => Ok(Secp256r1Base(-*s)),
        Secp256r1Scalar(s) => Ok(Secp256r1Scalar(-*s)),

        Curve25519Point(p) => Ok(Curve25519Point(-p)),
        Curve25519Base(s) => Ok(Curve25519Base(-*s)),
        Curve25519Scalar(s) => Ok(Curve25519Scalar(-*s)),

        _ => Err(anyhow::anyhow!(
            "Unsupported negation of {:?}",
            x.get_type(),
        )),
    }
}

/// Negates in-circuit the given input.
/// Negation is supported on:
///   - `Native`
///   - `Bool`
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
/// This function results in an error if the input type is not supported.
pub fn neg_incircuit(
    std_lib: &ZkStdLib,
    layouter: &mut impl Layouter<F>,
    x: &CircuitValue,
) -> Result<CircuitValue, plonk::Error> {
    use CircuitValue::*;
    match x {
        Native(a) => {
            let r = std_lib.neg(layouter, a)?;
            Ok(Native(r))
        }
        Bool(a) => {
            let r = std_lib.not(layouter, a)?;
            Ok(Bool(r))
        }
        JubjubPoint(p) => {
            let r = std_lib.jubjub().negate(layouter, p)?;
            Ok(JubjubPoint(r))
        }

        Secp256k1Point(p) => {
            let r = std_lib.secp256k1().negate(layouter, p)?;
            Ok(Secp256k1Point(r))
        }
        Secp256k1Base(a) => {
            let r = (std_lib.secp256k1().base_field_chip()).neg(layouter, a)?;
            Ok(Secp256k1Base(r))
        }
        Secp256k1Scalar(a) => {
            let r = (std_lib.secp256k1().scalar_field_chip()).neg(layouter, a)?;
            Ok(Secp256k1Scalar(r))
        }

        Secp256r1Point(p) => {
            let r = std_lib.p256().negate(layouter, p)?;
            Ok(Secp256r1Point(r))
        }
        Secp256r1Base(a) => {
            let r = (std_lib.p256().base_field_chip()).neg(layouter, a)?;
            Ok(Secp256r1Base(r))
        }
        Secp256r1Scalar(a) => {
            let r = (std_lib.p256().scalar_field_chip()).neg(layouter, a)?;
            Ok(Secp256r1Scalar(r))
        }

        Curve25519Point(p) => {
            let r = std_lib.curve25519().negate(layouter, p)?;
            Ok(Curve25519Point(r))
        }
        Curve25519Base(a) => {
            let r = (std_lib.curve25519().base_field_chip()).neg(layouter, a)?;
            Ok(Curve25519Base(r))
        }
        Curve25519Scalar(a) => {
            let r = (std_lib.curve25519().scalar_field_chip()).neg(layouter, a)?;
            Ok(Curve25519Scalar(r))
        }

        _ => Err(plonk::Error::Synthesis(format!(
            "Unsupported negation of {:?}",
            x.get_type(),
        ))),
    }
}

#[cfg(test)]
impl Neg for IrValue {
    type Output = Self;

    fn neg(self) -> Self {
        neg_offcircuit(&self).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use group::Group;
    use group::ff::Field;
    use midnight_curves::{JubjubSubgroup, curve25519, k256, p256};
    use rand_chacha::rand_core::OsRng;
    use transient_crypto::curve::Fr;

    use super::*;

    #[test]
    fn test_neg() {
        use IrValue::*;

        let x = Fr(F::random(OsRng));
        let p = JubjubSubgroup::random(OsRng);

        assert_eq!(-Native(x), Native(-x));
        assert_eq!(-Bool(true), Bool(false));
        assert_eq!(-Bool(false), Bool(true));
        assert_eq!(-JubjubPoint(p), JubjubPoint(-p));

        let p = k256::K256::random(OsRng);
        let x = k256::Fp::random(OsRng);
        let r = k256::Fq::random(OsRng);
        assert_eq!(-Secp256k1Point(p), Secp256k1Point(-p));
        assert_eq!(-Secp256k1Base(x), Secp256k1Base(-x));
        assert_eq!(-Secp256k1Scalar(r), Secp256k1Scalar(-r));

        let p = p256::P256::random(OsRng);
        let x = p256::Fp::random(OsRng);
        let r = p256::Fq::random(OsRng);
        assert_eq!(-Secp256r1Point(p), Secp256r1Point(-p));
        assert_eq!(-Secp256r1Base(x), Secp256r1Base(-x));
        assert_eq!(-Secp256r1Scalar(r), Secp256r1Scalar(-r));

        let p = curve25519::Curve25519Subgroup::random(OsRng);
        let x = curve25519::Fp::random(OsRng);
        let r = <curve25519::Scalar as Field>::random(OsRng);
        assert_eq!(-Curve25519Point(p), Curve25519Point(-p));
        assert_eq!(-Curve25519Base(x), Curve25519Base(-x));
        assert_eq!(-Curve25519Scalar(r), Curve25519Scalar(-r));
    }
}
