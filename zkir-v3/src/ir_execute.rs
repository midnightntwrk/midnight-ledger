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

//! ZKIR rehearsal interpreter — executes circuits against real ledger state.
//!
//! This module provides [`IrSource::execute`], which walks a ZKIR instruction
//! sequence, evaluates Impact blocks via [`QueryContext::query`], handles
//! cross-contract calls recursively, and produces transcripts for proving.
//!
//! Shared off-circuit evaluation helpers (`eval_computational_instruction`,
//! `eval_resolve_operand`, etc.) live in [`crate::ir_eval`] and are reused here.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use base_crypto::fab::AlignedValue;
use base_crypto::hash::HashOutput;
use coin_structure::contract::ContractAddress;
use onchain_runtime::context::{QueryContext, QueryResults};
use onchain_runtime_state::state::{ChargedState, EntryPointBuf, StateValue};
use onchain_vm::cost_model::CostModel;
use onchain_vm::ops::{Key, Op};
use onchain_vm::result_mode::{GatherEvent, ResultModeGather, ResultModeVerify};
use rand::{CryptoRng, Rng};
use storage::arena::Sp;
use storage::db::DB;
use storage::storage::Array;
use transient_crypto::curve::Fr;
use transient_crypto::fab::{AlignedValueExt, AlignmentExt};
use transient_crypto::hash::transient_commit;
use transient_crypto::repr::FromFieldRepr;

use crate::ir::{Identifier, Instruction as I, IrSource};
use crate::ir_eval::{
    eval_computational_instruction, eval_operand, eval_operand_bool, eval_operand_fr,
};
use crate::ir_instructions::decode::decode_offcircuit;
use crate::ir_types::IrValue;
use crate::zkir_mode::{ZkirKey, ZkirOp, ZkirPushValue};

/// Errors that can occur during ZKIR execution (rehearsal).
#[derive(Debug)]
pub enum ExecutionError {
    /// A `PrivateInput` instruction was reached in a cross-contract call
    /// context where no witness provider is available.
    WitnessNotAvailable {
        call_depth: u32,
        instruction_index: usize,
    },
    /// Maximum call depth exceeded.
    MaxCallDepthExceeded { max_depth: u32 },
    /// Failed to fetch callee's ZKIR or state.
    ProviderError(String),
    /// An instruction failed during execution.
    InstructionError {
        instruction_index: usize,
        message: String,
    },
    /// Transcript rejected by the on-chain runtime.
    TranscriptRejected(String),
    /// Generic internal error.
    Internal(String),
}

impl fmt::Display for ExecutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WitnessNotAvailable {
                call_depth,
                instruction_index,
            } => {
                write!(
                    f,
                    "PrivateInput encountered at instruction {instruction_index} \
                     with no witness provider (call depth {call_depth})"
                )
            }
            Self::MaxCallDepthExceeded { max_depth } => {
                write!(f, "maximum call depth {max_depth} exceeded")
            }
            Self::ProviderError(msg) => write!(f, "provider error: {msg}"),
            Self::InstructionError {
                instruction_index,
                message,
            } => {
                write!(f, "instruction {instruction_index} failed: {message}")
            }
            Self::TranscriptRejected(msg) => write!(f, "transcript rejected: {msg}"),
            Self::Internal(msg) => write!(f, "internal error: {msg}"),
        }
    }
}

impl std::error::Error for ExecutionError {}

impl From<anyhow::Error> for ExecutionError {
    fn from(err: anyhow::Error) -> Self {
        ExecutionError::Internal(format!("{err:#}"))
    }
}

/// Provider for fetching callee ZKIR and ledger state during cross-contract calls.
pub trait ZkirProvider<D: DB>: Send + Sync {
    /// Fetch the `IrSource` and `ContractOperation` for a deployed contract's entry point.
    fn fetch_zkir(&self, address: ContractAddress, entry_point: &[u8]) -> Result<IrSource, ExecutionError>;

