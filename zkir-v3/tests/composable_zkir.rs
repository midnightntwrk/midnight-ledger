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

//! Integration tests for ZKIR-interpreter-driven cross-contract composability.
//!
//! These tests exercise the `execute` method on `IrSource`, verifying that:
//!
//! 1. A single contract's Impact blocks execute correctly against ledger state.
//! 2. Cross-contract calls via `ContractCall` produce correct sub-call results.
//! 3. Witness limitation is enforced (callees cannot access `PrivateInput`).
//! 4. Recursion depth limiting works.
//!
//! The tests construct `IrSource` objects programmatically (not from JSON) with
//! structured Impact ops (`ZkirOp`), deploy contracts with ZKIR stored on-chain
//! via `ContractOperation::new_with_zkir`, and execute via the ZKIR interpreter.
//!
//! Full proving integration (partition → prove → verify) requires the `ledger`
//! crate's infrastructure and is deferred to a separate test in `ledger/tests/`.

use std::collections::HashMap;
use std::ops::Deref;
use std::sync::Arc;

use base_crypto::fab::{AlignedValue, Alignment, AlignmentAtom};
use base_crypto::hash::HashOutput;
use coin_structure::contract::ContractAddress;
use onchain_runtime_state::state::{ChargedState, EntryPointBuf, StateValue};
use onchain_vm::cost_model::INITIAL_COST_MODEL;
use onchain_vm::ops::Op;
use rand::SeedableRng;
use rand::rngs::StdRng;
use storage::arena::Sp;
use storage::db::DB;
use transient_crypto::curve::Fr;
use transient_crypto::repr::FieldRepr;

use midnight_zkir_v3::ir_execute::{
    ExecutionContext, ExecutionError, ExecutionResult, WitnessProvider, ZkirProvider,
};
use midnight_zkir_v3::{
    CircuitSignature, ContractTypeDescriptor, Identifier, Instruction, IrSource, IrType, Operand,
    TypedIdentifier, ZkirKey, ZkirOp,
};

// ─── Test helpers ────────────────────────────────────────────────────────────

/// Helper to create an `Identifier`.
fn id(name: &str) -> Identifier {
    Identifier(name.to_string())
}

/// Helper to create an `Operand::Variable`.
fn var(name: &str) -> Operand {
    Operand::Variable(id(name))
}

/// Helper to create an `Operand::Immediate` from a u64.
fn imm(v: u64) -> Operand {
    Operand::Immediate(Fr::from(v))
}

/// Alignment for a u8 value: Bytes { length: 1 }.
fn alignment_u8() -> Alignment {
    Alignment::singleton(AlignmentAtom::Bytes { length: 1 })
}

/// Alignment for a native field element.
fn alignment_field() -> Alignment {
    Alignment::singleton(AlignmentAtom::Field)
}

/// Construct a `ContractAddress` from a seed value.
fn make_address(seed: u64) -> ContractAddress {
    let mut bytes = [0u8; 32];
    bytes[..8].copy_from_slice(&seed.to_le_bytes());
    ContractAddress(HashOutput(bytes))
}

/// Convert a `ContractAddress` to a pair of field elements for `contract_ref`.
fn addr_to_frs(addr: ContractAddress) -> (Fr, Fr) {
    let mut fields = Vec::new();
    addr.0.field_repr(&mut fields);
    assert_eq!(fields.len(), 2);
    (fields[0], fields[1])
}

/// A trivial `ContractTypeDescriptor` for tests (conformance checking is stubbed).
fn stub_type_descriptor() -> ContractTypeDescriptor {
    ContractTypeDescriptor {
        circuits: vec![CircuitSignature {
            name: "test".to_string(),
            param_count: 0,
            return_count: 1,
        }],
    }
}

// ─── Test ZkirProvider ───────────────────────────────────────────────────────

/// A test `ZkirProvider` backed by a map of contract addresses to their
/// (IrSource, ContractOperation, ChargedState) tuples.
struct TestZkirProvider<D: DB> {
    contracts: HashMap<ContractAddress, ContractEntry<D>>,
}

struct ContractEntry<D: DB> {
    /// Map from entry point name → IrSource for that circuit.
    circuits: HashMap<String, IrSource>,
    /// The ledger state snapshot for this contract.
    state: ChargedState<D>,
}

impl<D: DB> TestZkirProvider<D> {
    fn new() -> Self {
        TestZkirProvider {
            contracts: HashMap::new(),
        }
    }

    fn register(
        &mut self,
        address: ContractAddress,
        circuits: HashMap<String, IrSource>,
        state: ChargedState<D>,
    ) {
        self.contracts.insert(
            address,
            ContractEntry {
                circuits,
                state,
            },
        );
    }
}

impl<D: DB> ZkirProvider<D> for TestZkirProvider<D>
where
    D: Send + Sync + 'static,
{
    fn fetch_zkir(
        &self,
        address: ContractAddress,
        entry_point: &[u8],
    ) -> Result<IrSource, ExecutionError> {
        let entry = self.contracts.get(&address).ok_or_else(|| {
            ExecutionError::ProviderError(format!("contract not found: {address:?}"))
        })?;
        let ep_str = std::str::from_utf8(entry_point)
            .map_err(|e| ExecutionError::ProviderError(format!("invalid entry point: {e}")))?;
        let ir = entry.circuits.get(ep_str).ok_or_else(|| {
            ExecutionError::ProviderError(format!(
                "entry point '{ep_str}' not found for {address:?}"
            ))
        })?;
        Ok(ir.clone())
    }

    fn fetch_state(&self, address: ContractAddress) -> Result<ChargedState<D>, ExecutionError> {
        let entry = self.contracts.get(&address).ok_or_else(|| {
            ExecutionError::ProviderError(format!("contract not found: {address:?}"))
        })?;
        Ok(entry.state.clone())
    }
}

// ─── Test WitnessProvider ────────────────────────────────────────────────────

/// A simple witness provider that returns values from a pre-loaded queue.
struct VecWitnessProvider {
    values: std::sync::Mutex<Vec<Fr>>,
}

impl VecWitnessProvider {
    fn new(values: Vec<Fr>) -> Self {
        VecWitnessProvider {
            values: std::sync::Mutex::new(values),
        }
    }
}

impl WitnessProvider for VecWitnessProvider {
    fn next_witness(&self) -> Result<Fr, ExecutionError> {
        let mut vals = self.values.lock().unwrap();
        if vals.is_empty() {
            Err(ExecutionError::Internal("no more witnesses".into()))
        } else {
            Ok(vals.remove(0))
        }
    }
}

