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

//! End-to-end happy-path tests for the composable-ZKIR pipeline:
//! deploy → execute → flatten → partition → prototype → transaction →
//! `erase_proofs` → `well_formed` → `apply`.
//!
//! Each test exercises the full stack but with a different circuit
//! complexity: the simplest is a "relay" call where the outer just returns
//! whatever the inner produced; the next two perform non-trivial
//! computation either side of the call boundary, and the last reads the
//! callee's address from the caller's ledger state (the DEX-discovery
//! pattern from the proposal).
//!
//! Negative tests (intentionally-defective transactions that should be
//! rejected by `well_formed`) live in `rejection.rs`. Tests that stop
//! short of full transaction construction (execution-level claim linkage
//! and partition behaviour with missing callees) live in `linkage.rs`.
//! Real-proof variants of these tests live in `proving.rs` behind the
//! `proving` feature.

use std::borrow::Cow;
use std::collections::HashMap as StdHashMap;
use std::ops::Deref;
use std::sync::Arc;

use base_crypto::time::Timestamp;
use midnight_ledger::construct::{ContractCallPrototype, partition_transcripts};
use midnight_ledger::structure::{ContractDeploy, INITIAL_PARAMETERS, Transaction};
use midnight_ledger::test_utilities::{TestState, test_intents};
use midnight_ledger::verify::WellFormedStrictness;
use onchain_runtime::state::{ChargedState, ContractOperation, ContractState, StateValue};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use storage::storage::HashMap;
use transient_crypto::curve::Fr;
use transient_crypto::hash::transient_commit;
use transient_crypto::proofs::{KeyLocation, VerifierKey};

use midnight_zkir_v3::ir_execute::{ExecutionContext, ExecutionResult};
use onchain_runtime::cost_model::INITIAL_COST_MODEL;

use crate::common::*;

// ── Test 1: trivial relay ────────────────────────────────────────────────
//
// The outer just returns what the inner produced. Establishes that the
// happy-path pipeline works end-to-end before either side does anything
// computationally interesting.

