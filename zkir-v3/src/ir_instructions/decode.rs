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
use group::ff::PrimeField;
use midnight_circuits::{ecc::curves::CircuitCurve, types::AssignedNative};
use midnight_curves::{Fr as JubjubFr, JubjubExtended, secp256k1};
use midnight_proofs::circuit::Value;
use midnight_proofs::{circuit::Layouter, plonk};
use midnight_zk_stdlib::ZkStdLib;
use num_bigint::BigUint;
use num_traits::{One, Zero};
use transient_crypto::curve::Fr;

use crate::ir_instructions::assign::assign_incircuit;
use crate::ir_instructions::constrain_eq::constrain_eq_incircuit;
use crate::ir_instructions::encode::encode_incircuit;
use crate::{
    ir_instructions::F,
    ir_types::{CircuitValue, IrType, IrValue},
};

/// Decodes the given Fr values as an IrValue value of the given type.
///
/// # Errors
///
/// This function returns an error if the provided raw values cannot be
/// decoded as the given type.
pub fn decode_offcircuit(encoded: &[Fr], val_t: &IrType) -> Result<IrValue, anyhow::Error> {
    match val_t {
        IrType::Native => match encoded {
            [x] => Ok(IrValue::Native(*x)),
            _ => Err(anyhow::Error::msg(
                "Expected exactly one value for Native decoding",
            )),
        },
        IrType::JubjubPoint => match encoded {
            [x, y] => {
                let p = JubjubExtended::from_xy(x.0, y.0).ok_or_else(|| {
                    anyhow::Error::msg("Failed to decode Jubjub point from coordinates")
                })?;
                Ok(IrValue::JubjubPoint(p.into_subgroup()))
            }
            _ => Err(anyhow::Error::msg(
                "Expected exactly two values for JubjubPoint decoding",
            )),
        },
        IrType::JubjubScalar => match encoded {
            [x] => Ok(IrValue::JubjubScalar(native_to_jubjub_scalar(x))),
            _ => Err(anyhow::Error::msg(
                "Expected exactly one value for JubjubScalar decoding",
            )),
        },

        IrType::Secp256k1Point => match encoded {
            [x1, x2, x3, x4, y1, y2, y3, y4] => {
                // This replicates the encoding of foreign field Weierstrass points.
                // See: https://github.com/midnightntwrk/midnight-zk/blob/zk-stdlib-v1/circuits/src/ecc/foreign/ecc_chip.rs#L188
                if x1.as_le_bytes()[8] != 0 {
                    return Ok(IrValue::Secp256k1Point(secp256k1::Secp256k1::identity()));
                }

                let x = decode_field::<secp256k1::Fp, 64>(&[*x1, *x2, *x3, *x4])?;
                let y = decode_field::<secp256k1::Fp, 64>(&[*y1, *y2, *y3, *y4])?;
                let p = secp256k1::Secp256k1::from_xy(x, y).ok_or_else(|| {
                    anyhow::Error::msg("Failed to decode Secp256k1 point from coordinates")
                })?;
                Ok(IrValue::Secp256k1Point(p))
            }
            _ => Err(anyhow::Error::msg(
                "Expected exactly 8 values for Secp256k1Point decoding",
            )),
        },
        IrType::Secp256k1Base => match encoded {
            [s1, s2, s3, s4] => {
                let s = decode_field::<secp256k1::Fp, 64>(&[*s1, *s2, *s3, *s4])?;
                Ok(IrValue::Secp256k1Base(s))
            }
            _ => Err(anyhow::Error::msg(
                "Expected exactly 4 values for Secp256k1Base decoding",
            )),
        },
        IrType::Secp256k1Scalar => match encoded {
            [s1, s2, s3, s4] => {
                let s = decode_field::<secp256k1::Fq, 64>(&[*s1, *s2, *s3, *s4])?;
                Ok(IrValue::Secp256k1Scalar(s))
            }
            _ => Err(anyhow::Error::msg(
                "Expected exactly 4 values for Secp256k1Scalar decoding",
            )),
        },
    }
}