// ─── IrSource builders ──────────────────────────────────────────────────────

/// Build an `IrSource` for the "inner" contract's "get" circuit.
///
/// This circuit:
/// 1. Reads cell[0] from its ledger state via an Impact block.
/// 2. Receives the read result via PublicInput.
/// 3. Outputs the value.
///
/// No inputs, no private witnesses.
fn build_inner_get_ir() -> IrSource {
    // Impact ops: Dup the state root, Idx to read at key 0, Popeq
    let ops: Vec<ZkirOp> = vec![
        Op::Dup { n: 0 },
        Op::Idx {
            cached: false,
            push_path: false,
            path: vec![ZkirKey::Value {
                alignment: alignment_u8(),
                operands: vec![imm(0)],
            }],
        },
        Op::Popeq {
            cached: false,
            result: (),
        },
    ];

    IrSource {
        inputs: vec![],
        do_communications_commitment: true,
        instructions: Arc::new(vec![
            // Impact block: execute the ledger read
            Instruction::Impact {
                guard: imm(1),
                ops,
                read_results: vec![vec![]], // Placeholder (unused by execute)
            },
            // PublicInput reads the value from the Impact's read results.
            // The read result is a single field element (the cell value is a field).
            Instruction::PublicInput {
                guard: None,
                val_t: IrType::Native,
                output: id("%read_val"),
            },
            // Output the read value
            Instruction::Output {
                val: var("%read_val"),
            },
        ]),
    }
}

/// Build an `IrSource` for the "outer" contract's "call_inner" circuit.
///
/// This circuit:
/// 1. Takes the inner contract's address as an input.
/// 2. Performs a `ContractCall` to the inner contract's "get" entry point.
/// 3. Outputs the value returned by the inner contract.
///
/// The top-level caller provides a witness for the communication commitment
/// randomness (PrivateInput), but the callee (inner) gets no witness.
fn build_outer_call_ir() -> IrSource {
    IrSource {
        inputs: vec![
            TypedIdentifier::new(id("%inner_addr_hi"), IrType::Native),
            TypedIdentifier::new(id("%inner_addr_lo"), IrType::Native),
        ],
        do_communications_commitment: false,
        instructions: Arc::new(vec![
            // ContractCall to inner's "get" entry point
            Instruction::ContractCall {
                contract_ref: (var("%inner_addr_hi"), var("%inner_addr_lo")),
                expected_type: stub_type_descriptor(),
                entry_point: "get".to_string(),
                args: vec![],
                outputs: vec![id("%call_result")],
            },
            // Output the call result
            Instruction::Output {
                val: var("%call_result"),
            },
        ]),
    }
}

/// Build an `IrSource` for a contract that tries to use `PrivateInput`.
///
/// Used for testing the witness limitation: when this is called as a callee
/// (cross-contract call), `PrivateInput` should fail with `WitnessNotAvailable`.
fn build_contract_with_private_input() -> IrSource {
    IrSource {
        inputs: vec![],
        do_communications_commitment: false,
        instructions: Arc::new(vec![
            Instruction::PrivateInput {
                guard: None,
                val_t: IrType::Native,
                output: id("%secret"),
            },
            Instruction::Output {
                val: var("%secret"),
            },
        ]),
    }
}

/// Build a simple `IrSource` that just outputs its input (pass-through).
fn build_passthrough_ir() -> IrSource {
    IrSource {
        inputs: vec![TypedIdentifier::new(id("%in"), IrType::Native)],
        do_communications_commitment: false,
        instructions: Arc::new(vec![Instruction::Output { val: var("%in") }]),
    }
}

/// Build an `IrSource` that makes a chain of calls: calls contract A which
/// calls contract B, etc. Used for testing depth limiting.
fn build_chained_caller(callee_addr_hi: &str, callee_addr_lo: &str, entry_point: &str) -> IrSource {
    IrSource {
        inputs: vec![
            TypedIdentifier::new(id(callee_addr_hi), IrType::Native),
            TypedIdentifier::new(id(callee_addr_lo), IrType::Native),
        ],
        do_communications_commitment: false,
        instructions: Arc::new(vec![
            Instruction::ContractCall {
                contract_ref: (var(callee_addr_hi), var(callee_addr_lo)),
                expected_type: stub_type_descriptor(),
                entry_point: entry_point.to_string(),
                args: vec![var(callee_addr_hi), var(callee_addr_lo)], // pass address through
                outputs: vec![id("%result")],
            },
            Instruction::Output {
                val: var("%result"),
            },
        ]),
    }
}

// ─── State helpers ───────────────────────────────────────────────────────────

/// Create a `ChargedState` with a simple Array containing one Field-aligned cell.
///
/// The state is `StateValue::Array([StateValue::Cell(value)])`.
fn make_cell_state<D: DB>(value: AlignedValue) -> ChargedState<D> {
    let cell = StateValue::Cell(Sp::new(value));
    let arr = StateValue::Array(vec![cell].into());
    ChargedState::new(arr)
}

/// Create a `ChargedState` with null state (for contracts that don't read state).
fn make_null_state<D: DB>() -> ChargedState<D> {
    ChargedState::new(StateValue::Null)
}

/// Create an `AlignedValue` representing a single native field element.
fn field_aligned_value(fr: Fr) -> AlignedValue {
    use base_crypto::fab::{Value, ValueAtom};
    let bytes = fr.0.to_bytes_le();
    let mut v = bytes.to_vec();
    // Normalize: remove trailing zeros
    while let Some(0) = v.last() {
        v.pop();
    }
    AlignedValue {
        value: Value(vec![ValueAtom(v)]),
        alignment: alignment_field(),
    }
}

// ─── Type alias for test DB ─────────────────────────────────────────────────

type D = storage::DefaultDB;

// ─── Tests ──────────────────────────────────────────────────────────────────

