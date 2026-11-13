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

use group::ff::FromUniformBytes;
use midnight_circuits::{
    CircuitField, instructions::DecompositionInstructions, types::AssignedByte,
};

use midnight_proofs::{circuit::Layouter, plonk};
use midnight_zk_stdlib::ZkStdLib;
use num_bigint::BigUint;
use num_traits::Euclid;
use transient_crypto::curve::Fr;

use crate::{
    ir_instructions::F,
    ir_types::{CircuitValue, IrType, IrValue},
};

/// Builds (off-circuit) a value of the given type from its 32-byte representation.
/// Supported for types:
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
/// This operation also accepts non-canonical 32-byte representation in prime fields
/// by applying the relevant modular reduction.
///
/// # Errors
///
/// Errors if the input is not a supported type.
pub fn from_bytes32_offcircuit(val_t: &IrType, bytes: &[u8; 32]) -> Result<IrValue, anyhow::Error> {
    use IrValue::*;

    let mut buffer = [0u8; 64];
    buffer[..32].copy_from_slice(bytes);

    match val_t {
        IrType::Native => Ok(Native(Fr(F::from_uniform_bytes(&buffer)))),

        IrType::Secp256k1Base => Ok(Secp256k1Base(from_le_bytes_with_reduction(bytes))),

        IrType::Secp256k1Scalar => Ok(Secp256k1Scalar(from_le_bytes_with_reduction(bytes))),

        IrType::Secp256r1Base => Ok(Secp256r1Base(from_le_bytes_with_reduction(bytes))),

        IrType::Secp256r1Scalar => Ok(Secp256r1Scalar(from_le_bytes_with_reduction(bytes))),

        IrType::Curve25519Base => Ok(Curve25519Base(from_le_bytes_with_reduction(bytes))),

        IrType::Curve25519Scalar => Ok(Curve25519Scalar(from_le_bytes_with_reduction(bytes))),

        _ => Err(anyhow::anyhow!(
            "Unsupported from_bytes32 for type {val_t:?}",
        )),
    }
}

/// Builds (in-circuit) a value of the given type from its 32-byte representation.
/// Supported for types:
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
/// This operation also accepts non-canonical 32-byte representation in prime fields
/// by applying the relevant modular reduction.
///
/// # Errors
///
/// Errors if the input is not a supported type.
pub fn from_bytes32_incircuit(
    std_lib: &ZkStdLib,
    layouter: &mut impl Layouter<F>,
    val_t: &IrType,
    bytes: &[AssignedByte<F>; 32],
) -> Result<CircuitValue, plonk::Error> {
    use CircuitValue::*;
    match val_t {
        IrType::Native => std_lib.assigned_from_le_bytes(layouter, bytes).map(Native),

        IrType::Secp256k1Base => std_lib
            .secp256k1()
            .base_field_chip()
            .assigned_from_le_bytes(layouter, bytes)
            .map(Secp256k1Base),

        IrType::Secp256k1Scalar => std_lib
            .secp256k1()
            .scalar_field_chip()
            .assigned_from_le_bytes(layouter, bytes)
            .map(Secp256k1Scalar),

        IrType::Secp256r1Base => std_lib
            .p256()
            .base_field_chip()
            .assigned_from_le_bytes(layouter, bytes)
            .map(Secp256r1Base),

        IrType::Secp256r1Scalar => std_lib
            .p256()
            .scalar_field_chip()
            .assigned_from_le_bytes(layouter, bytes)
            .map(Secp256r1Scalar),

        IrType::Curve25519Base => std_lib
            .curve25519()
            .base_field_chip()
            .assigned_from_le_bytes(layouter, bytes)
            .map(Curve25519Base),

        IrType::Curve25519Scalar => std_lib
            .curve25519()
            .scalar_field_chip()
            .assigned_from_le_bytes(layouter, bytes)
            .map(Curve25519Scalar),

        _ => Err(plonk::Error::Synthesis(format!(
            "Unsupported from_bytes32 for {val_t:?}",
        ))),
    }
}

/// Builds a prime field element from the given 32 bytes by interpreting them
/// in little-endian as an integer. The integer can be bigger than field order.
pub(crate) fn from_le_bytes_with_reduction<F: CircuitField>(bytes: &[u8; 32]) -> F {
    let (_, rem) = BigUint::from_bytes_le(bytes).div_rem_euclid(&F::modulus());
    let mut rem_bytes = rem.to_bytes_le();
    rem_bytes.resize(32, 0);
    F::from_bytes_le(&rem_bytes).unwrap()
}

#[cfg(test)]
mod tests {
    use group::ff::Field;
    use midnight_curves::{curve25519, k256, p256};
    use rand_chacha::rand_core::OsRng;
    use transient_crypto::curve::Fr;

    use super::*;
    use crate::ir_instructions::into_bytes32::into_bytes32_offcircuit;

