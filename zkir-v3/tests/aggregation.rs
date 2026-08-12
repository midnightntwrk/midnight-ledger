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
#![cfg(feature = "proof-aggregation")]

//! End-to-end check that two independently-proven `IrSource` circuits can be
//! folded into one IVC aggregation proof via [`AggregableIrSource`] and
//! verified as a whole.
//!
//! # Status: prototype, not reviewed
//!
//! This is the first exercise of `AggregableIrSource` (see
//! `src/ir_aggregation.rs`) against real proofs. It has not been run against
//! a compiler in this environment (no local toolchain / private-crate build
//! was available while writing it) and has not been reviewed by whoever owns
//! the aggregation circuit design. Treat any failure here as equally likely
//! to be a bug in this test as in `AggregableIrSource`.
//!
//! It deliberately stops at `Verifier::verify_aggregation` and does **not**
//! attempt to round-trip through the ledger's `AggregatedContractProof` /
//! `Transaction::verify_aggregated_proofs` path. As of the
//! `midnight-aggregation` revision this workspace pins
//! (`irakoton/batch-verify`), `midnight_aggregation::ivc::IvcInstance` has no
//! public constructor or (de)serialization API outside that crate — its
//! fields are `pub(crate)` and the only public accessor is `state()`. That
//! means `AggregatedContractProof::ivc_instance` (documented as "serialized
//! `IvcInstance<ProofAggregation>` bytes") cannot actually be produced or
//! consumed by an external crate like this one yet. Wiring this into the
//! ledger needs that gap closed upstream first.
//!
//! Picking a `k` too small for a given IR will fail proving with a clear
//! error; if that happens here, raise `INNER_K`. Likewise, if the aggregator
//! params for `AGGREGATOR_K` cost more than what's cached locally,
//! `TestParams` (see `tests/common/mod.rs`) will fail to find the file
//! under `$MIDNIGHT_PP` — fetch/generate one for that `k` first.

#[path = "common/mod.rs"]
mod common;

#[cfg(test)]
mod aggregation_tests {
    use std::borrow::Cow;

    use midnight_curves::Bls12;
    use midnight_proofs::poly::kzg::params::ParamsKZG;
    use midnight_zk_stdlib::{prove as zk_prove, setup_pk, setup_vk};
    use midnight_zkir_v3::{AggregableIrSource, IrSource};
    use rand::SeedableRng;
    use rand_chacha::ChaCha20Rng;
    use transient_crypto::aggregation::{
        AggregationWitness, InnerCircuitsContext, ProofAggregation,
    };
    use transient_crypto::proofs::{KeyLocation, ParamsProverProvider, ProofPreimage, TranscriptHash};

    use super::common::TestParams;

    /// Shared circuit size for every inner circuit aggregated below. Must be
    /// large enough for the biggest of them (both are tiny here) and small
    /// enough that `$MIDNIGHT_PP/bls_midnight_2p{INNER_K}` actually exists.
    const INNER_K: u8 = 13;
    /// Circuit size for the IVC aggregator circuit itself. Larger than
    /// `INNER_K` because it embeds an in-circuit verifier for the inner
    /// proofs on top of the aggregation bookkeeping. Taken from this crate's
    /// own `multi_circuit_aggregation` example; adjust if that no longer
    /// matches what's cached under `$MIDNIGHT_PP`.
    const AGGREGATOR_K: u32 = 19;

    /// Loads a tiny circuit, proves it against [`AggregableIrSource`], and
    /// returns the `(vk, instance, proof_bytes)` triple needed to build an
    /// [`AggregationWitness`].
    async fn prove_one(
        ir_raw: &str,
        input: u64,
        inner_params: &ParamsKZG<Bls12>,
    ) -> (
        midnight_zk_stdlib::MidnightVK,
        Vec<transient_crypto::curve::outer::Scalar>,
        Vec<u8>,
    ) {
        let ir = IrSource::load(ir_raw.as_bytes()).expect("IR JSON must parse");
        let aggregable = AggregableIrSource(ir);

        let vk = setup_vk(inner_params, &aggregable);
        let pk = setup_pk(&aggregable, &vk);

        let preimage = ProofPreimage {
            binding_input: 0u64.into(),
            communications_commitment: None,
            inputs: vec![input.into()],
            private_transcript: vec![],
            public_transcript_inputs: vec![],
            public_transcript_outputs: vec![],
            key_location: KeyLocation(Cow::Borrowed("builtin")),
        };
        let preprocessed = aggregable
            .preprocess(&preimage)
            .expect("preprocess must succeed for a satisfying witness");
        let instance = preprocessed.pis.clone();

        let proof = zk_prove::<AggregableIrSource, TranscriptHash>(
            inner_params,
            &pk,
            &aggregable,
            &instance,
            preprocessed,
            &mut ChaCha20Rng::from_seed([7; 32]),
        )
        .expect("proving AggregableIrSource must succeed");

        (vk, instance, proof)
    }

    #[actix_rt::test]
    async fn aggregates_two_real_proofs() {
        // Two trivially different circuits, so this also exercises that the
        // aggregator handles distinct VKs, not just distinct instances of
        // the same one.
        let ir_a = r#"{
           "version": { "major": 3, "minor": 0 },
           "inputs": [ { "name": "%v_0", "type": "Scalar<BLS12-381>" } ],
           "outputs": [],
           "do_communications_commitment": false,
           "instructions": [ { "op": "assert", "cond": "%v_0" } ]
        }"#;
        let ir_b = r#"{
           "version": { "major": 3, "minor": 0 },
           "inputs": [ { "name": "%v_0", "type": "Scalar<BLS12-381>" } ],
           "outputs": [],
           "do_communications_commitment": false,
           "instructions": [
               { "op": "assert", "cond": "%v_0" },
               { "op": "assert", "cond": "%v_0" }
           ]
        }"#;

        let inner_params = TestParams
            .get_params(INNER_K)
            .await
            .expect("inner SRS for INNER_K must be available under $MIDNIGHT_PP")
            .0;
        let aggregator_params = TestParams
            .get_params(AGGREGATOR_K as u8)
            .await
            .expect("aggregator SRS for AGGREGATOR_K must be available under $MIDNIGHT_PP")
            .0;

        let inner_ctx = InnerCircuitsContext::new(
            AggregableIrSource::aggregation_arch(),
            INNER_K as u32,
            inner_params.verifier_params(),
        );
        let (mut aggregator, verifier) =
            ProofAggregation::setup((*aggregator_params).clone(), AGGREGATOR_K, inner_ctx);

        let (vk_a, instance_a, proof_a) = prove_one(ir_a, 1, &inner_params).await;
        let witness_a = AggregationWitness::new::<AggregableIrSource>(vk_a, instance_a, proof_a);
        aggregator
            .aggregate(witness_a)
            .expect("aggregating the first proof must succeed");

        let (vk_b, instance_b, proof_b) = prove_one(ir_b, 1, &inner_params).await;
        let witness_b = AggregationWitness::new::<AggregableIrSource>(vk_b, instance_b, proof_b);
        let final_proof = aggregator
            .aggregate(witness_b)
            .expect("aggregating the second proof must succeed");

        let instance = aggregator.instance();
        verifier
            .verify_aggregation(&instance, &final_proof)
            .expect("the aggregated proof must verify");
        assert_eq!(
            instance.state().claims().len(),
            2,
            "both aggregated claims should be reflected in the final state"
        );
    }
}
