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

use super::ir::{Instruction as I, IrSource};
use anyhow::{anyhow, bail};
use base_crypto::fab::{Alignment, AlignmentAtom, AlignmentSegment};
use base_crypto::hash::persistent_hash;
use base_crypto::repr::BinaryHashRepr;
use group::Group;
use midnight_circuits::compact_std_lib::{Relation, ZkStdLib, ZkStdLibArch};
use midnight_circuits::instructions::{
    ArithInstructions, AssertionInstructions, AssignmentInstructions, BinaryInstructions,
    ControlFlowInstructions, ConversionInstructions, DecompositionInstructions, EccInstructions,
    EqualityInstructions, PublicInputInstructions, ZeroInstructions,
};
use midnight_circuits::types::{
    AssignedBit, AssignedByte, AssignedNative, AssignedNativePoint, InnerValue,
};
use midnight_proofs::{
    circuit::{Layouter, Value},
    plonk::Error,
};
use serialize::{Deserializable, Serializable, VecExt};
use std::cmp::Ordering;
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
    pub memory: Vec<outer::Scalar>,
    pub pis: Vec<outer::Scalar>,
    pub pi_skips: Vec<Option<usize>>,
    pub binding_input: outer::Scalar,
    pub comm_comm: Option<(outer::Scalar, outer::Scalar)>,
}

