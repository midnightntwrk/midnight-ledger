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

//! ZKIR preprocessing — the off-circuit pre-proving pass.
//!
//! [`IrSource::preprocess`] walks the instruction sequence against concrete
//! field-element inputs, verifying that all constraints hold and collecting
//! the public-input vector and skip metadata needed by the circuit.

use std::collections::HashMap;

use anyhow::{anyhow, bail};
use base_crypto::fab::{Alignment, AlignmentAtom};
use base_crypto::hash::{HashOutput, persistent_commit};
use transient_crypto::curve::{Fr, outer};
use transient_crypto::fab::AlignmentExt;
use transient_crypto::hash::transient_commit;
use transient_crypto::proofs::{ProofPreimage, ProvingError};
use transient_crypto::repr::FieldRepr;

use crate::ir::{Identifier, Instruction as I, IrSource};
use crate::ir_eval::{
    eval_computational_instruction, eval_operand, eval_operand_bool, eval_operand_fr,
};
use crate::ir_instructions::decode::decode_offcircuit;
use crate::ir_instructions::encode::encode_offcircuit;
use crate::ir_types::IrValue;
use crate::zkir_mode::zkir_ops_to_field_elements_with_sizes;

/// The number of individual `Op` objects produced by `kernel_claim_contract_call!`.
/// These are: Swap, Idx, Dup, Size, Push(Cell), Concat, Push(Null), Ins, Swap.
pub(crate) const CLAIM_OPS_COUNT: usize = 9;

/// Compute the number of `public_transcript_inputs` field elements produced
/// by the `kernel_claim_contract_call!` ops for a single cross-contract call.
///
/// The claim ops encode: (addr: ContractAddress[32 bytes], ep_hash: HashOutput[32 bytes],
/// comm_comm: Fr). The structure is fixed, so the field count is deterministic.
///
/// This mirrors the field encoding of `kernel_claim_contract_call!` in
/// `onchain-runtime/vendored/program_fragments.rs`.
pub(crate) fn claim_ops_field_count() -> usize {
    // Alignment for the concatenated (addr, ep_hash, comm_comm) in the Push op:
    let addr_align = Alignment::singleton(AlignmentAtom::Bytes { length: 32 });
    let hash_align = Alignment::singleton(AlignmentAtom::Bytes { length: 32 });
    let field_align = Alignment::singleton(AlignmentAtom::Field);
    let concat_alignment = Alignment::concat([&addr_align, &hash_align, &field_align]);
    let concat_av_field_size = concat_alignment.field_size() + concat_alignment.field_len();

    // AlignedValue::from(3u8) key in the Idx op:
    let key_3u8_alignment = Alignment::singleton(AlignmentAtom::Bytes { length: 1 });
    let key_3u8_field_size = key_3u8_alignment.field_size() + key_3u8_alignment.field_len();

    // Claim ops and their field sizes:
    // 1. Swap{0}                              → 1
    // 2. Idx{cached:true, push_path:true, path:[Value(3u8)]} → 1 (opcode) + key_field_size
    // 3. Dup{0}                               → 1
    // 4. Size                                 → 1
    // 5. Push{storage:false, Cell(concat_av)} → 1 (opcode) + 1 (Cell tag) + concat_av_field_size
    // 6. Concat{cached:true, n:160}           → 2
    // 7. Push{storage:false, Null}            → 1 (opcode) + 1 (Null tag)
    // 8. Ins{cached:true, n:2}                → 1
    // 9. Swap{0}                              → 1

    1                               // Swap
        + 1 + key_3u8_field_size    // Idx
        + 1                         // Dup
        + 1                         // Size
        + 1 + 1 + concat_av_field_size  // Push(Cell)
        + 2                         // Concat
        + 1 + 1                     // Push(Null)
        + 1                         // Ins
        + 1 // Swap
}