/// Decodes the given in-circuit `Native` values as CircuitValue value of the
/// given type.
///
/// # Errors
///
/// This function returns an error if the provided raw values cannot be
/// decoded as the given type.
pub fn decode_incircuit(
    std_lib: &ZkStdLib,
    layouter: &mut impl Layouter<F>,
    encoded: &[AssignedNative<F>],
    val_t: &IrType,
) -> Result<CircuitValue, plonk::Error> {
    // We witness the decoded value, encode it in-circuit and constraint the output of such
    // encoding to be equal to the `encoded` inputs to this function.
    // This guarantees that `decode` is the inverse of `encode` on the image of `encode`,
    // and that `decode` leads to an unsatisfiable circuit if given a vector of scalars that
    // is not in the image of `encode`.

    let encoded_val: Value<Vec<F>> = Value::from_iter(encoded.iter().map(|x| x.value().copied()));
    let decoded_val = encoded_val
        .map_with_result(|v| {
            decode_offcircuit(&v.iter().map(|f| Fr(*f)).collect::<Vec<_>>(), val_t)
        })
        .map_err(|e| plonk::Error::Synthesis(format!("{e:?}")))?;
    let decoded = assign_incircuit(std_lib, layouter, val_t, &[decoded_val])?[0].clone();

    let decoded_encoded = encode_incircuit(std_lib, layouter, &decoded)?;
    if decoded_encoded.len() != encoded.len() {
        return Err(plonk::Error::Synthesis(format!(
            "Cannot decode {} elements as {val_t:?}",
            encoded.len()
        )));
    }

    for (x, expected) in decoded_encoded.iter().zip(encoded) {
        constrain_eq_incircuit(
            std_lib,
            layouter,
            x,
            &CircuitValue::Native(expected.clone()),
        )?;
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

/// Decodes a vector of raw native field elements as an element of field `K`
/// by following the foreign-field encoding/decoding methodology used in `midnight-zk`,
/// which depends on `LOG2_BASE` and `NUM_LIMBS`.
///
/// See https://github.com/midnightntwrk/midnight-zk/blob/zk-stdlib-v1/circuits/src/field/foreign/field_chip.rs#L472
/// for details on how `K` elements are encoded.
pub fn decode_field<K: PrimeField, const LOG2_BASE: u32>(limbs: &[Fr]) -> Result<K, anyhow::Error> {
    let base = BigUint::from(2u64).pow(LOG2_BASE);
    let limbs_as_bi = limbs
        .iter()
        .map(|x| BigUint::from_bytes_le(x.0.to_repr().as_ref()))
        .collect::<Vec<_>>();
    let element_as_bi = bi_from_limbs(&base, &limbs_as_bi) + BigUint::one();
    let u64_chunks = element_as_bi.to_u64_digits();
    Ok(from_u64_le_digits::<K>(&u64_chunks))
}

/// Returns the BigUint represented by the given `limbs`, parsing them
/// in the given `base`, in little-endian.
///
/// NB: This function is borrowed from `midnight-zk` (it is not exposed there).
pub fn bi_from_limbs(base: &BigUint, limbs: &[BigUint]) -> BigUint {
    limbs
        .iter()
        .rev()
        .fold(BigUint::zero(), |acc, limb| acc * base + limb)
}

/// NB: This function is borrowed from `midnight-zk` (it is not exposed there).
fn from_u64_le_digits<F: PrimeField>(digits: &[u64]) -> F {
    if digits.is_empty() {
        return F::ZERO;
    }

    let mut acc = F::from(*digits.last().unwrap());
    for digit in digits.iter().rev().skip(1) {
        for _ in 0..64 {
            acc = acc.double();
        }
        acc += F::from(*digit)
    }
    acc
}
