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
//! [`IrSource::execute`] walks a ZKIR instruction sequence, evaluates Impact
//! blocks via [`QueryContext::query`], handles cross-contract calls
//! recursively, and produces transcripts for proving. The result is a flat
//! depth-first preorder list of [`Call`] records — index 0 is the top-level
//! call, subsequent indices are descendants linked via [`Call::parent`].
//!
//! Shared off-circuit evaluation helpers (`eval_computational_instruction`,
//! `eval_operand_*`, etc.) live in [`crate::ir_eval`] and are reused here.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use base_crypto::fab::{AlignedValue, Alignment, AlignmentAtom, AlignmentSegment};
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
use crate::ir_instructions::encode::encode_offcircuit;
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

/// Provider for fetching a contract's ZKIR and ledger state during a
/// `ContractCall`. Async to accommodate JS/WASM bridges that satisfy the
/// trait by awaiting Promises.
///
/// Consumed via generics throughout (no trait objects), matching the pattern
/// used by `transient_crypto::proofs::Resolver` and `ParamsProverProvider`.
pub trait ZkirProvider<D: DB> {
    /// Fetch a consistent snapshot of the contract at `address`: the
    /// `IrSource` for `entry_point` together with the contract's ledger
    /// state. Returned atomically so that the IR and state are observed
    /// at the same logical instant — implementations backed by mutable
    /// stores must serialize their reads (or return an error) rather
    /// than expose a torn snapshot. The executor uses this method both
    /// for the top-level call's setup and for every `ContractCall` it
    /// recurses into.
    #[allow(async_fn_in_trait)]
    async fn fetch_contract(
        &self,
        address: ContractAddress,
        entry_point: &[u8],
    ) -> Result<(IrSource, ChargedState<D>), ExecutionError>;
}

/// Provider for witness values (private inputs) during top-level execution.
///
/// Cross-contract calls set `witness_provider: None`, so callees cannot
/// access private inputs — this enforces the witness limitation. Kept
/// synchronous because witnesses are typically pre-supplied as a queue;
/// no JS callback is needed to fetch them lazily.
pub trait WitnessProvider {
    /// Provide the next witness value as a field element.
    fn next_witness(&self) -> Result<Fr, ExecutionError>;
}

/// Execution context for the ZKIR rehearsal interpreter. Generic over the
/// `ZkirProvider` implementation so we don't need trait objects (which
/// don't compose cleanly with `async fn in trait`).
pub struct ExecutionContext<D: DB, P: ZkirProvider<D>> {
    /// The contract's ledger state snapshot.
    pub ledger_state: ChargedState<D>,
    /// The contract's address.
    pub address: ContractAddress,
    /// The entry point under which this circuit is being invoked.
    pub entry_point: EntryPointBuf,
    /// Provider for fetching callee ZKIR from on-chain state. `Arc` so the
    /// recursive `ContractCall` handler can hand a clone to each child
    /// context without requiring `P: Clone`.
    pub zkir_provider: Arc<P>,
    /// Provider for witness callbacks (None for dynamically-called contracts).
    pub witness_provider: Option<Arc<dyn WitnessProvider>>,
    /// Current call depth (for recursion limiting).
    pub call_depth: u32,
    /// Maximum call depth.
    pub max_call_depth: u32,
    /// Cost model for runtime queries.
    pub cost_model: CostModel,
}

/// Role-specific data: this call's relationship to its caller.
///
/// The root of a call tree is invoked directly by the caller of `execute`;
/// a sub-call is invoked via `ContractCall` from another contract and is
/// bound to its parent by a communication commitment. Witness data
/// (`private_transcript_outputs`) lives on [`Call`] itself rather than the
/// role — it is needed for *every* call's prototype, not just the root.
#[derive(Clone, Debug)]
pub enum CallRole {
    /// Top-level invocation.
    Root,
    /// Sub-call invoked via `ContractCall`. Carries the commitment that links
    /// this proof to its parent's claim.
    Sub {
        /// The communication commitment value, computed by the parent as
        /// `transient_commit(input ∥ output, comm_comm_rand)`.
        comm_comm: Fr,
        /// Randomness used by the parent in the commitment.
        comm_comm_rand: Fr,
    },
}

