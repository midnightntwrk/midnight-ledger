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

//! Full end-to-end pipeline test: deploy → execute → flatten → partition →
//! prototype → transaction → well_formed → apply.

use std::borrow::Cow;
use std::collections::HashMap as StdHashMap;
use std::ops::Deref;
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
use storage::storage::HashMap;
use transient_crypto::curve::Fr;
use transient_crypto::fab::AlignedValueExt;
use transient_crypto::hash::transient_commit;
use transient_crypto::proofs::{KeyLocation, VerifierKey};

use midnight_zkir_v3::ir_execute::{ExecutionContext, ExecutionResult, PreTranscriptData};
use onchain_runtime::cost_model::INITIAL_COST_MODEL;

use crate::common::*;

/// Phase 4b + Phase 5: Full end-to-end pipeline from ZKIR execution through
/// transaction construction, proving, well-formedness checking, and state
/// application.
///
/// Pipeline:
///   1. Deploy inner and outer contracts to the ledger.
///   2. Execute the outer contract (which recursively calls inner) using
///      the deployed addresses.
///   3. `pre_transcripts` → Vec<PreTranscriptData>
///   4. Convert to Vec<PreTranscript>
///   5. `partition_transcripts` → Vec<TranscriptPair>
///   6. Build ContractCallPrototypes with deployed addresses.
///   7. Build composable Transaction → erase_proofs → well_formed → apply.
#[tokio::test]
async fn test_execute_flatten_partition_pipeline() {
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
        zkir_provider: Arc::new(provider),
        witness_provider: None,
        call_depth: 0,
        max_call_depth: 8,
        cost_model: INITIAL_COST_MODEL,
    };

    let (addr_hi, addr_lo) = addr_to_frs(inner_addr);
    let input_fields = vec![addr_hi, addr_lo];
    let result: ExecutionResult<D> = outer_ir
        .execute(input_fields, context, &mut rng)
        .expect("execute should succeed");

    // Verify execution output
    assert_eq!(result.outputs.len(), 1);
    assert_eq!(
        result.outputs[0], stored_value,
        "outer should relay inner's value"
    );
    assert_eq!(result.sub_calls.len(), 1, "one sub-call expected");

    // ── Step 3: pre_transcripts ──
    let flat: Vec<PreTranscriptData<D>> = result.pre_transcripts.clone();
    assert_eq!(
        flat.len(),
        2,
        "flattened should have two entries (outer + inner)"
    );
    assert!(
        flat[0].comm_comm.is_none(),
        "root caller should have no comm_comm"
    );
    assert!(
        flat[1].comm_comm.is_some(),
        "callee should have a comm_comm"
    );

    // ── Step 4: Convert to PreTranscript ──
    let pre_transcripts: Vec<PreTranscript<D>> = flat.into_iter().map(to_pre_transcript).collect();

    // ── Step 5: partition_transcripts ──
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

    // ── Step 6: Build ContractCallPrototype objects ──
    let sub = &result.sub_calls[0];
    let call_inner_proto = ContractCallPrototype {
        address: inner_addr,
        entry_point: sub.entry_point.clone(),
        op: inner_op.clone(),
        guaranteed_public_transcript: pairs[1].0.clone(),
        fallible_public_transcript: pairs[1].1.clone(),
        private_transcript_outputs: vec![],
        input: sub.input.clone(),
        output: sub.output.clone(),
        communication_commitment_rand: sub.communication_commitment_rand,
        key_location: KeyLocation(Cow::Borrowed("get")),
    };

    let call_outer_proto = ContractCallPrototype {
        address: outer_addr,
        entry_point: EntryPointBuf(b"call_inner".to_vec()),
        op: outer_op.clone(),
        guaranteed_public_transcript: pairs[0].0.clone(),
        fallible_public_transcript: pairs[0].1.clone(),
        private_transcript_outputs: vec![
            sub.output.clone(),
            sub.communication_commitment_rand.into(),
        ],
        input: fields_aligned_value(&[addr_hi, addr_lo]),
        output: field_aligned_value(stored_value),
        communication_commitment_rand: rng.r#gen(),
        key_location: KeyLocation(Cow::Borrowed("call_inner")),
    };

    // Verify communication commitment (using value_only_field_repr, matching
    // how ledger's add_call and execution code compute comm_comm).
    let expected_inner_cc = {
        let mut io_repr = Vec::new();
        sub.input.value_only_field_repr(&mut io_repr);
        sub.output.value_only_field_repr(&mut io_repr);
        transient_commit(&io_repr, sub.communication_commitment_rand)
    };
    assert_eq!(
        callee_comm_comm, expected_inner_cc,
        "callee comm_comm should match recomputed transient_commit"
    );

    // ── Step 7: Build composable transaction → well_formed → apply ──
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