/// Test 1: Execute a single contract with an Impact block that reads from
/// ledger state. Verifies that:
/// - Impact ops execute against real state via QueryContext::query
/// - Read results flow through to PublicInput
/// - Output values are collected correctly
/// - The verify-mode transcript is produced
#[test]
fn test_execute_single_contract_ledger_read() {
    let mut rng = StdRng::seed_from_u64(0x100);

    // Set up: inner contract reads cell[0] which contains a field element value
    let stored_value = Fr::from(42u64);
    let inner_ir = build_inner_get_ir();
    let inner_addr = make_address(1);

    let inner_state: ChargedState<D> = make_cell_state(field_aligned_value(stored_value));

    let mut provider = TestZkirProvider::<D>::new();
    provider.register(
        inner_addr,
        HashMap::from([("get".to_string(), inner_ir.clone())]),
        inner_state.clone(),
    );

    // Execute the inner contract's "get" circuit
    let context = ExecutionContext {
        ledger_state: inner_state,
        address: inner_addr,
        zkir_provider: Arc::new(provider),
        witness_provider: None,
        call_depth: 0,
        max_call_depth: 8,
        cost_model: INITIAL_COST_MODEL,
    };

    let result: ExecutionResult<D> = inner_ir
        .execute(vec![], context, &mut rng)
        .expect("execute should succeed");

    // Verify: the output should be the stored value
    assert_eq!(result.outputs.len(), 1, "expected one output");
    assert_eq!(
        result.outputs[0], stored_value,
        "output should match stored value"
    );

    // Verify: no sub-calls were made
    assert!(result.sub_calls.is_empty(), "no sub-calls expected");

    // Verify: the transcript program should contain verify-mode ops
    assert!(
        !result.pre_transcripts[0].program.is_empty(),
        "transcript should have verify-mode ops"
    );

    // Verify: no private transcript outputs (no PrivateInput instructions)
    assert!(
        result.private_transcript_outputs.is_empty(),
        "no private outputs expected"
    );
}

/// Test 2: Execute a cross-contract call where the outer contract calls the
/// inner contract's "get" entry point and returns the result. Verifies that:
/// - ContractCall fetches callee ZKIR via ZkirProvider
/// - Callee executes against its own ledger state
/// - Output values propagate from callee to caller
/// - SubCallResult is recorded correctly
/// - Communication commitment randomness is generated
#[test]
fn test_execute_cross_contract_call() {
    let mut rng = StdRng::seed_from_u64(0x200);

    let stored_value = Fr::from(99u64);
    let inner_ir = build_inner_get_ir();
    let outer_ir = build_outer_call_ir();
    let inner_addr = make_address(10);
    let outer_addr = make_address(20);

    let inner_state: ChargedState<D> = make_cell_state(field_aligned_value(stored_value));
    let outer_state: ChargedState<D> = make_null_state();

    let mut provider = TestZkirProvider::<D>::new();
    provider.register(
        inner_addr,
        HashMap::from([("get".to_string(), inner_ir.clone())]),
        inner_state,
    );
    provider.register(
        outer_addr,
        HashMap::from([("call_inner".to_string(), outer_ir.clone())]),
        outer_state.clone(),
    );

    // Execute the outer contract, passing the inner contract's address
    // as two field elements (hi, lo).
    let (addr_hi, addr_lo) = addr_to_frs(inner_addr);

    let context = ExecutionContext {
        ledger_state: outer_state,
        address: outer_addr,
        zkir_provider: Arc::new(provider),
        witness_provider: None,
        call_depth: 0,
        max_call_depth: 8,
        cost_model: INITIAL_COST_MODEL,
    };

    let result: ExecutionResult<D> = outer_ir
        .execute(vec![addr_hi, addr_lo], context, &mut rng)
        .expect("execute should succeed");

    // Verify: outer's output should be the value read by the inner contract
    assert_eq!(result.outputs.len(), 1, "expected one output");
    assert_eq!(
        result.outputs[0], stored_value,
        "outer should return inner's read value"
    );

    // Verify: one sub-call was recorded
    assert_eq!(result.sub_calls.len(), 1, "expected one sub-call");
    let sub = &result.sub_calls[0];

    // Verify sub-call metadata
    assert_eq!(sub.address, inner_addr, "sub-call address should be inner");
    assert_eq!(
        sub.entry_point,
        EntryPointBuf(b"get".to_vec()),
        "sub-call entry point should be 'get'"
    );

    // Verify the callee's execution result
    assert_eq!(
        sub.execution_result.outputs.len(),
        1,
        "callee should have one output"
    );
    assert_eq!(
        sub.execution_result.outputs[0], stored_value,
        "callee output should match stored value"
    );

    // Verify that the callee produced a transcript (index 1 in the
    // top-level pre_transcripts — index 0 is the caller's own transcript).
    assert!(
        !result.pre_transcripts[1].program.is_empty(),
        "callee should have transcript ops"
    );

    // Verify communication commitment randomness was generated (non-zero)
    assert_ne!(
        sub.communication_commitment_rand,
        Fr::from(0u64),
        "comm rand should be non-zero"
    );

    // Verify: the caller's transcript program should now contain claim ops
    // (kernel_claim_contract_call emits ops like Swap, Idx, Dup, Size, Push,
    // Concat, Push, Ins, Swap — and after program_with_results filtering,
    // the non-empty Idx and Ins ops should appear).
    assert!(
        !result.pre_transcripts[0].program.is_empty(),
        "caller transcript should contain claim ops from ContractCall"
    );

    // Verify: pre_transcripts should have two entries
    // (one for caller, one for callee) with correct comm_comm linkage.
    let flat = result.pre_transcripts.clone();
    assert_eq!(flat.len(), 2, "flattened should have two entries");
    assert!(
        flat[0].comm_comm.is_none(),
        "root caller should have no comm_comm"
    );
    assert!(
        flat[1].comm_comm.is_some(),
        "callee should have a comm_comm"
    );
}