    /// Fetch the full ledger state for a contract.
    fn fetch_state(&self, address: ContractAddress) -> Result<ChargedState<D>, ExecutionError>;
}

/// Provider for witness values (private inputs) during top-level execution.
///
/// Cross-contract calls set `witness_provider: None`, so callees cannot
/// access private inputs — this enforces the witness limitation.
pub trait WitnessProvider: Send + Sync {
    /// Provide the next witness value as a field element.
    fn next_witness(&self) -> Result<Fr, ExecutionError>;
}

/// Execution context for the ZKIR rehearsal interpreter.
pub struct ExecutionContext<D: DB> {
    /// The contract's ledger state snapshot.
    pub ledger_state: ChargedState<D>,
    /// The contract's address.
    pub address: ContractAddress,
    /// Provider for fetching callee ZKIR from on-chain state.
    pub zkir_provider: Arc<dyn ZkirProvider<D>>,
    /// Provider for witness callbacks (None for dynamically-called contracts).
    pub witness_provider: Option<Arc<dyn WitnessProvider>>,
    /// Current call depth (for recursion limiting).
    pub call_depth: u32,
    /// Maximum call depth.
    pub max_call_depth: u32,
    /// Cost model for runtime queries.
    pub cost_model: CostModel,
}

/// Result of executing a single contract's ZKIR.
pub struct ExecutionResult<D: DB> {
    /// Output values from the circuit (field elements).
    pub outputs: Vec<Fr>,
    /// Flattened pre-transcripts for this contract and all its sub-calls,
    /// in depth-first order. Index 0 is always this contract's own
    /// transcript; subsequent entries are sub-call transcripts (each with
    /// `comm_comm` already populated by the caller).
    pub pre_transcripts: Vec<PreTranscriptData<D>>,
    /// Sub-calls made during execution, each with their own results.
    pub sub_calls: Vec<SubCallResult<D>>,
    /// Private transcript outputs (witness values consumed, for top-level only).
    pub private_transcript_outputs: Vec<AlignedValue>,
}

/// The transcript data produced by execution, ready for conversion to
/// `PreTranscript` in the ledger crate.
pub struct PreTranscriptData<D: DB> {
    /// The query context after all Impact blocks have executed.
    pub context: QueryContext<D>,
    /// The verify-mode ops (with read results filled in) constituting the
    /// public transcript program.
    pub program: Vec<Op<ResultModeVerify, D>>,
    /// Communication commitment value (if this contract participates in
    /// cross-contract calls).
    pub comm_comm: Option<Fr>,
}

// Manual Clone impl: derive(Clone) would add an unnecessary `D: Clone` bound.
impl<D: DB> Clone for PreTranscriptData<D> {
    fn clone(&self) -> Self {
        Self {
            context: self.context.clone(),
            program: self.program.clone(),
            comm_comm: self.comm_comm,
        }
    }
}

/// Result of a single cross-contract sub-call.
pub struct SubCallResult<D: DB> {
    /// The callee's contract address.
    pub address: ContractAddress,
    /// The entry point invoked.
    pub entry_point: EntryPointBuf,
    /// Input values passed to the callee (as AlignedValue for commitment).
    pub input: AlignedValue,
    /// Output values returned by the callee (as AlignedValue for commitment).
    pub output: AlignedValue,
    /// Randomness used in the communication commitment.
    pub communication_commitment_rand: Fr,
    /// The callee's full execution result.
    pub execution_result: ExecutionResult<D>,
}

/// Construct an `AlignedValue` from a sequence of field elements, treating each
/// element as a `Field` atom.
fn build_field_aligned_value(fields: &[Fr]) -> AlignedValue {
    use base_crypto::fab::{Alignment, AlignmentAtom, AlignmentSegment, Value, ValueAtom};

    let segments = fields
        .iter()
        .map(|_| AlignmentSegment::Atom(AlignmentAtom::Field))
        .collect();
    let alignment = Alignment(segments);

    let value_atoms = fields
        .iter()
        .map(|fr| {
            let bytes = fr.0.to_bytes_le();
            let mut v = bytes.to_vec();
            while let Some(0) = v.last() {
                v.pop();
            }
            ValueAtom(v)
        })
        .collect();
    let value = Value(value_atoms);

    AlignedValue { value, alignment }
}

