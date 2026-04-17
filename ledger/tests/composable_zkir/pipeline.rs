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

//! Shared pipeline helper for composable ZKIR rejection tests.
//!
//! Deploys inner + outer contracts, executes the outer (which calls inner),
//! flattens + partitions, and returns everything needed to build a composable
//! transaction. Used by the rejection tests to get a valid baseline that
//! they then tamper with.

use std::collections::HashMap as StdHashMap;
use std::sync::Arc;

use base_crypto::time::Timestamp;
use midnight_ledger::construct::{TranscriptPair, partition_transcripts};
use midnight_ledger::structure::{ContractDeploy, INITIAL_PARAMETERS, Transaction};
use midnight_ledger::test_utilities::{TestState, test_intents};
use midnight_ledger::verify::WellFormedStrictness;
use onchain_runtime::state::{ChargedState, ContractOperation, ContractState, StateValue};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use storage::db::InMemoryDB;
use storage::storage::HashMap;
use transient_crypto::curve::Fr;

use midnight_zkir_v3::ir_execute::{ExecutionContext, ExecutionResult, PreTranscriptData};
use onchain_runtime::cost_model::INITIAL_COST_MODEL;

use crate::common::*;

type D = InMemoryDB;

pub struct PipelineResult {
    pub state: TestState<D>,
    pub strictness: WellFormedStrictness,
    pub inner_addr: coin_structure::contract::ContractAddress,
    pub outer_addr: coin_structure::contract::ContractAddress,
    pub inner_op: ContractOperation,
    pub outer_op: ContractOperation,
    pub result: ExecutionResult<D>,
    pub pairs: Vec<TranscriptPair<D>>,
    pub stored_value: Fr,
    pub addr_hi: Fr,
    pub addr_lo: Fr,
    pub rng: StdRng,
}

pub async fn run_pipeline(seed: u64) -> PipelineResult {
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
        zkir_provider: Arc::new(provider),
        witness_provider: None,
        call_depth: 0,
        max_call_depth: 8,
        cost_model: INITIAL_COST_MODEL,
    };

    let (addr_hi, addr_lo) = addr_to_frs(inner_addr);
    let result: ExecutionResult<D> = outer_ir
        .execute(vec![addr_hi, addr_lo], context, &mut rng)
        .expect("execute should succeed");

    // Flatten + partition
    let flat: Vec<PreTranscriptData<D>> = result.pre_transcripts.clone();
    let pre_transcripts: Vec<PreTranscript<D>> = flat.into_iter().map(to_pre_transcript).collect();
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