/// Test 3: Witness limitation — calling a contract with `PrivateInput` as a
/// callee (via ContractCall) should fail with `WitnessNotAvailable`.
#[test]
fn test_witness_limitation_in_cross_contract_call() {
    let mut rng = StdRng::seed_from_u64(0x300);

    let callee_ir = build_contract_with_private_input();
    let callee_addr = make_address(30);

    // The caller contract: calls the callee which has PrivateInput
    let caller_ir = IrSource {
        inputs: vec![
            TypedIdentifier::new(id("%addr_hi"), IrType::Native),
            TypedIdentifier::new(id("%addr_lo"), IrType::Native),
        ],
        do_communications_commitment: false,
        instructions: Arc::new(vec![
            Instruction::ContractCall {
                contract_ref: (var("%addr_hi"), var("%addr_lo")),
                expected_type: stub_type_descriptor(),
                entry_point: "run".to_string(),
                args: vec![],
                outputs: vec![id("%out")],
            },
            Instruction::Output { val: var("%out") },
        ]),
    };
    let caller_addr = make_address(31);

    let mut provider = TestZkirProvider::<D>::new();
    provider.register(
        callee_addr,
        HashMap::from([("run".to_string(), callee_ir)]),
        make_null_state(),
    );
    provider.register(
        caller_addr,
        HashMap::from([("call".to_string(), caller_ir.clone())]),
        make_null_state(),
    );

    let (addr_hi, addr_lo) = addr_to_frs(callee_addr);

    let context = ExecutionContext {
        ledger_state: make_null_state(),
        address: caller_addr,
        zkir_provider: Arc::new(provider),
        witness_provider: Some(Arc::new(VecWitnessProvider::new(vec![Fr::from(1u64)]))),
        call_depth: 0,
        max_call_depth: 8,
        cost_model: INITIAL_COST_MODEL,
    };

    let err = caller_ir
        .execute(vec![addr_hi, addr_lo], context, &mut rng)
        .map(|_| ())
        .expect_err("should fail due to witness limitation");

    match err {
        ExecutionError::WitnessNotAvailable { call_depth, .. } => {
            assert_eq!(call_depth, 1, "should fail at call depth 1 (callee)");
        }
        other => panic!("expected WitnessNotAvailable, got: {other}"),
    }
}

/// Test 4: Top-level `PrivateInput` works when a witness provider is present.
#[test]
fn test_private_input_at_top_level() {
    let mut rng = StdRng::seed_from_u64(0x400);

    let ir = build_contract_with_private_input();
    let addr = make_address(40);

    let witness_value = Fr::from(777u64);

    let provider = TestZkirProvider::<D>::new();

    let context = ExecutionContext {
        ledger_state: make_null_state(),
        address: addr,
        zkir_provider: Arc::new(provider),
        witness_provider: Some(Arc::new(VecWitnessProvider::new(vec![witness_value]))),
        call_depth: 0,
        max_call_depth: 8,
        cost_model: INITIAL_COST_MODEL,
    };

    let result: ExecutionResult<D> = ir
        .execute(vec![], context, &mut rng)
        .expect("execute should succeed with witness provider");

    assert_eq!(result.outputs.len(), 1);
    assert_eq!(result.outputs[0], witness_value);

    // Verify private transcript output was recorded
    assert_eq!(
        result.private_transcript_outputs.len(),
        1,
        "should record one private transcript output"
    );
}

/// Test 5: Maximum call depth exceeded.
#[test]
fn test_max_call_depth_exceeded() {
    let mut rng = StdRng::seed_from_u64(0x500);

    // Contract A calls itself recursively
    let self_caller = build_chained_caller("%addr_hi", "%addr_lo", "loop");
    let addr = make_address(50);

    let mut provider = TestZkirProvider::<D>::new();
    provider.register(
        addr,
        HashMap::from([("loop".to_string(), self_caller.clone())]),
        make_null_state(),
    );

    let (addr_hi, addr_lo) = addr_to_frs(addr);

    let context = ExecutionContext {
        ledger_state: make_null_state(),
        address: addr,
        zkir_provider: Arc::new(provider),
        witness_provider: None,
        call_depth: 0,
        max_call_depth: 3, // Allow only 3 levels
        cost_model: INITIAL_COST_MODEL,
    };

    let err = self_caller
        .execute(vec![addr_hi, addr_lo], context, &mut rng)
        .map(|_| ())
        .expect_err("should fail due to max depth");

    match err {
        ExecutionError::MaxCallDepthExceeded { max_depth } => {
            assert_eq!(max_depth, 3);
        }
        other => panic!("expected MaxCallDepthExceeded, got: {other}"),
    }
}

/// Test 6: Pass-through contract executes with no Impact and no sub-calls.
#[test]
fn test_execute_pure_computation() {
    let mut rng = StdRng::seed_from_u64(0x600);

    let ir = build_passthrough_ir();
    let addr = make_address(60);

    let input_val = Fr::from(123u64);

    let provider = TestZkirProvider::<D>::new();

    let context = ExecutionContext {
        ledger_state: make_null_state(),
        address: addr,
        zkir_provider: Arc::new(provider),
        witness_provider: None,
        call_depth: 0,
        max_call_depth: 8,
        cost_model: INITIAL_COST_MODEL,
    };

    let result: ExecutionResult<D> = ir
        .execute(vec![input_val], context, &mut rng)
        .expect("execute should succeed");

    assert_eq!(result.outputs, vec![input_val]);
    assert!(result.sub_calls.is_empty());
    assert!(result.pre_transcripts[0].program.is_empty());
}

/// Test 7: Input count mismatch is rejected.
#[test]
fn test_input_count_mismatch() {
    let mut rng = StdRng::seed_from_u64(0x700);

    let ir = build_passthrough_ir(); // expects 1 input
    let addr = make_address(70);

    let provider = TestZkirProvider::<D>::new();

    let context = ExecutionContext {
        ledger_state: make_null_state(),
        address: addr,
        zkir_provider: Arc::new(provider),
        witness_provider: None,
        call_depth: 0,
        max_call_depth: 8,
        cost_model: INITIAL_COST_MODEL,
    };

    // Pass 0 inputs when 1 is expected
    let err = ir
        .execute(vec![], context, &mut rng)
        .map(|_| ())
        .expect_err("should fail due to input mismatch");

    match err {
        ExecutionError::Internal(msg) => {
            assert!(
                msg.contains("expected 1 inputs, got 0"),
                "unexpected error: {msg}"
            );
        }
        other => panic!("expected Internal error, got: {other}"),
    }
}

/// Test 8: Provider returning an error propagates correctly.
#[test]
fn test_provider_error_propagates() {
    let mut rng = StdRng::seed_from_u64(0x800);

    let outer_ir = build_outer_call_ir();
    let outer_addr = make_address(80);
    let bogus_inner_addr = make_address(99); // not registered

    // Provider has no entry for the bogus address
    let provider = TestZkirProvider::<D>::new();

    let (addr_hi, addr_lo) = addr_to_frs(bogus_inner_addr);

    let context = ExecutionContext {
        ledger_state: make_null_state(),
        address: outer_addr,
        zkir_provider: Arc::new(provider),
        witness_provider: None,
        call_depth: 0,
        max_call_depth: 8,
        cost_model: INITIAL_COST_MODEL,
    };

    let err = outer_ir
        .execute(vec![addr_hi, addr_lo], context, &mut rng)
        .map(|_| ())
        .expect_err("should fail because callee not found");

    match err {
        ExecutionError::ProviderError(msg) => {
            assert!(
                msg.contains("contract not found"),
                "unexpected provider error: {msg}"
            );
        }
        other => panic!("expected ProviderError, got: {other}"),
    }
}

