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

//! End-to-end proving test for ZKIR-based cross-contract composable calls.
//!
//! This test exercises the complete pipeline including real ZK proof generation:
//!   deploy → execute → partition → construct_proof → tx_prove → well_formed → apply
//!
//! Unlike the other composable_zkir tests (which call `.erase_proofs()`), this
//! test runs the actual proving path: IrSource::keygen for key generation,
//! IrSource::preprocess + IrSource::prove for circuit synthesis, and
//! VerifierKey::verify for self-verification.
//!
//! Requires: `cargo test --features proving` and the `MIDNIGHT_PP` env var
//! pointing to KZG parameter files.

use std::borrow::Cow;
use std::collections::HashMap as StdHashMap;
use std::sync::Arc;

use base_crypto::rng::SplittableRng;
use base_crypto::time::Timestamp;
use midnight_ledger::construct::{
    ContractCallPrototype, partition_transcripts,
};
use midnight_ledger::structure::{ContractDeploy, INITIAL_PARAMETERS, Transaction};
use midnight_ledger::test_utilities::{TestState, test_intents, PUBLIC_PARAMS};
use midnight_ledger::verify::WellFormedStrictness;
use onchain_runtime::state::{
    ChargedState, ContractOperation, ContractState, EntryPointBuf, StateValue,
};
use rand::rngs::StdRng;
use rand::{CryptoRng, Rng, SeedableRng};
use serialize::tagged_serialize;
use storage::storage::HashMap;
use transient_crypto::curve::Fr;
use transient_crypto::fab::AlignedValueExt;
use transient_crypto::proofs::{
    KeyLocation, ParamsProver, ParamsProverProvider, Proof, ProofPreimage as ProofPreimageStruct,
    ProvingKeyMaterial, ProvingProvider, Resolver as ResolverTrait, Zkir,
};

use midnight_zkir_v3::ir_execute::{ExecutionContext, ExecutionResult};
use midnight_zkir_v3::IrSource;
use onchain_runtime::cost_model::INITIAL_COST_MODEL;

use crate::common::*;

// ─── On-the-fly key generation resolver ───────────────────────────────────
//
// Instead of reading pre-computed keys from disk (like composable.rs does),
// we generate keys on-the-fly from the IrSource. This is slower but
// self-contained — no external key files needed.

struct ZkirKeyEntry {
    prover_key: Vec<u8>,
    verifier_key: Vec<u8>,
    ir_source: Vec<u8>,
}

/// A resolver that serves pre-keygen'd ZKIR proving materials by KeyLocation name.
struct ZkirResolver {
    entries: StdHashMap<String, ZkirKeyEntry>,
}

impl ZkirResolver {
    fn new() -> Self {
        ZkirResolver {
            entries: StdHashMap::new(),
        }
    }

    /// Generate keys for an IrSource and register them under the given name.
    async fn register(&mut self, name: &str, ir: &IrSource) {
        let (pk, vk) = ir.keygen(&KzgParams).await.expect("keygen should succeed");
        let mut pk_bytes = Vec::new();
        tagged_serialize(&pk, &mut pk_bytes).expect("pk serialization failed");
        let mut vk_bytes = Vec::new();
        tagged_serialize(&vk, &mut vk_bytes).expect("vk serialization failed");
        let mut ir_bytes = Vec::new();
        tagged_serialize(ir, &mut ir_bytes).expect("ir serialization failed");
        self.entries.insert(
            name.to_string(),
            ZkirKeyEntry {
                prover_key: pk_bytes,
                verifier_key: vk_bytes,
                ir_source: ir_bytes,
            },
        );
    }

    /// Get the VerifierKey for a given name (deserialized from the stored bytes).
    fn verifier_key(&self, name: &str) -> Option<VerifierKey> {
        let entry = self.entries.get(name)?;
        serialize::tagged_deserialize(&mut &entry.verifier_key[..]).ok()
    }
}

impl ResolverTrait for ZkirResolver {
    async fn resolve_key(
        &self,
        key: KeyLocation,
    ) -> std::io::Result<Option<ProvingKeyMaterial>> {
        Ok(self.entries.get(key.0.as_ref()).map(|e| ProvingKeyMaterial {
            prover_key: e.prover_key.clone(),
            verifier_key: e.verifier_key.clone(),
            ir_source: e.ir_source.clone(),
        }))
    }
}

/// KZG parameter provider — delegates to the shared PUBLIC_PARAMS resolver.
struct KzgParams;

impl ParamsProverProvider for KzgParams {
    async fn get_params(&self, k: u8) -> std::io::Result<ParamsProver> {
        PUBLIC_PARAMS.get_params(k).await
    }
}

// ─── V3-aware LocalProvingProvider ────────────────────────────────────────
//
// The stock `LocalProvingProvider` (from `zkir` crate) is hardwired to v2
// ZKIR. We need one that deserializes v3 IrSource so that our ZKIR v3
// circuits can be proved.

struct V3ProvingProvider<'a, R: Rng + CryptoRng + SplittableRng> {
    rng: R,
    resolver: &'a ZkirResolver,
    params: &'a KzgParams,
}

