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

/// Builds (off-circuit) a value of the given type from its byte representation.
/// Supported for types (with their expected number of bytes):
///  - Native           (32 bytes)
///  - Secp256k1Base    (32 bytes)
///  - Secp256k1Scalar  (32 bytes)
///  - Curve25519Scalar (64 bytes)
///
/// In all the above prime fields, the byte representation is the little-endian
/// byte encoding of the underlying (canonical) integer.
///
/// This operation also accepts non-canonical byte representations in prime fields
/// by applying the relevant modular reduction.
///
/// # Errors
///
/// Errors if the input is not a supported type, or if the number of bytes does
/// not match the byte representation length of the type.
pub fn from_bytes_offcircuit(val_t: &IrType, bytes: &[u8]) -> Result<IrValue, anyhow::Error> {
    use IrValue::*;

    check_byte_repr_len(val_t, bytes.len()).map_err(|e| anyhow::anyhow!("from_bytes: {e}"))?;

    match val_t {
        IrType::Native => {
            let mut buffer = [0u8; 64];
            buffer[..32].copy_from_slice(bytes);
            Ok(Native(Fr(F::from_uniform_bytes(&buffer))))
        }

        IrType::Secp256k1Base => Ok(Secp256k1Base(from_le_bytes_with_reduction(bytes))),

        IrType::Secp256k1Scalar => Ok(Secp256k1Scalar(from_le_bytes_with_reduction(bytes))),

        IrType::Curve25519Scalar => Ok(Curve25519Scalar(from_le_bytes_with_reduction(bytes))),

        _ => Err(anyhow::anyhow!("Unsupported from_bytes for type {val_t:?}",)),
    }
}

/// Builds (in-circuit) a value of the given type from its byte representation.
/// Supported for types (with their expected number of bytes):
///  - Native           (32 bytes)
///  - Secp256k1Base    (32 bytes)
///  - Secp256k1Scalar  (32 bytes)
///  - Curve25519Scalar (64 bytes)
///
/// In all the above prime fields, the byte representation is the little-endian
/// byte encoding of the underlying (canonical) integer.
///
/// This operation also accepts non-canonical byte representations in prime fields
/// by applying the relevant modular reduction.
///
/// # Errors
///
/// Errors if the input is not a supported type, or if the number of bytes does
/// not match the byte representation length of the type.
pub fn from_bytes_incircuit(
    std_lib: &ZkStdLib,
    layouter: &mut impl Layouter<F>,
    val_t: &IrType,
    bytes: &[AssignedByte<F>],
) -> Result<CircuitValue, plonk::Error> {
    use CircuitValue::*;

    check_byte_repr_len(val_t, bytes.len())
        .map_err(|e| plonk::Error::Synthesis(format!("from_bytes: {e}")))?;

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

        IrType::Curve25519Scalar => std_lib
            .curve25519()
            .scalar_field_chip()
            .assigned_from_le_bytes(layouter, bytes)
            .map(Curve25519Scalar),

        _ => Err(plonk::Error::Synthesis(format!(
            "Unsupported from_bytes for {val_t:?}",
        ))),
    }
}

/// Checks that `len` matches the byte representation length of `val_t`.
/// Types without a byte representation pass this check; they are rejected by
/// the type dispatch of the caller.
fn check_byte_repr_len(val_t: &IrType, len: usize) -> Result<(), String> {
    match val_t.byte_repr_len() {
        Some(expected) if expected as usize != len => Err(format!(
            "{val_t:?} expects Bytes<{expected}>, got Bytes<{len}>"
        )),
        _ => Ok(()),
    }
}

/// Fixed 32-byte wrappers around [`from_bytes_offcircuit`] and
/// [`from_bytes_incircuit`], preserving the pre-generalization API.
pub mod from_bytes32 {
    use super::*;

    /// Builds (off-circuit) a value of the given type from its 32-byte representation.
    /// Supported for types:
    ///  - Native
    ///  - Secp256k1Base
    ///  - Secp256k1Scalar
    ///
    /// In all the above prime fields, the 32-byte representation is the little-endian
    /// byte encoding of the underlying (canonical) integer.
    ///
    /// This operation also accepts non-canonical 32-byte representation in prime fields
    /// by applying the relevant modular reduction.
    ///
    /// **Deprecated:** use [`from_bytes_offcircuit`] instead, which supports
    /// arbitrary byte representation lengths.
    ///
    /// # Errors
    ///
    /// Errors if the input is not a supported type.
    pub fn from_bytes32_offcircuit(
        val_t: &IrType,
        bytes: &[u8; 32],
    ) -> Result<IrValue, anyhow::Error> {
        from_bytes_offcircuit(val_t, bytes)
    }

    /// Builds (in-circuit) a value of the given type from its 32-byte representation.
    /// Supported for types:
    ///  - Native
    ///  - Secp256k1Base
    ///  - Secp256k1Scalar
    ///
    /// In all the above prime fields, the 32-byte representation is the little-endian
    /// byte encoding of the underlying (canonical) integer.
    ///
    /// This operation also accepts non-canonical 32-byte representation in prime fields
    /// by applying the relevant modular reduction.
    ///
    /// **Deprecated:** use [`from_bytes_incircuit`] instead, which supports
    /// arbitrary byte representation lengths.
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
        from_bytes_incircuit(std_lib, layouter, val_t, bytes)
    }
}