/// Test 9: Computational instructions (Add, Mul, Neg, etc.) work correctly
/// in the execute path.
#[test]
fn test_execute_arithmetic_instructions() {
    let mut rng = StdRng::seed_from_u64(0x900);

    let ir = IrSource {
        inputs: vec![
            TypedIdentifier::new(id("%a"), IrType::Native),
            TypedIdentifier::new(id("%b"), IrType::Native),
        ],
        do_communications_commitment: false,
        instructions: Arc::new(vec![
            // %sum = %a + %b
            Instruction::Add {
                a: var("%a"),
                b: var("%b"),
                output: id("%sum"),
            },
            // %prod = %a * %b
            Instruction::Mul {
                a: var("%a"),
                b: var("%b"),
                output: id("%prod"),
            },
            // %neg_a = -%a
            Instruction::Neg {
                a: var("%a"),
                output: id("%neg_a"),
            },
            // Output all three
            Instruction::Output { val: var("%sum") },
            Instruction::Output { val: var("%prod") },
            Instruction::Output { val: var("%neg_a") },
        ]),
    };
    let addr = make_address(90);
    let provider = TestZkirProvider::<D>::new();

    let a = Fr::from(5u64);
    let b = Fr::from(7u64);

    let context = ExecutionContext {
        ledger_state: make_null_state(),
        address: addr,
        zkir_provider: Arc::new(provider),
        witness_provider: None,
        call_depth: 0,
        max_call_depth: 8,
        cost_model: INITIAL_COST_MODEL,
    };

    let result: ExecutionResult<D> = ir
        .execute(vec![a, b], context, &mut rng)
        .expect("execute should succeed");

    assert_eq!(result.outputs.len(), 3);
    assert_eq!(result.outputs[0], Fr::from(12u64)); // 5 + 7
    assert_eq!(result.outputs[1], Fr::from(35u64)); // 5 * 7
    assert_eq!(result.outputs[2], -a); // -5
}

/// Test 10: Cross-contract call with outputs propagated back.
/// Two-level call: outer calls middle, middle calls inner.
/// Tests that a depth-2 call tree works and results propagate correctly.
#[test]
fn test_execute_two_level_call_chain() {
    let mut rng = StdRng::seed_from_u64(0xA00);

    let inner_value = Fr::from(42u64);

    // Inner: passthrough (returns its input)
    let inner_ir = build_passthrough_ir();
    let inner_addr = make_address(100);

    // Middle: calls inner, returns the result
    let middle_ir = IrSource {
        inputs: vec![
            TypedIdentifier::new(id("%inner_addr_hi"), IrType::Native),
            TypedIdentifier::new(id("%inner_addr_lo"), IrType::Native),
            TypedIdentifier::new(id("%val"), IrType::Native),
        ],
        do_communications_commitment: false,
        instructions: Arc::new(vec![
            Instruction::ContractCall {
                contract_ref: (var("%inner_addr_hi"), var("%inner_addr_lo")),
                expected_type: stub_type_descriptor(),
                entry_point: "passthrough".to_string(),
                args: vec![var("%val")],
                outputs: vec![id("%from_inner")],
            },
            Instruction::Output {
                val: var("%from_inner"),
            },
        ]),
    };
    let middle_addr = make_address(101);

    // Outer: calls middle with the inner address and a value
    let outer_ir = IrSource {
        inputs: vec![
            TypedIdentifier::new(id("%middle_addr_hi"), IrType::Native),
            TypedIdentifier::new(id("%middle_addr_lo"), IrType::Native),
            TypedIdentifier::new(id("%inner_addr_hi"), IrType::Native),
            TypedIdentifier::new(id("%inner_addr_lo"), IrType::Native),
            TypedIdentifier::new(id("%val"), IrType::Native),
        ],
        do_communications_commitment: false,
        instructions: Arc::new(vec![
            Instruction::ContractCall {
                contract_ref: (var("%middle_addr_hi"), var("%middle_addr_lo")),
                expected_type: stub_type_descriptor(),
                entry_point: "relay".to_string(),
                args: vec![var("%inner_addr_hi"), var("%inner_addr_lo"), var("%val")],
                outputs: vec![id("%result")],
            },
            Instruction::Output {
                val: var("%result"),
            },
        ]),
    };
    let outer_addr = make_address(102);

    let mut provider = TestZkirProvider::<D>::new();
    provider.register(
        inner_addr,
        HashMap::from([("passthrough".to_string(), inner_ir)]),
        make_null_state(),
    );
    provider.register(
        middle_addr,
        HashMap::from([("relay".to_string(), middle_ir)]),
        make_null_state(),
    );
    provider.register(
        outer_addr,
        HashMap::from([("outer".to_string(), outer_ir.clone())]),
        make_null_state(),
    );

    let (middle_addr_hi, middle_addr_lo) = addr_to_frs(middle_addr);
    let (inner_addr_hi, inner_addr_lo) = addr_to_frs(inner_addr);

    let context = ExecutionContext {
        ledger_state: make_null_state(),
        address: outer_addr,
        zkir_provider: Arc::new(provider),
        witness_provider: None,
        call_depth: 0,
        max_call_depth: 8,
        cost_model: INITIAL_COST_MODEL,
    };

    let result: ExecutionResult<D> = outer_ir
        .execute(
            vec![
                middle_addr_hi,
                middle_addr_lo,
                inner_addr_hi,
                inner_addr_lo,
                inner_value,
            ],
            context,
            &mut rng,
        )
        .expect("two-level call should succeed");

    // Outer should return the value that propagated through middle → inner
    assert_eq!(result.outputs, vec![inner_value]);

    // Outer made one sub-call (to middle)
    assert_eq!(result.sub_calls.len(), 1);
    let middle_call = &result.sub_calls[0];
    assert_eq!(middle_call.address, middle_addr);

    // Middle made one sub-call (to inner)
    assert_eq!(middle_call.execution_result.sub_calls.len(), 1);
    let inner_call = &middle_call.execution_result.sub_calls[0];
    assert_eq!(inner_call.address, inner_addr);

    // Inner returned the value
    assert_eq!(inner_call.execution_result.outputs, vec![inner_value]);

    // Verify: outer's transcript contains claim ops for middle, and middle's
    // transcript contains claim ops for inner. Each caller's effects should
    // include the callee's comm_comm in claimed_contract_calls.
    // Verify pre_transcripts has three entries (outer, middle, inner)
    let flat = result.pre_transcripts.clone();
    assert_eq!(flat.len(), 3, "three participants in the call tree");
    assert!(flat[0].comm_comm.is_none(), "root (outer) has no comm_comm");
    assert!(flat[1].comm_comm.is_some(), "middle has a comm_comm");
    assert!(flat[2].comm_comm.is_some(), "inner has a comm_comm");

    // Verify: the comm_comm on the flattened middle entry should match the
    // claim recorded in outer's effects.
    // Extract claimed comm_comm values from effects (same pattern as partition_transcripts).
    let outer_claimed_comms: Vec<Fr> = result
        .pre_transcripts[0]
        .context
        .effects
        .claimed_contract_calls
        .iter()
        .map(|sp| (*sp).deref().into_inner().3)
        .collect();
    let middle_comm = flat[1].comm_comm.unwrap();
    assert!(
        outer_claimed_comms.contains(&middle_comm),
        "middle's comm_comm should appear in outer's claimed_contract_calls"
    );

    // Similarly for inner's comm_comm vs middle's claim (index 1 in
    // the top-level pre_transcripts).
    let middle_claimed_comms: Vec<Fr> = result
        .pre_transcripts[1]
        .context
        .effects
        .claimed_contract_calls
        .iter()
        .map(|sp| (*sp).deref().into_inner().3)
        .collect();
    let inner_comm = flat[2].comm_comm.unwrap();
    assert!(
        middle_claimed_comms.contains(&inner_comm),
        "inner's comm_comm should appear in middle's claimed_contract_calls"
    );
}