impl<'a, R: Rng + CryptoRng + SplittableRng> ProvingProvider for V3ProvingProvider<'a, R> {
    async fn check(
        &self,
        preimage: &ProofPreimageStruct,
    ) -> Result<Vec<Option<usize>>, anyhow::Error> {
        let proving_data = self
            .resolver
            .resolve_key(preimage.key_location.clone())
            .await?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "attempted to check proof for '{}' without circuit data!",
                    preimage.key_location.0
                )
            })?;
        let ir: IrSource =
            serialize::tagged_deserialize(&mut &proving_data.ir_source[..])?;

        preimage.check(&ir)
    }

    async fn prove(
        self,
        preimage: &ProofPreimageStruct,
        overwrite_binding_input: Option<Fr>,
    ) -> Result<Proof, anyhow::Error> {
        let mut preimage = preimage.clone();
        if let Some(binding_input) = overwrite_binding_input {
            preimage.binding_input = binding_input;
        }
        Ok(preimage
            .prove::<IrSource>(self.rng, self.params, self.resolver)
            .await?
            .0)
    }

    fn split(&mut self) -> Self {
        Self {
            rng: self.rng.split(),
            resolver: self.resolver,
            params: self.params,
        }
    }
}

// ─── Test ─────────────────────────────────────────────────────────────────

/// End-to-end proving test for ZKIR composable cross-contract calls.
///
/// Inner (B): "add_state" — reads stored_val from state, takes input,
///            returns input + stored_val. Has Impact block → non-empty transcript.
/// Outer (A): "call_add" — takes B's address + value, calls B.add_state(value),
///            returns call_result + value. Has ContractCall → claim ops in transcript.
///
/// For input = 17, stored = 100:
///   inner returns 17 + 100 = 117
///   outer returns 117 + 17 = 134
///
/// Pipeline: keygen → deploy → execute → partition → construct_proof →
///           tx_prove (real proofs!) → well_formed → apply.
#[tokio::test]
async fn test_proving_composable_zkir_call() {
    let mut rng = StdRng::seed_from_u64(0x7001);

    let stored_val = Fr::from(100u64);
    let input_val = Fr::from(17u64);
    let expected_inner_result = Fr::from(117u64); // 17 + 100
    let expected_outer_result = Fr::from(134u64); // 117 + 17

    let inner_ir = build_inner_add_state_ir();
    let outer_ir = build_outer_call_add_ir();

    // ── Step 1: Key generation ──
    // Generate proving/verification keys on-the-fly from each IrSource.
    let mut zkir_resolver = ZkirResolver::new();
    zkir_resolver.register("add_state", &inner_ir).await;
    zkir_resolver.register("call_add", &outer_ir).await;

    let inner_vk = zkir_resolver
        .verifier_key("add_state")
        .expect("inner VK should exist");
    let outer_vk = zkir_resolver
        .verifier_key("call_add")
        .expect("outer VK should exist");

    let zkir_resolver = Arc::new(zkir_resolver);

    // ── Step 2: Build ContractOperations with real VKs + serialized ZKIR ──
    let inner_op =
        ContractOperation::new_with_zkir(Some(inner_vk), serialize_ir(&inner_ir));
    let outer_op =
        ContractOperation::new_with_zkir(Some(outer_vk), serialize_ir(&outer_ir));

    let inner_state: ChargedState<D> = make_cell_state(field_aligned_value(stored_val));
    let outer_state: ChargedState<D> = ChargedState::new(StateValue::Null);

    // ── Step 3: Deploy contracts ──
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
    // Deploy transactions have no contract calls, so erase_proofs is fine.
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

    // ── Step 4: Execute (interpreter) ──
    let mut provider = TestZkirProvider::<D>::new();
    provider.register(
        inner_addr,
        StdHashMap::from([("add_state".to_string(), inner_ir.clone())]),
        inner_op.clone(),
        inner_state.clone(),
    );
    provider.register(
        outer_addr,
        StdHashMap::from([("call_add".to_string(), outer_ir.clone())]),
        outer_op.clone(),
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

    // Verify execution output
    assert_eq!(
        result.sub_calls[0].execution_result.outputs[0], expected_inner_result,
        "inner should return input + stored_val"
    );
    assert_eq!(
        result.outputs[0], expected_outer_result,
        "outer should return (input + stored_val) + input"
    );

    // ── Step 5: Partition ──
    let flat: Vec<PreTranscriptData<D>> = result.pre_transcripts.clone();
    let pre_transcripts: Vec<PreTranscript<D>> = flat.into_iter().map(to_pre_transcript).collect();
    let pairs = partition_transcripts(&pre_transcripts, &INITIAL_PARAMETERS)
        .expect("partition should succeed");
    assert_eq!(pairs.len(), 2);

    // ── Step 6: Build ContractCallPrototypes ──
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
        // The ContractCall instruction consumes callee outputs and comm_rand
        // from private_transcript, matching the Compact pattern where
        // tmpDoCall() and tmpCallRand() provide them as private witnesses.
        private_transcript_outputs: vec![
            sub.output.clone(),
            sub.communication_commitment_rand.into(),
        ],
        input: fields_aligned_value(&[addr_hi, addr_lo, input_val]),
        output: field_aligned_value(expected_outer_result),
        communication_commitment_rand: rng.r#gen(),
        key_location: KeyLocation(Cow::Borrowed("call_add")),
    };

    // ── Step 7: Build transaction with REAL PROOFS ──
    let pre_tx = Transaction::from_intents(
        "local-test",
        test_intents(
            &mut rng,
            vec![call_inner_proto, call_outer_proto],
            Vec::new(),
            Vec::new(),
            Timestamp::from_secs(0),
        ),
    );

    let provider = V3ProvingProvider {
        rng: rng.split(),
        resolver: &*zkir_resolver,
        params: &KzgParams,
    };
    let tx = pre_tx
        .prove(provider, &INITIAL_COST_MODEL)
        .await
        .expect("composable tx proving should succeed");

    // ── Step 8: well_formed + apply ──
    tx.well_formed(&state.ledger, strictness, state.time)
        .expect("well_formed should pass with real proofs");

    state.assert_apply(&tx, strictness);
}
