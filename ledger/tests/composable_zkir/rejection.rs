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

//! Rejection tests for composable transactions.
//!
//! These tests verify that `well_formed` correctly rejects composable
//! transactions with deliberate defects. Each test uses `run_pipeline` to
//! get a valid execution result, then tampers with one aspect of the
//! `ContractCallPrototype` before building the transaction.
//!
//! `run_pipeline` and the surrounding helper machinery are private to this
//! module — they exist only to set up valid baselines for the negative
//! tests below.

use std::borrow::Cow;
use std::collections::HashMap as StdHashMap;
use std::sync::Arc;

use base_crypto::time::Timestamp;
use midnight_ledger::construct::{ContractCallPrototype, TranscriptPair, partition_transcripts};
use midnight_ledger::error::{EffectsCheckError, MalformedTransaction};
use midnight_ledger::structure::{ContractDeploy, INITIAL_PARAMETERS, Transaction};
use midnight_ledger::test_utilities::{TestState, test_intents};
use midnight_ledger::verify::WellFormedStrictness;
use onchain_runtime::cost_model::INITIAL_COST_MODEL;
use onchain_runtime::state::{ChargedState, ContractOperation, ContractState, StateValue};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use storage::storage::HashMap;
use transient_crypto::curve::Fr;
use transient_crypto::proofs::KeyLocation;

use midnight_zkir_v3::ir_execute::{ExecutionContext, ExecutionResult};

use crate::common::*;

// ── Pipeline helper ──────────────────────────────────────────────────────
//
// `run_pipeline` deploys inner + outer contracts, executes the outer
// (which calls inner), partitions the resulting transcripts, and bundles
// everything the rejection tests below need to construct a *valid*
// composable transaction. Each test then tampers with one prototype
// before submitting, to exercise a specific `well_formed` rejection path.

struct PipelineResult {
    state: TestState<D>,
    strictness: WellFormedStrictness,
    inner_addr: coin_structure::contract::ContractAddress,
    outer_addr: coin_structure::contract::ContractAddress,
    inner_op: ContractOperation,
    outer_op: ContractOperation,
    result: ExecutionResult<D>,
    pairs: Vec<TranscriptPair<D>>,
    stored_value: Fr,
    addr_hi: Fr,
    addr_lo: Fr,
    rng: StdRng,
}

async fn run_pipeline(seed: u64) -> PipelineResult {
    let mut rng = StdRng::seed_from_u64(seed);

    let stored_value = Fr::from(42u64);
    let inner_ir = build_inner_get_ir();
    let outer_ir = build_outer_call_ir();

    let inner_state: ChargedState<D> = make_cell_state(field_aligned_value(stored_value));
    let outer_state: ChargedState<D> = ChargedState::new(StateValue::Null);

    let dummy_vk_inner: VerifierKey = rng.r#gen();
    let dummy_vk_outer: VerifierKey = rng.r#gen();
    let inner_op = ContractOperation::new_with_zkir(Some(dummy_vk_inner), serialize_ir(&inner_ir));
    let outer_op = ContractOperation::new_with_zkir(Some(dummy_vk_outer), serialize_ir(&outer_ir));

    // Deploy contracts
    let mut state: TestState<D> = TestState::new(&mut rng);
    let mut strictness = WellFormedStrictness::default();
    strictness.enforce_balancing = false;

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

    // Execute
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
        ledger_state: outer_state,
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

    // Flatten + partition. `result` is already a flat depth-first preorder
    // list of `Call`s, so conversion to `PreTranscript` is a one-pass
    // projection.
    let pre_transcripts: Vec<PreTranscript<D>> = calls_to_pre_transcripts(result.clone());
    let pairs = partition_transcripts(&pre_transcripts, &INITIAL_PARAMETERS)
        .expect("partition should succeed");

    PipelineResult {
        state,
        strictness,
        inner_addr,
        outer_addr,
        inner_op,
        outer_op,
        result,
        pairs,
        stored_value,
        addr_hi,
        addr_lo,
        rng,
    }
}

/// Build the outer ContractCallPrototype shared by all rejection tests.
///
/// Each rejection test tampers with the *inner* prototype (or omits it);
/// the outer prototype is always constructed the same way.
fn build_outer_proto(p: &mut PipelineResult) -> ContractCallPrototype<D> {
    // p.result is Vec<Call<D>>: index 0 = outer (root), index 1 = inner (sub).
    ContractCallPrototype {
        address: p.outer_addr,
        entry_point: ep("call_inner"),
        op: p.outer_op.clone(),
        guaranteed_public_transcript: p.pairs[0].0.clone(),
        fallible_public_transcript: p.pairs[0].1.clone(),
        private_transcript_outputs: p.result[0].private_transcript_outputs.clone(),
        input: fields_aligned_value(&[p.addr_hi, p.addr_lo]),
        output: field_aligned_value(p.stored_value),
        communication_commitment_rand: p.rng.r#gen(),
        key_location: KeyLocation(Cow::Borrowed("call_inner")),
    }
}

// ── Rejection tests ──────────────────────────────────────────────────────