/// Test 11: Claim ops validation — the kernel_claim_contract_call ops emitted
/// by ContractCall produce effects that `partition_transcripts` can use to
/// reconstruct the call forest. Verifies the comm_comm linkage end-to-end.
#[test]
fn test_claim_ops_produce_correct_effects() {
    let mut rng = StdRng::seed_from_u64(0xB00);

    let stored_value = Fr::from(77u64);
    let inner_ir = build_inner_get_ir();
    let outer_ir = build_outer_call_ir();
    let inner_addr = make_address(110);
    let outer_addr = make_address(120);

    let inner_state: ChargedState<D> = make_cell_state(field_aligned_value(stored_value));
    let outer_state: ChargedState<D> = make_null_state();

    let mut provider = TestZkirProvider::<D>::new();
    provider.register(
        inner_addr,
        HashMap::from([("get".to_string(), inner_ir.clone())]),
        inner_state,
    );
    provider.register(
        outer_addr,
        HashMap::from([("call_inner".to_string(), outer_ir.clone())]),
        outer_state.clone(),
    );

    let (addr_hi, addr_lo) = addr_to_frs(inner_addr);

    let context = ExecutionContext {
        ledger_state: outer_state,
        address: outer_addr,
        zkir_provider: Arc::new(provider),
        witness_provider: None,
        call_depth: 0,
        max_call_depth: 8,
        cost_model: INITIAL_COST_MODEL,
    };

    let result: ExecutionResult<D> = outer_ir
        .execute(vec![addr_hi, addr_lo], context, &mut rng)
        .expect("execute should succeed");

    // 1. The caller's effects must contain exactly one claimed_contract_calls entry
    let claims: Vec<_> = result
        .pre_transcripts[0]
        .context
        .effects
        .claimed_contract_calls
        .iter()
        .map(|sp| (*sp).deref().into_inner())
        .collect();
    assert_eq!(
        claims.len(),
        1,
        "caller should have exactly one contract call claim"
    );

    // 2. The claim should reference the correct callee address and entry point hash
    let (_seq, claimed_addr, claimed_ep_hash, claimed_comm) = &claims[0];
    assert_eq!(
        *claimed_addr, inner_addr,
        "claim should reference inner contract"
    );

    let expected_ep_hash = EntryPointBuf(b"get".to_vec()).ep_hash();
    assert_eq!(
        *claimed_ep_hash, expected_ep_hash,
        "claim should have correct entry point hash"
    );

    // 3. The claimed comm_comm should match the callee's comm_comm in the
    //    flattened transcript (what partition_transcripts would see)
    let flat = result.pre_transcripts.clone();
    assert_eq!(flat.len(), 2);

    let callee_comm = flat[1].comm_comm.expect("callee should have comm_comm");
    assert_eq!(
        *claimed_comm, callee_comm,
        "claimed comm_comm must match callee's comm_comm for partition_transcripts linkage"
    );

    // 4. Manually verify the comm_comm computation matches the canonical formula:
    //    comm_comm = transient_commit(value_only_repr(input) || value_only_repr(output), rand)
    let sub = &result.sub_calls[0];
    let expected_comm = {
        use transient_crypto::fab::AlignedValueExt;
        let mut io_repr = Vec::new();
        sub.input.value_only_field_repr(&mut io_repr);
        sub.output.value_only_field_repr(&mut io_repr);
        transient_crypto::hash::transient_commit(&io_repr, sub.communication_commitment_rand)
    };
    assert_eq!(
        callee_comm, expected_comm,
        "comm_comm should match canonical transient_commit computation"
    );
}

