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

//! Non-trivial result tests: contracts that perform meaningful computation
//! and return results that depend on both the call and the caller's logic.
//!
//! Test 1 (call parameter): Contract A receives B's address as a circuit
//! parameter, calls B, and both return non-trivial results.
//!
//! Test 2 (ledger state): Contract A reads B's address from its own ledger
//! state, calls B, and both return non-trivial results.

use std::borrow::Cow;
use std::collections::HashMap as StdHashMap;
use std::sync::Arc;

use base_crypto::time::Timestamp;
use midnight_ledger::construct::{
    ContractCallPrototype, PreTranscript, partition_transcripts,
};
use midnight_ledger::structure::{ContractDeploy, INITIAL_PARAMETERS, Transaction};
use midnight_ledger::test_utilities::{TestState, test_intents};
use midnight_ledger::verify::WellFormedStrictness;
use onchain_runtime::state::{
    ChargedState, ContractOperation, ContractState, EntryPointBuf, StateValue,
};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use storage::db::InMemoryDB;
use storage::storage::HashMap;
use transient_crypto::curve::Fr;
use transient_crypto::proofs::{KeyLocation, VerifierKey};

use midnight_zkir_v3::ir_execute::{ExecutionContext, ExecutionResult, PreTranscriptData};
use onchain_runtime::cost_model::INITIAL_COST_MODEL;

use crate::common::*;

type D = InMemoryDB;

/// End-to-end test: contract A is passed contract B as a circuit parameter,
/// calls B, and both return non-trivial results that are functions of the call.
///
/// Inner (B): "add_state" — reads stored_val (100) from state, takes input,
///            returns input + stored_val.
/// Outer (A): "call_add" — takes B's address + value, calls B.add_state(value),
///            then returns call_result + value.
///
/// For input = 17, stored = 100:
///   inner returns 17 + 100 = 117
///   outer returns 117 + 17 = 134
///
/// Pipeline: deploy → execute → partition → prototype → tx → apply.
#[tokio::test]
async fn test_e2e_nontrivial_result_from_call_parameter() {
    let mut rng = StdRng::seed_from_u64(0x6001);

    let stored_val = Fr::from(100u64);
    let input_val = Fr::from(17u64);
    let expected_inner_result = Fr::from(117u64); // 17 + 100
    let expected_outer_result = Fr::from(134u64); // 117 + 17

    let inner_ir = build_inner_add_state_ir();
    let outer_ir = build_outer_call_add_ir();

    let inner_state: ChargedState<D> = make_cell_state(field_aligned_value(stored_val));
    let outer_state: ChargedState<D> = ChargedState::new(StateValue::Null);

    let dummy_vk_inner: VerifierKey = rng.r#gen();
    let dummy_vk_outer: VerifierKey = rng.r#gen();
    let inner_op = ContractOperation::new_with_zkir(Some(dummy_vk_inner), serialize_ir(&inner_ir));
    let outer_op = ContractOperation::new_with_zkir(Some(dummy_vk_outer), serialize_ir(&outer_ir));

    // Deploy
    let mut state: TestState<D> = TestState::new(&mut rng);
    let mut strictness = WellFormedStrictness::default();
    strictness.enforce_balancing = false;

    let inner_contract = ContractState::new(
        inner_state.get_ref().clone(),
        HashMap::new().insert(b"add_state"[..].into(), inner_op.clone()),
        Default::default(),
    );
    let inner_deploy = ContractDeploy::new(&mut rng, inner_contract);
    let inner_addr = inner_deploy.address();
    let deploy_inner_tx = Transaction::from_intents(
        "local-test",
        test_intents(
            &mut rng,
            Vec::new(),
            Vec::new(),
            vec![inner_deploy],
            Timestamp::from_secs(0),
        ),
    )
    .erase_proofs();
    state.assert_apply(&deploy_inner_tx, strictness);

    let outer_contract = ContractState::new(
        outer_state.get_ref().clone(),
        HashMap::new().insert(b"call_add"[..].into(), outer_op.clone()),
        Default::default(),
    );
    let outer_deploy = ContractDeploy::new(&mut rng, outer_contract);
    let outer_addr = outer_deploy.address();
    let deploy_outer_tx = Transaction::from_intents(
        "local-test",
        test_intents(
            &mut rng,
            Vec::new(),
            Vec::new(),
            vec![outer_deploy],
            Timestamp::from_secs(0),
        ),
    )
    .erase_proofs();
    state.assert_apply(&deploy_outer_tx, strictness);

    // Execute
    let mut provider = TestZkirProvider::<D>::new();
    provider.register(
        inner_addr,
        StdHashMap::from([("add_state".to_string(), inner_ir.clone())]),
        inner_state.clone(),
    );
    provider.register(
        outer_addr,
        StdHashMap::from([("call_add".to_string(), outer_ir.clone())]),
        outer_state.clone(),
    );

    let context = ExecutionContext {
        ledger_state: outer_state,
        address: outer_addr,
        zkir_provider: Arc::new(provider),
        witness_provider: None,
        call_depth: 0,
        max_call_depth: 8,
        cost_model: INITIAL_COST_MODEL,
    };

    let (addr_hi, addr_lo) = addr_to_frs(inner_addr);
    let result: ExecutionResult<D> = outer_ir
        .execute(vec![addr_hi, addr_lo, input_val], context, &mut rng)
        .expect("execute should succeed");

    // Verify non-trivial results
    assert_eq!(
        result.sub_calls[0].execution_result.outputs[0], expected_inner_result,
        "inner should return input + stored_val"
    );
    assert_eq!(
        result.outputs[0], expected_outer_result,
        "outer should return (input + stored_val) + input"
    );

    // Partition
    let flat: Vec<PreTranscriptData<D>> = result.pre_transcripts.clone();
    let pre_transcripts: Vec<PreTranscript<D>> = flat.into_iter().map(to_pre_transcript).collect();
    let pairs = partition_transcripts(&pre_transcripts, &INITIAL_PARAMETERS)
        .expect("partition should succeed");
    assert_eq!(pairs.len(), 2);

    // Build prototypes
    let sub = &result.sub_calls[0];
    let call_inner_proto = ContractCallPrototype {
        address: inner_addr,
        entry_point: sub.entry_point.clone(),
        op: inner_op,
        guaranteed_public_transcript: pairs[1].0.clone(),
        fallible_public_transcript: pairs[1].1.clone(),
        private_transcript_outputs: vec![],
        input: sub.input.clone(),
        output: sub.output.clone(),
        communication_commitment_rand: sub.communication_commitment_rand,
        key_location: KeyLocation(Cow::Borrowed("add_state")),
    };

    let call_outer_proto = ContractCallPrototype {
        address: outer_addr,
        entry_point: EntryPointBuf(b"call_add".to_vec()),
        op: outer_op,
        guaranteed_public_transcript: pairs[0].0.clone(),
        fallible_public_transcript: pairs[0].1.clone(),
        private_transcript_outputs: vec![
            sub.output.clone(),
            sub.communication_commitment_rand.into(),
        ],
        input: fields_aligned_value(&[addr_hi, addr_lo, input_val]),
        output: field_aligned_value(expected_outer_result),
        communication_commitment_rand: rng.r#gen(),
        key_location: KeyLocation(Cow::Borrowed("call_add")),
    };

    // Build transaction → apply
    let tx = Transaction::from_intents(
        "local-test",
        test_intents(
            &mut rng,
            vec![call_inner_proto, call_outer_proto],
            Vec::new(),
            Vec::new(),
            Timestamp::from_secs(0),
        ),
    )
    .erase_proofs();

    state.assert_apply(&tx, strictness);
}

