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
    field::foreign::params::MultiEmulationParams as MEP,
    instructions::{DecompositionInstructions, PublicInputInstructions, ZeroInstructions},
    types::{
        AssignedBigUint, AssignedBit, AssignedByte, AssignedField, AssignedForeignPoint,
        AssignedNative, AssignedNativePoint, AssignedScalarOfNativeCurve, Instantiable,
    },
};
use midnight_curves::{Fr as JubjubFr, JubjubExtended, k256};
use midnight_proofs::{circuit::Layouter, plonk::Error};
use midnight_zk_stdlib::ZkStdLib;
use transient_crypto::curve::Fr;

use crate::{
    ir_instructions::F,
    ir_types::{BYTES_PER_FIELD_ELEMENT, CircuitValue, IrType, IrValue},
};
use anyhow::anyhow;

/// Encodes the given off-circuit value as a vector of IrValue::Native.
pub fn encode_offcircuit(value: &IrValue) -> Vec<IrValue> {
    let encoded = match value {
        IrValue::Native(x) => AssignedNative::<F>::as_public_input(&x.0),
        IrValue::Bool(b) => AssignedBit::<F>::as_public_input(b),
        IrValue::Byte(b) => AssignedByte::<F>::as_public_input(b),
        IrValue::Bytes(bs) => bs
            .chunks(BYTES_PER_FIELD_ELEMENT)
            .map(|chunk| {
                // Pack up to `BYTES_PER_FIELD_ELEMENT` little-endian bytes into
                // one field element.
                let mut buf = [0u8; 32];
                buf[..chunk.len()].copy_from_slice(chunk);
                F::from_bytes_le(&buf).expect("a 31-byte value always fits in F")
            })
            .collect(),
        IrValue::JubjubPoint(p) => AssignedNativePoint::<JubjubExtended>::as_public_input(p),
        // A Jubjub scalar is at most 252 bits wide, so it takes a single native
        // field element, as `IrType::JubjubScalar.encoded_len()` declares. The
        // `encoded_width_matches_encoded_len` test pins that down: were it to
        // change upstream, every caller reports it (an `Encode` output-length
        // mismatch, a failed canonicity check, or a communications commitment
        // mismatch) rather than panicking here.
        IrValue::JubjubScalar(s) => {
            AssignedScalarOfNativeCurve::<JubjubExtended>::as_public_input(s)
        }

        IrValue::Secp256k1Point(p) => {
            AssignedForeignPoint::<F, k256::K256, MEP>::as_public_input(p)
        }
        IrValue::Secp256k1Base(s) => AssignedField::<F, k256::Fp, MEP>::as_public_input(s),
        IrValue::Secp256k1Scalar(s) => AssignedField::<F, k256::Fq, MEP>::as_public_input(s),
    };
    encoded
        .into_iter()
        .map(|s| IrValue::Native(Fr(s)))
        .collect()
}

/// Encodes the given in-circuit value as a vector of CircuitValue::Native.
pub fn encode_incircuit(
    std_lib: &ZkStdLib,
    layouter: &mut impl Layouter<F>,
    value: &CircuitValue,
) -> Result<Vec<CircuitValue>, Error> {
    let encoded = match value {
        CircuitValue::Native(x) => std_lib.as_public_input(layouter, x),
        CircuitValue::Bool(b) => std_lib.as_public_input(layouter, b),
        CircuitValue::Byte(b) => std_lib.as_public_input(layouter, b),
        CircuitValue::Bytes(bs) => bs
            .chunks(BYTES_PER_FIELD_ELEMENT)
            .map(|chunk| std_lib.assigned_from_le_bytes(layouter, chunk))
            .collect::<Result<Vec<_>, _>>(),
        CircuitValue::JubjubPoint(p) => std_lib.jubjub().as_public_input(layouter, p),
        CircuitValue::JubjubScalar(s) => {
            // Jubjub::Scalar::NUM_BITS is incorrectly set to 255 (instead of 252)
            // in midnight-curves v0.2.0. Consequently, Jubjub scalars may be encoded
            // unnecessarily as 2 native field values instead of one.
            // We return the first only and make sure the rest (supposedly one more)
            // are zero.
            let encoded = std_lib.jubjub().as_public_input(layouter, s)?;
            for x in encoded[1..].iter() {
                std_lib.assert_zero(layouter, x)?;
            }
            Ok(encoded[..1].to_vec())
        }

        CircuitValue::Secp256k1Point(p) => std_lib.secp256k1().as_public_input(layouter, p),
        CircuitValue::Secp256k1Base(s) => {
            (std_lib.secp256k1().base_field_chip()).as_public_input(layouter, s)
        }
        CircuitValue::Secp256k1Scalar(s) => {
            (std_lib.secp256k1().scalar_field_chip()).as_public_input(layouter, s)
        }
    }?;
    Ok(encoded.into_iter().map(CircuitValue::Native).collect())
}

