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

use midnight_circuits::{
    CircuitField,
    instructions::{AssignmentInstructions, DecompositionInstructions},
    types::AssignedByte,
};

use midnight_proofs::{circuit::Layouter, plonk};
use midnight_zk_stdlib::ZkStdLib;

use crate::{
    ir_instructions::F,
    ir_types::{CircuitValue, IrValue},
};

/// Converts (off-circuit) the given value into its 32-byte representation.
/// Supported on types:
///  - Native
///  - Secp256k1Base
///  - Secp256k1Scalar
///  - Secp256r1Base
///  - Secp256r1Scalar
///  - Curve25519Base
///  - Curve25519Scalar
///
/// In all the above prime fields, the 32-byte representation is the little-endian
/// byte encoding of the underlying (canonical) integer.
///
/// # Errors
///
/// Errors if the input is not a supported type.
pub fn into_bytes32_offcircuit(value: &IrValue) -> Result<IrValue, anyhow::Error> {
    to_le_bytes32_offcircuit(value).map(IrValue::Bytes32)
}

/// Converts (off-circuit) the given value into its 64-byte representation.
/// Supported on the same types as [into_bytes32_offcircuit].
///
/// Since all supported values fit in 32 bytes, the 32 most significant bytes
/// of the output are always zero. This operation exists to allow round-tripping
/// with `FromBytes64`.
///
/// # Errors
///
/// Errors if the input is not a supported type.
pub fn into_bytes64_offcircuit(value: &IrValue) -> Result<IrValue, anyhow::Error> {
    let mut bytes = [0u8; 64];
    bytes[..32].copy_from_slice(&to_le_bytes32_offcircuit(value)?);
    Ok(IrValue::Bytes64(bytes))
}

fn to_le_bytes32_offcircuit(value: &IrValue) -> Result<[u8; 32], anyhow::Error> {
    use IrValue::*;
    match value {
        Native(x) => Ok(x.0.to_bytes_le()),

        Secp256k1Base(s) => Ok(s.to_bytes_le()),

        Secp256k1Scalar(s) => Ok(s.to_bytes_le()),

        Secp256r1Base(s) => Ok(s.to_bytes_le()),

        Secp256r1Scalar(s) => Ok(s.to_bytes_le()),

        Curve25519Base(s) => Ok(s.to_bytes_le()),

        Curve25519Scalar(s) => Ok(s.to_bytes_le()),

        _ => Err(anyhow::anyhow!(
            "Unsupported into_bytes for {:?}",
            value.get_type(),
        )),
    }
}

/// Converts (in-circuit) the given value into its 32-byte representation.
/// Supported on types:
///  - Native
///  - Secp256k1Base
///  - Secp256k1Scalar
///  - Secp256r1Base
///  - Secp256r1Scalar
///  - Curve25519Base
///  - Curve25519Scalar
///
/// In all the above prime fields, the 32-byte representation is the little-endian
/// byte encoding of the underlying (canonical) integer.
///
/// # Errors
///
/// Errors if the input is not a supported type.
pub fn into_bytes32_incircuit(
    std_lib: &ZkStdLib,
    layouter: &mut impl Layouter<F>,
    value: &CircuitValue,
) -> Result<CircuitValue, plonk::Error> {
    let bytes = to_le_bytes32_incircuit(std_lib, layouter, value)?;
    Ok(CircuitValue::Bytes32(bytes.try_into().unwrap()))
}

/// Converts (in-circuit) the given value into its 64-byte representation.
/// Supported on the same types as [into_bytes32_incircuit].
///
/// Since all supported values fit in 32 bytes, the 32 most significant bytes
/// of the output are always (constrained to be) zero. This operation exists
/// to allow round-tripping with `FromBytes64`.
///
/// # Errors
///
/// Errors if the input is not a supported type.
pub fn into_bytes64_incircuit(
    std_lib: &ZkStdLib,
    layouter: &mut impl Layouter<F>,
    value: &CircuitValue,
) -> Result<CircuitValue, plonk::Error> {
    let mut bytes = to_le_bytes32_incircuit(std_lib, layouter, value)?;
    let zero = std_lib.assign_fixed(layouter, 0u8)?;
    bytes.resize(64, zero);
    Ok(CircuitValue::Bytes64(bytes.try_into().unwrap()))
}

