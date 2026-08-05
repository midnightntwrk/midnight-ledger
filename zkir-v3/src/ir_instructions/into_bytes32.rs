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

use midnight_circuits::{CircuitField, instructions::DecompositionInstructions};

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
    use IrValue::*;
    match value {
        Native(x) => Ok(Bytes32(x.0.to_bytes_le())),

        Secp256k1Base(s) => Ok(Bytes32(s.to_bytes_le())),

        Secp256k1Scalar(s) => Ok(Bytes32(s.to_bytes_le())),

        Secp256r1Base(s) => Ok(Bytes32(s.to_bytes_le())),

        Secp256r1Scalar(s) => Ok(Bytes32(s.to_bytes_le())),

        Curve25519Base(s) => Ok(Bytes32(s.to_bytes_le())),

        Curve25519Scalar(s) => Ok(Bytes32(s.to_bytes_le())),

        _ => Err(anyhow::anyhow!(
            "Unsupported into_bytes32 for {:?}",
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
    use CircuitValue::*;
    match value {
        Native(x) => std_lib
            .assigned_to_le_bytes(layouter, x, Some(32))
            .map(|bytes| Bytes32(bytes.try_into().unwrap())),

        Secp256k1Base(s) => std_lib
            .secp256k1()
            .base_field_chip()
            .assigned_to_le_bytes(layouter, s, Some(32))
            .map(|bytes| Bytes32(bytes.try_into().unwrap())),

        Secp256k1Scalar(s) => std_lib
            .secp256k1()
            .scalar_field_chip()
            .assigned_to_le_bytes(layouter, s, Some(32))
            .map(|bytes| Bytes32(bytes.try_into().unwrap())),

        Secp256r1Base(s) => std_lib
            .p256()
            .base_field_chip()
            .assigned_to_le_bytes(layouter, s, Some(32))
            .map(|bytes| Bytes32(bytes.try_into().unwrap())),

        Secp256r1Scalar(s) => std_lib
            .p256()
            .scalar_field_chip()
            .assigned_to_le_bytes(layouter, s, Some(32))
            .map(|bytes| Bytes32(bytes.try_into().unwrap())),

        Curve25519Base(s) => std_lib
            .curve25519()
            .base_field_chip()
            .assigned_to_le_bytes(layouter, s, Some(32))
            .map(|bytes| Bytes32(bytes.try_into().unwrap())),

        Curve25519Scalar(s) => std_lib
            .curve25519()
            .scalar_field_chip()
            .assigned_to_le_bytes(layouter, s, Some(32))
            .map(|bytes| Bytes32(bytes.try_into().unwrap())),

        _ => Err(plonk::Error::Synthesis(format!(
            "Unsupported into_bytes32 for {:?}",
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
    use crate::ir_instructions::from_bytes32::from_bytes32_offcircuit;

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
}