/// Decodes `encoded` (field elements packed by `encode_offcircuit`) into an
/// `n`-byte `Bytes` value. Returns `None` if the number of field elements is
/// wrong for `n`, or if any chunk carries non-canonical (non-zero) high bytes.
fn decode_bytes(encoded: &[F], n: usize) -> Option<IrValue> {
    if encoded.len() != n.div_ceil(BYTES_PER_FIELD_ELEMENT) {
        return None;
    }
    let mut bytes = Vec::with_capacity(n);
    for (i, f) in encoded.iter().enumerate() {
        let buf = f.to_bytes_le();
        // Bytes contributed by this chunk: `BYTES_PER_FIELD_ELEMENT`, or fewer
        // for the final chunk when `n` is not a multiple.
        let chunk_len = (n - i * BYTES_PER_FIELD_ELEMENT).min(BYTES_PER_FIELD_ELEMENT);
        // The remaining high bytes must be zero (canonical form).
        if buf[chunk_len..].iter().any(|b| *b != 0) {
            return None;
        }
        bytes.extend_from_slice(&buf[..chunk_len]);
    }
    Some(IrValue::Bytes(bytes))
}

/// Decodes the given Fr values as an IrValue of the given type.
///
/// # Errors
///
/// Returns an error if the provided raw values cannot be decoded as the given type.
pub fn decode_offcircuit(encoded: &[Fr], val_t: &IrType) -> Result<IrValue, anyhow::Error> {
    let encoded: Vec<F> = encoded.iter().map(|f| f.0).collect();
    let decoded = match val_t {
        IrType::Native => AssignedNative::<F>::from_public_input(&encoded)
            .map(Fr)
            .map(IrValue::Native),

        IrType::Bool => AssignedBit::<F>::from_public_input(&encoded).map(IrValue::Bool),

        IrType::Byte => AssignedByte::<F>::from_public_input(&encoded).map(IrValue::Byte),

        IrType::Bytes(n) => decode_bytes(&encoded, *n as usize),

        IrType::JubjubPoint => AssignedNativePoint::<JubjubExtended>::from_public_input(&encoded)
            .map(IrValue::JubjubPoint),

        IrType::JubjubScalar => {
            AssignedScalarOfNativeCurve::<JubjubExtended>::from_public_input(&encoded)
                .map(IrValue::JubjubScalar)
        }

        IrType::Secp256k1Point => {
            AssignedForeignPoint::<F, k256::K256, MEP>::from_public_input(&encoded)
                .map(IrValue::Secp256k1Point)
        }

        IrType::Secp256k1Base => AssignedField::<F, k256::Fp, MEP>::from_public_input(&encoded)
            .map(IrValue::Secp256k1Base),

        IrType::Secp256k1Scalar => AssignedField::<F, k256::Fq, MEP>::from_public_input(&encoded)
            .map(IrValue::Secp256k1Scalar),
    }
    .ok_or_else(|| anyhow!("Failed to decode {encoded:?} as {val_t:?}"))?;

    // We make sure that the encoded value was in canonical form by re-encoding
    // it and comparing it with the given input.
    let re_encoded: Vec<F> = encode_offcircuit(&decoded)
        .into_iter()
        .map(|x| x.try_into().unwrap())
        .map(|x: Fr| x.0)
        .collect();

    if re_encoded != encoded {
        return Err(anyhow!(
            "The encoded value of type {val_t:?} is not in canonical form: {encoded:?}"
        ));
    }

    Ok(decoded)
}

/// Converts a native field element to a Jubjub scalar by reducing modulo
/// the Jubjub scalar field order if necessary.
pub fn native_to_jubjub_scalar(native: &Fr) -> JubjubFr {
    let mut bytes = [0u8; 64];
    bytes[..32].copy_from_slice(&native.0.to_bytes_le());
    JubjubFr::from_bytes_wide(&bytes)
}