fn to_le_bytes32_incircuit(
    std_lib: &ZkStdLib,
    layouter: &mut impl Layouter<F>,
    value: &CircuitValue,
) -> Result<Vec<AssignedByte<F>>, plonk::Error> {
    use CircuitValue::*;
    match value {
        Native(x) => std_lib.assigned_to_le_bytes(layouter, x, Some(32)),

        Secp256k1Base(s) => std_lib
            .secp256k1()
            .base_field_chip()
            .assigned_to_le_bytes(layouter, s, Some(32)),

        Secp256k1Scalar(s) => std_lib
            .secp256k1()
            .scalar_field_chip()
            .assigned_to_le_bytes(layouter, s, Some(32)),

        Secp256r1Base(s) => std_lib
            .p256()
            .base_field_chip()
            .assigned_to_le_bytes(layouter, s, Some(32)),

        Secp256r1Scalar(s) => std_lib
            .p256()
            .scalar_field_chip()
            .assigned_to_le_bytes(layouter, s, Some(32)),

        Curve25519Base(s) => std_lib
            .curve25519()
            .base_field_chip()
            .assigned_to_le_bytes(layouter, s, Some(32)),

        Curve25519Scalar(s) => std_lib
            .curve25519()
            .scalar_field_chip()
            .assigned_to_le_bytes(layouter, s, Some(32)),

        _ => Err(plonk::Error::Synthesis(format!(
            "Unsupported into_bytes for {:?}",
            value.get_type(),
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
    use crate::ir_instructions::from_bytes32::{from_bytes32_offcircuit, from_bytes64_offcircuit};

    #[test]
    fn test_into_bytes32_roundtrip() {
        use IrValue::*;

        let x = Native(Fr(F::random(OsRng)));
        let bytes: [u8; 32] = into_bytes32_offcircuit(&x).unwrap().try_into().unwrap();
        assert_eq!(from_bytes32_offcircuit(&x.get_type(), &bytes).unwrap(), x);

        let x = Secp256k1Base(k256::Fp::random(OsRng));
        let bytes: [u8; 32] = into_bytes32_offcircuit(&x).unwrap().try_into().unwrap();
        assert_eq!(from_bytes32_offcircuit(&x.get_type(), &bytes).unwrap(), x);

        let x = Secp256k1Scalar(k256::Fq::random(OsRng));
        let bytes: [u8; 32] = into_bytes32_offcircuit(&x).unwrap().try_into().unwrap();
        assert_eq!(from_bytes32_offcircuit(&x.get_type(), &bytes).unwrap(), x);

        let x = Secp256r1Base(p256::Fp::random(OsRng));
        let bytes: [u8; 32] = into_bytes32_offcircuit(&x).unwrap().try_into().unwrap();
        assert_eq!(from_bytes32_offcircuit(&x.get_type(), &bytes).unwrap(), x);

        let x = Secp256r1Scalar(p256::Fq::random(OsRng));
        let bytes: [u8; 32] = into_bytes32_offcircuit(&x).unwrap().try_into().unwrap();
        assert_eq!(from_bytes32_offcircuit(&x.get_type(), &bytes).unwrap(), x);

        let x = Curve25519Base(curve25519::Fp::random(OsRng));
        let bytes: [u8; 32] = into_bytes32_offcircuit(&x).unwrap().try_into().unwrap();
        assert_eq!(from_bytes32_offcircuit(&x.get_type(), &bytes).unwrap(), x);

        let x = Curve25519Scalar(<curve25519::Scalar as Field>::random(OsRng));
        let bytes: [u8; 32] = into_bytes32_offcircuit(&x).unwrap().try_into().unwrap();
        assert_eq!(from_bytes32_offcircuit(&x.get_type(), &bytes).unwrap(), x);
    }

    #[test]
    fn test_into_bytes64_roundtrip() {
        use IrValue::*;

        let values = [
            Native(Fr(F::random(OsRng))),
            Secp256k1Base(k256::Fp::random(OsRng)),
            Secp256k1Scalar(k256::Fq::random(OsRng)),
            Secp256r1Base(p256::Fp::random(OsRng)),
            Secp256r1Scalar(p256::Fq::random(OsRng)),
            Curve25519Base(curve25519::Fp::random(OsRng)),
            Curve25519Scalar(<curve25519::Scalar as Field>::random(OsRng)),
        ];
        for x in values {
            let bytes: [u8; 64] = into_bytes64_offcircuit(&x).unwrap().try_into().unwrap();
            assert!(bytes[32..].iter().all(|b| *b == 0));
            assert_eq!(from_bytes64_offcircuit(&x.get_type(), &bytes).unwrap(), x);
        }
    }
}