/// End-to-end test: contract A reads contract B's address from its own ledger
/// state, then calls B. Both return non-trivial results.
///
/// Inner (B): "increment" — takes input, returns input + 1.
/// Outer (A): "call_from_state" — reads B's address from state[0], calls
///            B.add_state(value), then returns call_result + value.
///
/// Inner's state holds S = 50. For input V = 7:
///   inner returns V + S = 7 + 50 = 57
///   outer returns (V + S) + V = 57 + 7 = 64
///
/// Pipeline: deploy → execute → partition → prototype → tx → apply.
#[tokio::test]
async fn test_e2e_call_contract_from_ledger_state() {
    let mut rng = StdRng::seed_from_u64(0x6002);

    let inner_stored_val = Fr::from(50u64);
    let caller_val = Fr::from(7u64);
    let expected_inner_result = Fr::from(57u64); // 7 + 50
    let expected_outer_result = Fr::from(64u64); // 57 + 7

    let inner_ir = build_inner_add_state_ir();
    let outer_ir = build_outer_call_from_state_ir();

    let inner_state: ChargedState<D> = make_cell_state(field_aligned_value(inner_stored_val));

    let dummy_vk_inner: VerifierKey = rng.r#gen();
    let dummy_vk_outer: VerifierKey = rng.r#gen();
    let inner_op = ContractOperation::new_with_zkir(Some(dummy_vk_inner), serialize_ir(&inner_ir));
    let outer_op = ContractOperation::new_with_zkir(Some(dummy_vk_outer), serialize_ir(&outer_ir));

    // Deploy inner first to get its address
    let mut state: TestState<D> = TestState::new(&mut rng);
    let mut strictness = WellFormedStrictness::default();
    strictness.enforce_balancing = false;

    let inner_contract = ContractState::new(
        inner_state.get_ref().clone(),
        HashMap::new().insert(b"add_state"[..].into(), inner_op.clone()),
        Default::default(),
    );
    let inner_deploy = ContractDeploy::new(&mut rng, inner_contract);
    let inner_addr = inner_deploy.address();
    let deploy_inner_tx = Transaction::from_intents(
        "local-test",
        test_intents(
            &mut rng,
            Vec::new(),
            Vec::new(),
            vec![inner_deploy],
            Timestamp::from_secs(0),
        ),
    )
    .erase_proofs();
    state.assert_apply(&deploy_inner_tx, strictness);

    // Deploy outer with inner's address stored in its ledger state
    let outer_state: ChargedState<D> = make_address_state(inner_addr);

    let outer_contract = ContractState::new(
        outer_state.get_ref().clone(),
        HashMap::new().insert(b"call_from_state"[..].into(), outer_op.clone()),
        Default::default(),
    );
    let outer_deploy = ContractDeploy::new(&mut rng, outer_contract);
    let outer_addr = outer_deploy.address();
    let deploy_outer_tx = Transaction::from_intents(
        "local-test",
        test_intents(
            &mut rng,
            Vec::new(),
            Vec::new(),
            vec![outer_deploy],
            Timestamp::from_secs(0),
        ),
    )
    .erase_proofs();
    state.assert_apply(&deploy_outer_tx, strictness);

    // Execute
    let mut provider = TestZkirProvider::<D>::new();
    provider.register(
        inner_addr,
        StdHashMap::from([("add_state".to_string(), inner_ir.clone())]),
        inner_state.clone(),
    );
    provider.register(
        outer_addr,
        StdHashMap::from([("call_from_state".to_string(), outer_ir.clone())]),
        outer_state.clone(),
    );

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

    // Verify non-trivial results
    assert_eq!(result.sub_calls.len(), 1, "one sub-call expected");
    let sub = &result.sub_calls[0];
    assert_eq!(sub.address, inner_addr);
    assert_eq!(
        sub.execution_result.outputs[0], expected_inner_result,
        "inner should return input + stored_val"
    );
    assert_eq!(
        result.outputs[0], expected_outer_result,
        "outer should return (input + stored_val) + input"
    );

    // Partition
    let flat: Vec<PreTranscriptData<D>> = result.pre_transcripts.clone();
    let pre_transcripts: Vec<PreTranscript<D>> = flat.into_iter().map(to_pre_transcript).collect();
    let pairs = partition_transcripts(&pre_transcripts, &INITIAL_PARAMETERS)
        .expect("partition should succeed");
    assert_eq!(pairs.len(), 2);

    // Build prototypes
    let call_inner_proto = ContractCallPrototype {
        address: inner_addr,
        entry_point: sub.entry_point.clone(),
        op: inner_op,
        guaranteed_public_transcript: pairs[1].0.clone(),
        fallible_public_transcript: pairs[1].1.clone(),
        private_transcript_outputs: vec![],
        input: sub.input.clone(),
        output: sub.output.clone(),
        communication_commitment_rand: sub.communication_commitment_rand,
        key_location: KeyLocation(Cow::Borrowed("add_state")),
    };

    let call_outer_proto = ContractCallPrototype {
        address: outer_addr,
        entry_point: EntryPointBuf(b"call_from_state".to_vec()),
        op: outer_op,
        guaranteed_public_transcript: pairs[0].0.clone(),
        fallible_public_transcript: pairs[0].1.clone(),
        private_transcript_outputs: vec![
            sub.output.clone(),
            sub.communication_commitment_rand.into(),
        ],
        input: field_aligned_value(caller_val),
        output: field_aligned_value(expected_outer_result),
        communication_commitment_rand: rng.r#gen(),
        key_location: KeyLocation(Cow::Borrowed("call_from_state")),
    };

    // Build transaction → apply
    let tx = Transaction::from_intents(
        "local-test",
        test_intents(
            &mut rng,
            vec![call_inner_proto, call_outer_proto],
            Vec::new(),
            Vec::new(),
            Timestamp::from_secs(0),
        ),
    )
    .erase_proofs();

    state.assert_apply(&tx, strictness);
}