/// Builds a prime field element from the given bytes by interpreting them
/// in little-endian as an integer. The integer can be bigger than field order.
pub(crate) fn from_le_bytes_with_reduction<F: CircuitField>(bytes: &[u8]) -> F {
    let (_, rem) = BigUint::from_bytes_le(bytes).div_rem_euclid(&F::modulus());
    let mut rem_bytes = rem.to_bytes_le();
    rem_bytes.resize(F::NUM_BYTES, 0);
    F::from_bytes_le(&rem_bytes).unwrap()
}

#[cfg(test)]
mod tests {
    use group::ff::Field;
    use midnight_curves::{curve25519, k256};
    use rand_chacha::rand_core::OsRng;
    use transient_crypto::curve::Fr;

    use super::*;
    use crate::ir_instructions::to_bytes::to_bytes_offcircuit;

    // Starts from a random value, converts it into bytes (so as to obtain a
    // valid, canonical byte representation), then goes from those bytes
    // back into a value and into bytes again, checking that the
    // re-serialized bytes match the ones we started from.
    #[test]
    fn test_from_bytes_roundtrip() {
        use IrValue::*;

        let to_vec = |v: IrValue| -> Vec<u8> { <Vec<u8>>::try_from(v).unwrap() };

        for x in [
            Native(Fr(F::random(OsRng))),
            Secp256k1Base(k256::Fp::random(OsRng)),
            Secp256k1Scalar(k256::Fq::random(OsRng)),
            // Nb. dalek's inherent `Scalar::random` (which shadows
            // `ff::Field::random`) takes the rng by mutable reference.
            Curve25519Scalar(curve25519::Scalar::random(&mut OsRng)),
        ] {
            let val_t = x.get_type();
            let bytes = to_vec(to_bytes_offcircuit(&x).unwrap());
            assert_eq!(bytes.len(), val_t.byte_repr_len().unwrap() as usize);
            let y = from_bytes_offcircuit(&val_t, &bytes).unwrap();
            let bytes2 = to_vec(to_bytes_offcircuit(&y).unwrap());
            assert_eq!(bytes2, bytes, "{val_t:?}");
        }
    }

    // Non-canonical (out-of-range) bytes are accepted and reduced modulo
    // each field's characteristic, rather than rejected.
    #[test]
    fn test_from_bytes_reduces_non_canonical_input() {
        let bytes = [0xffu8; 32];

        assert_eq!(
            from_bytes_offcircuit(&IrType::Native, &bytes).unwrap(),
            IrValue::Native(Fr(from_le_bytes_with_reduction(&bytes)))
        );
        assert_eq!(
            from_bytes_offcircuit(&IrType::Secp256k1Base, &bytes).unwrap(),
            IrValue::Secp256k1Base(from_le_bytes_with_reduction(&bytes))
        );
        assert_eq!(
            from_bytes_offcircuit(&IrType::Secp256k1Scalar, &bytes).unwrap(),
            IrValue::Secp256k1Scalar(from_le_bytes_with_reduction(&bytes))
        );

        // Curve25519 scalars are built from 64 bytes (e.g. a SHA-512 digest,
        // as needed by ed25519), reduced modulo the group order.
        let wide = [0xffu8; 64];
        assert_eq!(
            from_bytes_offcircuit(&IrType::Curve25519Scalar, &wide).unwrap(),
            IrValue::Curve25519Scalar(from_le_bytes_with_reduction(&wide))
        );
        assert_eq!(
            from_bytes_offcircuit(&IrType::Curve25519Scalar, &wide).unwrap(),
            IrValue::Curve25519Scalar(curve25519::Scalar::from_bytes_mod_order_wide(&wide))
        );
    }

    // The 32-byte wrappers behave identically to the generic functions for
    // all the types they historically supported, and reject Curve25519Scalar,
    // which needs 64 bytes.
    #[test]
    fn test_from_bytes32_wrapper() {
        use from_bytes32::from_bytes32_offcircuit;

        let bytes = [0xffu8; 32];
        for val_t in [
            IrType::Native,
            IrType::Secp256k1Base,
            IrType::Secp256k1Scalar,
        ] {
            assert_eq!(
                from_bytes32_offcircuit(&val_t, &bytes).unwrap(),
                from_bytes_offcircuit(&val_t, &bytes).unwrap()
            );
        }
        assert!(from_bytes32_offcircuit(&IrType::Curve25519Scalar, &bytes).is_err());
    }

    // The number of input bytes must match the byte representation length of
    // the target type exactly.
    #[test]
    fn test_from_bytes_rejects_wrong_length() {
        assert!(from_bytes_offcircuit(&IrType::Native, &[0u8; 64]).is_err());
        assert!(from_bytes_offcircuit(&IrType::Secp256k1Base, &[0u8; 31]).is_err());
        assert!(from_bytes_offcircuit(&IrType::Curve25519Scalar, &[0u8; 32]).is_err());
    }
}