/// Resolve a `ZkirPushValue` to a `StateValue<D>` by resolving operands
/// from memory and parsing via alignment.
fn resolve_push_value<D: DB>(
    push_val: &ZkirPushValue,
    memory: &HashMap<Identifier, IrValue>,
) -> Result<StateValue<D>, ExecutionError> {
    let fields = push_val
        .operands
        .iter()
        .map(|op| eval_operand_fr(memory, op))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| ExecutionError::Internal(format!("{e:#}")))?;
    let aligned = push_val
        .alignment
        .parse_field_repr(&fields)
        .ok_or_else(|| {
            ExecutionError::Internal("failed to parse field repr via alignment for Push".into())
        })?;
    Ok(StateValue::Cell(Sp::new(aligned)))
}

/// Resolve a `Vec<ZkirKey>` to an `Array<Key, D>` by resolving operands
/// from memory and parsing via alignment.
fn resolve_key_path<D: DB>(
    keys: &[ZkirKey],
    memory: &HashMap<Identifier, IrValue>,
) -> Result<Array<Key, D>, ExecutionError> {
    keys.iter()
        .map(|k| match k {
            ZkirKey::Stack => Ok(Key::Stack),
            ZkirKey::Value {
                alignment,
                operands,
            } => {
                let fields: Vec<Fr> = operands
                    .iter()
                    .map(|op| eval_operand_fr(memory, op))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| ExecutionError::Internal(format!("{e:#}")))?;
                let aligned = alignment.parse_field_repr(&fields).ok_or_else(|| {
                    ExecutionError::Internal("failed to parse field repr for Idx key".into())
                })?;
                Ok(Key::Value(aligned))
            }
        })
        .collect::<Result<Array<Key, D>, _>>()
}

/// Convert a single `ZkirOp` to an `Op<ResultModeGather, D>` by resolving
/// symbolic operand references from memory.
fn translate_zkir_op<D: DB>(
    op: ZkirOp,
    memory: &HashMap<Identifier, IrValue>,
) -> Result<Op<ResultModeGather, D>, ExecutionError> {
    match &op {
        Op::Push { value, .. } => {
            let resolved = resolve_push_value::<D>(value, memory)?;
            let translated = op.translate_full(
                |_: ZkirPushValue| resolved,
                |_keys: Vec<ZkirKey>| -> Array<Key, D> { unreachable!("Push has no keys") },
                |()| (),
            );
            Ok(translated)
        }
        Op::Idx { path, .. } => {
            let resolved = resolve_key_path::<D>(path, memory)?;
            let translated = op.translate_full(
                |_: ZkirPushValue| -> StateValue<D> { unreachable!("Idx has no push value") },
                |_: Vec<ZkirKey>| resolved,
                |()| (),
            );
            Ok(translated)
        }
        _ => {
            let translated = op.translate_full(
                |_: ZkirPushValue| -> StateValue<D> { unreachable!("non-Push variant") },
                |_: Vec<ZkirKey>| -> Array<Key, D> { unreachable!("non-Idx variant") },
                |()| (),
            );
            Ok(translated)
        }
    }
}

/// Convert gathered events to verify-mode ops with read results filled in.
fn program_with_results<D: DB>(
    gather_ops: &[Op<ResultModeGather, D>],
    events: &[GatherEvent<D>],
) -> Vec<Op<ResultModeVerify, D>> {
    let mut read_iter = events.iter().filter_map(|e| match e {
        GatherEvent::Read(av) => Some(av),
        _ => None,
    });

    gather_ops
        .iter()
        .map(|op| {
            // Invariant: events contain one Read per Popeq.
            op.clone()
                .translate(|()| read_iter.next().expect("missing read result").clone())
        })
        .filter(|op| match op {
            Op::Idx { path, .. } => !path.is_empty(),
            Op::Ins { n, .. } => *n != 0,
            _ => true,
        })
        .collect()
}

