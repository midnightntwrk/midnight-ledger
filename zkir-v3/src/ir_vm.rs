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

use crate::ir_instructions::add::{add_incircuit, add_offcircuit};
use crate::ir_instructions::assign::assign_incircuit;
use crate::ir_instructions::constrain_eq::{constrain_eq_incircuit, constrain_eq_offcircuit};
use crate::ir_instructions::ec_mul::{ec_mul_incircuit, ec_mul_offcircuit};
use crate::ir_instructions::encode::{
    decode_offcircuit, encode_incircuit, encode_offcircuit, jubjub_scalar_from_biguint,
    native_to_jubjub_scalar,
};
use crate::ir_instructions::eq::{test_eq_incircuit, test_eq_offcircuit};
use crate::ir_instructions::from_bytes32::{from_bytes32_incircuit, from_bytes32_offcircuit};
use crate::ir_instructions::from_coordinates::{
    from_coordinates_incircuit, from_coordinates_offcircuit,
};
use crate::ir_instructions::into_bytes32::{into_bytes32_incircuit, into_bytes32_offcircuit};
use crate::ir_instructions::into_coordinates::{
    into_coordinates_incircuit, into_coordinates_offcircuit,
};
use crate::ir_instructions::inv::{inv_incircuit, inv_offcircuit};
use crate::ir_instructions::mul::{mul_incircuit, mul_offcircuit};
use crate::ir_instructions::neg::{neg_incircuit, neg_offcircuit};
use crate::ir_instructions::select::{select_incircuit, select_offcircuit};
use crate::ir_types::{CircuitValue, IrType, IrValue};

use super::ir::{Identifier, Instruction as I, IrSource, Operand};
use anyhow::{anyhow, bail};
use base_crypto::fab::{Alignment, AlignmentAtom, AlignmentSegment};
use base_crypto::repr::BinaryHashRepr;
use group::Group;
use midnight_circuits::instructions::{
    ArithInstructions, AssertionInstructions, AssignmentInstructions, BinaryInstructions,
    ControlFlowInstructions, ConversionInstructions, DecompositionInstructions,
    PublicInputInstructions, RangeCheckInstructions, ZeroInstructions,
};
use midnight_circuits::types::{AssignedBit, AssignedByte, AssignedNative, InnerValue};
use midnight_curves::{JubjubSubgroup, k256};
use midnight_proofs::{
    circuit::{Layouter, Value},
    plonk::Error,
};
use midnight_zk_stdlib::{Relation, ZkStdLib, ZkStdLibArch};
use num_bigint::BigUint;
use serialize::{Deserializable, Serializable, VecExt, tagged_deserialize, tagged_serialize};
use sha2::Sha256;
use sha3::{Digest, Keccak256};
use std::cmp::Ordering;
use std::collections::HashMap;
use transient_crypto::curve::outer;
use transient_crypto::curve::{FR_BITS, FR_BYTES_STORED, Fr};
use transient_crypto::fab::{AlignmentExt, ValueReprAlignedValue};
use transient_crypto::hash::{hash_to_curve, transient_commit, transient_hash};
use transient_crypto::proofs::{ProofPreimage, ProvingError};

/// The raw data prior to proving. Note that this should *not* be considered part of the public
/// API, and is subject to change at any time. It may be used in combination with
/// [`IrSource::prove_unchecked`] to test malicious prover behavior.
#[derive(Clone, Debug)]
#[allow(missing_docs)]
pub struct Preprocessed {
    pub memory: HashMap<Identifier, IrValue>,
    pub pis: Vec<outer::Scalar>,
    pub pi_skips: Vec<Option<usize>>,
    pub binding_input: outer::Scalar,
    pub comm_comm: Option<(outer::Scalar, outer::Scalar)>,
}

fn fab_decode_to_bytes(
    std: &ZkStdLib,
    layouter: &mut impl Layouter<outer::Scalar>,
    align: &Alignment,
    mut inputs: &[AssignedNative<outer::Scalar>],
) -> Result<Vec<AssignedByte<outer::Scalar>>, Error> {
    let mut res = Vec::with_bounded_capacity(align.bin_len());
    let _ = fab_decode_to_bytes_inner(std, layouter, align, &mut inputs, &mut res)?;
    let mut debug_values: Vec<u8> = Vec::new();
    for value in res.iter() {
        value.value().assert_if_known(|v| {
            debug_values.push(*v);
            true
        })
    }
    if !debug_values.is_empty() {
        trace!(bytes = ?debug_values, len = debug_values.len(), "bytes decoded in-circuit");
    }
    Ok(res)
}

fn fab_decode_to_bytes_inner(
    std: &ZkStdLib,
    layouter: &mut impl Layouter<outer::Scalar>,
    align: &Alignment,
    inputs: &mut &[AssignedNative<outer::Scalar>],
    res: &mut Vec<AssignedByte<outer::Scalar>>,
) -> Result<AssignedNative<outer::Scalar>, Error> {
    let mut acc = std.assign_fixed(layouter, 0.into())?;
    for segment in align.0.iter() {
        match segment {
            AlignmentSegment::Atom(atom) => {
                fab_decode_to_bytes_atom(std, layouter, atom, inputs, res)?;
                acc = std.add_constant(layouter, &acc, 1.into())?;
            }
            AlignmentSegment::Option(_) => {
                error!("in-circuit decoding of alignment options is not yet implemented!");
                return Err(Error::Synthesis(
                    "in-circuit decoding of alignment options is not yet implemented!".into(),
                ));
            }
        }
    }
    Ok(acc)
}