/// One execution of a contract circuit. The first element of an
/// [`ExecutionResult`] is the top-level call; subsequent elements are
/// descendants in depth-first preorder, linked to their parents via
/// [`Call::parent`].
///
/// `input` and `output` are flat sequences of field elements. The WASM
/// bridge wraps them at the language boundary into `AlignedValue`s using
/// [`Call::input_alignment`] and [`Call::output_alignment`] (via
/// `Alignment::parse_field_repr`) so that Compact descriptors compose
/// naturally (`desc.fromValue(av.value)` chains through `Value.shift()`);
/// the executor itself only ever sees the flat `Vec<Fr>`.
pub struct Call<D: DB> {
    /// Address of the contract whose circuit was executed.
    pub address: ContractAddress,
    /// Entry point under which the circuit was invoked.
    pub entry_point: EntryPointBuf,
    /// Input field elements supplied to the circuit. For the root, the count
    /// is `IrSource.inputs.iter().map(|t| t.val_t.encoded_len()).sum()`.
    /// For sub-calls, one Fr per `ContractCall.args` operand.
    pub input: Vec<Fr>,
    /// Alignment of `input` derived from the executed circuit's typed inputs
    /// (concatenation of `IrType::alignment()` over `IrSource.inputs`).
    /// Suitable for `Alignment::parse_field_repr(&input)` to recover the
    /// canonical `AlignedValue`.
    pub input_alignment: Alignment,
    /// Output field elements produced by the circuit, one per `Output`
    /// instruction.
    pub output: Vec<Fr>,
    /// Alignment of `output`. Each `Output` instruction emits a single `Fr`,
    /// so this is `[Field; output.len()]`.
    pub output_alignment: Alignment,
    /// Verify-mode public-transcript program with read results filled in.
    pub program: Vec<Op<ResultModeVerify, D>>,
    /// Query context after this call's Impact blocks executed.
    pub context: QueryContext<D>,
    /// Index of the parent call in the enclosing `Vec<Call<D>>`. `None` only
    /// for the root.
    pub parent: Option<usize>,
    /// Role-specific data: empty for the root, commitment fields for a
    /// sub-call.
    pub role: CallRole,
    /// The `ContractCallPrototype.private_transcript_outputs`-shaped witness
    /// sequence for this call, in instruction-emission order. Each entry is
    /// one logical witness:
    ///   - one AV per `PrivateInput` instruction (root only — the witness
    ///     limitation forbids private inputs in sub-calls), aligned per
    ///     that instruction's `val_t`;
    ///   - for each `ContractCall`, one single-`Field`-aligned AV per
    ///     callee output Fr followed by one single-`Field`-aligned AV for
    ///     the parent-supplied `comm_rand` (the Compact-emitted
    ///     `tmpDoCall` / `tmpCallRand` pattern).
    /// Concatenating each AV's `value_only_field_repr` produces exactly the
    /// flat `Vec<Fr>` the preimage's `private_transcript` carries — so this
    /// vec drops directly into the prototype builder, no post-pass needed.
    pub private_transcript_outputs: Vec<AlignedValue>,
}

impl<D: DB> Clone for Call<D> {
    fn clone(&self) -> Self {
        Self {
            address: self.address,
            entry_point: self.entry_point.clone(),
            input: self.input.clone(),
            input_alignment: self.input_alignment.clone(),
            output: self.output.clone(),
            output_alignment: self.output_alignment.clone(),
            program: self.program.clone(),
            context: self.context.clone(),
            parent: self.parent,
            role: self.role.clone(),
            private_transcript_outputs: self.private_transcript_outputs.clone(),
        }
    }
}