/// Rejection test: omit the callee's ContractCallPrototype from the intent.
///
/// The outer caller's transcript contains a `claimed_contract_calls` entry
/// for the inner call, but the inner call isn't present in the
/// transaction's intent. `well_formed` should reject because the claimed
/// call (addr, ep_hash, comm_comm) has no matching real call in the
/// intent.
#[tokio::test]
async fn test_reject_missing_callee_call() {
    let mut p = run_pipeline(0x5001).await;

    // Build ONLY the outer prototype — omit the inner callee entirely.
    let call_outer_proto = build_outer_proto(&mut p);

    let tx = Transaction::from_intents(
        "local-test",
        test_intents(
            &mut p.rng,
            vec![call_outer_proto], // inner callee missing!
            Vec::new(),
            Vec::new(),
            Timestamp::from_secs(0),
        ),
    )
    .erase_proofs();

    let err = p
        .state
        .apply(&tx, p.strictness)
        .expect_err("should reject: caller claims a sub-call that has no matching real call");

    match err {
        MalformedTransaction::EffectsCheckFailure(
            EffectsCheckError::RealCallsSubsetCheckFailure(_),
        ) => {}
        other => panic!("expected RealCallsSubsetCheckFailure, got: {other:?}"),
    }
}

/// Rejection test: tamper with the callee's communication commitment
/// randomness.
///
/// The inner callee's `ContractCallPrototype` uses a wrong
/// `communication_commitment_rand`, so its recomputed comm_comm won't
/// match the one in the caller's `claimed_contract_calls`. `well_formed`
/// should reject with `RealCallsSubsetCheckFailure`.
#[tokio::test]
async fn test_reject_comm_comm_mismatch() {
    let mut p = run_pipeline(0x5002).await;

    let sub = &p.result[1];

    // Build inner prototype with WRONG communication_commitment_rand.
    let tampered_rand: Fr = p.rng.r#gen();
    let call_inner_proto = ContractCallPrototype {
        address: p.inner_addr,
        entry_point: sub.entry_point.clone(),
        op: p.inner_op.clone(),
        guaranteed_public_transcript: p.pairs[1].0.clone(),
        fallible_public_transcript: p.pairs[1].1.clone(),
        private_transcript_outputs: vec![],
        input: frs_to_concat_av(&sub.input),
        output: frs_to_concat_av(&sub.output),
        communication_commitment_rand: tampered_rand, // TAMPERED
        key_location: KeyLocation(Cow::Borrowed("get")),
    };

    let call_outer_proto = build_outer_proto(&mut p);

    let tx = Transaction::from_intents(
        "local-test",
        test_intents(
            &mut p.rng,
            vec![call_inner_proto, call_outer_proto],
            Vec::new(),
            Vec::new(),
            Timestamp::from_secs(0),
        ),
    )
    .erase_proofs();

    let err = p
        .state
        .apply(&tx, p.strictness)
        .expect_err("should reject: tampered comm_comm_rand produces mismatched comm_comm");

    match err {
        MalformedTransaction::EffectsCheckFailure(
            EffectsCheckError::RealCallsSubsetCheckFailure(_),
        ) => {}
        other => panic!("expected RealCallsSubsetCheckFailure, got: {other:?}"),
    }
}

/// Rejection test: use wrong entry point on the callee's
/// ContractCallPrototype.
///
/// The caller claims a call to entry point "get", but the callee
/// prototype specifies "put" instead. The ep_hash won't match, so
/// `well_formed` should reject with `RealCallsSubsetCheckFailure`.
#[tokio::test]
async fn test_reject_wrong_entry_point() {
    let mut p = run_pipeline(0x5003).await;

    let sub = &p.result[1];
    let sub_comm_rand = sub.comm_comm_rand().expect("sub has comm_comm_rand");

    // Build inner prototype with WRONG entry point.
    let call_inner_proto = ContractCallPrototype {
        address: p.inner_addr,
        entry_point: ep("put"), // WRONG — should be "get"
        op: p.inner_op.clone(),
        guaranteed_public_transcript: p.pairs[1].0.clone(),
        fallible_public_transcript: p.pairs[1].1.clone(),
        private_transcript_outputs: vec![],
        input: frs_to_concat_av(&sub.input),
        output: frs_to_concat_av(&sub.output),
        communication_commitment_rand: sub_comm_rand,
        key_location: KeyLocation(Cow::Borrowed("get")),
    };

    let call_outer_proto = build_outer_proto(&mut p);

    let tx = Transaction::from_intents(
        "local-test",
        test_intents(
            &mut p.rng,
            vec![call_inner_proto, call_outer_proto],
            Vec::new(),
            Vec::new(),
            Timestamp::from_secs(0),
        ),
    )
    .erase_proofs();

    let err = p
        .state
        .apply(&tx, p.strictness)
        .expect_err("should reject: wrong entry point produces mismatched ep_hash");

    match err {
        MalformedTransaction::EffectsCheckFailure(
            EffectsCheckError::RealCallsSubsetCheckFailure(_),
        ) => {}
        other => panic!("expected RealCallsSubsetCheckFailure, got: {other:?}"),
    }
}
