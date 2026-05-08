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

//! Shared scaffolding for the integration tests.
//!
//! Cargo compiles each `tests/*.rs` file as its own crate root and pulls
//! in `tests/common/mod.rs` (via `mod common;` in each test file) into
//! every such crate. `proofs.rs` and `typed_outputs.rs` use overlapping
//! but non-identical subsets of the helpers below, so from the
//! perspective of any single test crate, some of these items are
//! unused. The repository builds with `-D warnings`, which turns those
//! "unused" warnings into hard `dead_code` errors.
//!
//! This is the canonical Rust pitfall for shared integration-test
//! submodules. The standard fix is to allow dead code at the module
//! level; the lint is a per-crate view, but the helpers really are used
//! across the test suite as a whole.
#![allow(dead_code)]

use std::borrow::Cow;
use std::fs::File;
use std::io::BufReader;

use midnight_zkir_v3::IrSource;
use midnight_zkir_v3::ir_instructions::encode::encode_offcircuit_for_commit;
use midnight_zkir_v3::ir_types::IrValue;
use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;
use serialize::tagged_serialize;
use transient_crypto::curve::Fr;
use transient_crypto::hash::transient_commit;
use transient_crypto::proofs::{
    KeyLocation, PARAMS_VERIFIER, ParamsProver, ParamsProverProvider, ProofPreimage,
    ProvingKeyMaterial, Resolver, VerifierKey, Zkir,
};

type ProverKey = transient_crypto::proofs::ProverKey<IrSource>;

/// Canonical binding input used by every typed-output test in this module.
/// This is the first of two public inputs the verifier checks against; the
/// second is the communications commitment, which is required because every
/// round-trip test sets `do_communications_commitment = true` so that the
/// `encode_offcircuit` / `encode_incircuit` paths actually run.
const BINDING: u64 = 42;

/// Deterministic randomness used to blind every commitment in the round-trip
/// tests. The value is arbitrary and only needs to match between the
/// commitment we hand to the prover and the commitment the prover recomputes
/// inside `IrSource::preprocess`. Production callers must of course use real
/// randomness; tests are reproducible by construction.
const COMM_RAND: u64 = 7;

pub struct TestResolver {
    pub pk: ProverKey,
    pub vk: VerifierKey,
    pub ir: IrSource,
}

impl Resolver for TestResolver {
    async fn resolve_key(
        &self,
        _: KeyLocation,
    ) -> std::io::Result<Option<ProvingKeyMaterial>> {
        let mut pk = Vec::new();
        tagged_serialize(&self.pk, &mut pk)?;
        let mut vk = Vec::new();
        tagged_serialize(&self.vk, &mut vk)?;
        let mut ir = Vec::new();
        tagged_serialize(&self.ir, &mut ir)?;
        Ok(Some(ProvingKeyMaterial {
            prover_key: pk,
            verifier_key: vk,
            ir_source: ir,
        }))
    }
}

pub struct TestParams;

impl ParamsProverProvider for TestParams {
    async fn get_params(&self, k: u8) -> std::io::Result<ParamsProver> {
        const DIR: &str = env!("MIDNIGHT_PP");
        ParamsProver::read(BufReader::new(File::open(format!("{DIR}/bls_midnight_2p{k}"))?))
    }
}

/// Build a `ProofPreimage` with the canonical empty transcripts and the given
/// `inputs` and `communications_commitment`. Callers that don't need a
/// commitment (e.g. negative-conformance tests) pass `None`.
fn make_preimage(
    inputs: Vec<Fr>,
    communications_commitment: Option<(Fr, Fr)>,
) -> ProofPreimage {
    ProofPreimage {
        binding_input: BINDING.into(),
        communications_commitment,
        inputs,
        private_transcript: vec![],
        public_transcript_inputs: vec![],
        public_transcript_outputs: vec![],
        key_location: KeyLocation(Cow::Borrowed("builtin")),
    }
}

/// Encode each typed expected output via `encode_offcircuit_for_commit` and
/// flatten into a single `Vec<Fr>`. This must exactly match what
/// `IrSource::preprocess` computes inside its commitment branch — which uses
/// `encode_offcircuit_for_commit` (the commit-bearing flavour) over the
/// declared output values.
fn encode_expected_outputs(expected: &[IrValue]) -> Vec<Fr> {
    let mut out = Vec::new();
    for v in expected {
        for ir_val in encode_offcircuit_for_commit(v) {
            // `encode_offcircuit_for_commit` always yields `IrValue::Native(_)`;
            // the `TryFrom<IrValue> for Fr` impl in `ir_types.rs` is the inverse.
            let f: Fr = ir_val
                .try_into()
                .expect("encode_offcircuit_for_commit yields Native variants");
            out.push(f);
        }
    }
    out
}

/// Positive round-trip helper. Loads the JSON IR, computes the communications
/// commitment over `inputs ++ encode(expected_outputs)` with deterministic
/// blinding, runs `keygen`, proves, then verifies the resulting proof against
/// `[binding_input, commitment]` as the public-input vector. Panics on any
/// failure along the way.
///
/// `expected_outputs` must list, in the order declared by the IR's `outputs`
/// signature, the exact typed value each output position is expected to take.
/// The helper encodes them with `encode_offcircuit`, so callers don't need to
/// know how individual `IrType`s flatten into Fr.
pub async fn assert_typed_output_roundtrip(
    ir_raw: &str,
    inputs: Vec<Fr>,
    expected_outputs: Vec<IrValue>,
) {
    let encoded_outputs = encode_expected_outputs(&expected_outputs);

    let rand: Fr = COMM_RAND.into();
    let mut comm_inputs: Vec<Fr> = Vec::with_capacity(inputs.len() + encoded_outputs.len());
    comm_inputs.extend(inputs.iter().cloned());
    comm_inputs.extend(encoded_outputs.iter().cloned());
    let comm = transient_commit(&comm_inputs, rand);

    let ir = IrSource::load(ir_raw.as_bytes()).expect("IR JSON must parse");
    let (pk, vk) = ir.keygen(&TestParams).await.expect("keygen must succeed");
    let preimage = make_preimage(inputs, Some((comm, rand)));
    let (proof, _) = preimage
        .prove::<IrSource>(
            &mut ChaCha20Rng::from_seed([42; 32]),
            &TestParams,
            &TestResolver {
                pk: pk.clone(),
                vk: vk.clone(),
                ir: ir.clone(),
            },
        )
        .await
        .expect("prove must succeed");
    vk.verify(
        &PARAMS_VERIFIER,
        &proof,
        [BINDING.into(), comm].into_iter(),
    )
    .expect("verify must succeed");
}

/// Negative-conformance helper. Loads the JSON IR, runs `preimage.check(&ir)`
/// (the off-circuit preprocess pass) with `do_communications_commitment =
/// false` semantics — i.e. no commitment in the preimage — and asserts that
/// preprocess fails with an error whose message contains every substring in
/// `substrings`. Panics if `check` unexpectedly succeeds or any substring is
/// missing.
pub fn assert_check_err_contains(ir_raw: &str, inputs: Vec<Fr>, substrings: &[&str]) {
    let ir = IrSource::load(ir_raw.as_bytes()).expect("IR JSON must parse");
    let preimage = make_preimage(inputs, None);
    let err = preimage
        .check(&ir)
        .expect_err("preprocess must reject this IR");
    let msg = err.to_string();
    for s in substrings {
        assert!(
            msg.contains(s),
            "error message {msg:?} missing expected substring {s:?}"
        );
    }
}
