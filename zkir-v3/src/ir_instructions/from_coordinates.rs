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

use group::cofactor::CofactorGroup;
use midnight_circuits::{ecc::curves::CircuitCurve, instructions::EccInstructions};

use midnight_curves::{JubjubExtended, JubjubSubgroup, curve25519, k256, p256};
use midnight_proofs::{circuit::Layouter, plonk};
use midnight_zk_stdlib::ZkStdLib;

use crate::{
    ir_instructions::F,
    ir_types::{CircuitValue, IrValue},
};

/// Constructs (off-circuit) a point from a pair of affine coordinates `(x, y)`.
/// Supported on:
///   - `(Native, Native)` -> `JubjubPoint`
///   - `(Secp256k1Base, Secp256k1Base)` -> `Secp256k1Point`
///   - `(Secp256r1Base, Secp256r1Base)` -> `Secp256r1Point`
///   - `(Curve25519Base, Curve25519Base)` -> `Curve25519Point`
///
/// NB: In Weierstrass curves, the identity point cannot be constructed through
/// this function.
///
/// # Errors
///
/// Errors if the inputs are of the supported types, declared above.
pub fn from_coordinates_offcircuit(x: &IrValue, y: &IrValue) -> Result<IrValue, anyhow::Error> {
    use IrValue::*;
    match (x, y) {
        (Native(x), Native(y)) => JubjubExtended::from_xy(x.0, y.0)
            .map(<JubjubExtended as CofactorGroup>::into_subgroup)
            .and_then(Into::<Option<JubjubSubgroup>>::into)
            .map(JubjubPoint)
            .ok_or(anyhow::anyhow!(
                "Cannot build a Jubjub point from ({x}, {y})",
            )),

        (Secp256k1Base(x), Secp256k1Base(y)) => k256::K256::from_xy(*x, *y)
            .map(Secp256k1Point)
            .ok_or(anyhow::anyhow!(
                "Cannot build a Secp256k1Point point from ({x:?}, {y:?})",
            )),

        (Secp256r1Base(x), Secp256r1Base(y)) => {
            p256::P256::from_xy(*x, *y)
                .map(Secp256r1Point)
                .ok_or(anyhow::anyhow!(
                    "Cannot build a Secp256r1Point point from ({x:?}, {y:?})",
                ))
        }

        (Curve25519Base(x), Curve25519Base(y)) => curve25519::Curve25519::from_xy(*x, *y)
            .and_then(|p| curve25519::Curve25519Subgroup::from_edwards(p.0))
            .map(Curve25519Point)
            .ok_or(anyhow::anyhow!(
                "Cannot build a Curve25519Point point from ({x:?}, {y:?})",
            )),

        (x, y) => Err(anyhow::anyhow!(
            "Unsupported `from_coordinates` on ({:?}, {:?})",
            x.get_type(),
            y.get_type(),
        )),
    }
}

/// Constructs (in-circuit) a point from a pair of affine coordinates `(x, y)`.
/// Supported on:
///   - `(Native, Native)` -> `JubjubPoint`
///   - `(Secp256k1Base, Secp256k1Base)` -> `Secp256k1Point`
///   - `(Secp256r1Base, Secp256r1Base)` -> `Secp256r1Point`
///   - `(Curve25519Base, Curve25519Base)` -> `Curve25519Point`
///
/// NB: In Weierstrass curves, the identity point cannot be constructed through
/// this function.
///
/// # Errors
///
/// Errors if the inputs are of the supported types, declared above.
pub fn from_coordinates_incircuit(
    std_lib: &ZkStdLib,
    layouter: &mut impl Layouter<F>,
    x: &CircuitValue,
    y: &CircuitValue,
) -> Result<CircuitValue, plonk::Error> {
    use CircuitValue::*;
    match (x, y) {
        (Native(x), Native(y)) => std_lib
            .jubjub()
            .point_from_coordinates(layouter, x, y)
            .map(JubjubPoint),

        (Secp256k1Base(x), Secp256k1Base(y)) => std_lib
            .secp256k1()
            .point_from_coordinates(layouter, x, y)
            .map(Secp256k1Point),

        (Secp256r1Base(x), Secp256r1Base(y)) => std_lib
            .p256()
            .point_from_coordinates(layouter, x, y)
            .map(Secp256r1Point),

        (Curve25519Base(x), Curve25519Base(y)) => std_lib
            .curve25519()
            .point_from_coordinates(layouter, x, y)
            .map(Curve25519Point),

        _ => Err(plonk::Error::Synthesis(format!(
            "Unsupported `from_coordinates` on ({:?}, {:?})",
            x.get_type(),
            y.get_type(),
        ))),
    }
}

#[cfg(test)]
mod tests {
    use group::Group;
    use midnight_curves::{JubjubSubgroup, curve25519, k256, p256};
    use rand_chacha::rand_core::OsRng;
    use transient_crypto::curve::Fr;

    use super::*;

    #[test]
    fn test_coordinates() {
        use IrValue::*;

        let p = JubjubSubgroup::random(OsRng);
        let (x, y) = Into::<JubjubExtended>::into(p).coordinates().unwrap();
        assert_eq!(
            from_coordinates_offcircuit(&Native(Fr(x)), &Native(Fr(y))).unwrap(),
            JubjubPoint(p)
        );

        let p = k256::K256::random(OsRng);
        let (x, y) = p.coordinates().unwrap();
        assert_eq!(
            from_coordinates_offcircuit(&Secp256k1Base(x), &Secp256k1Base(y)).unwrap(),
            Secp256k1Point(p)
        );

        assert!(from_coordinates_offcircuit(&Native(0.into()), &Secp256k1Base(y)).is_err());
        assert!(from_coordinates_offcircuit(&Native(0.into()), &Native(0.into())).is_err());

        let p = p256::P256::random(OsRng);
        let (x, y) = p.coordinates().unwrap();
        assert_eq!(
            from_coordinates_offcircuit(&Secp256r1Base(x), &Secp256r1Base(y)).unwrap(),
            Secp256r1Point(p)
        );

        // (x, y) not on the Secp256r1 curve.
        assert!(from_coordinates_offcircuit(&Secp256r1Base(x), &Secp256r1Base(x)).is_err());

        let p = curve25519::Curve25519Subgroup::random(OsRng);
        let (x, y) = Into::<curve25519::Curve25519>::into(p).coordinates().unwrap();
        assert_eq!(
            from_coordinates_offcircuit(&Curve25519Base(x), &Curve25519Base(y)).unwrap(),
            Curve25519Point(p)
        );

        // (x, y) not on the Curve25519 curve.
        assert!(from_coordinates_offcircuit(&Curve25519Base(x), &Curve25519Base(x)).is_err());
    }
}
