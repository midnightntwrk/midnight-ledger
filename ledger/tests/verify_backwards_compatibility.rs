// This file is part of midnight-ledger.
// Copyright (C) Midnight Foundation
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

//! Tests that proofs generated with the old (pinned) proving pipeline are
//! accepted by the ledger's proof verification path when the contract
//! operation only has a `v2` verifier key (no `v3`).

#![cfg(feature = "proof-verifying")]

use midnight_ledger_v9 as midnight_ledger;

use midnight_ledger::structure::{ContractCall, ProofKind, ProofMarker, ProofVersioned};
use midnight_ledger::verify::ProofVerificationMode;
use onchain_runtime::state::ContractOperation;
use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;
use std::borrow::Cow;
use std::fs::File;
use std::io::BufReader;
use storage::db::InMemoryDB;

type OldIrSource = zkir_old::IrSource;
type OldProverKey = transient_crypto_old::proofs::ProverKey<OldIrSource>;

struct OldTestParams;

impl transient_crypto_old::proofs::ParamsProverProvider for OldTestParams {
    async fn get_params(
        &self,
        k: u8,
    ) -> std::io::Result<transient_crypto_old::proofs::ParamsProver> {
        const DIR: &str = env!("MIDNIGHT_PP");
        transient_crypto_old::proofs::ParamsProver::read(BufReader::new(File::open(
            format!("{DIR}/bls_midnight_2p{k}"),
        )?))
    }
}

struct OldTestResolver {
    pk: OldProverKey,
    vk: transient_crypto_old::proofs::VerifierKey,
    ir: OldIrSource,
}

impl transient_crypto_old::proofs::Resolver for OldTestResolver {
    async fn resolve_key(
        &self,
        _key: transient_crypto_old::proofs::KeyLocation,
    ) -> std::io::Result<Option<transient_crypto_old::proofs::ProvingKeyMaterial>> {
        let mut pk = Vec::new();
        serialize_old::tagged_serialize(&self.pk, &mut pk)?;
        let mut vk = Vec::new();
        serialize_old::tagged_serialize(&self.vk, &mut vk)?;
        let mut ir = Vec::new();
        serialize_old::tagged_serialize(&self.ir, &mut ir)?;
        Ok(Some(transient_crypto_old::proofs::ProvingKeyMaterial {
            prover_key: pk,
            verifier_key: vk,
            ir_source: ir,
        }))
    }
}

/// Generates a proof with the old zkir pipeline and verifies it through the
/// ledger's `proof_verify`, which should delegate to the backwards-compatible
/// verification path because `ContractOperation.v3` is `None`.
#[actix_rt::test]
async fn old_proof_accepted_by_ledger_v2_op() {
    let ir_raw = r#"{
       "version": { "major": 2, "minor": 0 },
       "num_inputs": 1,
       "do_communications_commitment": false,
       "instructions": [
           { "op": "assert", "cond": 0 }
       ]
    }"#;
    let old_ir = OldIrSource::load(ir_raw.as_bytes()).unwrap();

    // ── keygen + prove with old zkir ─────────────────────────────────
    let (old_pk, old_vk) = {
        use transient_crypto_old::proofs::Zkir;
        old_ir.keygen(&OldTestParams).await.unwrap()
    };

    let old_preimage = transient_crypto_old::proofs::ProofPreimage {
        binding_input: 42.into(),
        communications_commitment: None,
        inputs: vec![1.into()],
        private_transcript: vec![],
        public_transcript_inputs: vec![],
        public_transcript_outputs: vec![],
        key_location: transient_crypto_old::proofs::KeyLocation(Cow::Borrowed("builtin")),
    };

    let (old_proof, _) = old_preimage
        .prove::<OldIrSource>(
            &mut ChaCha20Rng::from_seed([42; 32]),
            &OldTestParams,
            &OldTestResolver {
                pk: old_pk,
                vk: old_vk.clone(),
                ir: old_ir,
            },
        )
        .await
        .unwrap();

    // ── convert old VK to new type ───────────────────────────────────
    let mut old_vk_bytes = Vec::new();
    serialize_old::Serializable::serialize(&old_vk, &mut old_vk_bytes).unwrap();
    let new_vk: transient_crypto::proofs::VerifierKey =
        serialize::Deserializable::deserialize(&mut &old_vk_bytes[..], 0)
            .expect("deserialize old VK bytes into new VerifierKey");

    // ── build a v2-only ContractOperation (no v3 → backwards-compatible path)
    let op = ContractOperation::new(Some(new_vk), None);
    assert!(op.v3.is_none(), "v3 should be None for this test");

    // ── wrap proof in new types and verify via the ledger ────────────
    let new_proof = transient_crypto::proofs::Proof(old_proof.0);
    let versioned_proof = ProofVersioned::from(new_proof);
    let pis = vec![transient_crypto::curve::Fr::from(42u64)];

    let dummy_call: ContractCall<ProofMarker, InMemoryDB> = ContractCall {
        address: Default::default(),
        entry_point: b"test"[..].into(),
        guaranteed_transcript: None,
        fallible_transcript: None,
        communication_commitment: 0.into(),
        proof: versioned_proof.clone(),
    };

    <ProofMarker as ProofKind<InMemoryDB>>::proof_verify(
        &op,
        &versioned_proof,
        pis,
        &dummy_call,
        ProofVerificationMode::Real,
    )
    .expect("old proof should verify through ledger v2 path");
}