fn lnot(
    std: &ZkStdLib,
    layouter: &mut impl Layouter<outer::Scalar>,
    a: &AssignedNative<outer::Scalar>,
) -> Result<AssignedNative<outer::Scalar>, Error> {
    let bit = std.is_zero(layouter, a)?;
    std.convert(layouter, &bit)
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
                return Err(Error::Synthesis);
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
                return Err(Error::Synthesis);
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
                return Err(Error::Synthesis);
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
            Err(Error::Synthesis)
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
        if preimage.inputs.len() != self.num_inputs as usize {
            bail!(
                "Expected {} inputs, received {}",
                self.num_inputs,
                preimage.inputs.len()
            );
        }
        let mut memory: Vec<_> = preimage.inputs.clone();
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
        let idx = |memory: &[Fr], i: u32| {
            let res = memory
                .get(i as usize)
                .copied()
                .ok_or(anyhow!("index out of bounds: {i}"));
            trace!(?res, "retrieved from {i}");
            res
        };
        let idx_bool = |memory: &[Fr], i: u32| {
            idx(memory, i).and_then(|val| {
                if val == 0.into() {
                    Ok(false)
                } else if val == 1.into() {
                    Ok(true)
                } else {
                    bail!("Expected boolean, found: {val:?}");
                }
            })
        };
        let idx_point = |memory: &[Fr], x: u32, y: u32| {
            let x = idx(memory, x)?;
            let y = idx(memory, y)?;
            EmbeddedGroupAffine::new(x, y)
                .ok_or(anyhow!("Elliptic curve point not on curve: ({x:?}, {y:?})"))
        };
        let idx_bits = |memory: &[Fr], i: u32, constrain: Option<u32>| {
            idx(memory, i).and_then(|val| {
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
            let start_idx = memory.len();
            match ins {
                I::Add { a, b } => memory.push(idx(&memory, *a)? + idx(&memory, *b)?),
                I::Mul { a, b } => memory.push(idx(&memory, *a)? * idx(&memory, *b)?),
                I::Neg { a } => memory.push(-idx(&memory, *a)?),
                I::Not { a } => memory.push((!idx_bool(&memory, *a)?).into()),
                I::ConstrainEq { a, b } => {
                    if idx(&memory, *a)? != idx(&memory, *b)? {
                        bail!(
                            "Failed equality constraint: {:?} != {:?}",
                            idx(&memory, *a)?,
                            idx(&memory, *b)?
                        );
                    }
                }
                I::CondSelect { bit, a, b } => {
                    let (bit, a, b) = (
                        idx_bool(&memory, *bit)?,
                        idx(&memory, *a)?,
                        idx(&memory, *b)?,
                    );
                    memory.push(if bit { a } else { b })
                }
                I::Assert { cond } => {
                    if !idx_bool(&memory, *cond)? {
                        bail!("Failed direct assertion");
                    }
                }
                I::TestEq { a, b } => memory.push((idx(&memory, *a)? == idx(&memory, *b)?).into()),
                I::PublicInput { guard } => {
                    let val = match guard {
                        Some(guard) if !idx_bool(&memory, *guard)? => 0.into(),
                        _ => {
                            public_transcript_outputs_idx += 1;
                            preimage
                                .public_transcript_outputs
                                .get(public_transcript_outputs_idx - 1)
                                .copied()
                                .ok_or(anyhow!("Ran out of public transcript outputs"))?
                        }
                    };
                    memory.push(val);
                }
                I::DeclarePubInput { var } => {
                    pis.push(idx(&memory, *var)?);
                    public_transcript_inputs_idx += 1;
                }
                I::PrivateInput { guard } => match guard {
                    Some(guard) if !idx_bool(&memory, *guard)? => memory.push(0.into()),
                    _ => {
                        memory.push(
                            preimage
                                .private_transcript
                                .get(private_transcript_outputs_idx)
                                .copied()
                                .ok_or(anyhow!("Ran out of private transcript outputs"))?,
                        );
                        private_transcript_outputs_idx += 1;
                    }
                },
                I::Copy { var } => memory.push(idx(&memory, *var)?),
                I::ConstrainToBoolean { var } => drop(idx_bool(&memory, *var)?),
                I::ConstrainBits { var, bits } => drop(idx_bits(&memory, *var, Some(*bits))?),
                I::DivModPowerOfTwo { var, bits } => {
                    if *bits as usize > FR_BYTES_STORED * 8 {
                        bail!("Excessive bit count");
                    }
                    let var_bits = idx_bits(&memory, *var, None)?;
                    memory.push(from_bits(var_bits[*bits as usize..].iter().copied()));
                    memory.push(from_bits(var_bits[..*bits as usize].iter().copied()));
                }
                I::ReconstituteField {
                    divisor,
                    modulus,
                    bits,
                } => {
                    if *bits as usize > FR_BYTES_STORED * 8 {
                        bail!("Excessive bit count");
                    }
                    let fr_max = Fr::from(-1);
                    let max_bits = idx_bits(&[fr_max], 0, None)?;
                    let modulus_bits = idx_bits(&memory, *modulus, Some(*bits))?;
                    let divisor_bits = idx_bits(&memory, *divisor, Some(FR_BITS as u32 - *bits))?;
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
                    memory.push(power * idx(&memory, *divisor)? + idx(&memory, *modulus)?);
                }
                I::LessThan { a, b, bits } => memory.push(
                    (from_bits(idx_bits(&memory, *a, Some(*bits))?.into_iter())
                        < from_bits(idx_bits(&memory, *b, Some(*bits))?.into_iter()))
                    .into(),
                ),
                I::TransientHash { inputs } => memory.push(transient_hash(
                    &inputs
                        .iter()
                        .map(|i| idx(&memory, *i))
                        .collect::<Result<Vec<_>, _>>()?,
                )),
                I::PersistentHash { alignment, inputs } => {
                    let inputs = inputs
                        .iter()
                        .map(|i| idx(&memory, *i))
                        .collect::<Result<Vec<_>, _>>()?;
                    let value = alignment.parse_field_repr(&inputs).ok_or_else(|| {
                        error!("Inputs did not match alignment (inputs: {inputs:?}, alignment: {alignment:?})");
                        anyhow!("Inputs did not match alignment (inputs: {inputs:?}, alignment: {alignment:?})")
                    })?;
                    let mut repr = Vec::new();
                    ValueReprAlignedValue(value).binary_repr(&mut repr);
                    trace!(bytes = ?repr, "bytes decoded out-of-circuit");
                    let hash = persistent_hash(&repr);
                    memory.extend(hash.field_vec());
                }
                I::PiSkip { guard, count } => match guard {
                    Some(guard) if !idx_bool(&memory, *guard)? => {
                        pi_skips.push(Some(*count as usize));
                        public_transcript_inputs_idx -= *count as usize;
                    }
                    _ => {
                        pi_skips.push(None);
                        for i in 0..(*count as usize) {
                            let idx = public_transcript_inputs_idx - *count as usize + i;
                            let expected = preimage.public_transcript_inputs.get(idx).copied();
                            let computed = Some(pis[pis.len() - *count as usize + i]);
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
                },
                I::LoadImm { imm } => memory.push(*imm),
                I::Output { var } => outputs.push(idx(&memory, *var)?),
                I::EcAdd { a_x, a_y, b_x, b_y } => memory.extend(from_point(
                    idx_point(&memory, *a_x, *a_y)? + idx_point(&memory, *b_x, *b_y)?,
                )),
                I::HashToCurve { inputs } => {
                    let inputs = inputs
                        .iter()
                        .map(|var| idx(&memory, *var))
                        .collect::<Result<Vec<_>, _>>()?;
                    memory.extend(from_point(hash_to_curve(&inputs)))
                }
                I::EcMul { a_x, a_y, scalar } => memory.extend(from_point(
                    idx_point(&memory, *a_x, *a_y)? * idx(&memory, *scalar)?,
                )),
                I::EcMulGenerator { scalar } => memory.extend(from_point(
                    EmbeddedGroupAffine::generator() * idx(&memory, *scalar)?,
                )),
            }
            trace!(delta = ?memory[start_idx..], "Memory delta {}..{}", start_idx, memory.len());
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
            comm_comm_inputs.extend(outputs.iter());
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
            memory: memory.into_iter().map(|x| x.0).collect(),
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

    fn format_instance(instance: &Self::Instance) -> Vec<outer::Scalar> {
        instance.clone()
    }

    fn circuit(
        &self,
        std: &ZkStdLib,
        layouter: &mut impl Layouter<outer::Scalar>,
        _instance: Value<Self::Instance>,
        witness: Value<Self::Witness>,
    ) -> Result<(), Error> {
        let input_values = witness
            .as_ref()
            .map(|preproc| preproc.memory[..self.num_inputs as usize].to_vec());
        let binding_input_value = witness.as_ref().map(|preproc| preproc.binding_input);
        let comm_comm_value = witness.as_ref().map(|preproc| preproc.comm_comm);

        let mut memory = std.assign_many(
            layouter,
            &input_values.transpose_vec(self.num_inputs as usize),
        )?;

        let inputs = memory.clone();
        let binding_input = std.assign(layouter, binding_input_value)?;

        let mut outputs = Vec::new();

        fn idx(
            memory: &[AssignedNative<outer::Scalar>],
            i: u32,
        ) -> Result<&AssignedNative<outer::Scalar>, Error> {
            memory.get(i as usize).ok_or(Error::Synthesis)
        }
        let seq_push = |cell: AssignedNative<outer::Scalar>,
                        mem: &mut Vec<AssignedNative<outer::Scalar>>,
                        seq: for<'a> fn(&'a Preprocessed) -> &'a [outer::Scalar]|
         -> Result<(), Error> {
            let idx = mem.len();

            witness.as_ref()
                .zip(cell.value())
                .error_if_known_and(|(preproc, v)| {
                    if idx < seq(preproc).len() && seq(preproc)[idx] != **v {
                        error!(prepare = ?seq(preproc), ?idx, ?v, "Misalignment between `prepare` and `synthesize` runs. This is a bug.");
                        true
                    } else {
                        false
                    }
                })?;

            mem.push(cell);
            Ok(())
        };

        let mem_push =
            |cell: AssignedNative<outer::Scalar>,
             mem: &mut Vec<AssignedNative<outer::Scalar>>|
             -> Result<(), Error> { seq_push(cell, mem, |preproc| &preproc.memory) };

        let pi_push = |cell: AssignedNative<outer::Scalar>,
                       pis: &mut Vec<AssignedNative<outer::Scalar>>|
         -> Result<(), Error> { seq_push(cell, pis, |preproc| &preproc.pis) };

        let mut public_inputs = vec![];
        pi_push(binding_input, &mut public_inputs)?;

        if self.do_communications_commitment {
            let comm_comm_value = comm_comm_value.map(|c| {
                c.ok_or_else(|| {
                    error!("Communication commitment not present despite preproc. This is a bug.");
                    Error::Synthesis
                })
                .unwrap()
                .0
            });
            let comm_comm = std.assign(layouter, comm_comm_value)?;
            pi_push(comm_comm, &mut public_inputs)?;
        }
        for ins in self.instructions.iter() {
            match ins {
                I::Assert { cond } => std.assert_non_zero(layouter, idx(&memory, *cond)?)?,
                I::CondSelect { bit, a, b } => {
                    let bit = std.is_zero(layouter, idx(&memory, *bit)?)?;
                    // Note that b comes first here, because the is_zero negates the bit.
                    // The negation is to ensure the bit bound. This may be
                    // excessive, but user input could violate it otherwise.
                    let result =
                        std.select(layouter, &bit, idx(&memory, *b)?, idx(&memory, *a)?)?;
                    mem_push(result, &mut memory)?;
                }
                I::ConstrainBits { var, bits } => drop(std.assigned_to_le_bits(
                    layouter,
                    idx(&memory, *var)?,
                    Some(*bits as usize),
                    *bits as usize >= FR_BITS,
                )?),
                I::ConstrainEq { a, b } => {
                    std.assert_equal(layouter, idx(&memory, *a)?, idx(&memory, *b)?)?
                }
                I::ConstrainToBoolean { var } => {
                    // Yes, this does insert a constraint.
                    let _: AssignedBit<_> = std.convert(layouter, idx(&memory, *var)?)?;
                }
                I::Copy { var } => mem_push(idx(&memory, *var)?.clone(), &mut memory)?,
                I::DeclarePubInput { var } => {
                    pi_push(idx(&memory, *var)?.clone(), &mut public_inputs)?
                }
                I::PiSkip { .. } => {}
                I::LoadImm { imm } => mem_push(std.assign_fixed(layouter, imm.0)?, &mut memory)?,
                I::Output { var } => outputs.push(idx(&memory, *var)?.clone()),
                I::TransientHash { inputs } => mem_push(
                    std.poseidon(
                        layouter,
                        &inputs
                            .iter()
                            .map(|inp| idx(&memory, *inp).cloned())
                            .collect::<Result<Vec<_>, _>>()?,
                    )?,
                    &mut memory,
                )?,
                I::PersistentHash { alignment, inputs } => {
                    let inputs = inputs
                        .iter()
                        .map(|i| idx(&memory, *i).cloned())
                        .collect::<Result<Vec<_>, _>>()?;
                    let bytes = fab_decode_to_bytes(std, layouter, alignment, &inputs)?;
                    let res_bytes = std.sha256(layouter, &bytes)?;
                    mem_push(std.convert(layouter, &res_bytes[31])?, &mut memory)?;
                    mem_push(
                        assemble_bytes(std, layouter, &res_bytes[..31])?,
                        &mut memory,
                    )?;
                }
                I::TestEq { a, b } => {
                    let bit = std.is_equal(layouter, idx(&memory, *a)?, idx(&memory, *b)?)?;
                    mem_push(std.convert(layouter, &bit)?, &mut memory)?;
                }
                I::Add { a, b } => mem_push(
                    std.add(layouter, idx(&memory, *a)?, idx(&memory, *b)?)?,
                    &mut memory,
                )?,
                I::Mul { a, b } => mem_push(
                    std.mul(layouter, idx(&memory, *a)?, idx(&memory, *b)?, None)?,
                    &mut memory,
                )?,
                I::Neg { a } => mem_push(std.neg(layouter, idx(&memory, *a)?)?, &mut memory)?,
                I::Not { a } => mem_push(lnot(std, layouter, idx(&memory, *a)?)?, &mut memory)?,
                I::LessThan { a, b, bits } => {
                    // Adding mod 2 to meet library constraint that this is even
                    // Hidden req that this is >= 4
                    let bit = std.lower_than(
                        layouter,
                        idx(&memory, *a)?,
                        idx(&memory, *b)?,
                        u32::max(*bits + *bits % 2, 4),
                    )?;
                    mem_push(std.convert(layouter, &bit)?, &mut memory)?;
                }
                I::PublicInput { guard } | I::PrivateInput { guard } => {
                    let guard = guard.map(|guard| idx(&memory, guard)).transpose()?;
                    witness.error_if_known_and(|preproc| memory.len() > preproc.memory.len())?;
                    let value = witness.as_ref().map(|preproc| preproc.memory[memory.len()]);
                    let value_cell = std.assign(layouter, value)?;
                    // If `guard` is Some, then we want to ensure that
                    // `value` is 0 if `guard` is 0
                    // That is: guard == 0 -> value == 0
                    // => value == 0 || guard
                    if let Some(guard) = guard {
                        let value_is_zero = std.is_zero(layouter, &value_cell)?;
                        let guard_bit = std.convert(layouter, guard)?;
                        let is_ok = std.or(layouter, &[value_is_zero, guard_bit])?;
                        let is_ok_field = std.convert(layouter, &is_ok)?;
                        std.assert_non_zero(layouter, &is_ok_field)?;
                    }
                    mem_push(value_cell, &mut memory)?;
                }
                I::DivModPowerOfTwo { var, bits } => {
                    let var = idx(&memory, *var)?;
                    let var_bits = std.assigned_to_le_bits(layouter, var, None, true)?;
                    let modulus =
                        std.assigned_from_le_bits(layouter, &var_bits[..*bits as usize])?;

                    let divisor =
                        std.assigned_from_le_bits(layouter, &var_bits[*bits as usize..])?;

                    mem_push(divisor, &mut memory)?;
                    mem_push(modulus, &mut memory)?;
                }
                I::ReconstituteField {
                    divisor,
                    modulus,
                    bits,
                } => {
                    let divisor_bits = std.assigned_to_le_bits(
                        layouter,
                        idx(&memory, *divisor)?,
                        Some(FR_BITS - *bits as usize),
                        true,
                    )?;
                    let modulus_bits = std.assigned_to_le_bits(
                        layouter,
                        idx(&memory, *modulus)?,
                        Some(*bits as usize),
                        true,
                    )?;
                    let reconstituted = std
                        .assigned_from_le_bits(layouter, &[modulus_bits, divisor_bits].concat())?;
                    mem_push(reconstituted, &mut memory)?;
                }
                I::EcAdd { a_x, a_y, b_x, b_y } => {
                    let a =
                        ecc_from_parts(std, layouter, idx(&memory, *a_x)?, idx(&memory, *a_y)?)?;
                    let b =
                        ecc_from_parts(std, layouter, idx(&memory, *b_x)?, idx(&memory, *b_y)?)?;
                    let c = std.jubjub().add(layouter, &a, &b)?;
                    mem_push(std.jubjub().x_coordinate(&c), &mut memory)?;
                    mem_push(std.jubjub().y_coordinate(&c), &mut memory)?;
                }
                I::EcMul { a_x, a_y, scalar } => {
                    let a =
                        ecc_from_parts(std, layouter, idx(&memory, *a_x)?, idx(&memory, *a_y)?)?;
                    let scalar = std.jubjub().convert(layouter, idx(&memory, *scalar)?)?;
                    let b = std.jubjub().msm(layouter, &[scalar], &[a])?;
                    mem_push(std.jubjub().x_coordinate(&b), &mut memory)?;
                    mem_push(std.jubjub().y_coordinate(&b), &mut memory)?;
                }
                I::EcMulGenerator { scalar } => {
                    let g: AssignedNativePoint<embedded::AffineExtended> = std
                        .jubjub()
                        .assign_fixed(layouter, embedded::Affine::generator())?;
                    let scalar = std.jubjub().convert(layouter, idx(&memory, *scalar)?)?;
                    let b = std.jubjub().msm(layouter, &[scalar], &[g])?;
                    mem_push(std.jubjub().x_coordinate(&b), &mut memory)?;
                    mem_push(std.jubjub().y_coordinate(&b), &mut memory)?;
                }
                I::HashToCurve { inputs } => {
                    let inputs = inputs
                        .iter()
                        .map(|input| idx(&memory, *input).cloned())
                        .collect::<Result<Vec<_>, _>>()?;
                    let point = std.hash_to_curve(layouter, &inputs)?;
                    mem_push(std.jubjub().x_coordinate(&point), &mut memory)?;
                    mem_push(std.jubjub().y_coordinate(&point), &mut memory)?;
                }
            }
        }
        if self.do_communications_commitment {
            let comm_comm_rand_value = comm_comm_value.map(|c| {
                c.ok_or_else(|| {
                    error!("Communication commitment not present despite preproc. This is a bug.");
                    Error::Synthesis
                })
                .unwrap()
                .1
            });
            let comm_comm_rand = std.assign(layouter, comm_comm_rand_value)?;

            let mut preimage = vec![comm_comm_rand];
            preimage.extend(inputs.iter().cloned());
            preimage.extend(outputs.iter().cloned());
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
        use midnight_circuits::compact_std_lib::ShaTableSize;
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
        let sha256 = self
            .instructions
            .iter()
            .any(|op| matches!(op, I::PersistentHash { .. }));
        ZkStdLibArch {
            jubjub: jubjub || hash_to_curve,
            poseidon: poseidon || hash_to_curve,
            sha256: if sha256 {
                Some(ShaTableSize::Table11)
            } else {
                None
            },
            secp256k1: false,
            bls12_381: false,
            base64: false,
        }
    }

    fn write_relation<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        Serializable::serialize(&self, writer)
    }

    fn read_relation<R: std::io::Read>(reader: &mut R) -> std::io::Result<Self> {
        Deserializable::deserialize(reader, 0)
    }
}