/// Offsets of the variable fields within the 24 claim-ops field elements.
pub(crate) const CLAIM_ADDR_HI_OFFSET: usize = 13;
pub(crate) const CLAIM_ADDR_LO_OFFSET: usize = 14;
pub(crate) const CLAIM_EP_HASH_HI_OFFSET: usize = 15;
pub(crate) const CLAIM_EP_HASH_LO_OFFSET: usize = 16;
pub(crate) const CLAIM_COMM_COMM_OFFSET: usize = 17;

/// Compute the entry-point hash from the entry-point string.
///
/// Equivalent to `EntryPointBuf(entry_point.as_bytes().to_vec()).ep_hash()`
/// but avoids the `onchain-state` dependency.
pub(crate) fn compute_ep_hash(entry_point: &str) -> HashOutput {
    persistent_commit(
        entry_point.as_bytes(),
        HashOutput(*b"midnight:entry-point\0\0\0\0\0\0\0\0\0\0\0\0"),
    )
}

/// Compute the 24 field elements that encode the `kernel_claim_contract_call!`
/// ops for a single cross-contract call.
pub(crate) fn compute_claim_field_elements(
    addr_hi: Fr,
    addr_lo: Fr,
    ep_hash_hi: Fr,
    ep_hash_lo: Fr,
    comm_comm: Fr,
) -> Vec<Fr> {
    vec![
        Fr::from(0x40u64), // [0]  Swap{0}
        Fr::from(0x80u64), // [1]  Idx{cached:true, push_path:true, 1 key}
        Fr::from(1u64),    // [2]  Key alignment: 1 segment
        Fr::from(1u64),    // [3]  Key Bytes{1} length
        Fr::from(3u64),    // [4]  Key value: 3u8
        Fr::from(0x30u64), // [5]  Dup{0}
        Fr::from(0x04u64), // [6]  Size
        Fr::from(0x10u64), // [7]  Push{storage:false}
        Fr::from(1u64),    // [8]  StateValue::Cell tag
        Fr::from(3u64),    // [9]  Alignment: 3 segments
        Fr::from(32u64),   // [10] Alignment: Bytes{32} (addr)
        Fr::from(32u64),   // [11] Alignment: Bytes{32} (ep_hash)
        Fr::from(-2i32),   // [12] Alignment: Field (comm_comm)
        addr_hi,           // [13] Address field element 0
        addr_lo,           // [14] Address field element 1
        ep_hash_hi,        // [15] Entry point hash field element 0
        ep_hash_lo,        // [16] Entry point hash field element 1
        comm_comm,         // [17] Communication commitment
        Fr::from(0x17u64), // [18] Concat{cached:true}
        Fr::from(160u64),  // [19] Concat n=160
        Fr::from(0x10u64), // [20] Push{storage:false}
        Fr::from(0u64),    // [21] StateValue::Null tag
        Fr::from(0xa2u64), // [22] Ins{cached:true, n:2}
        Fr::from(0x40u64), // [23] Swap{0}
    ]
}

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
    /// Per-ContractCall communication commitment randomness, extracted from
    /// `private_transcript` during preprocessing. The circuit uses these to
    /// compute and constrain `comm_comm = Poseidon(comm_rand, args..., outputs...)`.
    pub contract_call_comm_rands: Vec<Fr>,
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
        // Pre-populate unguarded PublicInput outputs so Impact's
        // read_results can reference them. We stop at the first guarded
        // PublicInput because its guard hasn't been evaluated yet, making
        // the stream offset for subsequent PIs indeterminate.
        {
            let mut peek_idx: usize = 0;
            for ins in self.instructions.iter() {
                match ins {
                    I::PublicInput {
                        guard: None,
                        val_t,
                        output,
                    } => {
                        let w = val_t.encoded_len();
                        if peek_idx + w <= preimage.public_transcript_outputs.len() {
                            let value = decode_offcircuit(
                                &preimage.public_transcript_outputs[peek_idx..peek_idx + w],
                                val_t,
                            )?;
                            memory.insert(output.clone(), value);
                        }
                        peek_idx += w;
                    }
                    I::PublicInput { guard: Some(_), .. } => break,
                    _ => {}
                }
            }
        }

        let mut pi_skips = Vec::new();
        let mut public_transcript_inputs_idx: usize = 0;
        let mut public_transcript_outputs_idx: usize = 0;
        let mut private_transcript_outputs_idx: usize = 0;
        let mut contract_call_comm_rands_out: Vec<Fr> = Vec::new();
        let mut outputs = Vec::new();

        for ins in self.instructions.iter() {
            trace!(?ins, "preprocess gate");
            if eval_computational_instruction(ins, &mut memory)?.is_some() {
                continue;
            }
            match ins {
                I::PublicInput {
                    guard,
                    val_t,
                    output,
                } => {
                    let val = match guard {
                        Some(guard) if !eval_operand_bool(&memory, guard)? => {
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
                        Some(guard) if !eval_operand_bool(&memory, guard)? => {
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
                I::Impact {
                    guard,
                    ops,
                    read_results,
                } => {
                    // Per-op sizes needed: prove.rs consumes one pi_skips entry per transcript op.
                    let (field_elements, per_op_sizes) =
                        zkir_ops_to_field_elements_with_sizes(ops.clone(), &read_results, &memory)?;
                    let count = field_elements.len();
                    if !eval_operand_bool(&memory, guard)? {
                        // Inactive: push zeros (matching the circuit's
                        // select(0, val, 0) = 0 for each field element).
                        for _ in 0..count {
                            pis.push(Fr::from(0u64));
                        }
                        for &op_size in &per_op_sizes {
                            pi_skips.push(Some(op_size));
                        }
                    } else {
                        // Active: push real values and validate against preimage.
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
                I::ContractCall {
                    contract_ref,
                    expected_type: _,
                    entry_point,
                    args,
                    outputs: call_outputs,
                } => {
                    // ── 1. Consume callee output values from private_transcript ──
                    // Each `TypedIdentifier` consumes `val_t.encoded_len()`
                    // Frs from `private_transcript` and is reconstructed as a
                    // typed value via `decode_offcircuit`. The witness
                    // provider serializes typed callee outputs in declaration
                    // order with their `encode_offcircuit` flat-Fr layout, so
                    // this `decode_offcircuit` is the exact inverse.
                    let total_fr_count: usize = call_outputs
                        .iter()
                        .map(|t| t.val_t.encoded_len())
                        .sum();
                    if private_transcript_outputs_idx + total_fr_count
                        > preimage.private_transcript.len()
                    {
                        bail!(
                            "ContractCall: not enough private_transcript for callee outputs: \
                             need {} more but only {} remain",
                            total_fr_count,
                            preimage.private_transcript.len() - private_transcript_outputs_idx
                        );
                    }
                    for typed_out_id in call_outputs.iter() {
                        let w = typed_out_id.val_t.encoded_len();
                        let frs = &preimage.private_transcript[
                            private_transcript_outputs_idx
                                ..private_transcript_outputs_idx + w
                        ];
                        let value = decode_offcircuit(frs, &typed_out_id.val_t)?;
                        memory.insert(typed_out_id.name.clone(), value);
                        private_transcript_outputs_idx += w;
                    }

                    // ── 2. Resolve instruction parameters ──
                    let (addr_hi_op, addr_lo_op) = contract_ref;
                    let addr_hi = eval_operand_fr(&memory, addr_hi_op)?;
                    let addr_lo = eval_operand_fr(&memory, addr_lo_op)?;

                    let ep_hash = compute_ep_hash(entry_point);
                    let ep_hash_fields = ep_hash.field_vec();
                    let ep_hash_hi = ep_hash_fields[0];
                    let ep_hash_lo = ep_hash_fields[1];

                    // ── 3. Compute comm_comm from args, outputs, and comm_rand ──
                    // comm_rand comes from private_transcript, equivalent to
                    // the Compact pattern where tmpCallRand() provides it as
                    // a private witness (IrType::Native, 1 Fr).
                    if private_transcript_outputs_idx >= preimage.private_transcript.len() {
                        bail!(
                            "ContractCall: not enough private_transcript for comm_rand: \
                             need 1 more but none remain"
                        );
                    }
                    let comm_rand = preimage.private_transcript[private_transcript_outputs_idx];
                    private_transcript_outputs_idx += 1;
                    contract_call_comm_rands_out.push(comm_rand);

                    let mut io_fields: Vec<Fr> = Vec::new();
                    for arg in args.iter() {
                        io_fields.push(eval_operand_fr(&memory, arg)?);
                    }
                    // Flatten each typed output back into Frs via
                    // `encode_offcircuit`. This re-walks the typed values in
                    // declaration order producing the same Fr stream that the
                    // callee's own preprocess used to compute its comm-comm.
                    for typed_out_id in call_outputs.iter() {
                        let value = memory
                            .get(&typed_out_id.name)
                            .cloned()
                            .ok_or_else(|| {
                                anyhow!(
                                    "ContractCall output {:?} not in memory",
                                    typed_out_id.name
                                )
                            })?;
                        if value.get_type() != typed_out_id.val_t {
                            bail!(
                                "ContractCall output {:?} has runtime type {:?} but \
                                 instruction declares {:?}",
                                typed_out_id.name,
                                value.get_type(),
                                typed_out_id.val_t,
                            );
                        }
                        for ir_val in encode_offcircuit(&value) {
                            let fr: Fr = ir_val.try_into()?;
                            io_fields.push(fr);
                        }
                    }
                    let comm_comm = transient_commit(&io_fields, comm_rand);

                    // ── 4. Compute expected claim field elements and verify ──
                    let expected = compute_claim_field_elements(
                        addr_hi, addr_lo, ep_hash_hi, ep_hash_lo, comm_comm,
                    );
                    let claim_field_count = expected.len();
                    assert_eq!(claim_field_count, claim_ops_field_count());

                    if public_transcript_inputs_idx + claim_field_count
                        > preimage.public_transcript_inputs.len()
                    {
                        bail!(
                            "ContractCall: not enough public_transcript_inputs for claim ops: \
                             need {} more but only {} remain",
                            claim_field_count,
                            preimage.public_transcript_inputs.len() - public_transcript_inputs_idx
                        );
                    }
                    for i in 0..claim_field_count {
                        let actual =
                            preimage.public_transcript_inputs[public_transcript_inputs_idx + i];
                        if actual != expected[i] {
                            error!(
                                idx = public_transcript_inputs_idx + i,
                                ?actual,
                                expected = ?expected[i],
                                "ContractCall claim field element mismatch"
                            );
                            bail!(
                                "ContractCall claim field element mismatch at offset {}; \
                                 expected: {:?}, actual: {:?}",
                                i,
                                expected[i],
                                actual
                            );
                        }
                        pis.push(actual);
                    }

                    // One pi_skips entry per claim op (prove.rs consumes one
                    // entry per transcript op). Claim ops are always active.
                    for _ in 0..CLAIM_OPS_COUNT {
                        pi_skips.push(None);
                    }
                    public_transcript_inputs_idx += claim_field_count;
                }
                _ => bail!("unhandled instruction in preprocess: {ins:?}"),
            }
        }
        // Materialize declared outputs from memory at end of execution.
        // Each declared output must be bound in memory, must have a type
        // matching the signature, and contributes its flat-Fr encoding to
        // the comm-comm input.
        for typed_id in self.outputs.iter() {
            let value = memory.get(&typed_id.name).cloned().ok_or_else(|| {
                anyhow!(
                    "declared output {:?} not bound in memory at end of execution",
                    typed_id.name
                )
            })?;
            if value.get_type() != typed_id.val_t {
                bail!(
                    "declared output {:?} has runtime type {:?} but signature declares {:?}",
                    typed_id.name,
                    value.get_type(),
                    typed_id.val_t
                );
            }
            for ir_val in encode_offcircuit(&value) {
                outputs.push(ir_val);
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
            contract_call_comm_rands: contract_call_comm_rands_out,
        })
    }
}

// ── Cross-implementation consistency tests ───────────────────────────────
//
// The off-circuit preprocess re-implements two pieces of canonical logic
// that are also defined in the executor / runtime:
//
//   1. Entry-point hashing: `compute_ep_hash` here vs.
//      `EntryPointBuf::ep_hash` in `onchain-state`.
//   2. Claim-ops field-element layout: `compute_claim_field_elements` here
//      vs. `kernel_claim_contract_call!` (in `onchain-runtime`) flowed
//      through `Op::field_repr`.
//
// Drift between either pair would silently break commitment binding for
// cross-contract calls. These tests pin the two pairs at fixed test
// inputs so any future change to the canonical side trips a test failure
// before it can land.
#[cfg(test)]
mod tests {
    use super::{
        CLAIM_ADDR_HI_OFFSET, CLAIM_ADDR_LO_OFFSET, CLAIM_COMM_COMM_OFFSET,
        CLAIM_EP_HASH_HI_OFFSET, CLAIM_EP_HASH_LO_OFFSET, claim_ops_field_count,
        compute_claim_field_elements, compute_ep_hash,
    };
    use base_crypto::fab::AlignedValue;
    use onchain_runtime::kernel_claim_contract_call;
    use onchain_runtime_state::state::{EntryPointBuf, StateValue};
    use onchain_vm::ops::{Key, Op};
    use onchain_vm::result_mode::{ResultModeGather, ResultModeVerify};
    use storage::arena::Sp;
    use storage::db::InMemoryDB;
    use transient_crypto::curve::Fr;
    use transient_crypto::repr::FieldRepr;

    /// `compute_ep_hash` (preprocess-side) must agree with
    /// `EntryPointBuf::ep_hash` (executor-side) on every entry-point name.
    /// These two functions hash the same domain-separated input but live in
    /// separate crates because `ir_preprocess` cannot depend on
    /// `onchain-state` unconditionally.
    #[test]
    fn ep_hash_matches_entry_point_buf_ep_hash() {
        // A representative slice of names: empty, ASCII, with separators,
        // multi-byte (UTF-8), and a few of the strings used by the existing
        // composable_zkir tests.
        let names: &[&str] = &[
            "",
            "get",
            "call_inner",
            "add_state",
            "call_add",
            "call_from_state",
            "x.y.z",
            "snake_case_name",
            "with-dashes",
            "with spaces",
            "α",
            "🌒",
        ];
        for name in names {
            let preprocess_hash = compute_ep_hash(name);
            let executor_hash = EntryPointBuf(name.as_bytes().to_vec()).ep_hash();
            assert_eq!(
                preprocess_hash, executor_hash,
                "ep_hash mismatch for entry-point name {name:?}: \
                 preprocess={preprocess_hash:?}, executor={executor_hash:?}",
            );
        }
    }

    /// `compute_claim_field_elements` must produce exactly the field-element
    /// sequence that `kernel_claim_contract_call!` does when its ops are
    /// flowed through `Op::field_repr`. This guarantees that the
    /// hand-written 24-element vec stays in lock-step with the canonical
    /// macro that the executor and circuit both consume.
    #[test]
    fn claim_field_elements_match_kernel_claim_contract_call() {
        use base_crypto::hash::HashOutput;

        // We pick a deterministic 32-byte address and a 32-byte ep-hash,
        // then derive their (hi, lo) field-element split via the same
        // `AlignedValue::from(HashOutput).field_repr` path the macro
        // exercises at runtime — that way `compute_claim_field_elements`
        // sees exactly the Frs the macro will encode.
        let mut addr_bytes = [0u8; 32];
        for (i, b) in addr_bytes.iter_mut().enumerate() {
            *b = (i as u8).wrapping_mul(0x9b);
        }
        let address = HashOutput(addr_bytes);
        let address_av: AlignedValue = address.into();

        let mut ep_bytes = [0u8; 32];
        for (i, b) in ep_bytes.iter_mut().enumerate() {
            *b = ((i as u8).wrapping_mul(0x57)).wrapping_add(7);
        }
        let ep = HashOutput(ep_bytes);
        let ep_av: AlignedValue = ep.into();

        let comm_comm: Fr = Fr::from(0x8888_9999_aaaa_bbbbu64);
        let comm_av: AlignedValue = comm_comm.into();

        // `AlignedValue::field_repr` emits `alignment.field_repr() ++ value`.
        // For HashOutput([u8; 32]) the value portion is exactly two Frs
        // (the (byte31, bytes0..31) split per `<[u8;32]>::field_repr`); the
        // alignment portion is a constant prefix shared by every such AV.
        // We slice the trailing two Frs to recover the (hi, lo) pair.
        let split_hi_lo = |av: &AlignedValue| -> (Fr, Fr) {
            let mut v: Vec<Fr> = Vec::new();
            av.field_repr(&mut v);
            assert!(
                v.len() >= 2,
                "AlignedValue::field_repr produced fewer than 2 Frs for a 32-byte HashOutput"
            );
            (v[v.len() - 2], v[v.len() - 1])
        };
        let (addr_hi, addr_lo) = split_hi_lo(&address_av);
        let (ep_hash_hi, ep_hash_lo) = split_hi_lo(&ep_av);

        // ── Run the macro and serialize via Op::field_repr ──
        let claim_ops: Vec<Op<ResultModeGather, InMemoryDB>> =
            kernel_claim_contract_call!((), (), address_av, ep_av, comm_av).to_vec();

        // Translate gather → verify so we can call `Op::field_repr`. None
        // of the claim ops are Popeq, so the read-result closure is never
        // invoked.
        let verify_ops: Vec<Op<ResultModeVerify, InMemoryDB>> = claim_ops
            .into_iter()
            .map(|op| op.translate(|()| unreachable!("claim ops contain no Popeq")))
            .collect();

        let mut macro_frs: Vec<Fr> = Vec::new();
        for op in &verify_ops {
            op.field_repr(&mut macro_frs);
        }

        // ── Run the hand-written builder ──
        let direct_frs = compute_claim_field_elements(
            addr_hi, addr_lo, ep_hash_hi, ep_hash_lo, comm_comm,
        );

        // ── Length must match `claim_ops_field_count()` and each other ──
        assert_eq!(
            direct_frs.len(),
            claim_ops_field_count(),
            "compute_claim_field_elements length disagrees with claim_ops_field_count",
        );
        assert_eq!(
            macro_frs.len(),
            direct_frs.len(),
            "kernel_claim_contract_call! field-repr length differs from \
             compute_claim_field_elements length",
        );

        // ── Element-by-element equality ──
        for (i, (m, d)) in macro_frs.iter().zip(direct_frs.iter()).enumerate() {
            assert_eq!(
                m, d,
                "claim field-element mismatch at offset {i}: \
                 macro={m:?}, direct={d:?}",
            );
        }

        // ── Cross-check the variable-position offsets ──
        // The CLAIM_*_OFFSET constants must point at the slots that the
        // in-circuit ContractCall arm constrains to (addr_hi, addr_lo,
        // ep_hash_hi, ep_hash_lo, comm_comm).
        assert_eq!(
            macro_frs[CLAIM_ADDR_HI_OFFSET], addr_hi,
            "CLAIM_ADDR_HI_OFFSET points at the wrong slot",
        );
        assert_eq!(
            macro_frs[CLAIM_ADDR_LO_OFFSET], addr_lo,
            "CLAIM_ADDR_LO_OFFSET points at the wrong slot",
        );
        assert_eq!(
            macro_frs[CLAIM_EP_HASH_HI_OFFSET], ep_hash_hi,
            "CLAIM_EP_HASH_HI_OFFSET points at the wrong slot",
        );
        assert_eq!(
            macro_frs[CLAIM_EP_HASH_LO_OFFSET], ep_hash_lo,
            "CLAIM_EP_HASH_LO_OFFSET points at the wrong slot",
        );
        assert_eq!(
            macro_frs[CLAIM_COMM_COMM_OFFSET], comm_comm,
            "CLAIM_COMM_COMM_OFFSET points at the wrong slot",
        );
    }
}