    // Starts from a random value, converts it into bytes (so as to obtain a
    // valid, canonical 32-byte representation), then goes from those bytes
    // back into a value and into bytes again, checking that the
    // re-serialized bytes match the ones we started from.
    #[test]
    fn test_from_bytes32_roundtrip() {
        use IrValue::*;

        let x = Native(Fr(F::random(OsRng)));
        let bytes: [u8; 32] = into_bytes32_offcircuit(&x).unwrap().try_into().unwrap();
        let y = from_bytes32_offcircuit(&IrType::Native, &bytes).unwrap();
        let bytes2: [u8; 32] = into_bytes32_offcircuit(&y).unwrap().try_into().unwrap();
        assert_eq!(bytes2, bytes);

        let x = Secp256k1Base(k256::Fp::random(OsRng));
        let bytes: [u8; 32] = into_bytes32_offcircuit(&x).unwrap().try_into().unwrap();
        let y = from_bytes32_offcircuit(&IrType::Secp256k1Base, &bytes).unwrap();
        let bytes2: [u8; 32] = into_bytes32_offcircuit(&y).unwrap().try_into().unwrap();
        assert_eq!(bytes2, bytes);

        let x = Secp256k1Scalar(k256::Fq::random(OsRng));
        let bytes: [u8; 32] = into_bytes32_offcircuit(&x).unwrap().try_into().unwrap();
        let y = from_bytes32_offcircuit(&IrType::Secp256k1Scalar, &bytes).unwrap();
        let bytes2: [u8; 32] = into_bytes32_offcircuit(&y).unwrap().try_into().unwrap();
        assert_eq!(bytes2, bytes);

        let x = Secp256r1Base(p256::Fp::random(OsRng));
        let bytes: [u8; 32] = into_bytes32_offcircuit(&x).unwrap().try_into().unwrap();
        let y = from_bytes32_offcircuit(&IrType::Secp256r1Base, &bytes).unwrap();
        let bytes2: [u8; 32] = into_bytes32_offcircuit(&y).unwrap().try_into().unwrap();
        assert_eq!(bytes2, bytes);

        let x = Secp256r1Scalar(p256::Fq::random(OsRng));
        let bytes: [u8; 32] = into_bytes32_offcircuit(&x).unwrap().try_into().unwrap();
        let y = from_bytes32_offcircuit(&IrType::Secp256r1Scalar, &bytes).unwrap();
        let bytes2: [u8; 32] = into_bytes32_offcircuit(&y).unwrap().try_into().unwrap();
        assert_eq!(bytes2, bytes);

        let x = Curve25519Base(curve25519::Fp::random(OsRng));
        let bytes: [u8; 32] = into_bytes32_offcircuit(&x).unwrap().try_into().unwrap();
        let y = from_bytes32_offcircuit(&IrType::Curve25519Base, &bytes).unwrap();
        let bytes2: [u8; 32] = into_bytes32_offcircuit(&y).unwrap().try_into().unwrap();
        assert_eq!(bytes2, bytes);

        let x = Curve25519Scalar(<curve25519::Scalar as Field>::random(OsRng));
        let bytes: [u8; 32] = into_bytes32_offcircuit(&x).unwrap().try_into().unwrap();
        let y = from_bytes32_offcircuit(&IrType::Curve25519Scalar, &bytes).unwrap();
        let bytes2: [u8; 32] = into_bytes32_offcircuit(&y).unwrap().try_into().unwrap();
        assert_eq!(bytes2, bytes);
    }

    // Non-canonical (out-of-range) bytes are accepted and reduced modulo
    // each field's characteristic, rather than rejected.
    #[test]
    fn test_from_bytes32_reduces_non_canonical_input() {
        let bytes = [0xffu8; 32];

        assert_eq!(
            from_bytes32_offcircuit(&IrType::Native, &bytes).unwrap(),
            IrValue::Native(Fr(from_le_bytes_with_reduction(&bytes)))
        );
        assert_eq!(
            from_bytes32_offcircuit(&IrType::Secp256k1Base, &bytes).unwrap(),
            IrValue::Secp256k1Base(from_le_bytes_with_reduction(&bytes))
        );
        assert_eq!(
            from_bytes32_offcircuit(&IrType::Secp256k1Scalar, &bytes).unwrap(),
            IrValue::Secp256k1Scalar(from_le_bytes_with_reduction(&bytes))
        );
        assert_eq!(
            from_bytes32_offcircuit(&IrType::Secp256r1Base, &bytes).unwrap(),
            IrValue::Secp256r1Base(from_le_bytes_with_reduction(&bytes))
        );
        assert_eq!(
            from_bytes32_offcircuit(&IrType::Secp256r1Scalar, &bytes).unwrap(),
            IrValue::Secp256r1Scalar(from_le_bytes_with_reduction(&bytes))
        );
        assert_eq!(
            from_bytes32_offcircuit(&IrType::Curve25519Base, &bytes).unwrap(),
            IrValue::Curve25519Base(from_le_bytes_with_reduction(&bytes))
        );
        assert_eq!(
            from_bytes32_offcircuit(&IrType::Curve25519Scalar, &bytes).unwrap(),
            IrValue::Curve25519Scalar(from_le_bytes_with_reduction(&bytes))
        );
    }
}