/// Test 12: Cross-contract call with non-trivial result computation.
///
/// Inner contract "double": takes one input, returns input * 2.
/// Outer contract "add_doubled": takes an inner address and a value,
/// calls inner.double(value), then computes call_result + value and outputs it.
///
/// This verifies that:
/// - Contract A (outer) is passed contract B (inner) as a circuit parameter
/// - Contract B performs a non-trivial computation on its input
/// - Contract A performs a non-trivial computation on B's result
/// - Both return values that are functions of the call interaction
#[test]
fn test_nontrivial_result_from_call_parameter() {
    let mut rng = StdRng::seed_from_u64(0xC00);

    // Inner: takes one input, outputs input * 2
    let inner_ir = IrSource {
        inputs: vec![TypedIdentifier::new(id("%x"), IrType::Native)],
        do_communications_commitment: true,
        instructions: Arc::new(vec![
            // %doubled = %x + %x
            Instruction::Add {
                a: var("%x"),
                b: var("%x"),
                output: id("%doubled"),
            },
            Instruction::Output {
                val: var("%doubled"),
            },
        ]),
    };
    let inner_addr = make_address(0xC01);

    // Outer: takes inner address (2 fields) + a value, calls inner.double(value),
    // then outputs call_result + value (i.e. 2*value + value = 3*value).
    let outer_ir = IrSource {
        inputs: vec![
            TypedIdentifier::new(id("%inner_hi"), IrType::Native),
            TypedIdentifier::new(id("%inner_lo"), IrType::Native),
            TypedIdentifier::new(id("%val"), IrType::Native),
        ],
        do_communications_commitment: false,
        instructions: Arc::new(vec![
            Instruction::ContractCall {
                contract_ref: (var("%inner_hi"), var("%inner_lo")),
                expected_type: stub_type_descriptor(),
                entry_point: "double".to_string(),
                args: vec![var("%val")],
                outputs: vec![id("%doubled")],
            },
            // %result = %doubled + %val  (= 2*val + val = 3*val)
            Instruction::Add {
                a: var("%doubled"),
                b: var("%val"),
                output: id("%result"),
            },
            Instruction::Output {
                val: var("%result"),
            },
        ]),
    };
    let outer_addr = make_address(0xC02);

    let mut provider = TestZkirProvider::<D>::new();
    provider.register(
        inner_addr,
        HashMap::from([("double".to_string(), inner_ir)]),
        make_null_state(),
    );
    provider.register(
        outer_addr,
        HashMap::from([("add_doubled".to_string(), outer_ir.clone())]),
        make_null_state(),
    );

    let (addr_hi, addr_lo) = addr_to_frs(inner_addr);
    let input_val = Fr::from(17u64);

    let context = ExecutionContext {
        ledger_state: make_null_state(),
        address: outer_addr,
        zkir_provider: Arc::new(provider),
        witness_provider: None,
        call_depth: 0,
        max_call_depth: 8,
        cost_model: INITIAL_COST_MODEL,
    };

    let result: ExecutionResult<D> = outer_ir
        .execute(vec![addr_hi, addr_lo, input_val], context, &mut rng)
        .expect("execute should succeed");

    // Inner should have returned 2 * 17 = 34
    let sub = &result.sub_calls[0];
    assert_eq!(sub.execution_result.outputs.len(), 1);
    assert_eq!(
        sub.execution_result.outputs[0],
        Fr::from(34u64),
        "inner should return 2 * input"
    );

    // Outer should return 34 + 17 = 51 (= 3 * input)
    assert_eq!(result.outputs.len(), 1);
    assert_eq!(
        result.outputs[0],
        Fr::from(51u64),
        "outer should return 3 * input (doubled + original)"
    );

    // Verify call tree structure
    assert_eq!(result.sub_calls.len(), 1);
    assert_eq!(sub.address, inner_addr);
    assert_eq!(sub.entry_point, EntryPointBuf(b"double".to_vec()));

    // Verify pre_transcripts has two transcripts with proper linkage
    let flat = result.pre_transcripts.clone();
    assert_eq!(flat.len(), 2);
    assert!(flat[0].comm_comm.is_none());
    assert!(flat[1].comm_comm.is_some());
}

/// Test 13: Contract reads callee address from its own ledger state, then calls it.
///
/// Contract A's ledger state stores contract B's address at key 0. A's circuit:
/// 1. Reads the address from state via Impact (Dup, Idx, Popeq).
/// 2. Receives the two field elements via PublicInput.
/// 3. Calls B using the address read from state.
/// 4. Computes a non-trivial result from B's return value.
///
/// Contract B is a simple "increment" circuit: takes one input, returns input + 1.
///
/// This tests the pattern where a contract reference lives in ledger state
/// (the DEX pattern) rather than being passed as a circuit parameter.
#[test]
fn test_call_contract_from_ledger_state() {
    let mut rng = StdRng::seed_from_u64(0xD00);

    // Inner contract B: "increment" — takes one input, returns input + 1
    let inner_ir = IrSource {
        inputs: vec![TypedIdentifier::new(id("%x"), IrType::Native)],
        do_communications_commitment: true,
        instructions: Arc::new(vec![
            Instruction::Add {
                a: var("%x"),
                b: imm(1),
                output: id("%result"),
            },
            Instruction::Output {
                val: var("%result"),
            },
        ]),
    };
    let inner_addr = make_address(0xD01);

    // Contract A stores inner_addr in its ledger state as a Bytes<32> cell at key 0.
    // The state layout is: Array([Cell(AlignedValue::from(inner_addr.0))])
    //
    // A's circuit:
    // 1. Impact block: Dup state root, Idx[0], Popeq → reads the cell
    //    The cell is a Bytes<32> = 2 field elements via value_only_field_repr
    // 2. PublicInput × 2 to capture the two field elements of the address
    // 3. ContractCall using those two field elements as contract_ref
    // 4. Arithmetic on the result
    let outer_ir = IrSource {
        inputs: vec![
            // A value to pass to the callee
            TypedIdentifier::new(id("%caller_val"), IrType::Native),
        ],
        do_communications_commitment: false,
        instructions: Arc::new(vec![
            // Impact block: read the contract address from state
            Instruction::Impact {
                guard: imm(1),
                ops: vec![
                    Op::Dup { n: 0 },
                    Op::Idx {
                        cached: false,
                        push_path: false,
                        path: vec![ZkirKey::Value {
                            alignment: alignment_u8(),
                            operands: vec![imm(0)],
                        }],
                    },
                    Op::Popeq {
                        cached: false,
                        result: (),
                    },
                ],
                read_results: vec![vec![]],
            },
            // The read result is a Bytes<32> (contract address), which encodes
            // as 2 field elements in the public transcript outputs.
            Instruction::PublicInput {
                guard: None,
                val_t: IrType::Native,
                output: id("%addr_hi"),
            },
            Instruction::PublicInput {
                guard: None,
                val_t: IrType::Native,
                output: id("%addr_lo"),
            },
            // Call the contract at the address we just read from state
            Instruction::ContractCall {
                contract_ref: (var("%addr_hi"), var("%addr_lo")),
                expected_type: stub_type_descriptor(),
                entry_point: "increment".to_string(),
                args: vec![var("%caller_val")],
                outputs: vec![id("%call_result")],
            },
            // Non-trivial result: multiply the call result by 10
            Instruction::Mul {
                a: var("%call_result"),
                b: imm(10),
                output: id("%final"),
            },
            Instruction::Output {
                val: var("%final"),
            },
        ]),
    };
    let outer_addr = make_address(0xD02);

    // Build outer's state: an Array containing a Cell with the inner address
    // The address is stored as Bytes<32>, so we use the HashOutput → AlignedValue
    // conversion that matches how Compact stores contract addresses.
    let addr_aligned = {
        use base_crypto::fab::{Aligned, Value, ValueAtom};
        AlignedValue {
            value: Value(vec![ValueAtom::from(inner_addr.0)]),
            alignment: HashOutput::alignment(),
        }
    };
    let outer_state: ChargedState<D> = make_cell_state(addr_aligned);

    let mut provider = TestZkirProvider::<D>::new();
    provider.register(
        inner_addr,
        HashMap::from([("increment".to_string(), inner_ir)]),
        make_null_state(),
    );
    provider.register(
        outer_addr,
        HashMap::from([("call_from_state".to_string(), outer_ir.clone())]),
        outer_state.clone(),
    );

    let caller_val = Fr::from(7u64);

    let context = ExecutionContext {
        ledger_state: outer_state,
        address: outer_addr,
        zkir_provider: Arc::new(provider),
        witness_provider: None,
        call_depth: 0,
        max_call_depth: 8,
        cost_model: INITIAL_COST_MODEL,
    };

    let result: ExecutionResult<D> = outer_ir
        .execute(vec![caller_val], context, &mut rng)
        .expect("execute should succeed");

    // Inner should have computed: 7 + 1 = 8
    assert_eq!(result.sub_calls.len(), 1, "one sub-call expected");
    let sub = &result.sub_calls[0];
    assert_eq!(sub.address, inner_addr, "sub-call should target inner");
    assert_eq!(
        sub.execution_result.outputs[0],
        Fr::from(8u64),
        "inner should return input + 1"
    );

    // Outer should have computed: 8 * 10 = 80
    assert_eq!(result.outputs.len(), 1);
    assert_eq!(
        result.outputs[0],
        Fr::from(80u64),
        "outer should return (input + 1) * 10"
    );

    // Verify the address was correctly resolved from state
    assert_eq!(
        sub.entry_point,
        EntryPointBuf(b"increment".to_vec()),
        "entry point should be 'increment'"
    );

    // Verify pre_transcripts has correct transcript structure
    let flat = result.pre_transcripts.clone();
    assert_eq!(flat.len(), 2, "two participants");
    assert!(flat[0].comm_comm.is_none(), "root has no comm_comm");
    assert!(flat[1].comm_comm.is_some(), "callee has a comm_comm");
}