impl IrSource {
    /// Execute this ZKIR circuit against real ledger state.
    ///
    /// This is the rehearsal interpreter: it walks the instruction sequence,
    /// executes Impact blocks via `QueryContext::query`, handles cross-contract
    /// calls recursively, and produces transcripts for subsequent proving.
    pub fn execute<D: DB>(
        &self,
        inputs: Vec<Fr>,
        context: ExecutionContext<D>,
        rng: &mut (impl CryptoRng + Rng),
    ) -> Result<ExecutionResult<D>, ExecutionError> {
        // Initialize memory with input bindings, decoding multi-element
        // types (e.g. JubjubPoint = 2 field elements) via encoded_len().
        let mut memory: HashMap<Identifier, IrValue> = HashMap::new();
        let expected_len: usize = self.inputs.iter().map(|id| id.val_t.encoded_len()).sum();
        if inputs.len() != expected_len {
            return Err(ExecutionError::Internal(format!(
                "expected {expected_len} inputs, got {}",
                inputs.len()
            )));
        }
        {
            let mut idx = 0;
            for typed_id in self.inputs.iter() {
                let w = typed_id.val_t.encoded_len();
                let value = decode_offcircuit(&inputs[idx..idx + w], &typed_id.val_t)
                    .map_err(|e| ExecutionError::Internal(format!("input decode: {e}")))?;
                memory.insert(typed_id.name.clone(), value);
                idx += w;
            }
        }

        // Execution state
        let mut query_context = QueryContext::new(context.ledger_state.clone(), context.address);
        let mut transcript_program: Vec<Op<ResultModeVerify, D>> = Vec::new();
        let mut outputs: Vec<Fr> = Vec::new();
        let mut sub_calls: Vec<SubCallResult<D>> = Vec::new();
        let mut sub_pre_transcripts: Vec<PreTranscriptData<D>> = Vec::new();
        let mut private_transcript_outputs: Vec<AlignedValue> = Vec::new();

        let mut public_transcript_output_values: Vec<Fr> = Vec::new();
        let mut public_transcript_outputs_idx: usize = 0;

        for (ip, ins) in self.instructions.iter().enumerate() {
            if eval_computational_instruction(ins, &mut memory)
                .map_err(|e| ExecutionError::InstructionError {
                    instruction_index: ip,
                    message: format!("{e:#}"),
                })?
                .is_some()
            {
                continue;
            }
            match ins {
                I::Impact {
                    guard,
                    ops,
                    read_results: _,
                } => {
                    let guard_val = eval_operand_bool(&memory, guard).map_err(|e| {
                        ExecutionError::InstructionError {
                            instruction_index: ip,
                            message: format!("{e:#}"),
                        }
                    })?;

                    if guard_val {
                        let gather_ops = ops
                            .iter()
                            .cloned()
                            .map(|op| translate_zkir_op(op, &memory))
                            .collect::<Result<Vec<_>, _>>()?;

                        let query_results = query_context
                            .query(&gather_ops, None, &context.cost_model)
                            .map_err(|e| ExecutionError::TranscriptRejected(format!("{e:?}")))?;

                        let verify_ops = program_with_results(&gather_ops, &query_results.events);
                        transcript_program.extend(verify_ops);

                        query_context = query_results.context;

                        for event in &query_results.events {
                            if let GatherEvent::Read(av) = event {
                                let mut fields = Vec::new();
                                av.value_only_field_repr(&mut fields);
                                public_transcript_output_values.extend(fields);
                            }
                        }
                    }
                }
                I::Output { val } => {
                    let value = eval_operand(&memory, val).map_err(|e| {
                        ExecutionError::InstructionError {
                            instruction_index: ip,
                            message: format!("{e:#}"),
                        }
                    })?;
                    let fr_val: Fr =
                        value
                            .clone()
                            .try_into()
                            .map_err(|e| ExecutionError::InstructionError {
                                instruction_index: ip,
                                message: format!("output value not native: {e}"),
                            })?;
                    outputs.push(fr_val);
                }
                I::PublicInput {
                    guard,
                    val_t,
                    output,
                } => {
                    let val = match guard {
                        Some(guard)
                        if !eval_operand_bool(&memory, guard).map_err(|e| {
                            ExecutionError::InstructionError {
                                instruction_index: ip,
                                message: format!("{e:#}"),
                            }
                        })? =>
                            {
                                IrValue::default(val_t)
                            }
                        _ => {
                            let w = val_t.encoded_len();
                            if public_transcript_outputs_idx + w
                                > public_transcript_output_values.len()
                            {
                                return Err(ExecutionError::InstructionError {
                                    instruction_index: ip,
                                    message: format!(
                                        "not enough public transcript outputs: need {w} more, \
                                         have {} remaining",
                                        public_transcript_output_values.len()
                                            - public_transcript_outputs_idx
                                    ),
                                });
                            }
                            let raw = &public_transcript_output_values
                                [public_transcript_outputs_idx..public_transcript_outputs_idx + w];
                            public_transcript_outputs_idx += w;
                            decode_offcircuit(raw, val_t).map_err(|e| {
                                ExecutionError::InstructionError {
                                    instruction_index: ip,
                                    message: format!("PublicInput decode failed: {e}"),
                                }
                            })?
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
                        Some(guard)
                        if !eval_operand_bool(&memory, guard).map_err(|e| {
                            ExecutionError::InstructionError {
                                instruction_index: ip,
                                message: format!("{e:#}"),
                            }
                        })? =>
                            {
                                IrValue::default(val_t)
                            }
                        _ => {
                            let provider = context.witness_provider.as_ref().ok_or(
                                ExecutionError::WitnessNotAvailable {
                                    call_depth: context.call_depth,
                                    instruction_index: ip,
                                },
                            )?;
                            let w = val_t.encoded_len();
                            let mut raw = Vec::with_capacity(w);
                            for _ in 0..w {
                                raw.push(provider.next_witness()?);
                            }
                            private_transcript_outputs.push(build_field_aligned_value(&raw));
                            decode_offcircuit(&raw, val_t).map_err(|e| {
                                ExecutionError::InstructionError {
                                    instruction_index: ip,
                                    message: format!("PrivateInput decode failed: {e}"),
                                }
                            })?
                        }
                    };
                    memory.insert(output.clone(), val);
                }

                // ── ContractCall ──
                I::ContractCall {
                    contract_ref,
                    expected_type: _, // TODO: conformance checking
                    entry_point,
                    args,
                    outputs: call_outputs,
                } => {
                    if context.call_depth + 1 > context.max_call_depth {
                        return Err(ExecutionError::MaxCallDepthExceeded {
                            max_depth: context.max_call_depth,
                        });
                    }

                    let (addr_hi, addr_lo) = contract_ref;
                    let fr_hi = eval_operand_fr(&memory, addr_hi).map_err(|e| {
                        ExecutionError::InstructionError {
                            instruction_index: ip,
                            message: format!("{e:#}"),
                        }
                    })?;
                    let fr_lo = eval_operand_fr(&memory, addr_lo).map_err(|e| {
                        ExecutionError::InstructionError {
                            instruction_index: ip,
                            message: format!("{e:#}"),
                        }
                    })?;
                    let addr_fields = [fr_hi, fr_lo];
                    let hash_bytes =
                        <[u8; 32]>::from_field_repr(&addr_fields).ok_or_else(|| {
                            ExecutionError::InstructionError {
                                instruction_index: ip,
                                message:
                                "contract_ref field elements do not encode a valid 32-byte address"
                                    .to_string(),
                            }
                        })?;
                    let callee_address = ContractAddress(HashOutput(hash_bytes));

                    let input_fields: Vec<Fr> = args
                        .iter()
                        .map(|a| eval_operand_fr(&memory, a))
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(|e| ExecutionError::InstructionError {
                            instruction_index: ip,
                            message: format!("{e:#}"),
                        })?;

                    let entry_point_bytes = entry_point.as_bytes();
                    let callee_ir = context.zkir_provider.fetch_zkir(callee_address, entry_point_bytes)?;
                    let callee_state = context.zkir_provider.fetch_state(callee_address)?;

                    let child_context = ExecutionContext {
                        ledger_state: callee_state,
                        address: callee_address,
                        zkir_provider: context.zkir_provider.clone(),
                        witness_provider: None,
                        call_depth: context.call_depth + 1,
                        max_call_depth: context.max_call_depth,
                        cost_model: context.cost_model.clone(),
                    };

                    let callee_result =
                        callee_ir.execute(input_fields.clone(), child_context, rng)?;

                    if callee_result.outputs.len() != call_outputs.len() {
                        return Err(ExecutionError::InstructionError {
                            instruction_index: ip,
                            message: format!(
                                "callee returned {} outputs, expected {}",
                                callee_result.outputs.len(),
                                call_outputs.len()
                            ),
                        });
                    }
                    for (out_id, out_val) in call_outputs.iter().zip(callee_result.outputs.iter()) {
                        memory.insert(out_id.clone(), IrValue::Native(*out_val));
                    }

                    let input_av = build_field_aligned_value(&input_fields);
                    let output_av = build_field_aligned_value(&callee_result.outputs);

                    let comm_rand: Fr = rng.r#gen::<u64>().into();
                    let mut io_repr = Vec::new();
                    input_av.value_only_field_repr(&mut io_repr);
                    output_av.value_only_field_repr(&mut io_repr);
                    let comm_comm = transient_commit(&io_repr, comm_rand);

                    let ep_buf = EntryPointBuf(entry_point_bytes.to_vec());
                    let ep_hash = ep_buf.ep_hash();

                    let claim_ops: Vec<Op<ResultModeGather, D>> =
                        onchain_runtime::kernel_claim_contract_call!(
                            (),
                            (),
                            AlignedValue::from(callee_address.0),
                            AlignedValue::from(ep_hash),
                            AlignedValue::from(comm_comm)
                        )
                            .to_vec();

                    let claim_results: QueryResults<ResultModeGather, D> = query_context
                        .query(&claim_ops, None, &context.cost_model)
                        .map_err(|e| {
                            ExecutionError::TranscriptRejected(format!("claim ops rejected: {e:?}"))
                        })?;

                    let verify_claim_ops = program_with_results(&claim_ops, &claim_results.events);
                    transcript_program.extend(verify_claim_ops);

                    query_context = claim_results.context;

                    // Copy the callee's pre_transcripts into our accumulator.
                    // The callee's own transcript is at index 0; set its
                    // comm_comm (computed above) before copying.
                    let mut callee_result = callee_result;
                    callee_result.pre_transcripts[0].comm_comm = Some(comm_comm);
                    sub_pre_transcripts.extend(callee_result.pre_transcripts.clone());

                    sub_calls.push(SubCallResult {
                        address: callee_address,
                        entry_point: ep_buf,
                        input: input_av,
                        output: output_av,
                        communication_commitment_rand: comm_rand,
                        execution_result: callee_result,
                    });
                }

                _ => {
                    return Err(ExecutionError::InstructionError {
                        instruction_index: ip,
                        message: format!("unhandled instruction in execute: {ins:?}"),
                    });
                }
            }
        }

        // Build the flat pre_transcripts vec: this contract first, then all sub-call transcripts
        // (already in depth-first order).
        let mut pre_transcripts = Vec::new();
        pre_transcripts.push(PreTranscriptData {
            context: query_context,
            program: transcript_program,
            comm_comm: None,
        });
        pre_transcripts.extend(sub_pre_transcripts);

        Ok(ExecutionResult {
            outputs,
            pre_transcripts,
            sub_calls,
            private_transcript_outputs,
        })
    }
}

