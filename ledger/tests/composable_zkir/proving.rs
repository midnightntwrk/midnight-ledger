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

use std::collections::HashMap as StdHashMap;
use std::sync::Arc;

use base_crypto::rng::SplittableRng;
use base_crypto::time::Timestamp;
use midnight_ledger::structure::Transaction;
use midnight_ledger::test_utilities::{PUBLIC_PARAMS, test_intents};
use rand::{CryptoRng, Rng};
use serialize::tagged_serialize;
use transient_crypto::curve::Fr;
use transient_crypto::proofs::{
    KeyLocation, ParamsProver, ParamsProverProvider, Proof, ProofPreimage as ProofPreimageStruct,
    ProvingKeyMaterial, ProvingProvider, Resolver as ResolverTrait, Zkir,
};

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
    async fn resolve_key(&self, key: KeyLocation) -> std::io::Result<Option<ProvingKeyMaterial>> {
        Ok(self
            .entries
            .get(key.0.as_ref())
            .map(|e| ProvingKeyMaterial {
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
        let ir: IrSource = serialize::tagged_deserialize(&mut &proving_data.ir_source[..])?;

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
    // ── Step 1: Generate real verifier keys via on-the-fly keygen ──
    let inner_ir = build_inner_add_state_ir();
    let outer_ir = build_outer_call_add_ir();

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

    // ── Step 2: Run the shared deploy → execute → partition → prototype ──
    let mut p = add_state_pipeline(
        0x7001,
        Fr::from(100u64),
        Fr::from(17u64),
        inner_vk,
        outer_vk,
    )
    .await;

    // ── Step 3: Build the transaction with REAL PROOFS ──
    let pre_tx = Transaction::from_intents(
        "local-test",
        test_intents(
            &mut p.rng,
            vec![p.call_inner_proto, p.call_outer_proto],
            Vec::new(),
            Vec::new(),
            Timestamp::from_secs(0),
        ),
    );

    let provider = V3ProvingProvider {
        rng: p.rng.split(),
        resolver: &*zkir_resolver,
        params: &KzgParams,
    };
    let tx = pre_tx
        .prove(provider, &INITIAL_COST_MODEL)
        .await
        .expect("composable tx proving should succeed");

    // ── Step 4: well_formed + apply ──
    tx.well_formed(&p.state.ledger, p.strictness, p.state.time)
        .expect("well_formed should pass with real proofs");

    p.state.assert_apply(&tx, p.strictness);
}
