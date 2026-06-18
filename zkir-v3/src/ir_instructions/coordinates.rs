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
    instructions::{AssertionInstructions as _, EccInstructions as _},
};

use midnight_curves::{JubjubExtended, secp256k1};
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
///
/// # Errors
///
/// Errors if the input is not a supported type, or if it is the Secp256k1
/// identity (which has no affine coordinates).
fn coordinates_offcircuit(point: &IrValue) -> Result<(IrValue, IrValue), anyhow::Error> {
    use IrValue::*;
    match point {
        JubjubPoint(p) => {
            let p_ext: JubjubExtended = (*p).into();
            let (x, y) = p_ext
                .coordinates()
                .expect("Jubjub points have affine coordinates");
            Ok((Native(Fr(x)), Native(Fr(y))))
        }
        Secp256k1Point(p) => {
            if bool::from(p.is_identity()) {
                return Err(anyhow::anyhow!(
                    "Cannot extract coordinates of the Secp256k1 identity"
                ));
            }
            let (x, y) = p
                .coordinates()
                .expect("non-identity points have coordinates");
            Ok((Secp256k1Base(x), Secp256k1Base(y)))
        }
        _ => Err(anyhow::anyhow!(
            "Unsupported coordinate extraction of {:?}",
            point.get_type(),
        )),
    }
}

/// Extracts off-circuit the affine x-coordinate of an elliptic curve point.
/// See [`coordinates_offcircuit`].
pub fn x_coordinate_offcircuit(point: &IrValue) -> Result<IrValue, anyhow::Error> {
    Ok(coordinates_offcircuit(point)?.0)
}

/// Extracts off-circuit the affine y-coordinate of an elliptic curve point.
/// See [`coordinates_offcircuit`].
pub fn y_coordinate_offcircuit(point: &IrValue) -> Result<IrValue, anyhow::Error> {
    Ok(coordinates_offcircuit(point)?.1)
}

/// Extracts in-circuit the affine `(x, y)` coordinates of an elliptic curve
/// point as a pair of assigned base field values. Supported on:
///   - `JubjubPoint`    -> `(Native, Native)`
///   - `Secp256k1Point` -> `(Secp256k1Base, Secp256k1Base)`
///
/// For Secp256k1 this constrains the point to not be the identity.
///
/// # Errors
///
/// Errors if the input is not a supported type.
fn coordinates_incircuit(
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
            let curve = std_lib.assert_false(layouter, p.is_zero());
            Ok((
                Secp256k1Base(curve.x_coordinate(p)),
                Secp256k1Base(curve.y_coordinate(p)),
            ))
        }
        _ => Err(plonk::Error::Synthesis(format!(
            "Unsupported coordinate extraction of {:?}",
            point.get_type(),
        ))),
    }
}

/// Extracts in-circuit the affine x-coordinate of an elliptic curve point.
/// See [`coordinates_incircuit`].
pub fn x_coordinate_incircuit(
    std_lib: &ZkStdLib,
    layouter: &mut impl Layouter<F>,
    point: &CircuitValue,
) -> Result<CircuitValue, plonk::Error> {
    Ok(coordinates_incircuit(std_lib, layouter, point)?.0)
}

/// Extracts in-circuit the affine y-coordinate of an elliptic curve point.
/// See [`coordinates_incircuit`].
pub fn y_coordinate_incircuit(
    std_lib: &ZkStdLib,
    layouter: &mut impl Layouter<F>,
    point: &CircuitValue,
) -> Result<CircuitValue, plonk::Error> {
    Ok(coordinates_incircuit(std_lib, layouter, point)?.1)
}

#[cfg(test)]
mod tests {
    use midnight_curves::JubjubSubgroup;
    use rand_chacha::rand_core::OsRng;

    use super::*;

    #[test]
    fn test_coordinates() {
        use IrValue::*;

        // Jubjub: coordinates are native field values.
        let p = JubjubSubgroup::random(OsRng);
        let p_ext: JubjubExtended = p.into();
        let (ex, ey) = p_ext.coordinates().unwrap();
        assert_eq!(
            x_coordinate_offcircuit(&JubjubPoint(p)).unwrap(),
            Native(Fr(ex))
        );
        assert_eq!(
            y_coordinate_offcircuit(&JubjubPoint(p)).unwrap(),
            Native(Fr(ey))
        );

        // Secp256k1: coordinates are base field values.
        let q = secp256k1::Secp256k1::random(OsRng);
        let (eqx, eqy) = q.coordinates().unwrap();
        assert_eq!(
            x_coordinate_offcircuit(&Secp256k1Point(q)).unwrap(),
            Secp256k1Base(eqx)
        );
        assert_eq!(
            y_coordinate_offcircuit(&Secp256k1Point(q)).unwrap(),
            Secp256k1Base(eqy)
        );

        // The Secp256k1 identity has no affine coordinates.
        assert!(
            x_coordinate_offcircuit(&Secp256k1Point(secp256k1::Secp256k1::identity())).is_err()
        );

        // Coordinate extraction on a scalar is unsupported.
        assert!(x_coordinate_offcircuit(&Native(Fr::from(1))).is_err());
    }
}