/// Reduces the given biguint modulo the Jubjub scalar field order and returns the
/// result as an `AssignedScalarOfNativeCurve<JubjubExtended>` of exactly 252 bits.
pub fn jubjub_scalar_from_biguint(
    std_lib: &ZkStdLib,
    layouter: &mut impl Layouter<F>,
    x: AssignedBigUint<F>,
) -> Result<AssignedScalarOfNativeCurve<JubjubExtended>, Error> {
    let jubjub_order = {
        let p = JubjubFr::modulus();
        std_lib.biguint().assign_fixed_biguint(layouter, p)?
    };
    let (_q, r) = std_lib.biguint().div_rem(layouter, &x, &jubjub_order)?;

    let r_le_bytes = std_lib.biguint().to_le_bytes(layouter, &r)?;
    std_lib.jubjub().scalar_from_le_bytes(layouter, &r_le_bytes)
}

#[cfg(test)]
mod tests {
    use group::{Group, ff::Field};
    use midnight_curves::{JubjubSubgroup, k256::K256};
    use rand_chacha::rand_core::OsRng;

    use super::*;

    /// The off-circuit encoding of `value`, as raw field elements.
    fn raw(value: &IrValue) -> Vec<Fr> {
        encode_offcircuit(value)
            .into_iter()
            .map(|v| v.try_into().unwrap())
            .collect()
    }

    #[test]
    fn encode_decode_bool_roundtrip() {
        for b in [true, false] {
            let value = IrValue::Bool(b);
            let encoded: Vec<Fr> = raw(&value);
            // A Bool encodes to a single native field element.
            assert_eq!(encoded.len(), IrType::Bool.encoded_len());
            let decoded = decode_offcircuit(&encoded, &IrType::Bool).unwrap();
            assert_eq!(decoded, value);
        }
    }

    #[test]
    fn encode_decode_byte_roundtrip() {
        for b in [0u8, 1, 42, 255] {
            let value = IrValue::Byte(b);
            let encoded: Vec<Fr> = raw(&value);
            // A Byte encodes to a single native field element.
            assert_eq!(encoded.len(), IrType::Byte.encoded_len());
            let decoded = decode_offcircuit(&encoded, &IrType::Byte).unwrap();
            assert_eq!(decoded, value);
        }
    }

    #[test]
    fn encode_decode_bytes_roundtrip() {
        // Cover lengths straddling the field-element chunk boundary.
        let c = BYTES_PER_FIELD_ELEMENT;
        for n in [1, c - 1, c, c + 1, 2 * c, 2 * c + 1, 100] {
            let bytes: Vec<u8> = (0..n)
                .map(|i| (i as u8).wrapping_mul(7).wrapping_add(1))
                .collect();
            let value = IrValue::Bytes(bytes);
            let encoded: Vec<Fr> = raw(&value);
            assert_eq!(encoded.len(), IrType::Bytes(n as u32).encoded_len());
            let decoded = decode_offcircuit(&encoded, &IrType::Bytes(n as u32)).unwrap();
            assert_eq!(decoded, value, "n = {n}");
        }
    }

    #[test]
    fn decode_bytes_wrong_element_count_fails() {
        // Bytes(32) needs exactly 2 field elements.
        assert!(decode_offcircuit(&[Fr::from(0u64)], &IrType::Bytes(32)).is_err());
    }

    // `IrType::encoded_len` hardcodes the width of every encoding, and the
    // transcripts are sliced according to it, so a value must encode to exactly
    // that many field elements. For most types the width comes from a
    // `midnight-circuits` chip, so this also guards against a dependency bump
    // silently changing one of them.
    #[test]
    fn encoded_width_matches_encoded_len() {
        // The widest value of each type: the largest field elements, and (for
        // `Bytes`) a length that is not a multiple of the chunk size.
        let values = [
            IrValue::Native(Fr(-F::ONE)),
            IrValue::Bool(true),
            IrValue::Byte(u8::MAX),
            IrValue::Bytes(vec![u8::MAX; BYTES_PER_FIELD_ELEMENT + 1]),
            IrValue::JubjubPoint(JubjubSubgroup::generator()),
            IrValue::JubjubScalar(-JubjubFr::ONE),
            IrValue::Secp256k1Point(K256::generator()),
            IrValue::Secp256k1Base(-k256::Fp::ONE),
            IrValue::Secp256k1Scalar(-k256::Fq::ONE),
        ];
        for value in values {
            let val_t = value.get_type();
            assert_eq!(raw(&value).len(), val_t.encoded_len(), "{val_t:?}");
        }
    }

