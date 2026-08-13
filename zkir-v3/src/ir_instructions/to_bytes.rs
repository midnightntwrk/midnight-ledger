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
};

use midnight_proofs::{circuit::Layouter, plonk};
use midnight_zk_stdlib::ZkStdLib;

use crate::{
    ir_instructions::F,
    ir_types::{CircuitValue, IrValue},
};

/// Converts (off-circuit) the given value into its byte representation.
/// Supported on types (with their number of bytes):
///  - Native           (32 bytes)
///  - Secp256k1Base    (32 bytes)
///  - Secp256k1Scalar  (32 bytes)
///  - Curve25519Scalar (64 bytes)
///
/// In all the above prime fields, the byte representation is the little-endian
/// byte encoding of the underlying (canonical) integer. Curve25519 scalars are
/// padded to 64 bytes (the most significant 32 bytes are zero) so that
/// `to_bytes` round-trips with `from_bytes`, whose 64-byte input allows
/// reducing a 512-bit hash output as required by ed25519.
///
/// # Errors
///
/// Errors if the input is not a supported type.
pub fn to_bytes_offcircuit(value: &IrValue) -> Result<IrValue, anyhow::Error> {
    use IrValue::*;
    match value {
        Native(x) => Ok(Bytes(x.0.to_bytes_le().to_vec())),

        Secp256k1Base(s) => Ok(Bytes(s.to_bytes_le().to_vec())),

        Secp256k1Scalar(s) => Ok(Bytes(s.to_bytes_le().to_vec())),

        Curve25519Scalar(s) => {
            let mut bytes = s.to_bytes_le().to_vec();
            bytes.resize(64, 0);
            Ok(Bytes(bytes))
        }

        _ => Err(anyhow::anyhow!(
            "Unsupported to_bytes for {:?}",
            value.get_type(),
        )),
    }
}

/// Converts (in-circuit) the given value into its byte representation.
/// Supported on types (with their number of bytes):
///  - Native           (32 bytes)
///  - Secp256k1Base    (32 bytes)
///  - Secp256k1Scalar  (32 bytes)
///  - Curve25519Scalar (64 bytes)
///
/// In all the above prime fields, the byte representation is the little-endian
/// byte encoding of the underlying (canonical) integer. Curve25519 scalars are
/// padded to 64 bytes with (constrained) zero bytes; see
/// [`to_bytes_offcircuit`].
///
/// # Errors
///
/// Errors if the input is not a supported type.
pub fn to_bytes_incircuit(
    std_lib: &ZkStdLib,
    layouter: &mut impl Layouter<F>,
    value: &CircuitValue,
) -> Result<CircuitValue, plonk::Error> {
    use CircuitValue::*;
    match value {
        Native(x) => std_lib
            .assigned_to_le_bytes(layouter, x, Some(32))
            .map(Bytes),

        Secp256k1Base(s) => std_lib
            .secp256k1()
            .base_field_chip()
            .assigned_to_le_bytes(layouter, s, Some(32))
            .map(Bytes),

        Secp256k1Scalar(s) => std_lib
            .secp256k1()
            .scalar_field_chip()
            .assigned_to_le_bytes(layouter, s, Some(32))
            .map(Bytes),

        Curve25519Scalar(s) => {
            let mut bytes = std_lib
                .curve25519()
                .scalar_field_chip()
                .assigned_to_le_bytes(layouter, s, Some(32))?;
            for _ in 32..64 {
                bytes.push(std_lib.assign_fixed(layouter, 0u8)?);
            }
            Ok(Bytes(bytes))
        }

        _ => Err(plonk::Error::Synthesis(format!(
            "Unsupported to_bytes for {:?}",
            value.get_type(),
        ))),
    }
}

#[cfg(test)]
mod tests {
    use group::ff::Field;
    use midnight_curves::{curve25519, k256};
    use rand_chacha::rand_core::OsRng;
    use transient_crypto::curve::Fr;

    use super::*;
    use crate::ir_instructions::from_bytes::from_bytes_offcircuit;

    #[test]
    fn test_to_bytes_roundtrip() {
        use IrValue::*;

        for x in [
            Native(Fr(F::random(OsRng))),
            Secp256k1Base(k256::Fp::random(OsRng)),
            Secp256k1Scalar(k256::Fq::random(OsRng)),
            // Nb. dalek's inherent `Scalar::random` (which shadows
            // `ff::Field::random`) takes the rng by mutable reference.
            Curve25519Scalar(curve25519::Scalar::random(&mut OsRng)),
        ] {
            let bytes: Vec<u8> = to_bytes_offcircuit(&x).unwrap().try_into().unwrap();
            assert_eq!(from_bytes_offcircuit(&x.get_type(), &bytes).unwrap(), x);
        }
    }

    // The 64-byte representation of a Curve25519 scalar has its most
    // significant 32 bytes set to zero.
    #[test]
    fn test_to_bytes_curve25519_scalar_padding() {
        let x = IrValue::Curve25519Scalar(curve25519::Scalar::random(&mut OsRng));
        let bytes: Vec<u8> = to_bytes_offcircuit(&x).unwrap().try_into().unwrap();
        assert_eq!(bytes.len(), 64);
        assert!(bytes[32..].iter().all(|b| *b == 0));
    }
}