fn fab_decode_to_bytes_atom(
    std: &ZkStdLib,
    layouter: &mut impl Layouter<outer::Scalar>,
    align: &AlignmentAtom,
    inputs: &mut &[AssignedNative<outer::Scalar>],
    res: &mut Vec<AssignedByte<outer::Scalar>>,
) -> Result<(), Error> {
    match align {
        AlignmentAtom::Field => {
            if inputs.is_empty() {
                return Err(Error::Synthesis(
                    "cannot decode field element from no data".into(),
                ));
            }
            let value = &inputs[0];
            *inputs = &inputs[1..];
            res.extend(std.assigned_to_le_bytes(layouter, value, None)?);
            Ok(())
        }
        AlignmentAtom::Bytes { length } => {
            let stray = *length as usize % FR_BYTES_STORED;
            let chunks = *length as usize / FR_BYTES_STORED;
            let expected_size = chunks + (stray != 0) as usize;
            let mut bytes_from =
                |slice: &mut Vec<AssignedByte<outer::Scalar>>,
                 k,
                 f: AssignedNative<outer::Scalar>| {
                    let repr = std.assigned_to_le_bytes(layouter, &f, Some(k))?;
                    slice.extend(repr[..k].iter().cloned());
                    Ok::<_, Error>(())
                };
            if inputs.len() < expected_size {
                return Err(Error::Synthesis(
                    "cannot decode bytes from to little data".into(),
                ));
            }
            let mut res_vec = Vec::with_bounded_capacity(*length as usize - stray);
            if stray > 0 {
                bytes_from(&mut res_vec, stray, inputs[0].clone())?;
                *inputs = &inputs[1..];
            }
            for i in 0..chunks {
                bytes_from(res, FR_BYTES_STORED, inputs[chunks - 1 - i].clone())?;
            }
            *inputs = &inputs[chunks..];
            res.extend(res_vec);
            Ok(())
        }
        AlignmentAtom::Compress => {
            error!("Cannot decode compressed value from field elements");
            Err(Error::Synthesis(
                "Cannot decode compressed value from field elements".into(),
            ))
        }
    }
}