impl<D: DB> Call<D> {
    /// Communication commitment value, if this call is a sub-call.
    pub fn comm_comm(&self) -> Option<Fr> {
        match &self.role {
            CallRole::Sub { comm_comm, .. } => Some(*comm_comm),
            CallRole::Root => None,
        }
    }

    /// Communication commitment randomness, if this call is a sub-call.
    pub fn comm_comm_rand(&self) -> Option<Fr> {
        match &self.role {
            CallRole::Sub { comm_comm_rand, .. } => Some(*comm_comm_rand),
            CallRole::Root => None,
        }
    }

    /// Borrow this call's [`Self::private_transcript_outputs`] field.
    /// Provided as a method for symmetry with [`Self::comm_comm`] and
    /// [`Self::comm_comm_rand`], and to keep call-site syntax stable for
    /// readers used to the previous role-dispatched accessor.
    pub fn private_transcript_outputs(&self) -> &[AlignedValue] {
        &self.private_transcript_outputs
    }
}

/// Result of executing an `IrSource`. Always non-empty; index 0 is the root
/// call. Order is depth-first preorder, suitable for direct conversion to
/// the flat input that `partition_transcripts` consumes.
pub type ExecutionResult<D> = Vec<Call<D>>;

/// The top-level call in an `ExecutionResult` — index 0 by construction.
///
/// Mirrors `rootOf` in the TypeScript surface. Panics if `calls` is empty,
/// which `IrSource::execute` never returns.
pub fn root_of<D: DB>(calls: &[Call<D>]) -> &Call<D> {
    calls
        .first()
        .expect("execution result has no root call (this is a bug)")
}

