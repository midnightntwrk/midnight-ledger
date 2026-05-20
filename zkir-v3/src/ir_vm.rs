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
use crate::ir_instructions::decode::{decode_incircuit, decode_offcircuit};
use crate::ir_instructions::encode::{
    encode_incircuit, encode_incircuit_for_commit, encode_offcircuit, encode_offcircuit_for_commit,
};
use crate::ir_instructions::eq::{test_eq_incircuit, test_eq_offcircuit};
use crate::ir_instructions::select::{select_incircuit, select_offcircuit};
use crate::ir_types::{CircuitValue, IrType, IrValue};
use crate::zkir_mode::{
    ZkirKey, ZkirOp, ZkirStateValue, u128_to_fr, zkir_ops_to_field_elements_with_sizes,
};

use super::ir::{Identifier, Instruction as I, IrSource, Operand};
use anyhow::{anyhow, bail};
use base_crypto::fab::{Alignment, AlignmentAtom, AlignmentSegment};
use base_crypto::hash::{HashOutput, persistent_hash};
use base_crypto::repr::BinaryHashRepr;
use group::Group;
use midnight_circuits::instructions::{
    ArithInstructions, AssertionInstructions, AssignmentInstructions, BinaryInstructions,
    ControlFlowInstructions, ConversionInstructions, DecompositionInstructions, EccInstructions,
    PublicInputInstructions, RangeCheckInstructions, ZeroInstructions,
};
use midnight_circuits::types::{
    AssignedBit, AssignedByte, AssignedNative, AssignedNativePoint, AssignedScalarOfNativeCurve,
    InnerValue,
};
use midnight_curves::{Fr as JubjubFr, JubjubExtended, JubjubSubgroup};
use midnight_proofs::{
    circuit::{Layouter, Value},
    plonk::Error,
};
use midnight_zk_stdlib::{Relation, ZkStdLib, ZkStdLibArch};
use num_bigint::BigUint;
use serialize::{Deserializable, Serializable, VecExt, tagged_deserialize, tagged_serialize};
use sha3::{Digest, Keccak256};
use std::cmp::Ordering;
use std::collections::HashMap;
use transient_crypto::curve::outer;
use transient_crypto::curve::{FR_BITS, FR_BYTES_STORED, Fr};
use transient_crypto::fab::{AlignmentExt, ValueReprAlignedValue};
use transient_crypto::hash::{hash_to_curve, transient_commit, transient_hash};
use transient_crypto::proofs::{ProofPreimage, ProvingError};
use transient_crypto::repr::FieldRepr;

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

/// Compute the width (in `Fr`s) of a transcript-stream slot for a
/// declared `IrType`, peeking at the leading `byte_len` Fr for
/// `IrType::Opaque`.
///
/// Used by the `I::PublicInput` and `I::PrivateInput` arms to slice a
/// `[byte_len, packed_preimage_frs ...]` Opaque entry out of
/// `preimage.public_transcript_outputs` or `preimage.private_transcript`.
/// The WASM bridge emits each Compress-aligned popeq result and witness
/// AV in this layout (see
/// `zkir-v3-wasm/src/lib.rs:flatten_av_with_opaque_preimages`); the
/// IR-side decoder handles other types via the standard
/// fixed-width `IrType::encoded_len()` path.
fn transcript_slot_width(
    val_t: &IrType,
    transcript: &[Fr],
    idx: usize,
    transcript_name: &'static str,
    output: &Identifier,
) -> Result<usize, anyhow::Error> {
    match val_t {
        IrType::Opaque => {
            if idx >= transcript.len() {
                bail!(
                    "Opaque {output:?}: ran out of {transcript_name} at offset {idx} \
                     (need at least the byte_len Fr)"
                );
            }
            let byte_len = u32::try_from(transcript[idx]).map_err(|_| {
                anyhow!(
                    "Opaque {output:?}: byte_len Fr at {transcript_name}[{idx}] out of \
                     u32 range"
                )
            })? as usize;
            let w = 1 + byte_len.div_ceil(FR_BYTES_STORED);
            if idx + w > transcript.len() {
                bail!(
                    "Opaque {output:?}: {transcript_name} truncated; need {} Frs from \
                     offset {idx} but only {} are available",
                    w,
                    transcript.len() - idx
                );
            }
            Ok(w)
        }
        _ => {
            let w = val_t.encoded_len();
            if idx + w > transcript.len() {
                bail!(
                    "{output:?}: {transcript_name} truncated; need {w} Frs of {val_t:?} \
                     from offset {idx} but only {} are available",
                    transcript.len() - idx
                );
            }
            Ok(w)
        }
    }
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
                *inputs = &inputs[1..];
            }
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