impl IrSource {
    /// Performs a non-ZK run of a circuit, to ensure that constraints hold, and
    /// to produce a public input vector, and public input skip information.
    pub(crate) fn preprocess(
        &self,
        preimage: &ProofPreimage,
    ) -> Result<Preprocessed, ProvingError> {
        let mut memory: HashMap<Identifier, IrValue> = HashMap::new();

        let mut idx = 0;
        for input_id in self.inputs.iter() {
            let w = input_id.val_t.encoded_len();
            if idx + w > preimage.inputs.len() {
                bail!(
                    "Not enough raw inputs: ran out at index {} while decoding {:?}",
                    idx,
                    input_id.name
                );
            }
            let value = decode_offcircuit(&preimage.inputs[idx..idx + w], &input_id.val_t)?;
            memory.insert(input_id.name.clone(), value);
            idx += w;
        }
        if idx != preimage.inputs.len() {
            bail!(
                "Expected {} raw inputs, received {}",
                idx,
                preimage.inputs.len()
            );
        }

        let mut pis = vec![preimage.binding_input];
        if self.do_communications_commitment {
            pis.push(
                preimage
                    .communications_commitment
                    .ok_or(anyhow!("Expected communications commitment"))?
                    .0,
            );
        }
        let mut pi_skips = Vec::new();
        let mut public_transcript_inputs_idx: usize = 0;
        let mut public_transcript_outputs_idx: usize = 0;
        let mut private_transcript_outputs_idx: usize = 0;
        let mut outputs = Vec::new();
        let idx = |memory: &HashMap<Identifier, IrValue>, id: &Identifier| {
            let res = memory
                .get(id)
                .cloned()
                .ok_or(anyhow!("variable not found: {:?}", id));
            trace!(?res, "retrieved from {:?}", id);
            res
        };
        let resolve_operand =
            |memory: &HashMap<Identifier, IrValue>, operand: &Operand| match operand {
                Operand::Variable(id) => idx(memory, id),
                Operand::Immediate(imm) => Ok(IrValue::Native(*imm)),
            };
        let resolve_operand_bool = |memory: &HashMap<Identifier, IrValue>, operand: &Operand| {
            resolve_operand(memory, operand).and_then(|val| {
                let val: Fr = val.try_into()?;
                if val == 0.into() {
                    Ok(false)
                } else if val == 1.into() {
                    Ok(true)
                } else {
                    bail!("Expected boolean, found: {val:?}");
                }
            })
        };

        let resolve_operand_bits =
            |memory: &HashMap<Identifier, IrValue>, operand: &Operand, constrain: Option<u32>| {
                resolve_operand(memory, operand).and_then(|val| {
                    let val: Fr = val.try_into()?;
                    let mut bits = val
                        .0
                        .to_bytes_le()
                        .into_iter()
                        .flat_map(|byte| {
                            [0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80]
                                .into_iter()
                                .map(move |mask| byte & mask != 0)
                        })
                        .collect::<Vec<_>>();
                    if let Some(n) = constrain {
                        if n as usize >= FR_BITS {
                            bail!("Excessive bit bound");
                        }
                        if bits[n as usize..].iter().any(|b| *b) {
                            bail!("Bit bound failed: {val:?} is not {n}-bit");
                        }
                        bits.truncate(n as usize);
                    }
                    Ok(bits)
                })
            };

        fn from_bits<I: DoubleEndedIterator<Item = bool>>(bits: I) -> Fr {
            bits.rev()
                .fold(0.into(), |acc, bit| acc * 2.into() + bit.into())
        }
        for ins in self.instructions.iter() {
            trace!(?ins, "preprocess gate");
            match ins {
                I::Encode { input, outputs } => {
                    let val = resolve_operand(&memory, input)?;
                    let encoded = encode_offcircuit(&val);
                    if encoded.len() != outputs.len() {
                        return Err(anyhow::Error::msg(format!(
                            "Unexpected output length of encode instruction: {:?}",
                            val.get_type()
                        )));
                    }
                    for (out_id, enc_val) in outputs.iter().zip(encoded) {
                        memory.insert(out_id.clone(), enc_val);
                    }
                }
                I::Add { a, b, output } => {
                    let a = resolve_operand(&memory, a)?;
                    let b = resolve_operand(&memory, b)?;
                    let result = add_offcircuit(&a, &b)?;
                    memory.insert(output.clone(), result);
                }
                I::Mul { a, b, output } => {
                    let a = resolve_operand(&memory, a)?;
                    let b = resolve_operand(&memory, b)?;
                    let result = mul_offcircuit(&a, &b)?;
                    memory.insert(output.clone(), result);
                }
                I::Neg { a, output } => {
                    let a = resolve_operand(&memory, a)?;
                    let result = neg_offcircuit(&a)?;
                    memory.insert(output.clone(), result);
                }
                I::Inv { a, output } => {
                    let a = resolve_operand(&memory, a)?;
                    let result = inv_offcircuit(&a)?;
                    memory.insert(output.clone(), result);
                }
                I::Not { a, output } => {
                    let result = IrValue::Native((!resolve_operand_bool(&memory, a)?).into());
                    memory.insert(output.clone(), result);
                }
                I::ConstrainEq { a, b } => {
                    let a = resolve_operand(&memory, a)?;
                    let b = resolve_operand(&memory, b)?;
                    constrain_eq_offcircuit(&a, &b)?;
                }
                I::CondSelect { bit, a, b, output } => {
                    let bit_val = resolve_operand_bool(&memory, bit)?;
                    let a_val = resolve_operand(&memory, a)?;
                    let b_val = resolve_operand(&memory, b)?;
                    memory.insert(output.clone(), select_offcircuit(bit_val, &a_val, &b_val)?);
                }
                I::Assert { cond } => {
                    if !resolve_operand_bool(&memory, cond)? {
                        bail!("Failed direct assertion");
                    }
                }
                I::TestEq { a, b, output } => {
                    let a = resolve_operand(&memory, a)?;
                    let b = resolve_operand(&memory, b)?;
                    let result = test_eq_offcircuit(&a, &b)?;
                    memory.insert(output.clone(), IrValue::Native(result.into()));
                }
                I::PublicInput {
                    guard,
                    val_t,
                    output,
                } => {
                    let val = match guard {
                        Some(guard) if !resolve_operand_bool(&memory, guard)? => {
                            IrValue::default(val_t)
                        }
                        _ => {
                            let w = val_t.encoded_len();
                            let raw_outputs = &preimage.public_transcript_outputs
                                [public_transcript_outputs_idx..public_transcript_outputs_idx + w];
                            public_transcript_outputs_idx += w;
                            decode_offcircuit(raw_outputs, val_t)?
                        }
                    };
                    memory.insert(output.clone(), val);
                }
                I::PrivateInput {
                    guard,
                    val_t,
                    output,
                } => {
                    let val = match guard {
                        Some(guard) if !resolve_operand_bool(&memory, guard)? => {
                            IrValue::default(val_t)
                        }
                        _ => {
                            let w = val_t.encoded_len();
                            let raw_outputs = &preimage.private_transcript
                                [private_transcript_outputs_idx
                                    ..private_transcript_outputs_idx + w];
                            private_transcript_outputs_idx += w;
                            decode_offcircuit(raw_outputs, val_t)?
                        }
                    };
                    memory.insert(output.clone(), val);
                }
                I::Copy { val, output } => {
                    let val = resolve_operand(&memory, val)?;
                    memory.insert(output.clone(), val);
                }
                I::ConstrainToBoolean { val } => drop(resolve_operand_bool(&memory, val)?),
                I::ConstrainBits { val, bits } => {
                    drop(resolve_operand_bits(&memory, val, Some(*bits))?)
                }
                I::DivModPowerOfTwo { val, bits, outputs } => {
                    if outputs.len() != 2 {
                        bail!("DivModPowerOfTwo requires exactly 2 outputs");
                    }
                    if *bits as usize > FR_BYTES_STORED * 8 {
                        bail!("Excessive bit count");
                    }
                    let val_bits = resolve_operand_bits(&memory, val, None)?;
                    memory.insert(
                        outputs[0].clone(),
                        IrValue::Native(from_bits(val_bits[*bits as usize..].iter().copied())),
                    );
                    memory.insert(
                        outputs[1].clone(),
                        IrValue::Native(from_bits(val_bits[..*bits as usize].iter().copied())),
                    );
                }
                I::ReconstituteField {
                    divisor,
                    modulus,
                    bits,
                    output,
                } => {
                    if *bits as usize > FR_BYTES_STORED * 8 {
                        bail!("Excessive bit count");
                    }
                    let fr_max = Fr::from(-1);
                    let max_bits: Vec<bool> = fr_max
                        .0
                        .to_bytes_le()
                        .into_iter()
                        .flat_map(|byte| {
                            [0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80]
                                .into_iter()
                                .map(move |mask| byte & mask != 0)
                        })
                        .collect();
                    let modulus_bits = resolve_operand_bits(&memory, modulus, Some(*bits))?;
                    let divisor_bits =
                        resolve_operand_bits(&memory, divisor, Some(FR_BITS as u32 - *bits))?;
                    let cmp = modulus_bits
                        .iter()
                        .chain(divisor_bits.iter())
                        .rev()
                        .zip(max_bits[..FR_BITS].iter().rev())
                        .map(|(ab, max)| ab.cmp(max))
                        .fold(
                            Ordering::Equal,
                            |prefix, local| if prefix.is_eq() { local } else { prefix },
                        );
                    if cmp.is_gt() {
                        bail!("Reconstituted element overflows field");
                    }
                    let power = (0..*bits).fold(Fr::from(1), |acc, _| Fr::from(2) * acc);
                    let modulus: Fr = resolve_operand(&memory, modulus)?.try_into()?;
                    let divisor: Fr = resolve_operand(&memory, divisor)?.try_into()?;
                    let result = IrValue::Native(power * divisor + modulus);
                    memory.insert(output.clone(), result);
                }
                I::LessThan { a, b, bits, output } => {
                    let result =
                        (from_bits(resolve_operand_bits(&memory, a, Some(*bits))?.into_iter())
                            < from_bits(
                                resolve_operand_bits(&memory, b, Some(*bits))?.into_iter(),
                            ))
                        .into();
                    memory.insert(output.clone(), IrValue::Native(result));
                }
                I::JubjubScalarFromNative { native, output } => {
                    let x: Fr = resolve_operand(&memory, native)?.try_into()?;
                    let s = native_to_jubjub_scalar(&x);
                    memory.insert(output.clone(), IrValue::JubjubScalar(s));
                }
                I::TransientHash { inputs, output } => {
                    let result = transient_hash(
                        &inputs
                            .iter()
                            .map(|i| resolve_operand(&memory, i))
                            .map(|r| r.and_then(|v| v.try_into()))
                            .collect::<Result<Vec<Fr>, _>>()?,
                    );
                    memory.insert(output.clone(), IrValue::Native(result));
                }
                I::PersistentHash {
                    alignment,
                    inputs,
                    output,
                }
                | I::Keccak256 {
                    alignment,
                    inputs,
                    output,
                } => {
                    let inputs = inputs
                        .iter()
                        .map(|i| resolve_operand(&memory, i))
                        .map(|r| r.and_then(|v| v.try_into()))
                        .collect::<Result<Vec<_>, _>>()?;
                    let value = alignment.parse_field_repr(&inputs).ok_or_else(|| {
                        error!("Inputs did not match alignment (inputs: {inputs:?}, alignment: {alignment:?})");
                        anyhow!("Inputs did not match alignment (inputs: {inputs:?}, alignment: {alignment:?})")
                    })?;
                    let mut repr = Vec::new();
                    ValueReprAlignedValue(value).binary_repr(&mut repr);
                    trace!(bytes = ?repr, "bytes decoded out-of-circuit");
                    let hash_output: [u8; 32] = match ins {
                        I::PersistentHash { .. } => Sha256::digest(&repr).into(),
                        I::Keccak256 { .. } => Keccak256::digest(&repr).into(),
                        _ => unreachable!(),
                    };
                    memory.insert(output.clone(), IrValue::Bytes32(hash_output));
                }
                I::Impact { guard, inputs } => {
                    let count = inputs.len();
                    if !resolve_operand_bool(&memory, guard)? {
                        // A guarded-off impact contributes zeroed public inputs,
                        // matching the in-circuit `select(guard, x, 0)` of the
                        // `synthesize` run, and is recorded as skipped.
                        for _ in inputs {
                            pis.push(0.into());
                        }
                        pi_skips.push(Some(count));
                    } else {
                        for input in inputs {
                            let x: Fr = resolve_operand(&memory, input)?.try_into()?;
                            pis.push(x);
                            public_transcript_inputs_idx += 1;
                        }
                        pi_skips.push(None);
                        for i in 0..count {
                            let idx = public_transcript_inputs_idx - count + i;
                            let expected = preimage.public_transcript_inputs.get(idx).copied();
                            let computed = Some(pis[pis.len() - count + i]);
                            if expected != computed {
                                error!(
                                    ?idx,
                                    ?expected,
                                    ?computed,
                                    ?memory,
                                    ?pis,
                                    "Public transcript input mismatch"
                                );
                                bail!(
                                    "Public transcript input mismatch for input {idx}; expected: {expected:?}, computed: {computed:?}"
                                );
                            }
                        }
                    }
                }
                I::HashToCurve { inputs, output } => {
                    let inputs = inputs
                        .iter()
                        .map(|var| resolve_operand(&memory, var))
                        .map(|r| r.and_then(|v| v.try_into()))
                        .collect::<Result<Vec<Fr>, _>>()?;
                    let point = hash_to_curve(&inputs);
                    memory.insert(output.clone(), IrValue::JubjubPoint(point.0));
                }
                I::EcMul { a, scalar, output } => {
                    let p = resolve_operand(&memory, a)?;
                    let s = resolve_operand(&memory, scalar)?;
                    let r = ec_mul_offcircuit(&p, &s)?;
                    memory.insert(output.clone(), r);
                }
                I::EcMulGenerator { scalar, output } => {
                    let s = resolve_operand(&memory, scalar)?;
                    let p = match s.get_type() {
                        IrType::JubjubScalar => IrValue::JubjubPoint(JubjubSubgroup::generator()),
                        IrType::Secp256k1Scalar => IrValue::Secp256k1Point(k256::K256::generator()),
                        t => bail!("Unsupported EcMulGenerator for scalar of type {t:?}"),
                    };
                    let r = ec_mul_offcircuit(&p, &s)?;
                    memory.insert(output.clone(), r);
                }
                I::IntoCoordinates { point, outputs } => {
                    let p = resolve_operand(&memory, point)?;
                    let coordinates = into_coordinates_offcircuit(&p)?;
                    memory.insert(outputs.0.clone(), coordinates.0);
                    memory.insert(outputs.1.clone(), coordinates.1);
                }
                I::FromCoordinates { inputs, output } => {
                    let x = resolve_operand(&memory, &inputs.0)?;
                    let y = resolve_operand(&memory, &inputs.1)?;
                    let p = from_coordinates_offcircuit(&x, &y)?;
                    memory.insert(output.clone(), p);
                }
                I::IntoBytes32 { input, output } => {
                    let x = resolve_operand(&memory, input)?;
                    let bytes = into_bytes32_offcircuit(&x)?;
                    memory.insert(output.clone(), bytes);
                }
                I::FromBytes32 {
                    val_t,
                    bytes,
                    output,
                } => {
                    let bytes = resolve_operand(&memory, bytes)?;
                    let bytes: [u8; 32] = bytes.try_into()?;
                    let x = from_bytes32_offcircuit(val_t, &bytes)?;
                    memory.insert(output.clone(), x);
                }
                I::ReverseBytes { bytes, output } => {
                    let bytes = resolve_operand(&memory, bytes)?;
                    let mut bytes: [u8; 32] = bytes.try_into()?;
                    bytes.reverse();
                    memory.insert(output.clone(), IrValue::Bytes32(bytes));
                }
                I::Bytes32IntoLowHigh { bytes, outputs } => {
                    let bytes = resolve_operand(&memory, bytes)?;
                    let mut bytes: [u8; 32] = bytes.try_into()?;
                    let high = IrValue::Native(Fr::from(bytes[31]));
                    bytes[31] = 0;
                    let low = from_bytes32_offcircuit(&IrType::Native, &bytes)?;
                    memory.insert(outputs.0.clone(), low);
                    memory.insert(outputs.1.clone(), high);
                }
                I::Bytes32FromLowHigh { inputs, output } => {
                    let low = resolve_operand(&memory, &inputs.0)?;
                    let high = resolve_operand(&memory, &inputs.1)?;
                    let bytes_low: [u8; 32] = into_bytes32_offcircuit(&low)?.try_into()?;
                    let bytes_high: [u8; 32] = into_bytes32_offcircuit(&high)?.try_into()?;
                    if bytes_low[31] != 0 || bytes_high[1..].iter().any(|b| *b != 0) {
                        bail!(
                            "Bytes32FromLowHigh: low operand must fit in 31 bytes (be less than 2^248) and high operand must fit in a single byte (be less than 256)"
                        );
                    }
                    let mut out_bytes = bytes_low;
                    out_bytes[31] = bytes_high[0];
                    memory.insert(output.clone(), IrValue::Bytes32(out_bytes));
                }
                I::Output { vals } => {
                    if vals.len() != self.outputs.len() {
                        bail!(
                            "Output: signature declares {} return values but instruction has {}",
                            self.outputs.len(),
                            vals.len()
                        );
                    }
                    for (i, (val, expected_t)) in vals.iter().zip(self.outputs.iter()).enumerate() {
                        let value = resolve_operand(&memory, val)?;
                        if value.get_type() != *expected_t {
                            bail!(
                                "Output position {i}: signature declares {:?} but operand has runtime type {:?}",
                                expected_t,
                                value.get_type()
                            );
                        }
                        outputs.push(value);
                    }
                }
            }
        }
        trace!(?outputs, "Finished instructions with output");
        if preimage.public_transcript_inputs.len() != public_transcript_inputs_idx
            || preimage.public_transcript_outputs.len() != public_transcript_outputs_idx
            || preimage.private_transcript.len() != private_transcript_outputs_idx
        {
            error!(
                public_transcript_inputs = ?preimage.public_transcript_inputs,
                public_transcript_outputs = ?preimage.public_transcript_outputs,
                private_transcript_outputs = ?preimage.private_transcript,
                ?public_transcript_inputs_idx,
                ?public_transcript_outputs_idx,
                ?private_transcript_outputs_idx,
                "Transcripts not fully consumed");
            bail!("Transcripts not fully consumed");
        }
        if self.do_communications_commitment {
            let comm_comm = preimage
                .communications_commitment
                .ok_or(anyhow!("Expected communications randomness"))?;
            let mut comm_comm_inputs: Vec<Fr> = Vec::new();
            comm_comm_inputs.extend(preimage.inputs.iter());
            for value in outputs.iter() {
                for ir_val in encode_offcircuit(value) {
                    comm_comm_inputs.push(ir_val.try_into()?);
                }
            }
            if comm_comm.0 != transient_commit(&comm_comm_inputs[..], comm_comm.1) {
                error!(
                    ?comm_comm,
                    ?comm_comm_inputs,
                    "Communications commitment mismatch"
                );
                bail!("Communications commitment mismatch");
            }
        }
        Ok(Preprocessed {
            memory,
            pis: pis.into_iter().map(|x| x.0).collect(),
            pi_skips,
            binding_input: preimage.binding_input.0,
            comm_comm: preimage
                .communications_commitment
                .map(|(comm, rand)| (comm.0, rand.0)),
        })
    }
}