/// Iterator over the immediate children of `parent_index` in `calls`, paired
/// with their indices. Useful for tree navigation; recurse on returned
/// indices to walk the full subtree.
///
/// Mirrors `subCallsOf` in the TypeScript surface (which returns just
/// `Call[]`; the Rust form additionally yields each child's index, since
/// indices are how further navigation is expressed).
pub fn sub_calls_of<'a, D: DB>(
    calls: &'a [Call<D>],
    parent_index: usize,
) -> impl Iterator<Item = (usize, &'a Call<D>)> + 'a {
    calls
        .iter()
        .enumerate()
        .filter(move |(_, c)| c.parent == Some(parent_index))
}

/// Internal tree representation built up during recursive execution. Flattened
/// into `Vec<Call<D>>` (preorder) once execution completes.
struct CallTree<D: DB> {
    address: ContractAddress,
    entry_point: EntryPointBuf,
    input: Vec<Fr>,
    input_alignment: Alignment,
    output: Vec<Fr>,
    output_alignment: Alignment,
    program: Vec<Op<ResultModeVerify, D>>,
    context: QueryContext<D>,
    private_transcript_outputs: Vec<AlignedValue>,
    sub_data: Option<(Fr, Fr)>,
    children: Vec<CallTree<D>>,
}

impl<D: DB> CallTree<D> {
    fn flatten(self) -> Vec<Call<D>> {
        let mut out = Vec::new();
        self.flatten_into(&mut out, None);
        out
    }

    fn flatten_into(self, out: &mut Vec<Call<D>>, parent: Option<usize>) {
        let my_index = out.len();
        let role = match self.sub_data {
            Some((comm_comm, comm_comm_rand)) => CallRole::Sub {
                comm_comm,
                comm_comm_rand,
            },
            None => CallRole::Root,
        };
        out.push(Call {
            address: self.address,
            entry_point: self.entry_point,
            input: self.input,
            input_alignment: self.input_alignment,
            output: self.output,
            output_alignment: self.output_alignment,
            program: self.program,
            context: self.context,
            parent,
            role,
            private_transcript_outputs: self.private_transcript_outputs,
        });
        for child in self.children {
            child.flatten_into(out, Some(my_index));
        }
    }
}

/// Build an alignment of `n` `Field` atoms — used for outputs (each `Output`
/// instruction emits exactly one Fr).
fn field_atoms_alignment(n: usize) -> Alignment {
    Alignment(
        std::iter::repeat(AlignmentSegment::Atom(AlignmentAtom::Field))
            .take(n)
            .collect(),
    )
}

// ── Helper functions ─────────────────────────────────────────────────────

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

// ── IrSource::execute ────────────────────────────────────────────────────

impl IrSource {
    /// Execute this ZKIR circuit against real ledger state.
    ///
    /// Walks the instruction sequence, executes Impact blocks via
    /// `QueryContext::query`, handles cross-contract calls recursively, and
    /// produces a flat depth-first preorder list of [`Call`] records suitable
    /// for proving and `partition_transcripts` consumption.
    ///
    /// `inputs` is the flat field-element representation of the typed inputs
    /// declared in `self.inputs`, in declaration order. The expected length is
    /// `self.inputs.iter().map(|t| t.val_t.encoded_len()).sum()`. The WASM
    /// bridge accepts a single `[Field; n]`-aligned `AlignedValue` at the
    /// language boundary and projects it down to this `Vec<Fr>` via
    /// `value_only_field_repr` before calling the executor.
    pub async fn execute<D: DB, P: ZkirProvider<D>>(
        &self,
        inputs: Vec<Fr>,
        context: ExecutionContext<D, P>,
        rng: &mut (impl CryptoRng + Rng),
    ) -> Result<ExecutionResult<D>, ExecutionError> {
        let tree = self.execute_tree(inputs, context, rng).await?;
        Ok(tree.flatten())
    }

    /// Recursive helper that builds a `CallTree` rather than the flat list.
    /// The returned tree has `sub_data: None`; the parent `ContractCall`
    /// handler sets it to `Some((comm_comm, comm_rand))` on its children.
    ///
    /// The recursive `Box::pin` in the `ContractCall` branch is required
    /// because directly awaiting a same-named `async fn` from inside itself
    /// would produce an infinitely-sized future type.
    fn execute_tree<'a, D: DB, P: ZkirProvider<D> + 'a>(
        &'a self,
        inputs: Vec<Fr>,
        context: ExecutionContext<D, P>,
        rng: &'a mut (impl CryptoRng + Rng),
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<CallTree<D>, ExecutionError>> + 'a>> {
        Box::pin(self.execute_tree_inner(inputs, context, rng))
    }

    async fn execute_tree_inner<D: DB, P: ZkirProvider<D>>(
        &self,
        inputs: Vec<Fr>,
        context: ExecutionContext<D, P>,
        rng: &mut (impl CryptoRng + Rng),
    ) -> Result<CallTree<D>, ExecutionError> {
        // Validate that the flat input length matches the sum of per-typed-input
        // encoded widths, then decode each typed input from its slice.
        let expected_len: usize = self.inputs.iter().map(|t| t.val_t.encoded_len()).sum();
        if inputs.len() != expected_len {
            return Err(ExecutionError::Internal(format!(
                "expected {expected_len} input field elements, got {}",
                inputs.len()
            )));
        }

        // Build the input alignment from the IR's typed inputs. Concatenation
        // is honest about each typed input's structure — e.g. a `JubjubPoint`
        // typed input contributes `[Field, Field]`, a `Native` contributes
        // `[Field]`. For any future non-Field-atom variant, the per-`IrType`
        // alignment is the load-bearing piece; the WASM bridge then uses
        // `Alignment::parse_field_repr` to reconstruct the canonical AV.
        let input_alignment = Alignment(
            self.inputs
                .iter()
                .flat_map(|t| t.val_t.alignment().0.into_iter())
                .collect(),
        );
        let mut memory: HashMap<Identifier, IrValue> = HashMap::new();
        let mut idx = 0;
        for typed_id in self.inputs.iter() {
            let w = typed_id.val_t.encoded_len();
            let value = decode_offcircuit(&inputs[idx..idx + w], &typed_id.val_t)
                .map_err(|e| ExecutionError::Internal(format!("input decode: {e}")))?;
            memory.insert(typed_id.name.clone(), value);
            idx += w;
        }

        // Destructure context so we can move fields (entry_point, ledger_state)
        // into local owners while keeping providers/cost_model usable in the
        // ContractCall recursion.
        let ExecutionContext {
            ledger_state,
            address,
            entry_point,
            zkir_provider,
            witness_provider,
            call_depth,
            max_call_depth,
            cost_model,
        } = context;

        let mut query_context = QueryContext::new(ledger_state, address);
        let mut transcript_program: Vec<Op<ResultModeVerify, D>> = Vec::new();
        let mut outputs: Vec<Fr> = Vec::new();
        let mut children: Vec<CallTree<D>> = Vec::new();
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
                            .query(&gather_ops, None, &cost_model)
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
                            let provider = witness_provider.as_ref().ok_or(
                                ExecutionError::WitnessNotAvailable {
                                    call_depth,
                                    instruction_index: ip,
                                },
                            )?;
                            let w = val_t.encoded_len();
                            let mut raw: Vec<Fr> = Vec::with_capacity(w);
                            for _ in 0..w {
                                raw.push(provider.next_witness()?);
                            }
                            // Record one `AlignedValue` per `PrivateInput`,
                            // matching the per-logical-witness granularity
                            // that `ContractCallPrototype.private_transcript_outputs`
                            // expects. The preimage's
                            // `private_transcript: Vec<Fr>` flattens these
                            // via `value_only_field_repr` downstream;
                            // that yields the same flat `Fr` sequence as the
                            // witness queue we just consumed.
                            let av = val_t.alignment().parse_field_repr(&raw).ok_or_else(
                                || ExecutionError::InstructionError {
                                    instruction_index: ip,
                                    message: format!(
                                        "could not parse {} witness field elements as alignment for {val_t:?}",
                                        raw.len()
                                    ),
                                },
                            )?;
                            private_transcript_outputs.push(av);
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

                I::ContractCall {
                    contract_ref,
                    expected_type: _, // TODO: conformance checking
                    entry_point: callee_ep,
                    args,
                    outputs: call_outputs,
                } => {
                    if call_depth + 1 > max_call_depth {
                        return Err(ExecutionError::MaxCallDepthExceeded {
                            max_depth: max_call_depth,
                        });
                    }

                    // Resolve the callee address from the (hi, lo) operands.
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

                    // Resolve each arg to a single Fr — `ContractCall.args` are
                    // IR operands, which always evaluate to a Native field
                    // element.
                    let input_frs: Vec<Fr> = args
                        .iter()
                        .map(|a| eval_operand_fr(&memory, a))
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(|e| ExecutionError::InstructionError {
                            instruction_index: ip,
                            message: format!("{e:#}"),
                        })?;

                    let entry_point_bytes = callee_ep.as_bytes();
                    let (callee_ir, callee_state) = zkir_provider
                        .fetch_contract(callee_address, entry_point_bytes)
                        .await?;
                    let ep_buf = EntryPointBuf(entry_point_bytes.to_vec());

                    let child_context = ExecutionContext {
                        ledger_state: callee_state,
                        address: callee_address,
                        entry_point: ep_buf.clone(),
                        zkir_provider: zkir_provider.clone(),
                        witness_provider: None,
                        call_depth: call_depth + 1,
                        max_call_depth,
                        cost_model: cost_model.clone(),
                    };

                    let mut child_tree = callee_ir
                        .execute_tree(input_frs, child_context, rng)
                        .await?;

                    if child_tree.output.len() != call_outputs.len() {
                        return Err(ExecutionError::InstructionError {
                            instruction_index: ip,
                            message: format!(
                                "callee returned {} outputs, expected {}",
                                child_tree.output.len(),
                                call_outputs.len()
                            ),
                        });
                    }

                    // Install callee outputs in caller's memory. Each callee
                    // output is a single Fr at the IR level (one `Output`
                    // instruction emits one Fr).
                    for (out_id, out_fr) in call_outputs.iter().zip(child_tree.output.iter()) {
                        memory.insert(out_id.clone(), IrValue::Native(*out_fr));
                    }

                    // comm_comm = transient_commit(input ∥ output, rand)
                    // Both `child_tree.input` and `child_tree.output` are
                    // already flat `Vec<Fr>`, so the IO representation is
                    // simply their concatenation.
                    //
                    // `comm_rand` is sampled uniformly from `Fr` (~254 bits)
                    // — the hiding property of `transient_commit` requires
                    // full-field randomness. Sampling only `u64` (as a
                    // narrower distribution) would let an adversary who
                    // observes (input, output, comm_comm) brute-force the
                    // opening in 2^64 work.
                    let comm_rand: Fr = rng.r#gen();
                    let mut io_repr: Vec<Fr> =
                        Vec::with_capacity(child_tree.input.len() + child_tree.output.len());
                    io_repr.extend_from_slice(&child_tree.input);
                    io_repr.extend_from_slice(&child_tree.output);
                    let comm_comm = transient_commit(&io_repr, comm_rand);

                    // Append this `ContractCall`'s witness contributions to
                    // *this* call's `private_transcript_outputs` — one
                    // single-`Field`-aligned AV per callee output Fr,
                    // followed by one such AV for `comm_rand`. The order
                    // (callee outputs first, then `comm_rand`) and the
                    // singleton-Field alignment match the `tmpDoCall` /
                    // `tmpCallRand` layout the Compact compiler emits and
                    // that `ir_preprocess` consumes.
                    for fr in &child_tree.output {
                        private_transcript_outputs.push(AlignedValue::from(*fr));
                    }
                    private_transcript_outputs.push(AlignedValue::from(comm_rand));

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
                        .query(&claim_ops, None, &cost_model)
                        .map_err(|e| {
                            ExecutionError::TranscriptRejected(format!("claim ops rejected: {e:?}"))
                        })?;

                    let verify_claim_ops = program_with_results(&claim_ops, &claim_results.events);
                    transcript_program.extend(verify_claim_ops);

                    query_context = claim_results.context;

                    child_tree.sub_data = Some((comm_comm, comm_rand));
                    children.push(child_tree);
                }

                _ => {
                    return Err(ExecutionError::InstructionError {
                        instruction_index: ip,
                        message: format!("unhandled instruction in execute: {ins:?}"),
                    });
                }
            }
        }

        // Materialize declared outputs from memory at end of execution.
        // Each declared output must be bound in memory, must have a type
        // matching the signature, and contributes its flat-Fr encoding to
        // the call's output stream in declaration order.
        for typed_id in self.outputs.iter() {
            let value = memory.get(&typed_id.name).cloned().ok_or_else(|| {
                ExecutionError::InstructionError {
                    instruction_index: usize::MAX,
                    message: format!(
                        "declared output {:?} not bound in memory at end of execution",
                        typed_id.name
                    ),
                }
            })?;
            if value.get_type() != typed_id.val_t {
                return Err(ExecutionError::InstructionError {
                    instruction_index: usize::MAX,
                    message: format!(
                        "declared output {:?} has runtime type {:?} but signature declares {:?}",
                        typed_id.name,
                        value.get_type(),
                        typed_id.val_t
                    ),
                });
            }
            for ir_val in encode_offcircuit(&value) {
                let fr: Fr = ir_val.try_into().map_err(|e| {
                    ExecutionError::InstructionError {
                        instruction_index: usize::MAX,
                        message: format!("encoded output not native: {e}"),
                    }
                })?;
                outputs.push(fr);
            }
        }

        let output_alignment = field_atoms_alignment(outputs.len());

        Ok(CallTree {
            address,
            entry_point,
            input: inputs,
            input_alignment,
            output: outputs,
            output_alignment,
            program: transcript_program,
            context: query_context,
            private_transcript_outputs,
            sub_data: None,
            children,
        })
    }
}
