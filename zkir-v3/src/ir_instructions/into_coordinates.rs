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

use group::Group;
use midnight_circuits::{
    ecc::curves::CircuitCurve,
    instructions::{EccInstructions, ZeroInstructions},
};

use midnight_curves::JubjubExtended;
use midnight_proofs::{circuit::Layouter, plonk};
use midnight_zk_stdlib::ZkStdLib;
use transient_crypto::curve::Fr;

use crate::{
    ir_instructions::F,
    ir_types::{CircuitValue, IrValue},
};

/// Extracts off-circuit the affine `(x, y)` coordinates of an elliptic curve
/// point as a pair of base field values. Supported on:
///   - `JubjubPoint`    -> `(Native, Native)`
///   - `Secp256k1Point` -> `(Secp256k1Base, Secp256k1Base)`
///   - `P256Point`      -> `(P256Base, P256Base)`
///
/// # Errors
///
/// Errors if the input is not a supported type, or if it is the identity of a
/// Weierstrass curve (which has no affine coordinates).
pub fn into_coordinates_offcircuit(point: &IrValue) -> Result<(IrValue, IrValue), anyhow::Error> {
    use IrValue::*;
    match point {
        JubjubPoint(p) => {
            let p_ext: JubjubExtended = (*p).into();
            let (x, y) = p_ext.coordinates().unwrap();
            Ok((Native(Fr(x)), Native(Fr(y))))
        }
        Secp256k1Point(p) => {
            if bool::from(p.is_identity()) {
                return Err(anyhow::anyhow!(
                    "Cannot extract coordinates of the Secp256k1 identity"
                ));
            }
            let (x, y) = p.coordinates().unwrap();
            Ok((Secp256k1Base(x), Secp256k1Base(y)))
        }
        P256Point(p) => {
            if bool::from(p.is_identity()) {
                return Err(anyhow::anyhow!(
                    "Cannot extract coordinates of the P256 identity"
                ));
            }
            let (x, y) = p.coordinates().unwrap();
            Ok((P256Base(x), P256Base(y)))
        }
        _ => Err(anyhow::anyhow!(
            "Unsupported coordinate extraction of {:?}",
            point.get_type(),
        )),
    }
}

/// Extracts in-circuit the affine `(x, y)` coordinates of an elliptic curve
/// point as a pair of assigned base field values. Supported on:
///   - `JubjubPoint`    -> `(Native, Native)`
///   - `Secp256k1Point` -> `(Secp256k1Base, Secp256k1Base)`
///   - `P256Point`      -> `(P256Base, P256Base)`
///
/// For Weierstrass curves this constrains the point to not be the identity
/// (which has no affine coordinates), making the circuit unsatisfiable on the
/// identity.
///
/// # Errors
///
/// Errors if the input is not a supported type.
pub fn into_coordinates_incircuit(
    std_lib: &ZkStdLib,
    layouter: &mut impl Layouter<F>,
    point: &CircuitValue,
) -> Result<(CircuitValue, CircuitValue), plonk::Error> {
    use CircuitValue::*;
    match point {
        JubjubPoint(p) => {
            let jubjub = std_lib.jubjub();
            Ok((
                Native(jubjub.x_coordinate(p)),
                Native(jubjub.y_coordinate(p)),
            ))
        }
        Secp256k1Point(p) => {
            let curve = std_lib.secp256k1();
            curve.assert_non_zero(layouter, p)?;
            Ok((
                Secp256k1Base(curve.x_coordinate(p)),
                Secp256k1Base(curve.y_coordinate(p)),
            ))
        }
        P256Point(p) => {
            let curve = std_lib.p256();
            curve.assert_non_zero(layouter, p)?;
            Ok((
                P256Base(curve.x_coordinate(p)),
                P256Base(curve.y_coordinate(p)),
            ))
        }
        _ => Err(plonk::Error::Synthesis(format!(
            "Unsupported coordinate extraction of {:?}",
            point.get_type(),
        ))),
    }
}

#[cfg(test)]
mod tests {
    use midnight_curves::{JubjubSubgroup, k256, p256};
    use rand_chacha::rand_core::OsRng;

    use super::*;

    #[test]
    fn test_coordinates() {
        use IrValue::*;

        let p = JubjubSubgroup::random(OsRng);
        let (x, y) = Into::<JubjubExtended>::into(p).coordinates().unwrap();
        assert_eq!(
            into_coordinates_offcircuit(&JubjubPoint(p)).unwrap(),
            (Native(Fr(x)), Native(Fr(y)))
        );

        let p = k256::K256::random(OsRng);
        let (x, y) = p.coordinates().unwrap();
        assert_eq!(
            into_coordinates_offcircuit(&Secp256k1Point(p)).unwrap(),
            (Secp256k1Base(x), Secp256k1Base(y))
        );

        // The Secp256k1 identity has no affine coordinates.
        assert!(into_coordinates_offcircuit(&Secp256k1Point(k256::K256::identity())).is_err());

        let p = p256::P256::random(OsRng);
        let (x, y) = p.coordinates().unwrap();
        assert_eq!(
            into_coordinates_offcircuit(&P256Point(p)).unwrap(),
            (P256Base(x), P256Base(y))
        );

        // The P256 identity has no affine coordinates.
        assert!(into_coordinates_offcircuit(&P256Point(p256::P256::identity())).is_err());

        // Coordinate extraction on a scalar is unsupported.
        assert!(into_coordinates_offcircuit(&Native(Fr::from(1))).is_err());
    }
}
