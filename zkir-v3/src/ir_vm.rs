// This file is part of midnight-ledger.
// Copyright (C) 2025 Midnight Foundation
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

use crate::ir_instructions::assign::assign_incircuit;
use crate::ir_instructions::decode::{decode_incircuit, decode_offcircuit};
use crate::ir_instructions::encode::{encode_incircuit, encode_offcircuit};
use crate::ir_types::{CircuitValue, IrValue};

use super::ir::{Identifier, Instruction as I, IrSource, Operand};
use anyhow::{anyhow, bail};
use base_crypto::fab::{Alignment, AlignmentAtom, AlignmentSegment};
use base_crypto::hash::persistent_hash;
use base_crypto::repr::BinaryHashRepr;
use group::Group;
use midnight_circuits::instructions::{
    ArithInstructions, AssertionInstructions, AssignmentInstructions, BinaryInstructions,
    ControlFlowInstructions, ConversionInstructions, DecompositionInstructions, EccInstructions,
    EqualityInstructions, PublicInputInstructions, ZeroInstructions,
};
use midnight_circuits::types::{
    AssignedBit, AssignedByte, AssignedNative, AssignedNativePoint, InnerValue,
};
use midnight_curves::{JubjubExtended, JubjubSubgroup};
use midnight_proofs::{
    circuit::{Layouter, Value},
    plonk::Error,
};
use midnight_zk_stdlib::{Relation, ZkStdLib, ZkStdLibArch};
use serialize::{Deserializable, Serializable, VecExt};
use std::cmp::Ordering;
use std::collections::HashMap;
use transient_crypto::curve::EmbeddedGroupAffine;
use transient_crypto::curve::{FR_BITS, FR_BYTES_STORED, Fr};
use transient_crypto::curve::{embedded, outer};
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

fn ecc_from_parts(
    std: &ZkStdLib,
    layouter: &mut impl Layouter<outer::Scalar>,
    x: &AssignedNative<outer::Scalar>,
    y: &AssignedNative<outer::Scalar>,
) -> Result<AssignedNativePoint<embedded::AffineExtended>, Error> {
    let point = x
        .value()
        .zip(y.value())
        .map(|(x, y)| EmbeddedGroupAffine::new(Fr(*x), Fr(*y)));
    point.as_ref().error_if_known_and(|p| p.is_none())?;
    let point = point.map(|p| p.expect("After is_none check, point should exist").0);
    let point_var: AssignedNativePoint<embedded::AffineExtended> =
        std.jubjub().assign(layouter, point)?;

    std.assert_equal(layouter, x, &std.jubjub().x_coordinate(&point_var))?;
    std.assert_equal(layouter, y, &std.jubjub().y_coordinate(&point_var))?;
    Ok(point_var)
}