    // The curve and emulated-field encodings are the ones where `decode` is not
    // injective on its own, so make sure the canonical encodings still decode
    // back to the value they came from (including the edge values: identity,
    // zero, and the largest field element).
    #[test]
    fn encode_decode_curve_types_roundtrip() {
        let values = [
            IrValue::JubjubPoint(JubjubSubgroup::identity()),
            IrValue::JubjubPoint(JubjubSubgroup::generator()),
            IrValue::JubjubPoint(JubjubSubgroup::random(OsRng)),
            IrValue::JubjubScalar(JubjubFr::ZERO),
            IrValue::JubjubScalar(-JubjubFr::ONE),
            IrValue::JubjubScalar(JubjubFr::random(OsRng)),
            IrValue::Secp256k1Point(K256::identity()),
            IrValue::Secp256k1Point(K256::generator()),
            IrValue::Secp256k1Point(K256::random(OsRng)),
            IrValue::Secp256k1Base(k256::Fp::ZERO),
            IrValue::Secp256k1Base(-k256::Fp::ONE),
            IrValue::Secp256k1Base(k256::Fp::random(OsRng)),
            IrValue::Secp256k1Scalar(k256::Fq::ZERO),
            IrValue::Secp256k1Scalar(-k256::Fq::ONE),
            IrValue::Secp256k1Scalar(k256::Fq::random(OsRng)),
        ];
        for value in values {
            let val_t = value.get_type();
            let encoded = raw(&value);
            assert_eq!(encoded.len(), val_t.encoded_len(), "{val_t:?}");
            let decoded = decode_offcircuit(&encoded, &val_t).unwrap();
            assert_eq!(decoded, value, "{val_t:?}");
        }
    }

    // `JubjubExtended::from_xy` recovers the point from `y` and the parity bit of
    // `x`, so tampering with the rest of `x` used to decode to the same point.
    #[test]
    fn decode_rejects_non_canonical_jubjub_point() {
        let value = IrValue::JubjubPoint(JubjubSubgroup::generator());
        let encoded = raw(&value);
        // `+ 2` leaves the parity of `x` (and hence the decoded point) untouched.
        let tampered = vec![Fr(encoded[0].0 + F::from(2u64)), encoded[1]];
        assert_ne!(tampered, encoded);
        assert!(decode_offcircuit(&tampered, &IrType::JubjubPoint).is_err());
    }

    // A Jubjub scalar is encoded in the low 252 bits of a single field element,
    // and the two bits above them used to be ignored on decoding.
    #[test]
    fn decode_rejects_non_canonical_jubjub_scalar() {
        let value = IrValue::JubjubScalar(JubjubFr::from(12345u64));
        let encoded = raw(&value);
        let two_pow_252 = {
            let mut buf = [0u8; 32];
            buf[31] = 0x10;
            F::from_bytes_le(&buf).unwrap()
        };
        let tampered = vec![Fr(encoded[0].0 + two_pow_252)];
        assert!(decode_offcircuit(&tampered, &IrType::JubjubScalar).is_err());
    }

    // An emulated secp256k1 field element is packed into 4 64-bit limbs, i.e.
    // 256 bits, which is wider than either modulus. The limb representation of
    // `(k - 1) + q` therefore used to decode to `k` as well.
    #[test]
    fn decode_rejects_non_canonical_secp256k1_scalar() {
        // (7 - 1) + q, as little-endian 64-bit limbs.
        let limbs: [u64; 4] = [
            0xBFD2_5E8C_D036_4141 + 6,
            0xBAAE_DCE6_AF48_A03B,
            0xFFFF_FFFF_FFFF_FFFE,
            0xFFFF_FFFF_FFFF_FFFF,
        ];
        // The first field element carries three limbs, the second the last one.
        let mut buf = [0u8; 32];
        for (i, limb) in limbs[..3].iter().enumerate() {
            buf[8 * i..8 * (i + 1)].copy_from_slice(&limb.to_le_bytes());
        }
        let tampered = vec![Fr(F::from_bytes_le(&buf).unwrap()), Fr(F::from(limbs[3]))];
        assert_ne!(
            tampered,
            raw(&IrValue::Secp256k1Scalar(k256::Fq::from(7u64)))
        );
        assert!(decode_offcircuit(&tampered, &IrType::Secp256k1Scalar).is_err());
    }

    // A secp256k1 point carries an "is identity" flag as its last field element,
    // and the coordinates used to be ignored altogether when that flag is set.
    #[test]
    fn decode_rejects_non_canonical_secp256k1_identity() {
        let value = IrValue::Secp256k1Point(K256::identity());
        let encoded = raw(&value);
        assert_eq!(*encoded.last().unwrap(), Fr::from(1u64));
        // Garbage coordinates, identity flag still set.
        let mut tampered = encoded.clone();
        tampered[0] = Fr(encoded[0].0 + F::ONE);
        assert!(decode_offcircuit(&tampered, &IrType::Secp256k1Point).is_err());
    }
}