impl Relation for IrSource {
    type Instance = Vec<outer::Scalar>;
    type Witness = Preprocessed;
    type Error = midnight_proofs::plonk::Error;

    fn format_instance(
        instance: &Self::Instance,
    ) -> Result<Vec<outer::Scalar>, midnight_proofs::plonk::Error> {
        Ok(instance.clone())
    }

    fn circuit(
        &self,
        std: &ZkStdLib,
        layouter: &mut impl Layouter<outer::Scalar>,
        _instance: Value<Self::Instance>,
        witness: Value<Self::Witness>,
    ) -> Result<(), Error> {
        let mut input_values = Vec::new();
        for id in &self.inputs {
            let value = witness.as_ref().map(|preproc| {
                preproc
                    .memory
                    .get(&id.name)
                    .cloned()
                    .unwrap_or(IrValue::Native(0.into()))
            });
            input_values.push(value);
        }

        let binding_input_value = witness.as_ref().map(|preproc| preproc.binding_input);
        let comm_comm_value = witness.as_ref().map(|preproc| preproc.comm_comm);

        let mut memory: HashMap<Identifier, CircuitValue> = HashMap::new();

        for (id, value) in self.inputs.iter().zip(input_values) {
            let assigned = assign_incircuit(std, layouter, &id.val_t, &[value])?[0].clone();
            memory.insert(id.name.clone(), assigned);
        }

        let binding_input = std.assign(layouter, binding_input_value)?;

        let mut outputs = Vec::new();

        fn idx<'a>(
            memory: &'a HashMap<Identifier, CircuitValue>,
            id: &Identifier,
        ) -> Result<&'a CircuitValue, Error> {
            memory
                .get(id)
                .ok_or(Error::Synthesis(format!("value {id:?} not in memory")))
        }

        fn resolve_operand<'a>(
            std: &ZkStdLib,
            layouter: &mut impl Layouter<outer::Scalar>,
            memory: &'a HashMap<Identifier, CircuitValue>,
            operand: &'a Operand,
        ) -> Result<CircuitValue, Error> {
            match operand {
                Operand::Variable(id) => idx(memory, id).cloned(),
                Operand::Immediate(imm) => {
                    std.assign_fixed(layouter, imm.0).map(CircuitValue::Native)
                }
            }
        }

        let mem_insert = |id: Identifier,
                          cell: CircuitValue,
                          mem: &mut HashMap<Identifier, CircuitValue>|
         -> Result<(), Error> {
            // If id exists in the witness memory, make sure the value that
            // we are inserting is the same.
            // Miguel: This seems unnecessary to me. I would fail when calling
            // `mem_insert` with an id that exists in the witness memory.
            witness.as_ref()
                .zip(cell.value())
                .error_if_known_and(|(preproc, v)| {
                    if let Some(expected) = preproc.memory.get(&id) && *expected != *v  {
                        error!(id = ?id, expected = ?expected, actual = ?v, "Misalignment between `prepare` and `synthesize` runs. This is a bug.");
                        return true;
                    }
                    false
                })?;

            mem.insert(id, cell);
            Ok(())
        };

        let pi_push = |cell: AssignedNative<outer::Scalar>,
                       pis: &mut Vec<AssignedNative<outer::Scalar>>|
         -> Result<(), Error> {
            let idx = pis.len();
            witness.as_ref()
                .zip(cell.value())
                .error_if_known_and(|(preproc, v)| {
                    if idx < preproc.pis.len() && preproc.pis[idx] != **v {
                        error!(prepare = ?preproc.pis, ?idx, ?v, "Misalignment between `prepare` and `synthesize` runs. This is a bug.");
                        true
                    } else {
                        false
                    }
                })?;
            pis.push(cell);
            Ok(())
        };

        let mut public_inputs = vec![];
        pi_push(binding_input, &mut public_inputs)?;

        if self.do_communications_commitment {
            let comm_comm_value = comm_comm_value.map(|c| {
                c.ok_or_else(|| {
                    error!("Communication commitment not present despite preproc. This is a bug.");
                    Error::Synthesis("Communication commitment not present despite preproc.".into())
                })
                .unwrap()
                .0
            });
            let comm_comm = std.assign(layouter, comm_comm_value)?;
            pi_push(comm_comm, &mut public_inputs)?;
        }
        for ins in self.instructions.iter() {
            match ins {
                I::Encode { input, outputs } => {
                    let val = resolve_operand(std, layouter, &memory, input)?;
                    let encoded = encode_incircuit(std, layouter, &val)?;
                    if encoded.len() != outputs.len() {
                        return Err(Error::Synthesis(
                            "Unexpected output length of encode instruction".into(),
                        ));
                    }
                    for (out_id, enc_val) in outputs.iter().zip(encoded) {
                        mem_insert(out_id.clone(), enc_val, &mut memory)?;
                    }
                }
                I::Assert { cond } => {
                    let cond_val = resolve_operand(std, layouter, &memory, cond)?;
                    let cond: AssignedNative<_> = cond_val.try_into()?;
                    std.assert_non_zero(layouter, &cond)?;
                }
                I::CondSelect { bit, a, b, output } => {
                    let bit_val = resolve_operand(std, layouter, &memory, bit)?;
                    let bit: AssignedNative<_> = bit_val.try_into()?;
                    let bit: AssignedBit<outer::Scalar> = std.convert(layouter, &bit)?;
                    let a_val = resolve_operand(std, layouter, &memory, a)?;
                    let b_val = resolve_operand(std, layouter, &memory, b)?;
                    let result = select_incircuit(std, layouter, &bit, &a_val, &b_val)?;
                    mem_insert(output.clone(), result, &mut memory)?;
                }
                I::ConstrainBits { val, bits } => {
                    let val_assigned = resolve_operand(std, layouter, &memory, val)?;
                    let x: AssignedNative<_> = val_assigned.try_into()?;
                    drop(std.assigned_to_le_bits(
                        layouter,
                        &x,
                        Some(*bits as usize),
                        *bits as usize >= FR_BITS,
                    )?);
                }
                I::ConstrainEq { a, b } => {
                    let a_val = resolve_operand(std, layouter, &memory, a)?;
                    let b_val = resolve_operand(std, layouter, &memory, b)?;
                    constrain_eq_incircuit(std, layouter, &a_val, &b_val)?;
                }
                I::ConstrainToBoolean { val } => {
                    // Yes, this does insert a constraint.
                    let val_assigned = resolve_operand(std, layouter, &memory, val)?;
                    let x: AssignedNative<_> = val_assigned.try_into()?;
                    let _: AssignedBit<_> = std.convert(layouter, &x)?;
                }
                I::Copy { val, output } => {
                    let val = resolve_operand(std, layouter, &memory, val)?;
                    mem_insert(output.clone(), val, &mut memory)?;
                }
                I::Impact { guard, inputs } => {
                    let zero = std.assign_fixed(layouter, outer::Scalar::from(0))?;
                    let guard: AssignedBit<_> = {
                        let guard = resolve_operand(std, layouter, &memory, guard)?;
                        let guard: AssignedNative<_> = guard.try_into()?;
                        std.convert(layouter, &guard)?
                    };
                    for input in inputs {
                        let val_assigned = resolve_operand(std, layouter, &memory, input)?;
                        let x: AssignedNative<_> = val_assigned.try_into()?;
                        let guarded_x = std.select(layouter, &guard, &x, &zero)?;
                        pi_push(guarded_x, &mut public_inputs)?;
                    }
                }
                I::TransientHash { inputs, output } => {
                    let mut resolved_inputs = Vec::new();
                    for inp in inputs {
                        let x = resolve_operand(std, layouter, &memory, inp)?;
                        let x: AssignedNative<_> = x.try_into()?;
                        resolved_inputs.push(x);
                    }
                    let result = CircuitValue::Native(std.poseidon(layouter, &resolved_inputs)?);
                    mem_insert(output.clone(), result, &mut memory)?;
                }
                I::PersistentHash {
                    alignment,
                    inputs,
                    output,
                }
                | I::Keccak256 {
                    alignment,
                    inputs,
                    output,
                } => {
                    let mut resolved_inputs = Vec::new();
                    for inp in inputs {
                        let x = resolve_operand(std, layouter, &memory, inp)?;
                        let x: AssignedNative<_> = x.try_into()?;
                        resolved_inputs.push(x);
                    }
                    let inputs = resolved_inputs;
                    let bytes = fab_decode_to_bytes(std, layouter, alignment, &inputs)?;
                    let hash_output = match ins {
                        I::PersistentHash { .. } => std.sha2_256(layouter, &bytes)?,
                        I::Keccak256 { .. } => std.keccak_256(layouter, &bytes)?,
                        _ => unreachable!(),
                    };
                    mem_insert(
                        output.clone(),
                        CircuitValue::Bytes32(hash_output),
                        &mut memory,
                    )?;
                }
                I::TestEq { a, b, output } => {
                    let a_val = resolve_operand(std, layouter, &memory, a)?;
                    let b_val = resolve_operand(std, layouter, &memory, b)?;
                    let bit = test_eq_incircuit(std, layouter, &a_val, &b_val)?;
                    let result = CircuitValue::Native(std.convert(layouter, &bit)?);
                    mem_insert(output.clone(), result, &mut memory)?;
                }
                I::Add { a, b, output } => {
                    let a_val = resolve_operand(std, layouter, &memory, a)?;
                    let b_val = resolve_operand(std, layouter, &memory, b)?;
                    let result = add_incircuit(std, layouter, &a_val, &b_val)?;
                    mem_insert(output.clone(), result, &mut memory)?;
                }
                I::Mul { a, b, output } => {
                    let a_val = resolve_operand(std, layouter, &memory, a)?;
                    let b_val = resolve_operand(std, layouter, &memory, b)?;
                    let result = mul_incircuit(std, layouter, &a_val, &b_val)?;
                    mem_insert(output.clone(), result, &mut memory)?;
                }
                I::Neg { a, output } => {
                    let a_val = resolve_operand(std, layouter, &memory, a)?;
                    let result = neg_incircuit(std, layouter, &a_val)?;
                    mem_insert(output.clone(), result, &mut memory)?;
                }
                I::Inv { a, output } => {
                    let a_val = resolve_operand(std, layouter, &memory, a)?;
                    let result = inv_incircuit(std, layouter, &a_val)?;
                    mem_insert(output.clone(), result, &mut memory)?;
                }
                I::Not { a, output } => {
                    let a_val = resolve_operand(std, layouter, &memory, a)?;
                    let a: AssignedNative<_> = a_val.try_into()?;
                    let bit: AssignedBit<_> = std.convert(layouter, &a)?;
                    let neg_bit = std.not(layouter, &bit)?;
                    let result = CircuitValue::Native(std.convert(layouter, &neg_bit)?);
                    mem_insert(output.clone(), result, &mut memory)?;
                }
                I::LessThan { a, b, bits, output } => {
                    // Adding mod 2 to meet library constraint that this is even
                    // Hidden req that this is >= 4
                    let a_val = resolve_operand(std, layouter, &memory, a)?;
                    let b_val = resolve_operand(std, layouter, &memory, b)?;
                    let a: AssignedNative<_> = a_val.try_into()?;
                    let b: AssignedNative<_> = b_val.try_into()?;
                    let bit = std.lower_than(layouter, &a, &b, u32::max(*bits + *bits % 2, 4))?;
                    let result = CircuitValue::Native(std.convert(layouter, &bit)?);
                    mem_insert(output.clone(), result, &mut memory)?;
                }
                I::JubjubScalarFromNative { native, output } => {
                    let x: AssignedNative<_> =
                        resolve_operand(std, layouter, &memory, native)?.try_into()?;
                    let x_bytes = std.assigned_to_le_bytes(layouter, &x, None)?;
                    let x_big = std.biguint().from_le_bytes(layouter, &x_bytes)?;
                    let s = jubjub_scalar_from_biguint(std, layouter, x_big)?;
                    mem_insert(output.clone(), CircuitValue::JubjubScalar(s), &mut memory)?;
                }
                I::PublicInput {
                    guard: _,
                    val_t,
                    output,
                }
                | I::PrivateInput {
                    guard: _,
                    val_t,
                    output,
                } => {
                    let value = witness.as_ref().map_with_result(|preproc| {
                        preproc
                            .memory
                            .get(output)
                            .cloned()
                            .ok_or(Error::Synthesis(format!(
                                "Output {:?} not found in witness memory",
                                output
                            )))
                    })?;

                    mem_insert(
                        output.clone(),
                        assign_incircuit(std, layouter, val_t, &[value])?[0].clone(),
                        &mut memory,
                    )?;
                }
                I::DivModPowerOfTwo { val, bits, outputs } => {
                    if outputs.len() != 2 {
                        return Err(Error::Synthesis(
                            "Unexpected output length of DivModPowerOfTwo instruction".into(),
                        ));
                    }
                    let val = resolve_operand(std, layouter, &memory, val)?;
                    let val: AssignedNative<_> = val.try_into()?;
                    let val_bits = std.assigned_to_le_bits(layouter, &val, None, true)?;
                    let modulus = CircuitValue::Native(
                        std.assigned_from_le_bits(layouter, &val_bits[..*bits as usize])?,
                    );

                    let divisor = CircuitValue::Native(
                        std.assigned_from_le_bits(layouter, &val_bits[*bits as usize..])?,
                    );

                    mem_insert(outputs[0].clone(), divisor, &mut memory)?;
                    mem_insert(outputs[1].clone(), modulus, &mut memory)?;
                }
                I::ReconstituteField {
                    divisor,
                    modulus,
                    bits,
                    output,
                } => {
                    let divisor_val = resolve_operand(std, layouter, &memory, divisor)?;
                    let modulus_val = resolve_operand(std, layouter, &memory, modulus)?;
                    let divisor: AssignedNative<_> = divisor_val.try_into()?;
                    let modulus: AssignedNative<_> = modulus_val.try_into()?;

                    std.assert_lower_than_fixed(
                        layouter,
                        &divisor,
                        &(BigUint::from(1u32) << (FR_BITS as u32 - *bits)),
                    )?;
                    std.assert_lower_than_fixed(
                        layouter,
                        &modulus,
                        &(BigUint::from(1u32) << *bits),
                    )?;

                    use group::ff::Field;
                    let result = CircuitValue::Native(std.linear_combination(
                        layouter,
                        &[
                            (outer::Scalar::from(1), modulus),
                            (outer::Scalar::from(2).pow([*bits as u64]), divisor),
                        ],
                        outer::Scalar::from(0),
                    )?);
                    mem_insert(output.clone(), result, &mut memory)?;
                }
                I::EcMul { a, scalar, output } => {
                    let p = resolve_operand(std, layouter, &memory, a)?;
                    let s = resolve_operand(std, layouter, &memory, scalar)?;
                    let r = ec_mul_incircuit(std, layouter, &p, &s)?;
                    mem_insert(output.clone(), r, &mut memory)?;
                }
                I::EcMulGenerator { scalar, output } => {
                    let s = resolve_operand(std, layouter, &memory, scalar)?;
                    let p = match s.get_type() {
                        IrType::JubjubScalar => CircuitValue::JubjubPoint(
                            std.jubjub()
                                .assign_fixed(layouter, JubjubSubgroup::generator())?,
                        ),
                        IrType::Secp256k1Scalar => CircuitValue::Secp256k1Point(
                            std.secp256k1()
                                .assign_fixed(layouter, k256::K256::generator())?,
                        ),
                        t => {
                            return Err(Error::Synthesis(format!(
                                "Unsupported EcMulGenerator for scalar of type {t:?}"
                            )));
                        }
                    };
                    let r = ec_mul_incircuit(std, layouter, &p, &s)?;
                    mem_insert(output.clone(), r, &mut memory)?;
                }
                I::HashToCurve { inputs, output } => {
                    let mut resolved_inputs = Vec::new();
                    for inp in inputs {
                        let x = resolve_operand(std, layouter, &memory, inp)?;
                        let x: AssignedNative<_> = x.try_into()?;
                        resolved_inputs.push(x);
                    }
                    let point = std.hash_to_curve(layouter, &resolved_inputs)?;
                    mem_insert(
                        output.clone(),
                        CircuitValue::JubjubPoint(point),
                        &mut memory,
                    )?;
                }
                I::IntoCoordinates { point, outputs } => {
                    let p = resolve_operand(std, layouter, &memory, point)?;
                    let coordinates = into_coordinates_incircuit(std, layouter, &p)?;
                    mem_insert(outputs.0.clone(), coordinates.0, &mut memory)?;
                    mem_insert(outputs.1.clone(), coordinates.1, &mut memory)?;
                }
                I::FromCoordinates { inputs, output } => {
                    let x = resolve_operand(std, layouter, &memory, &inputs.0)?;
                    let y = resolve_operand(std, layouter, &memory, &inputs.1)?;
                    let p = from_coordinates_incircuit(std, layouter, &x, &y)?;
                    mem_insert(output.clone(), p, &mut memory)?;
                }
                I::IntoBytes32 { input, output } => {
                    let x = resolve_operand(std, layouter, &memory, input)?;
                    let bytes = into_bytes32_incircuit(std, layouter, &x)?;
                    mem_insert(output.clone(), bytes, &mut memory)?;
                }
                I::FromBytes32 {
                    val_t,
                    bytes,
                    output,
                } => {
                    let bytes = resolve_operand(std, layouter, &memory, bytes)?;
                    let bytes: [AssignedByte<outer::Scalar>; 32] = bytes.try_into()?;
                    let x = from_bytes32_incircuit(std, layouter, val_t, &bytes)?;
                    memory.insert(output.clone(), x);
                }
                I::ReverseBytes { bytes, output } => {
                    let bytes = resolve_operand(std, layouter, &memory, bytes)?;
                    let mut bytes: [AssignedByte<outer::Scalar>; 32] = bytes.try_into()?;
                    bytes.reverse();
                    memory.insert(output.clone(), CircuitValue::Bytes32(bytes));
                }
                I::Bytes32IntoLowHigh { bytes, outputs } => {
                    let bytes = resolve_operand(std, layouter, &memory, bytes)?;
                    let mut bytes: [AssignedByte<outer::Scalar>; 32] = bytes.try_into()?;
                    let high = CircuitValue::Native(std.convert(layouter, &bytes[31])?);
                    bytes[31] = std.assign_fixed(layouter, 0u8)?;
                    let low = from_bytes32_incircuit(std, layouter, &IrType::Native, &bytes)?;
                    memory.insert(outputs.0.clone(), low);
                    memory.insert(outputs.1.clone(), high);
                }
                I::Bytes32FromLowHigh { inputs, output } => {
                    let low = resolve_operand(std, layouter, &memory, &inputs.0)?;
                    let high: AssignedNative<_> =
                        resolve_operand(std, layouter, &memory, &inputs.1)?.try_into()?;
                    let bytes_low: [AssignedByte<outer::Scalar>; 32] =
                        into_bytes32_incircuit(std, layouter, &low)?.try_into()?;
                    std.assert_equal_to_fixed(layouter, &bytes_low[31], 0u8)?;
                    let mut out_bytes = bytes_low;
                    out_bytes[31] = std.convert(layouter, &high)?;
                    memory.insert(output.clone(), CircuitValue::Bytes32(out_bytes));
                }
                I::Output { vals } => {
                    if vals.len() != self.outputs.len() {
                        return Err(Error::Synthesis(format!(
                            "Output: signature declares {} return values but instruction has {}",
                            self.outputs.len(),
                            vals.len()
                        )));
                    }
                    for (i, (val, expected_t)) in vals.iter().zip(self.outputs.iter()).enumerate() {
                        let value = resolve_operand(std, layouter, &memory, val)?;
                        if value.get_type() != *expected_t {
                            return Err(Error::Synthesis(format!(
                                "Output position {i}: signature declares {:?} but operand has runtime type {:?}",
                                expected_t,
                                value.get_type()
                            )));
                        }
                        outputs.push(value);
                    }
                }
            }
        }
        if self.do_communications_commitment {
            let comm_comm_rand_value = comm_comm_value.map(|c| {
                c.ok_or_else(|| {
                    error!("Communication commitment not present despite preproc. This is a bug.");
                    Error::Synthesis("Communication commitment not present despite preproc.".into())
                })
                .unwrap()
                .1
            });
            let comm_comm_rand = std.assign(layouter, comm_comm_rand_value)?;

            let mut preimage = vec![comm_comm_rand];
            for id in &self.inputs {
                if let Some(val) = memory.get(&id.name) {
                    for cv in encode_incircuit(std, layouter, val)? {
                        let x: AssignedNative<_> = cv.try_into()?;
                        preimage.push(x);
                    }
                }
            }

            for value in &outputs {
                for cv in encode_incircuit(std, layouter, value)? {
                    let x: AssignedNative<_> = cv.try_into()?;
                    preimage.push(x);
                }
            }

            let comm_comm = std.poseidon(layouter, &preimage)?;
            // Nb. The communications commitment is the second public input
            // by convention
            std.assert_equal(layouter, &comm_comm, &public_inputs[1])?;
        }

        public_inputs
            .iter()
            .try_for_each(|x| std.constrain_as_public_input(layouter, x))
    }

    fn used_chips(&self) -> ZkStdLibArch {
        let involves_types = |target_types: &[IrType]| -> bool {
            let types_in_inputs = self
                .inputs
                .iter()
                .any(|id| target_types.contains(&id.val_t));

            // We can figure out if a type is used in the circuit by looking at the entry
            // points, currently: PublicInput or PrivateInput.
            let types_in_instructions = self.instructions.iter().any(|op| match op {
                I::PublicInput { val_t, .. } | I::PrivateInput { val_t, .. } => {
                    target_types.contains(val_t)
                }
                _ => false,
            });

            types_in_inputs || types_in_instructions
        };

        let involves_instructions = |match_predicate: &dyn Fn(&I) -> bool| -> bool {
            self.instructions.iter().any(match_predicate)
        };

        ZkStdLibArch {
            jubjub: involves_types(&[IrType::JubjubPoint, IrType::JubjubScalar])
                || involves_instructions(&|op| matches!(op, I::HashToCurve { .. })),
            poseidon: self.do_communications_commitment
                || involves_instructions(&|op| {
                    matches!(op, I::TransientHash { .. } | I::HashToCurve { .. })
                }),
            sha2_256: involves_instructions(&|op| matches!(op, I::PersistentHash { .. })),
            sha2_512: false,
            keccak_256: involves_instructions(&|op| matches!(op, I::Keccak256 { .. })),
            sha3_256: false,
            blake2b: false,
            nr_pow2range_cols: 4,
            secp256k1: involves_types(&[
                IrType::Secp256k1Point,
                IrType::Secp256k1Base,
                IrType::Secp256k1Scalar,
            ]),
            p256: involves_types(&[IrType::Secp256r1Point, IrType::Secp256r1Base, IrType::Secp256r1Scalar]),
            bls12_381: false,
            curve25519: involves_types(&[
                IrType::Curve25519Point,
                IrType::Curve25519Base,
                IrType::Curve25519Scalar,
            ]),
            base64: false,
            automaton: false,
        }
    }

    fn write_relation<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        let mut raw = Vec::new();
        tagged_serialize(&self, &mut raw)?;
        raw.serialize(writer)
    }

    fn read_relation<R: std::io::Read>(reader: &mut R) -> std::io::Result<Self> {
        let raw: Vec<u8> = Deserializable::deserialize(reader, 0)?;
        tagged_deserialize(&mut &raw[..])
    }
}
