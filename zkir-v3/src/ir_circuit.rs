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

//! ZKIR circuit synthesis — the ZK constraint generation pass.
//!
//! [`Relation`] for [`IrSource`] walks the instruction sequence inside the
//! proving backend, producing constrained witness assignments and public-input
//! commitments. The [`Preprocessed`] witness (from [`crate::ir_preprocess`])
//! supplies the concrete values that the circuit constrains.

use std::collections::HashMap;

use base_crypto::fab::{Alignment, AlignmentAtom, AlignmentSegment};
use group::Group;
use midnight_circuits::instructions::RangeCheckInstructions;
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
use num_bigint::BigUint;
use serialize::{Deserializable, Serializable, VecExt};
use transient_crypto::curve::outer;
use transient_crypto::curve::{FR_BITS, FR_BYTES_STORED, Fr};
use transient_crypto::fab::AlignmentExt;
use transient_crypto::repr::FieldRepr;

use crate::ir::{Identifier, Instruction as I, IrSource, Operand};
use crate::ir_instructions::add::add_incircuit;
use crate::ir_instructions::assign::assign_incircuit;
use crate::ir_instructions::decode::decode_incircuit;
use crate::ir_instructions::encode::encode_incircuit;
use crate::ir_preprocess::Preprocessed;
use crate::ir_types::{CircuitValue, IrType, IrValue};

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
                    "cannot decode bytes from too little data".into(),
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

        // The communications commitment is always the second public input
        // (the verifier unconditionally includes it in its PI vector).
        // `do_communications_commitment` controls whether the value is
        // *constrained* via Poseidon, not whether it appears as a PI.
        {
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
        // Pre-populate unguarded PublicInput output variables so that
        // Impact read_results can reference them. Impact must precede
        // PublicInput in instruction order (the executor populates
        // public_transcript_outputs from Impact's Popeq results), but the
        // circuit synthesis needs the values when resolving read_results
        // operands in push_operand_pi!.
        for ins in self.instructions.iter() {
            if let I::PublicInput {
                guard: None,
                val_t,
                output,
            } = ins
            {
                let value = witness.as_ref().map_with_result(|preproc| {
                    preproc
                        .memory
                        .get(output)
                        .cloned()
                        .ok_or(Error::Synthesis(format!(
                            "Pre-populate: {:?} not found in witness memory",
                            output
                        )))
                })?;
                mem_insert(
                    output.clone(),
                    assign_incircuit(std, layouter, val_t, &[value])?[0].clone(),
                    &mut memory,
                )?;
            }
        }

        let mut contract_call_idx: usize = 0;
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
                    let val_assigned = resolve_operand(std, layouter, &memory, val)?;
                    let x: AssignedNative<_> = val_assigned.try_into()?;
                    let _: AssignedBit<_> = std.convert(layouter, &x)?;
                }
                I::Copy { val, output } => {
                    let val = resolve_operand(std, layouter, &memory, val)?;
                    mem_insert(output.clone(), val, &mut memory)?;
                }
                I::Impact {
                    guard,
                    ops,
                    read_results,
                } => {
                    // Mirrors zkir_ops_to_field_elements in-circuit.
                    // Every field element is gated via select(guard, val, zero).
                    use crate::zkir_mode::ZkirKey;
                    use onchain_vm::ops::Op;

                    let impact_zero = std.assign_fixed(layouter, outer::Scalar::from(0))?;
                    let impact_guard: AssignedBit<_> = {
                        let guard_val = resolve_operand(std, layouter, &memory, guard)?;
                        let guard_native: AssignedNative<_> = guard_val.try_into()?;
                        std.convert(layouter, &guard_native)?
                    };

                    // Macros used instead of closures because closures cannot
                    // use `impl Trait` parameters (needed for Layouter).
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
                                let guarded = std.select(
                                    layouter,
                                    &impact_guard,
                                    &assigned,
                                    &impact_zero,
                                )?;
                                pi_push(guarded, &mut public_inputs)?;
                            }
                        }};
                    }

                    let mut popeq_idx: usize = 0;

                    for op in ops {
                        match op {
                            // --- Fixed-opcode variants (no payload) ---
                            Op::Noop { n } => {
                                for _ in 0..*n {
                                    push_const_pi!(0);
                                }
                            }
                            Op::Lt => push_const_pi!(0x01),
                            Op::Eq => push_const_pi!(0x02),
                            Op::Type => push_const_pi!(0x03),
                            Op::Size => push_const_pi!(0x04),
                            Op::New => push_const_pi!(0x05),
                            Op::And => push_const_pi!(0x06),
                            Op::Or => push_const_pi!(0x07),
                            Op::Neg => push_const_pi!(0x08),
                            Op::Log => push_const_pi!(0x09),
                            Op::Root => push_const_pi!(0x0a),
                            Op::Pop => push_const_pi!(0x0b),
                            Op::Add => push_const_pi!(0x14),
                            Op::Sub => push_const_pi!(0x15),
                            Op::Member => push_const_pi!(0x18),
                            Op::Ckpt => push_const_pi!(0xff),

                            // --- Popeq: opcode + read-result operands ---
                            Op::Popeq { cached, .. } => {
                                push_const_pi!((0x0cu8 + *cached as u8));
                                let rr = read_results.get(popeq_idx).ok_or_else(|| {
                                    Error::Synthesis(format!(
                                        "Impact synthesis: more Popeq ops than read_results (#{popeq_idx})"
                                    ))
                                })?;
                                for operand in rr {
                                    push_operand_pi!(operand);
                                }
                                popeq_idx += 1;
                            }

                            // --- Addi / Subi: opcode + immediate ---
                            Op::Addi { immediate } => {
                                push_const_pi!(0x0e);
                                push_const_pi!(*immediate);
                            }
                            Op::Subi { immediate } => {
                                push_const_pi!(0x0f);
                                push_const_pi!(*immediate);
                            }

                            // --- Push: opcode + Cell tag + alignment + resolved operands ---
                            Op::Push { storage, value } => {
                                push_const_pi!((0x10u8 + *storage as u8));
                                push_const_pi!(1u64); // StateValue::Cell tag
                                push_alignment_pi!(&value.alignment);
                                for operand in &value.operands {
                                    push_operand_pi!(operand);
                                }
                            }

                            // --- Branch / Jmp: opcode + skip ---
                            Op::Branch { skip } => {
                                push_const_pi!(0x12);
                                push_const_pi!(*skip);
                            }
                            Op::Jmp { skip } => {
                                push_const_pi!(0x13);
                                push_const_pi!(*skip);
                            }

                            // --- Concat: opcode + n ---
                            Op::Concat { cached, n } => {
                                push_const_pi!((0x16u8 + *cached as u8));
                                push_const_pi!(*n);
                            }

                            // --- Rem: single opcode ---
                            Op::Rem { cached } => {
                                push_const_pi!((0x19u8 + *cached as u8));
                            }

                            // --- Dup / Swap: opcode encodes n in lower nibble ---
                            Op::Dup { n } => push_const_pi!((0x30u8 | *n)),
                            Op::Swap { n } => push_const_pi!((0x40u8 | *n)),

                            // --- Idx: opcode + key field reprs ---
                            Op::Idx {
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
                                                // Key::Stack encodes as -1.
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
                                            ZkirKey::Value { alignment, operands } => {
                                                push_alignment_pi!(alignment);
                                                for operand in operands {
                                                    push_operand_pi!(operand);
                                                }
                                            }
                                        }
                                    }
                                }
                            }

                            // --- Ins: opcode encodes cached + n ---
                            Op::Ins { cached, n } => {
                                let base: u8 = if *cached { 0xa0 } else { 0x90 };
                                push_const_pi!((base | *n));
                            }

                            _ => {
                                return Err(Error::Synthesis(
                                    "unsupported Op variant in structured Impact circuit synthesis"
                                        .into(),
                                ));
                            }
                        }
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
                    // Skip if already pre-populated (unguarded PublicInput
                    // outputs are pre-assigned before the instruction loop
                    // so Impact read_results can reference them).
                    if memory.contains_key(output) {
                        continue;
                    }
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
                    let scalar: AssignedNative<_> = scalar_val.try_into()?;
                    let scalar = std.jubjub().convert(layouter, &scalar)?;
                    let b = std.jubjub().msm(layouter, &[scalar], &[a])?;
                    mem_insert(output.clone(), CircuitValue::JubjubPoint(b), &mut memory)?;
                }
                I::EcMulGenerator { scalar, output } => {
                    let g: AssignedNativePoint<JubjubExtended> = std
                        .jubjub()
                        .assign_fixed(layouter, JubjubSubgroup::generator())?;
                    let scalar_val = resolve_operand(std, layouter, &memory, scalar)?;
                    let scalar: AssignedNative<_> = scalar_val.try_into()?;
                    let scalar = std.jubjub().convert(layouter, &scalar)?;
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
                I::ContractCall {
                    contract_ref,
                    expected_type: _,
                    entry_point,
                    args,
                    outputs,
                } => {
                    // ── 1. Assign callee output values from witness ──
                    for out_id in outputs {
                        let value =
                            witness.as_ref().map_with_result(|preproc| {
                                preproc.memory.get(out_id).cloned().ok_or(Error::Synthesis(
                                    format!(
                                        "ContractCall output {:?} not found in witness memory",
                                        out_id
                                    ),
                                ))
                            })?;
                        mem_insert(
                            out_id.clone(),
                            // TODO: Eventually shouldn't assign everything &IrType::Native - use actual type of output.
                            assign_incircuit(std, layouter, &IrType::Native, &[value])?[0].clone(),
                            &mut memory,
                        )?;
                    }

                    // ── 2. Resolve contract_ref to constrained addr ──
                    let (addr_hi_op, addr_lo_op) = contract_ref;
                    let addr_hi: AssignedNative<_> =
                        resolve_operand(std, layouter, &memory, addr_hi_op)?.try_into()?;
                    let addr_lo: AssignedNative<_> =
                        resolve_operand(std, layouter, &memory, addr_lo_op)?.try_into()?;

                    // ── 3. Compute ep_hash as a circuit constant ──
                    let ep_hash = crate::ir_preprocess::compute_ep_hash(entry_point);
                    let ep_hash_fields = ep_hash.field_vec();
                    let ep_hash_hi = std.assign_fixed(layouter, ep_hash_fields[0].0)?;
                    let ep_hash_lo = std.assign_fixed(layouter, ep_hash_fields[1].0)?;

                    // ── 4. Compute comm_comm via in-circuit Poseidon ──
                    // comm_comm = Poseidon(comm_rand, args..., outputs...)
                    let comm_rand_val = witness.as_ref().map_with_result(|preproc| {
                        preproc
                            .contract_call_comm_rands
                            .get(contract_call_idx)
                            .map(|fr| fr.0)
                            .ok_or(Error::Synthesis(format!(
                                "ContractCall comm_rand[{}] out of range (len={})",
                                contract_call_idx,
                                preproc.contract_call_comm_rands.len()
                            )))
                    })?;
                    let comm_rand = std.assign(layouter, comm_rand_val)?;
                    contract_call_idx += 1;

                    let mut poseidon_preimage = vec![comm_rand];
                    for arg in args {
                        let val: AssignedNative<_> =
                            resolve_operand(std, layouter, &memory, arg)?.try_into()?;
                        poseidon_preimage.push(val);
                    }
                    for out_id in outputs {
                        let val: AssignedNative<_> = idx(&memory, out_id)?.clone().try_into()?;
                        poseidon_preimage.push(val);
                    }
                    let comm_comm = std.poseidon(layouter, &poseidon_preimage)?;

                    // ── 5. Build claim PIs: constants from witness, variable ──
                    // positions constrained to in-circuit values.
                    let claim_count = crate::ir_preprocess::claim_ops_field_count();
                    let mut claim_pis: Vec<AssignedNative<_>> = Vec::with_capacity(claim_count);
                    for i in 0..claim_count {
                        let pi_idx = public_inputs.len() + i;
                        let val = witness.as_ref().map_with_result(|preproc| {
                            preproc.pis.get(pi_idx).copied().ok_or(Error::Synthesis(
                                format!(
                                    "ContractCall claim ops: pis[{}] out of range (pis.len()={})",
                                    pi_idx,
                                    preproc.pis.len()
                                ),
                            ))
                        })?;
                        claim_pis.push(std.assign(layouter, val)?);
                    }

                    // Constrain the variable positions to match in-circuit values.
                    use crate::ir_preprocess::{
                        CLAIM_ADDR_HI_OFFSET, CLAIM_ADDR_LO_OFFSET,
                        CLAIM_EP_HASH_HI_OFFSET, CLAIM_EP_HASH_LO_OFFSET,
                        CLAIM_COMM_COMM_OFFSET,
                    };
                    std.assert_equal(layouter, &claim_pis[CLAIM_ADDR_HI_OFFSET], &addr_hi)?;
                    std.assert_equal(layouter, &claim_pis[CLAIM_ADDR_LO_OFFSET], &addr_lo)?;
                    std.assert_equal(layouter, &claim_pis[CLAIM_EP_HASH_HI_OFFSET], &ep_hash_hi)?;
                    std.assert_equal(layouter, &claim_pis[CLAIM_EP_HASH_LO_OFFSET], &ep_hash_lo)?;
                    std.assert_equal(layouter, &claim_pis[CLAIM_COMM_COMM_OFFSET], &comm_comm)?;

                    for assigned in claim_pis {
                        pi_push(assigned, &mut public_inputs)?;
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
        let involves_types = |target_types: &[IrType]| -> bool {
            let types_in_inputs = self
                .inputs
                .iter()
                .any(|id| target_types.contains(&id.val_t));

            let types_in_instructions = self.instructions.iter().any(|op| match op {
                I::Decode { val_t, .. } => target_types.contains(val_t),
                _ => false,
            });

            types_in_inputs || types_in_instructions
        };

        let jubjub = involves_types(&[IrType::JubjubPoint])
            || self.instructions.iter().any(|op| {
                matches!(op, I::EcMul { .. } | I::EcMulGenerator { .. } | I::HashToCurve { .. })
            });
        let hash_to_curve = self
            .instructions
            .iter()
            .any(|op| matches!(op, I::HashToCurve { .. }));
        let poseidon = self.do_communications_commitment
            || self
                .instructions
                .iter()
                .any(|op| matches!(op, I::TransientHash { .. } | I::ContractCall { .. }));
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