/// Test: Guarded PublicInput before unguarded PublicInput.
///
/// Verifies that the preprocessor correctly computes stream offsets when a
/// guarded PublicInput precedes an unguarded one. The pre-populate pass
/// must stop at the guarded PI (whose stream consumption depends on the
/// guard value), allowing the main loop to handle offsets correctly.
///
/// Contract: takes %cond as input.
///   PublicInput { guard: Some(%cond), output: %guarded }  — consumes offset 0 when active
///   PublicInput { guard: None, output: %unguarded }       — consumes offset 1
///   Output { val: %unguarded }
///
/// With %cond=1, both PIs are active. public_transcript_outputs = [10, 20].
/// The output should be 20 (the unguarded value), not 10.
#[test]
fn test_guarded_public_input_before_unguarded() {
    use std::borrow::Cow;
    use transient_crypto::proofs::{KeyLocation, ProofPreimage};

    let ir = IrSource {
        inputs: vec![TypedIdentifier::new(id("%cond"), IrType::Native)],
        do_communications_commitment: false,
        instructions: Arc::new(vec![
            Instruction::PublicInput {
                guard: Some(var("%cond")),
                val_t: IrType::Native,
                output: id("%guarded"),
            },
            Instruction::PublicInput {
                guard: None,
                val_t: IrType::Native,
                output: id("%unguarded"),
            },
            Instruction::Output {
                val: var("%unguarded"),
            },
        ]),
    };

    let preimage = ProofPreimage {
        binding_input: 0.into(),
        communications_commitment: Some((0.into(), 0.into())),
        inputs: vec![1.into()], // %cond = 1 (guard active)
        private_transcript: vec![],
        public_transcript_inputs: vec![],
        public_transcript_outputs: vec![10.into(), 20.into()],
        key_location: KeyLocation(Cow::Borrowed("test")),
    };

    // check() calls preprocess() internally. If the pre-populate pass
    // reads %unguarded from offset 0 instead of 1, the output would be
    // 10 instead of 20, and comm_comm validation would fail.
    let result = preimage.check(&ir);
    assert!(result.is_ok(), "preprocessing should succeed: {result:?}");
}

/// Test: Executor correctly handles multi-element input types (JubjubPoint).
///
/// A JubjubPoint is encoded as 2 field elements. Before the fix, the executor
/// compared `inputs.len()` against `self.inputs.len()` (number of typed
/// identifiers) instead of the total field element count, and stored each
/// element as IrValue::Native instead of decoding via encoded_len().
#[test]
fn test_execute_jubjub_point_input() {
    use group::Group;
    use midnight_curves::JubjubSubgroup;
    use rand::rngs::OsRng;
    use transient_crypto::curve::EmbeddedGroupAffine;

    let mut rng = StdRng::seed_from_u64(0xF00);

    // Contract: takes a JubjubPoint, encodes it to (x, y), outputs x.
    let ir = IrSource {
        inputs: vec![TypedIdentifier::new(id("%p"), IrType::JubjubPoint)],
        do_communications_commitment: false,
        instructions: Arc::new(vec![
            Instruction::Encode {
                input: var("%p"),
                outputs: vec![id("%x"), id("%y")],
            },
            Instruction::Output { val: var("%x") },
        ]),
    };

    let addr = make_address(0xF0);
    let provider = TestZkirProvider::<D>::new();

    let point: EmbeddedGroupAffine = JubjubSubgroup::random(OsRng).into();
    let px = point.x().unwrap();
    let py = point.y().unwrap();

    let context = ExecutionContext {
        ledger_state: make_null_state(),
        address: addr,
        zkir_provider: Arc::new(provider),
        witness_provider: None,
        call_depth: 0,
        max_call_depth: 8,
        cost_model: INITIAL_COST_MODEL,
    };

    // Pass 2 field elements for 1 JubjubPoint input.
    let result: ExecutionResult<D> = ir
        .execute(vec![px, py], context, &mut rng)
        .expect("execute should succeed with JubjubPoint input");

    assert_eq!(result.outputs.len(), 1);
    assert_eq!(result.outputs[0], px, "output should be the x coordinate");
}