/// Pipeline with a "relay" outer that simply returns whatever the inner
/// callee read from its own ledger state.
///
/// Inner stores `42` at cell[0]. Outer calls inner.get(); the result
/// propagates back unchanged.
#[tokio::test]
async fn test_e2e_relay_pipeline() {
    let mut rng = StdRng::seed_from_u64(0x4b01);

    // ── Setup ──
    let stored_value = Fr::from(42u64);
    let inner_ir = build_inner_get_ir();
    let outer_ir = build_outer_call_ir();

    let inner_state: ChargedState<D> = make_cell_state(field_aligned_value(stored_value));
    let outer_state: ChargedState<D> = ChargedState::new(StateValue::Null);

    // Use dummy verifier keys — we erase proofs so they're never verified,
    // but well_formed requires that a VK is set for each operation.
    let dummy_vk_inner: VerifierKey = rng.r#gen();
    let dummy_vk_outer: VerifierKey = rng.r#gen();
    let inner_op = ContractOperation::new_with_zkir(Some(dummy_vk_inner), serialize_ir(&inner_ir));
    let outer_op = ContractOperation::new_with_zkir(Some(dummy_vk_outer), serialize_ir(&outer_ir));

    // ── Step 1: Deploy contracts to get real addresses ──
    let mut state: TestState<D> = TestState::new(&mut rng);
    let mut strictness = WellFormedStrictness::default();
    strictness.enforce_balancing = false;

    // Deploy inner contract
    let inner_contract = ContractState::new(
        inner_state.get_ref().clone(),
        HashMap::new().insert(b"get"[..].into(), inner_op.clone()),
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

    // Deploy outer contract
    let outer_contract = ContractState::new(
        outer_state.get_ref().clone(),
        HashMap::new().insert(b"call_inner"[..].into(), outer_op.clone()),
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

    // ── Step 2: Execute outer (which calls inner) using deployed addresses ──
    let mut provider = TestZkirProvider::<D>::new();
    provider.register(
        inner_addr,
        StdHashMap::from([("get".to_string(), inner_ir.clone())]),
        inner_state.clone(),
    );
    provider.register(
        outer_addr,
        StdHashMap::from([("call_inner".to_string(), outer_ir.clone())]),
        outer_state.clone(),
    );

    let context = ExecutionContext {
        ledger_state: outer_state.clone(),
        address: outer_addr,
        entry_point: ep("call_inner"),
        zkir_provider: Arc::new(provider),
        witness_provider: None,
        call_depth: 0,
        max_call_depth: 8,
        cost_model: INITIAL_COST_MODEL,
    };

    let (addr_hi, addr_lo) = addr_to_frs(inner_addr);
    let result: ExecutionResult<D> = outer_ir
        .execute(addr_to_fr_vec(inner_addr), context, &mut rng)
        .await
        .expect("execute should succeed");

    // Verify execution: two calls (outer at idx 0, inner at idx 1)
    assert_eq!(result.len(), 2, "two calls expected (outer + inner)");
    assert_eq!(
        result[0].output,
        vec![stored_value],
        "outer should relay inner's value"
    );
    assert!(result[0].comm_comm().is_none(), "root has no comm_comm");
    assert!(result[1].comm_comm().is_some(), "callee has a comm_comm");
    assert_eq!(result[1].parent, Some(0));

    // ── Step 3: Convert to PreTranscript ──
    let pre_transcripts: Vec<PreTranscript<D>> = calls_to_pre_transcripts(result.clone());

    // ── Step 4: partition_transcripts ──
    let pairs = partition_transcripts(&pre_transcripts, &INITIAL_PARAMETERS)
        .expect("partition_transcripts failed");
    assert_eq!(
        pairs.len(),
        2,
        "partition should produce two transcript pairs"
    );

    // Verify claim linkage
    let caller_claimed: Vec<Fr> = pre_transcripts[0]
        .context
        .effects
        .claimed_contract_calls
        .iter()
        .map(|sp| (*sp).deref().into_inner().3)
        .collect();
    let callee_comm_comm = pre_transcripts[1].comm_comm.unwrap();
    assert!(
        caller_claimed.contains(&callee_comm_comm),
        "caller's claimed_contract_calls should contain callee's comm_comm"
    );

    // ── Step 5: Build ContractCallPrototype objects ──
    let sub = &result[1];
    let sub_comm_rand = sub.comm_comm_rand().expect("sub-call has comm_comm_rand");

    let call_inner_proto = ContractCallPrototype {
        address: inner_addr,
        entry_point: sub.entry_point.clone(),
        op: inner_op.clone(),
        guaranteed_public_transcript: pairs[1].0.clone(),
        fallible_public_transcript: pairs[1].1.clone(),
        private_transcript_outputs: vec![],
        // Sub-call I/O for the prototype: wrap the flat Fr sequence in a
        // single multi-Field-aligned AV — `to_proof_preimage` reads the
        // field elements via `value_only_field_repr`, which yields exactly
        // these Fr values regardless of how they're grouped.
        input: frs_to_concat_av(&sub.input),
        output: frs_to_concat_av(&sub.output),
        communication_commitment_rand: sub_comm_rand,
        key_location: KeyLocation(Cow::Borrowed("get")),
    };

    let call_outer_proto = ContractCallPrototype {
        address: outer_addr,
        entry_point: ep("call_inner"),
        op: outer_op.clone(),
        guaranteed_public_transcript: pairs[0].0.clone(),
        fallible_public_transcript: pairs[0].1.clone(),
        private_transcript_outputs: result[0].private_transcript_outputs.clone(),
        input: fields_aligned_value(&[addr_hi, addr_lo]),
        output: field_aligned_value(stored_value),
        communication_commitment_rand: rng.r#gen(),
        key_location: KeyLocation(Cow::Borrowed("call_inner")),
    };

    // Verify communication commitment. With flat Fr I/O the canonical
    // representation is just `input ∥ output`, matching what
    // `value_only_field_repr` yields on the wrapped AVs above.
    let expected_inner_cc = {
        let mut io_repr: Vec<Fr> = Vec::with_capacity(sub.input.len() + sub.output.len());
        io_repr.extend_from_slice(&sub.input);
        io_repr.extend_from_slice(&sub.output);
        transient_commit(&io_repr, sub_comm_rand)
    };
    assert_eq!(
        callee_comm_comm, expected_inner_cc,
        "callee comm_comm should match recomputed transient_commit"
    );

    // ── Step 6: Build composable transaction → well_formed → apply ──
    let composable_tx = Transaction::from_intents(
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

    state.assert_apply(&composable_tx, strictness);
}

// ── Test 2: non-trivial computation, callee address as parameter ──────────
//
// Inner: "add_state" — reads stored_val (100) from state, takes input,
//        returns input + stored_val.
// Outer: "call_add" — takes B's address + value, calls B.add_state(value),
//        then returns call_result + value.
//
// For input = 17, stored = 100:
//   inner returns 17 + 100 = 117
//   outer returns 117 + 17 = 134
//
// Verifies that values flow correctly across the call boundary in both
// directions when both sides perform real arithmetic.

#[tokio::test]
async fn test_e2e_nontrivial_result_from_call_parameter() {
    // Set up VKs (dummy random for the erase-proofs path).
    let mut rng_for_keys = StdRng::seed_from_u64(0x6001);
    let dummy_vk_inner: VerifierKey = rng_for_keys.r#gen();
    let dummy_vk_outer: VerifierKey = rng_for_keys.r#gen();

    // Run the shared deploy → execute → partition → prototype-build pipeline.
    let mut p = add_state_pipeline(
        0x6001,
        Fr::from(100u64),
        Fr::from(17u64),
        dummy_vk_inner,
        dummy_vk_outer,
    )
    .await;

    // Build the transaction with proofs erased (fast path) and apply.
    let tx = Transaction::from_intents(
        "local-test",
        test_intents(
            &mut p.rng,
            vec![p.call_inner_proto, p.call_outer_proto],
            Vec::new(),
            Vec::new(),
            Timestamp::from_secs(0),
        ),
    )
    .erase_proofs();

    p.state.assert_apply(&tx, p.strictness);
}

// ── Test 3: callee address read from caller's ledger state ────────────────
//
// The DEX-discovery pattern: caller doesn't take the callee's address as
// a parameter; it reads it from its own ledger state at a known key.
//
// Inner stores S = 50; on each call returns input + 50.
// Outer reads inner's address from state[0], calls inner.add_state(V),
// then returns call_result + V.
//
// For V = 7:
//   inner returns 7 + 50 = 57
//   outer returns 57 + 7 = 64

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
        entry_point: ep("call_from_state"),
        zkir_provider: Arc::new(provider),
        witness_provider: None,
        call_depth: 0,
        max_call_depth: 8,
        cost_model: INITIAL_COST_MODEL,
    };

    let result: ExecutionResult<D> = outer_ir
        .execute(vec![caller_val], context, &mut rng)
        .await
        .expect("execute should succeed");

    // Verify non-trivial results
    assert_eq!(result.len(), 2);
    let inner_call = &result[1];
    assert_eq!(inner_call.address, inner_addr);
    assert_eq!(
        inner_call.output,
        vec![expected_inner_result],
        "inner should return input + stored_val"
    );
    assert_eq!(
        result[0].output,
        vec![expected_outer_result],
        "outer should return (input + stored_val) + input"
    );

    // Partition
    let pre_transcripts: Vec<PreTranscript<D>> = calls_to_pre_transcripts(result.clone());
    let pairs = partition_transcripts(&pre_transcripts, &INITIAL_PARAMETERS)
        .expect("partition should succeed");
    assert_eq!(pairs.len(), 2);

    // Build prototypes
    let sub = inner_call;
    let sub_comm_rand = sub.comm_comm_rand().expect("sub has comm_comm_rand");

    let call_inner_proto = ContractCallPrototype {
        address: inner_addr,
        entry_point: sub.entry_point.clone(),
        op: inner_op,
        guaranteed_public_transcript: pairs[1].0.clone(),
        fallible_public_transcript: pairs[1].1.clone(),
        private_transcript_outputs: vec![],
        input: frs_to_concat_av(&sub.input),
        output: frs_to_concat_av(&sub.output),
        communication_commitment_rand: sub_comm_rand,
        key_location: KeyLocation(Cow::Borrowed("add_state")),
    };

    let call_outer_proto = ContractCallPrototype {
        address: outer_addr,
        entry_point: ep("call_from_state"),
        op: outer_op,
        guaranteed_public_transcript: pairs[0].0.clone(),
        fallible_public_transcript: pairs[0].1.clone(),
        private_transcript_outputs: result[0].private_transcript_outputs.clone(),
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
