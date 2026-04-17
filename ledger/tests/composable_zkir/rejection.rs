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

//! Tier 3: Rejection tests for composable transactions.
//!
//! These tests verify that `well_formed` correctly rejects composable
//! transactions with deliberate defects. Each test uses `run_pipeline` to
//! get a valid execution result, then tampers with one aspect of the
//! `ContractCallPrototype` before building the transaction.

use std::borrow::Cow;

use base_crypto::time::Timestamp;
use midnight_ledger::construct::ContractCallPrototype;
use midnight_ledger::error::{EffectsCheckError, MalformedTransaction};
use midnight_ledger::structure::Transaction;
use midnight_ledger::test_utilities::test_intents;
use onchain_runtime::state::EntryPointBuf;
use rand::Rng;
use transient_crypto::curve::Fr;
use transient_crypto::proofs::KeyLocation;

use crate::common::*;
use crate::pipeline::{PipelineResult, run_pipeline};

/// Build the outer ContractCallPrototype shared by all rejection tests.
///
/// Each rejection test tampers with the *inner* prototype (or omits it); the
/// outer prototype is always constructed the same way.
fn build_outer_proto(p: &mut PipelineResult) -> ContractCallPrototype<D> {
    let sub = &p.result.sub_calls[0];
    ContractCallPrototype {
        address: p.outer_addr,
        entry_point: EntryPointBuf(b"call_inner".to_vec()),
        op: p.outer_op.clone(),
        guaranteed_public_transcript: p.pairs[0].0.clone(),
        fallible_public_transcript: p.pairs[0].1.clone(),
        private_transcript_outputs: vec![
            sub.output.clone(),
            sub.communication_commitment_rand.into(),
        ],
        input: fields_aligned_value(&[p.addr_hi, p.addr_lo]),
        output: field_aligned_value(p.stored_value),
        communication_commitment_rand: p.rng.r#gen(),
        key_location: KeyLocation(Cow::Borrowed("call_inner")),
    }
}

/// Rejection test: omit the callee's ContractCallPrototype from the intent.
///
/// The outer caller's transcript contains a `claimed_contract_calls` entry
/// for the inner call, but the inner call isn't present in the transaction's
/// intent. `well_formed` should reject because the claimed call (addr,
/// ep_hash, comm_comm) has no matching real call in the intent.
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

    let err = p.state
        .apply(&tx, p.strictness)
        .expect_err("should reject: caller claims a sub-call that has no matching real call");

    match err {
        MalformedTransaction::EffectsCheckFailure(
            EffectsCheckError::RealCallsSubsetCheckFailure(_),
        ) => {}
        other => panic!("expected RealCallsSubsetCheckFailure, got: {other:?}"),
    }
}

/// Rejection test: tamper with the callee's communication commitment randomness.
///
/// The inner callee's `ContractCallPrototype` uses a wrong `communication_commitment_rand`,
/// so its recomputed comm_comm won't match the one in the caller's
/// `claimed_contract_calls`. `well_formed` should reject with
/// `RealCallsSubsetCheckFailure`.
#[tokio::test]
async fn test_reject_comm_comm_mismatch() {
    let mut p = run_pipeline(0x5002).await;

    let sub = &p.result.sub_calls[0];

    // Build inner prototype with WRONG communication_commitment_rand.
    let tampered_rand: Fr = p.rng.r#gen(); // different from sub.communication_commitment_rand
    let call_inner_proto = ContractCallPrototype {
        address: p.inner_addr,
        entry_point: sub.entry_point.clone(),
        op: p.inner_op.clone(),
        guaranteed_public_transcript: p.pairs[1].0.clone(),
        fallible_public_transcript: p.pairs[1].1.clone(),
        private_transcript_outputs: vec![],
        input: sub.input.clone(),
        output: sub.output.clone(),
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

    let err = p.state
        .apply(&tx, p.strictness)
        .expect_err("should reject: tampered comm_comm_rand produces mismatched comm_comm");

    match err {
        MalformedTransaction::EffectsCheckFailure(
            EffectsCheckError::RealCallsSubsetCheckFailure(_),
        ) => {}
        other => panic!("expected RealCallsSubsetCheckFailure, got: {other:?}"),
    }
}

/// Rejection test: use wrong entry point on the callee's ContractCallPrototype.
///
/// The caller claims a call to entry point "get", but the callee prototype
/// specifies "put" instead. The ep_hash won't match, so `well_formed`
/// should reject with `RealCallsSubsetCheckFailure`.
#[tokio::test]
async fn test_reject_wrong_entry_point() {
    let mut p = run_pipeline(0x5003).await;

    let sub = &p.result.sub_calls[0];

    // Build inner prototype with WRONG entry point.
    let call_inner_proto = ContractCallPrototype {
        address: p.inner_addr,
        entry_point: EntryPointBuf(b"put".to_vec()), // WRONG — should be "get"
        op: p.inner_op.clone(),
        guaranteed_public_transcript: p.pairs[1].0.clone(),
        fallible_public_transcript: p.pairs[1].1.clone(),
        private_transcript_outputs: vec![],
        input: sub.input.clone(),
        output: sub.output.clone(),
        communication_commitment_rand: sub.communication_commitment_rand,
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

    let err = p.state
        .apply(&tx, p.strictness)
        .expect_err("should reject: wrong entry point produces mismatched ep_hash");

    match err {
        MalformedTransaction::EffectsCheckFailure(
            EffectsCheckError::RealCallsSubsetCheckFailure(_),
        ) => {}
        other => panic!("expected RealCallsSubsetCheckFailure, got: {other:?}"),
    }
}