impl IrSource {
    /// Performs a non-ZK run of a circuit, to ensure that constraints hold, and
    /// to produce a public input vector, and public input skip information.
    pub(crate) fn preprocess(
        &self,
        preimage: &ProofPreimage,
    ) -> Result<Preprocessed, ProvingError> {
        if preimage.inputs.len() != self.inputs.len() {
            bail!(
                "Expected {} inputs, received {}",
                self.inputs.len(),
                preimage.inputs.len()
            );
        }
        let mut memory: HashMap<Identifier, IrValue> = HashMap::new();
        for (id, input) in self.inputs.iter().zip(preimage.inputs.iter()) {
            memory.insert(id.name.clone(), IrValue::Native(*input));
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
        let resolve_operand_point =
            |memory: &HashMap<Identifier, IrValue>, x: &Operand, y: &Operand| {
                let x = resolve_operand(memory, x)?;
                let y = resolve_operand(memory, y)?;
                let x: Fr = x.try_into()?;
                let y: Fr = y.try_into()?;
                EmbeddedGroupAffine::new(x, y)
                    .ok_or(anyhow!("Elliptic curve point not on curve: ({x:?}, {y:?})"))
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
        let from_point =
            |p: EmbeddedGroupAffine| [p.x().unwrap_or(0.into()), p.y().unwrap_or(0.into())];
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
                    for (out_id, enc_val) in outputs.iter().zip(encoded.into_iter()) {
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
                    let a: Fr = a.try_into().unwrap();
                    let b: Fr = b.try_into().unwrap();
                    let result = IrValue::Native(a + b);
                    memory.insert(output.clone(), result);
                }
                I::Mul { a, b, output } => {
                    let a = resolve_operand(&memory, a)?;
                    let b = resolve_operand(&memory, b)?;
                    let a: Fr = a.try_into().unwrap();
                    let b: Fr = b.try_into().unwrap();
                    let result = IrValue::Native(a * b);
                    memory.insert(output.clone(), result);
                }
                I::Neg { a, output } => {
                    let a = resolve_operand(&memory, a)?;
                    let a: Fr = a.try_into().unwrap();
                    let result = IrValue::Native(-a);
                    memory.insert(output.clone(), result);
                }
                I::Not { a, output } => {
                    let result = IrValue::Native((!resolve_operand_bool(&memory, a)?).into());
                    memory.insert(output.clone(), result);
                }
                I::ConstrainEq { a, b } => {
                    if resolve_operand(&memory, a)? != resolve_operand(&memory, b)? {
                        bail!(
                            "Failed equality constraint: {:?} != {:?}",
                            resolve_operand(&memory, a)?,
                            resolve_operand(&memory, b)?
                        );
                    }
                }
                I::CondSelect { bit, a, b, output } => {
                    let (bit_val, a_val, b_val) = (
                        resolve_operand_bool(&memory, bit)?,
                        resolve_operand(&memory, a)?,
                        resolve_operand(&memory, b)?,
                    );
                    memory.insert(output.clone(), if bit_val { a_val } else { b_val });
                }
                I::Assert { cond } => {
                    if !resolve_operand_bool(&memory, cond)? {
                        bail!("Failed direct assertion");
                    }
                }
                I::TestEq { a, b, output } => {
                    let result = IrValue::Native(
                        (resolve_operand(&memory, a)? == resolve_operand(&memory, b)?).into(),
                    );
                    memory.insert(output.clone(), result);
                }
                I::PublicInput { guard, output } => {
                    let val = match guard {
                        Some(guard) if !resolve_operand_bool(&memory, guard)? => 0.into(),
                        _ => {
                            public_transcript_outputs_idx += 1;
                            preimage
                                .public_transcript_outputs
                                .get(public_transcript_outputs_idx - 1)
                                .copied()
                                .ok_or(anyhow!("Ran out of public transcript outputs"))?
                        }
                    };
                    memory.insert(output.clone(), IrValue::Native(val));
                }
                I::PrivateInput { guard, output } => {
                    let val = match guard {
                        Some(guard) if !resolve_operand_bool(&memory, guard)? => 0.into(),
                        _ => {
                            private_transcript_outputs_idx += 1;
                            preimage
                                .private_transcript
                                .get(private_transcript_outputs_idx - 1)
                                .copied()
                                .ok_or(anyhow!("Ran out of private transcript outputs"))?
                        }
                    };
                    memory.insert(output.clone(), IrValue::Native(val));
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
                    let hash = persistent_hash(&repr);
                    let hash_fields = hash.field_vec();
                    if hash_fields.len() >= 2 {
                        memory.insert(outputs[0].clone(), IrValue::Native(hash_fields[0]));
                        memory.insert(outputs[1].clone(), IrValue::Native(hash_fields[1]));
                    } else {
                        bail!("PersistentHash did not produce expected output");
                    }
                }
                I::Impact { guard, inputs } => {
                    let count = inputs.len();
                    for input in inputs {
                        let x: Fr = resolve_operand(&memory, input)?.try_into()?;
                        pis.push(x);
                        public_transcript_inputs_idx += 1;
                    }
                    if !resolve_operand_bool(&memory, guard)? {
                        pi_skips.push(Some(count));
                        public_transcript_inputs_idx -= count;
                    } else {
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
                I::Output { val } => outputs.push(resolve_operand(&memory, val)?),
                I::EcAdd { a, b, output } => {
                    let a: JubjubSubgroup = resolve_operand(&memory, a)?.try_into()?;
                    let b: JubjubSubgroup = resolve_operand(&memory, b)?.try_into()?;
                    let c = IrValue::JubjubPoint(a + b);
                    memory.insert(output.clone(), c);
                }
                I::HashToCurve { inputs, outputs } => {
                    if outputs.len() != 2 {
                        bail!("HashToCurve requires exactly 2 outputs");
                    }
                    let inputs = inputs
                        .iter()
                        .map(|var| resolve_operand(&memory, var))
                        .map(|r| r.and_then(|v| v.try_into()))
                        .collect::<Result<Vec<Fr>, _>>()?;
                    let point = hash_to_curve(&inputs);
                    let [x, y] = from_point(point);
                    memory.insert(outputs[0].clone(), IrValue::Native(x));
                    memory.insert(outputs[1].clone(), IrValue::Native(y));
                }
                I::EcMul {
                    a_x,
                    a_y,
                    scalar,
                    outputs,
                } => {
                    if outputs.len() != 2 {
                        bail!("EcMul requires exactly 2 outputs");
                    }
                    let s: Fr = resolve_operand(&memory, scalar)?.try_into()?;
                    let point = resolve_operand_point(&memory, a_x, a_y)? * s;
                    let [x, y] = from_point(point);
                    memory.insert(outputs[0].clone(), IrValue::Native(x));
                    memory.insert(outputs[1].clone(), IrValue::Native(y));
                }
                I::EcMulGenerator { scalar, outputs } => {
                    if outputs.len() != 2 {
                        bail!("EcMulGenerator requires exactly 2 outputs");
                    }
                    let s: Fr = resolve_operand(&memory, scalar)?.try_into()?;
                    let point = EmbeddedGroupAffine::generator() * s;
                    let [x, y] = from_point(point);
                    memory.insert(outputs[0].clone(), IrValue::Native(x));
                    memory.insert(outputs[1].clone(), IrValue::Native(y));
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
            for output in outputs.iter() {
                comm_comm_inputs.push(output.clone().try_into()?);
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

        for (id, value) in self.inputs.iter().zip(input_values.into_iter()) {
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
                    for (out_id, enc_val) in outputs.iter().zip(encoded.into_iter()) {
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
                    let b_val = resolve_operand(std, layouter, &memory, b)?;
                    let a_val = resolve_operand(std, layouter, &memory, a)?;
                    let a: AssignedNative<_> = a_val.try_into()?;
                    let b: AssignedNative<_> = b_val.try_into()?;
                    let result = CircuitValue::Native(std.select(layouter, &bit, &a, &b)?);
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
                    let a: AssignedNative<_> = a_val.try_into()?;
                    let b: AssignedNative<_> = b_val.try_into()?;
                    std.assert_equal(layouter, &a, &b)?;
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
                I::Impact { guard: _, inputs } => {
                    for input in inputs {
                        let val_assigned = resolve_operand(std, layouter, &memory, input)?;
                        let x: AssignedNative<_> = val_assigned.try_into()?;
                        pi_push(x, &mut public_inputs)?;
                    }
                }
                I::Output { val } => {
                    let val = resolve_operand(std, layouter, &memory, val)?;
                    outputs.push(val);
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
                    let res_bytes = std.sha2_256(layouter, &bytes)?;
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
                    let a: AssignedNative<_> = a_val.try_into()?;
                    let b: AssignedNative<_> = b_val.try_into()?;
                    let bit = std.is_equal(layouter, &a, &b)?;
                    let result = CircuitValue::Native(std.convert(layouter, &bit)?);
                    mem_insert(output.clone(), result, &mut memory)?;
                }
                I::Add { a, b, output } => {
                    let a_val = resolve_operand(std, layouter, &memory, a)?;
                    let b_val = resolve_operand(std, layouter, &memory, b)?;
                    let a: AssignedNative<_> = a_val.try_into()?;
                    let b: AssignedNative<_> = b_val.try_into()?;
                    let result = CircuitValue::Native(std.add(layouter, &a, &b)?);
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
                I::PublicInput { guard, output } | I::PrivateInput { guard, output } => {
                    let guard = match guard {
                        Some(g) => Some(resolve_operand(std, layouter, &memory, g)?),
                        None => None,
                    };
                    let value = witness.as_ref().map_with_result(|preproc| {
                        let x = preproc
                            .memory
                            .get(output)
                            .cloned()
                            .unwrap_or(IrValue::Native(0.into()));
                        let x: Fr = x.try_into().map_err(|_| {
                            Error::Synthesis(format!(
                                "expected native value for public/private input {:?}",
                                output
                            ))
                        })?;
                        Ok::<_, midnight_proofs::plonk::Error>(x.0)
                    })?;
                    let value_cell = std.assign(layouter, value)?;
                    // If `guard` is Some, then we want to ensure that
                    // `value` is 0 if `guard` is 0
                    // That is: guard == 0 -> value == 0
                    // => value == 0 || guard
                    if let Some(guard) = guard {
                        let value_is_zero = std.is_zero(layouter, &value_cell)?;
                        let guard: AssignedNative<_> = guard.try_into()?;
                        let guard_bit = std.convert(layouter, &guard)?;
                        let is_ok = std.or(layouter, &[value_is_zero, guard_bit])?;
                        let is_ok_field = std.convert(layouter, &is_ok)?;
                        std.assert_non_zero(layouter, &is_ok_field)?;
                    }
                    mem_insert(
                        output.clone(),
                        CircuitValue::Native(value_cell),
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
                I::EcAdd { a, b, output } => {
                    let a_val = resolve_operand(std, layouter, &memory, a)?;
                    let b_val = resolve_operand(std, layouter, &memory, b)?;
                    let a: AssignedNativePoint<JubjubExtended> = a_val.try_into()?;
                    let b: AssignedNativePoint<JubjubExtended> = b_val.try_into()?;
                    let c = std.jubjub().add(layouter, &a, &b)?;
                    mem_insert(output.clone(), CircuitValue::JubjubPoint(c), &mut memory)?;
                }
                I::EcMul {
                    a_x,
                    a_y,
                    scalar,
                    outputs,
                } => {
                    if outputs.len() != 2 {
                        return Err(Error::Synthesis(
                            "Unexpected output length of EcMul instruction".into(),
                        ));
                    }
                    let a_x_val = resolve_operand(std, layouter, &memory, a_x)?;
                    let a_y_val = resolve_operand(std, layouter, &memory, a_y)?;
                    let scalar_val = resolve_operand(std, layouter, &memory, scalar)?;
                    let a_x: AssignedNative<_> = a_x_val.try_into()?;
                    let a_y: AssignedNative<_> = a_y_val.try_into()?;
                    let scalar: AssignedNative<_> = scalar_val.try_into()?;
                    let a = ecc_from_parts(std, layouter, &a_x, &a_y)?;
                    let scalar = std.jubjub().convert(layouter, &scalar)?;
                    let b = std.jubjub().msm(layouter, &[scalar], &[a])?;
                    mem_insert(
                        outputs[0].clone(),
                        CircuitValue::Native(std.jubjub().x_coordinate(&b)),
                        &mut memory,
                    )?;
                    mem_insert(
                        outputs[1].clone(),
                        CircuitValue::Native(std.jubjub().y_coordinate(&b)),
                        &mut memory,
                    )?;
                }
                I::EcMulGenerator { scalar, outputs } => {
                    if outputs.len() != 2 {
                        return Err(Error::Synthesis(
                            "Unexpected output length of EcMulGenerator instruction".into(),
                        ));
                    }
                    let g: AssignedNativePoint<embedded::AffineExtended> = std
                        .jubjub()
                        .assign_fixed(layouter, embedded::Affine::generator())?;
                    let scalar_val = resolve_operand(std, layouter, &memory, scalar)?;
                    let scalar: AssignedNative<_> = scalar_val.try_into()?;
                    let scalar = std.jubjub().convert(layouter, &scalar)?;
                    let b = std.jubjub().msm(layouter, &[scalar], &[g])?;
                    mem_insert(
                        outputs[0].clone(),
                        CircuitValue::Native(std.jubjub().x_coordinate(&b)),
                        &mut memory,
                    )?;
                    mem_insert(
                        outputs[1].clone(),
                        CircuitValue::Native(std.jubjub().y_coordinate(&b)),
                        &mut memory,
                    )?;
                }
                I::HashToCurve { inputs, outputs } => {
                    if outputs.len() != 2 {
                        return Err(Error::Synthesis(
                            "Unexpected output length of HashToCurve instruction".into(),
                        ));
                    }
                    let mut resolved_inputs = Vec::new();
                    for inp in inputs {
                        let x = resolve_operand(std, layouter, &memory, inp)?;
                        let x: AssignedNative<_> = x.try_into()?;
                        resolved_inputs.push(x);
                    }
                    let point = std.hash_to_curve(layouter, &resolved_inputs)?;
                    mem_insert(
                        outputs[0].clone(),
                        CircuitValue::Native(std.jubjub().x_coordinate(&point)),
                        &mut memory,
                    )?;
                    mem_insert(
                        outputs[1].clone(),
                        CircuitValue::Native(std.jubjub().y_coordinate(&point)),
                        &mut memory,
                    )?;
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
                    let x: AssignedNative<_> = val.clone().try_into()?;
                    preimage.push(x);
                }
            }

            for output in &outputs {
                let x: AssignedNative<_> = output.clone().try_into()?;
                preimage.push(x);
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
        let jubjub = self.instructions.iter().any(|op| {
            matches!(
                op,
                I::EcAdd { .. }
                    | I::EcMul { .. }
                    | I::EcMulGenerator { .. }
                    | I::HashToCurve { .. }
            )
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
        ZkStdLibArch {
            jubjub: jubjub || hash_to_curve,
            poseidon: poseidon || hash_to_curve,
            sha2_256,
            sha2_512: false,
            keccak_256: false,
            sha3_256: false,
            blake2b: false,
            nr_pow2range_cols: 1,
            secp256k1: false,
            bls12_381: false,
            base64: false,
            automaton: false,
        }
    }

    fn write_relation<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        Serializable::serialize(&self, writer)
    }

    fn read_relation<R: std::io::Read>(reader: &mut R) -> std::io::Result<Self> {
        Deserializable::deserialize(reader, 0)
    }
}