fn assemble_bytes(
    std: &ZkStdLib,
    layouter: &mut impl Layouter<outer::Scalar>,
    bytes: &[AssignedByte<outer::Scalar>],
) -> Result<AssignedNative<outer::Scalar>, Error> {
    const BITS: usize = 8;
    let mut powers = Vec::with_bounded_capacity(bytes.len());
    powers.push(std.convert(layouter, &bytes[0])?);
    for (i, byte) in bytes.iter().enumerate().skip(1) {
        let power = (0..i * BITS)
            .fold(Fr::from(1), |acc, _| acc * Fr::from(2))
            .0;
        let byte = std.convert(layouter, byte)?;
        powers.push(std.mul_by_constant(layouter, &byte, power)?);
    }
    let mut acc = powers[0].clone();
    for limb in powers[1..].iter() {
        acc = std.add(layouter, &acc, limb)?;
    }
    Ok(acc)
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
            let w = match &input_id.val_t {
                IrType::Opaque => {
                    if idx >= preimage.inputs.len() {
                        bail!(
                            "Not enough raw inputs: ran out at index {} while \
                             decoding Opaque {:?} (need at least the byte_len Fr)",
                            idx,
                            input_id.name
                        );
                    }
                    let byte_len = u32::try_from(preimage.inputs[idx]).map_err(|_| {
                        anyhow!(
                            "Opaque {:?}: byte_len Fr at offset {} out of u32 range",
                            input_id.name,
                            idx
                        )
                    })? as usize;
                    1 + byte_len.div_ceil(FR_BYTES_STORED)
                }
                _ => input_id.val_t.encoded_len(),
            };
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
                        return Err(anyhow::Error::msg(
                            "Unexpected output length of encode instruction",
                        ));
                    }
                    for (out_id, enc_val) in outputs.iter().zip(encoded) {
                        memory.insert(out_id.clone(), enc_val);
                    }
                }
                I::Decode {
                    inputs,
                    val_t,
                    output,
                } => {
                    let raw_inputs = inputs
                        .iter()
                        .map(|inp_id| resolve_operand(&memory, inp_id)?.try_into())
                        .collect::<Result<Vec<Fr>, _>>()?;
                    let decoded = decode_offcircuit(&raw_inputs, val_t)?;
                    memory.insert(output.clone(), decoded);
                }
                I::Add { a, b, output } => {
                    let a = resolve_operand(&memory, a)?;
                    let b = resolve_operand(&memory, b)?;
                    let result = add_offcircuit(&a, &b)?;
                    memory.insert(output.clone(), result);
                }
                I::Mul { a, b, output } => {
                    let a: Fr = resolve_operand(&memory, a)?.try_into()?;
                    let b: Fr = resolve_operand(&memory, b)?.try_into()?;
                    let result = IrValue::Native(a * b);
                    memory.insert(output.clone(), result);
                }
                I::Neg { a, output } => {
                    let a: Fr = resolve_operand(&memory, a)?.try_into()?;
                    let result = IrValue::Native(-a);
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
                            let w = transcript_slot_width(
                                val_t,
                                &preimage.public_transcript_outputs,
                                public_transcript_outputs_idx,
                                "public_transcript_outputs",
                                output,
                            )?;
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
                            let w = transcript_slot_width(
                                val_t,
                                &preimage.private_transcript,
                                private_transcript_outputs_idx,
                                "private_transcript",
                                output,
                            )?;
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
                    outputs,
                }
                | I::Keccak256 {
                    alignment,
                    inputs,
                    outputs,
                } => {
                    if outputs.len() != 2 {
                        bail!("PersistentHash requires exactly 2 outputs");
                    }
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
                    let hash = match ins {
                        I::PersistentHash { .. } => persistent_hash(&repr),
                        I::Keccak256 { .. } => HashOutput(Keccak256::digest(&repr).into()),
                        _ => unreachable!(),
                    };
                    let hash_fields = hash.field_vec();
                    if hash_fields.len() >= 2 {
                        memory.insert(outputs[0].clone(), IrValue::Native(hash_fields[0]));
                        memory.insert(outputs[1].clone(), IrValue::Native(hash_fields[1]));
                    } else {
                        bail!("PersistentHash did not produce expected output");
                    }
                }
                I::Impact { guard, ops } => {
                    // prove.rs needs one pi_skips entry per op
                    let (field_elements, per_op_sizes) =
                        zkir_ops_to_field_elements_with_sizes(ops.clone(), &memory)?;
                    let count = field_elements.len();
                    if !resolve_operand_bool(&memory, guard)? {
                        // guard=false: emit zeros (matches in-circuit
                        // select(guard, val, 0) = 0) and mark all slots skipped
                        for _ in 0..count {
                            pis.push(Fr::from(0u64));
                        }
                        for &op_size in &per_op_sizes {
                            pi_skips.push(Some(op_size));
                        }
                    } else {
                        for x in &field_elements {
                            pis.push(*x);
                        }
                        for _ in 0..per_op_sizes.len() {
                            pi_skips.push(None);
                        }
                        for i in 0..count {
                            let expected = preimage
                                .public_transcript_inputs
                                .get(public_transcript_inputs_idx + i)
                                .copied();
                            let computed = Some(pis[pis.len() - count + i]);
                            if expected != computed {
                                error!(
                                    idx = public_transcript_inputs_idx + i,
                                    ?expected,
                                    ?computed,
                                    ?memory,
                                    ?pis,
                                    "Public transcript input mismatch"
                                );
                                bail!(
                                    "Public transcript input mismatch for input {}; expected: {expected:?}, computed: {computed:?}",
                                    public_transcript_inputs_idx + i
                                );
                            }
                        }
                        public_transcript_inputs_idx += count;
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
                    let a: JubjubSubgroup = resolve_operand(&memory, a)?.try_into()?;
                    let s: JubjubFr = resolve_operand(&memory, scalar)?.try_into()?;
                    let c = IrValue::JubjubPoint(a * s);
                    memory.insert(output.clone(), c);
                }
                I::EcMulGenerator { scalar, output } => {
                    let s: JubjubFr = resolve_operand(&memory, scalar)?.try_into()?;
                    let p = JubjubSubgroup::generator() * s;
                    memory.insert(output.clone(), IrValue::JubjubPoint(p));
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
            // We must rebuild the comm_comm preimage in commit-bearing
            // form to match what the JS bridge fed into `transient_hash`
            // for the commitment.
            let mut comm_comm_inputs: Vec<Fr> = Vec::new();
            for input_id in self.inputs.iter() {
                let value = memory.get(&input_id.name).ok_or_else(|| {
                    anyhow!(
                        "comm_comm: declared input {:?} missing from memory after preprocess",
                        input_id.name
                    )
                })?;
                for ir_val in encode_offcircuit_for_commit(value) {
                    comm_comm_inputs.push(ir_val.try_into()?);
                }
            }
            for value in outputs.iter() {
                for ir_val in encode_offcircuit(value) {
                    for ir_val in encode_offcircuit_for_commit(value) {
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
            if mem.contains_key(&id) {
                error!(id = ?id, "Identifier bound twice during synthesize. This is a bug.");
                return Err(Error::Synthesis(format!(
                    "Identifier {id:?} bound twice during synthesize; \
                     each IR variable must be assigned at most once."
                )));
            }
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
                I::Decode {
                    inputs,
                    val_t,
                    output,
                } => {
                    let raw_inputs = inputs
                        .iter()
                        .map(|inp_id| resolve_operand(std, layouter, &memory, inp_id)?.try_into())
                        .collect::<Result<Vec<AssignedNative<_>>, Error>>()?;
                    let decoded = decode_incircuit(std, layouter, &raw_inputs, val_t)?;
                    mem_insert(output.clone(), decoded, &mut memory)?;
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
                I::Impact { guard, ops } => {
                    // In-circuit mirror of zkir_ops_to_field_elements; each Fr
                    // is gated by select(guard, val, 0).
                    let impact_zero = std.assign_fixed(layouter, outer::Scalar::from(0))?;
                    let impact_guard: AssignedBit<_> = {
                        let guard_val = resolve_operand(std, layouter, &memory, guard)?;
                        let guard_native: AssignedNative<_> = guard_val.try_into()?;
                        std.convert(layouter, &guard_native)?
                    };

                    macro_rules! push_const_pi {
                        ($val:expr) => {{
                            let assigned =
                                std.assign_fixed(layouter, outer::Scalar::from($val as u64))?;
                            let guarded =
                                std.select(layouter, &impact_guard, &assigned, &impact_zero)?;
                            pi_push(guarded, &mut public_inputs)?;
                        }};
                    }

                    macro_rules! push_operand_pi {
                        ($operand:expr) => {{
                            let val = resolve_operand(std, layouter, &memory, $operand)?;
                            let x: AssignedNative<_> = val.try_into()?;
                            let guarded = std.select(layouter, &impact_guard, &x, &impact_zero)?;
                            pi_push(guarded, &mut public_inputs)?;
                        }};
                    }

                    macro_rules! push_alignment_pi {
                        ($alignment:expr) => {{
                            let mut fields: Vec<Fr> = Vec::new();
                            $alignment.field_repr(&mut fields);
                            for fr_val in &fields {
                                let assigned = std.assign_fixed(layouter, fr_val.0)?;
                                let guarded =
                                    std.select(layouter, &impact_guard, &assigned, &impact_zero)?;
                                pi_push(guarded, &mut public_inputs)?;
                            }
                        }};
                    }

                    for op in ops {
                        match op {
                            ZkirOp::Noop { n } => {
                                for _ in 0..*n {
                                    push_const_pi!(0);
                                }
                            }
                            ZkirOp::Lt => push_const_pi!(0x01),
                            ZkirOp::Eq => push_const_pi!(0x02),
                            ZkirOp::Type => push_const_pi!(0x03),
                            ZkirOp::Size => push_const_pi!(0x04),
                            ZkirOp::New => push_const_pi!(0x05),
                            ZkirOp::And => push_const_pi!(0x06),
                            ZkirOp::Or => push_const_pi!(0x07),
                            ZkirOp::Neg => push_const_pi!(0x08),
                            ZkirOp::Log => push_const_pi!(0x09),
                            ZkirOp::Root => push_const_pi!(0x0a),
                            ZkirOp::Pop => push_const_pi!(0x0b),
                            ZkirOp::Add => push_const_pi!(0x14),
                            ZkirOp::Sub => push_const_pi!(0x15),
                            ZkirOp::Member => push_const_pi!(0x18),
                            ZkirOp::Ckpt => push_const_pi!(0xff),

                            ZkirOp::Popeq { cached, result } => {
                                push_const_pi!((0x0cu8 + *cached as u8));
                                push_alignment_pi!(&result.alignment);
                                for operand in &result.operands {
                                    push_operand_pi!(operand);
                                }
                            }

                            ZkirOp::Addi { immediate } => {
                                push_const_pi!(0x0e);
                                push_operand_pi!(immediate);
                            }
                            ZkirOp::Subi { immediate } => {
                                push_const_pi!(0x0f);
                                push_operand_pi!(immediate);
                            }

                            // Push: opcode | recursive ZkirStateValue encoding.
                            // The recursive walk mirrors encode_state_value
                            // in zkir_mode.rs. We use an explicit todo
                            // queue to avoid Rust closure-recursion
                            // headaches; each `ZkEncTodo` represents one
                            // unit of work (a constant push, an alignment
                            // push, an operand push, or "expand this
                            // ZkirStateValue into its sub-actions").
                            ZkirOp::Push { storage, value } => {
                                push_const_pi!((0x10u8 + *storage as u8));

                                // Action queue: process front-to-back. Map
                                // and Array variants expand into a flat
                                // sequence of nested actions when reached,
                                // preserving declaration order.
                                enum ZkEncTodo<'a> {
                                    Const(u64),
                                    ConstFr(Fr),
                                    Alignment(&'a Alignment),
                                    Operand(&'a Operand),
                                    State(&'a ZkirStateValue),
                                }

                                use std::collections::VecDeque;
                                let mut todo: VecDeque<ZkEncTodo> = VecDeque::new();
                                todo.push_back(ZkEncTodo::State(value));

                                while let Some(item) = todo.pop_front() {
                                    match item {
                                        ZkEncTodo::Const(c) => push_const_pi!(c),
                                        ZkEncTodo::ConstFr(fr) => {
                                            let assigned = std.assign_fixed(layouter, fr.0)?;
                                            let guarded = std.select(
                                                layouter,
                                                &impact_guard,
                                                &assigned,
                                                &impact_zero,
                                            )?;
                                            pi_push(guarded, &mut public_inputs)?;
                                        }
                                        ZkEncTodo::Alignment(a) => push_alignment_pi!(a),
                                        ZkEncTodo::Operand(op) => push_operand_pi!(op),
                                        ZkEncTodo::State(sv) => match sv {
                                            ZkirStateValue::Null => {
                                                push_const_pi!(0u64);
                                            }
                                            ZkirStateValue::Cell(av) => {
                                                push_const_pi!(1u64);
                                                push_alignment_pi!(&av.alignment);
                                                for op in &av.operands {
                                                    push_operand_pi!(op);
                                                }
                                            }
                                            ZkirStateValue::Map(entries) => {
                                                let tag =
                                                    2u128 | ((entries.len() as u128) << 4);
                                                let tag_fr = u128_to_fr(tag);
                                                // Front-load: this tag goes
                                                // first, then per entry the
                                                // (alignment, operands,
                                                // recursed value) actions —
                                                // all in declaration order.
                                                let mut nested = VecDeque::new();
                                                nested.push_back(ZkEncTodo::ConstFr(tag_fr));
                                                for (k, v) in entries {
                                                    nested.push_back(ZkEncTodo::Alignment(
                                                        &k.alignment,
                                                    ));
                                                    for op in &k.operands {
                                                        nested.push_back(ZkEncTodo::Operand(
                                                            op,
                                                        ));
                                                    }
                                                    nested.push_back(ZkEncTodo::State(v));
                                                }
                                                while let Some(t) = nested.pop_back() {
                                                    todo.push_front(t);
                                                }
                                            }
                                            ZkirStateValue::Array(entries) => {
                                                let tag = 3u64
                                                    | ((entries.len() as u64) << 4);
                                                let mut nested = VecDeque::new();
                                                nested.push_back(ZkEncTodo::Const(tag));
                                                for v in entries {
                                                    nested.push_back(ZkEncTodo::State(v));
                                                }
                                                while let Some(t) = nested.pop_back() {
                                                    todo.push_front(t);
                                                }
                                            }
                                            ZkirStateValue::BoundedMerkleTree {
                                                height,
                                                entries,
                                            } => {
                                                let tag = 4u128
                                                    | ((*height as u128) << 4)
                                                    | ((entries.len() as u128) << 12);
                                                let tag_fr = u128_to_fr(tag);
                                                let mut nested = VecDeque::new();
                                                nested.push_back(ZkEncTodo::ConstFr(tag_fr));
                                                for (idx, v) in entries {
                                                    // u64 idx encodes as
                                                    // a single Fr atom in
                                                    // u64.field_repr; for
                                                    // values < 2^64 a
                                                    // direct push works.
                                                    nested
                                                        .push_back(ZkEncTodo::Const(*idx));
                                                    nested.push_back(ZkEncTodo::State(v));
                                                }
                                                while let Some(t) = nested.pop_back() {
                                                    todo.push_front(t);
                                                }
                                            }
                                        },
                                    }
                                }
                            }

                            ZkirOp::Branch { skip } => {
                                push_const_pi!(0x12);
                                push_const_pi!(*skip);
                            }
                            ZkirOp::Jmp { skip } => {
                                push_const_pi!(0x13);
                                push_const_pi!(*skip);
                            }

                            ZkirOp::Concat { cached, n } => {
                                push_const_pi!((0x16u8 + *cached as u8));
                                push_const_pi!(*n);
                            }

                            ZkirOp::Rem { cached } => {
                                push_const_pi!((0x19u8 + *cached as u8));
                            }

                            ZkirOp::Dup { n } => push_const_pi!((0x30u8 | *n)),
                            ZkirOp::Swap { n } => push_const_pi!((0x40u8 | *n)),

                            ZkirOp::Idx {
                                cached,
                                push_path,
                                path,
                            } => {
                                if !path.is_empty() {
                                    let base: u8 = match (*cached, *push_path) {
                                        (false, false) => 0x50,
                                        (true, false) => 0x60,
                                        (false, true) => 0x70,
                                        (true, true) => 0x80,
                                    };
                                    let opcode = base | (path.len() as u8 - 1);
                                    push_const_pi!(opcode);
                                    for key in path {
                                        match key {
                                            ZkirKey::Stack => {
                                                // Stack -> -1
                                                let neg_one = std.assign_fixed(
                                                    layouter,
                                                    -outer::Scalar::from(1),
                                                )?;
                                                let guarded = std.select(
                                                    layouter,
                                                    &impact_guard,
                                                    &neg_one,
                                                    &impact_zero,
                                                )?;
                                                pi_push(guarded, &mut public_inputs)?;
                                            }
                                            ZkirKey::Value {
                                                alignment,
                                                operands,
                                            } => {
                                                push_alignment_pi!(alignment);
                                                for operand in operands {
                                                    push_operand_pi!(operand);
                                                }
                                            }
                                        }
                                    }
                                }
                            }

                            ZkirOp::Ins { cached, n } => {
                                let base: u8 = if *cached { 0xa0 } else { 0x90 };
                                push_const_pi!((base | *n));
                            }
                        }
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
                    outputs,
                }
                | I::Keccak256 {
                    alignment,
                    inputs,
                    outputs,
                } => {
                    if outputs.len() != 2 {
                        return Err(Error::Synthesis(
                            "Unexpected output length of persistent hash instruction".into(),
                        ));
                    }
                    let mut resolved_inputs = Vec::new();
                    for inp in inputs {
                        let x = resolve_operand(std, layouter, &memory, inp)?;
                        let x: AssignedNative<_> = x.try_into()?;
                        resolved_inputs.push(x);
                    }
                    let inputs = resolved_inputs;
                    let bytes = fab_decode_to_bytes(std, layouter, alignment, &inputs)?;
                    let res_bytes = match ins {
                        I::PersistentHash { .. } => std.sha2_256(layouter, &bytes)?,
                        I::Keccak256 { .. } => std.keccak_256(layouter, &bytes)?,
                        _ => unreachable!(),
                    };
                    mem_insert(
                        outputs[0].clone(),
                        CircuitValue::Native(std.convert(layouter, &res_bytes[31])?),
                        &mut memory,
                    )?;
                    mem_insert(
                        outputs[1].clone(),
                        CircuitValue::Native(assemble_bytes(std, layouter, &res_bytes[..31])?),
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
                    let a: AssignedNative<_> = a_val.try_into()?;
                    let b: AssignedNative<_> = b_val.try_into()?;
                    let result = CircuitValue::Native(std.mul(layouter, &a, &b, None)?);
                    mem_insert(output.clone(), result, &mut memory)?;
                }
                I::Neg { a, output } => {
                    let a_val = resolve_operand(std, layouter, &memory, a)?;
                    let a: AssignedNative<_> = a_val.try_into()?;
                    let result = CircuitValue::Native(std.neg(layouter, &a)?);
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
                    let a_val = resolve_operand(std, layouter, &memory, a)?;
                    let scalar_val = resolve_operand(std, layouter, &memory, scalar)?;
                    let a: AssignedNativePoint<JubjubExtended> = a_val.try_into()?;
                    let scalar: AssignedScalarOfNativeCurve<_> = scalar_val.try_into()?;
                    let b = std.jubjub().msm(layouter, &[scalar], &[a])?;
                    mem_insert(output.clone(), CircuitValue::JubjubPoint(b), &mut memory)?;
                }
                I::EcMulGenerator { scalar, output } => {
                    let g: AssignedNativePoint<JubjubExtended> = std
                        .jubjub()
                        .assign_fixed(layouter, JubjubSubgroup::generator())?;
                    let scalar_val = resolve_operand(std, layouter, &memory, scalar)?;
                    let scalar: AssignedScalarOfNativeCurve<_> = scalar_val.try_into()?;
                    let b = std.jubjub().msm(layouter, &[scalar], &[g])?;
                    mem_insert(output.clone(), CircuitValue::JubjubPoint(b), &mut memory)?;
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
                    // Outputs MUST go through the commit-form encoder so
                // that an `Opaque` output emits its single cached
                // `commit` AssignedNative.
                for cv in encode_incircuit_for_commit(std, layouter, value)? {
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

            let types_in_instructions = self.instructions.iter().any(|op| match op {
                I::Decode { val_t, .. }
                | I::PublicInput { val_t, .. }
                | I::PrivateInput { val_t, .. } => target_types.contains(val_t),
                _ => false,
            });

            types_in_inputs || types_in_instructions
        };

        let jubjub = self.instructions.iter().any(|op| {
            involves_types(&[IrType::JubjubPoint, IrType::JubjubScalar]) || {
                matches!(
                    op,
                    I::EcMul { .. } | I::EcMulGenerator { .. } | I::HashToCurve { .. }
                )
            }
        });
        let hash_to_curve = self
            .instructions
            .iter()
            .any(|op| matches!(op, I::HashToCurve { .. }));
        let poseidon = self.do_communications_commitment
            || self
                .instructions
                .iter()
                .any(|op| matches!(op, I::TransientHash { .. }));
        let sha2_256 = self
            .instructions
            .iter()
            .any(|op| matches!(op, I::PersistentHash { .. }));
        let keccak_256 = self
            .instructions
            .iter()
            .any(|op| matches!(op, I::Keccak256 { .. }));
        ZkStdLibArch {
            jubjub: jubjub || hash_to_curve,
            poseidon: poseidon || hash_to_curve,
            sha2_256,
            sha2_512: false,
            keccak_256,
            sha3_256: false,
            blake2b: false,
            nr_pow2range_cols: 4,
            secp256k1: false,
            bls12_381: false,
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
