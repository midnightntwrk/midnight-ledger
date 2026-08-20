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

//! The published accumulator sits at its recorded offset, and still does after
//! the verifying key has been through the wire.
//!
//! Pins the layout: slot 0 is the binding input, the accumulator follows,
//! nothing else. The offsets read here are the IR's —
//! `VerifierKey::accumulator_offsets` is private to `transient-crypto` — and
//! that the VK's own copy agrees is what the two verifications show, since each
//! extracts at the VK's offsets to pair.
//!
//! Read the off-circuit comparison for less than its name suggests: `preprocess`
//! *builds* the public inputs with `verify_proof_offcircuit`, and the assertion
//! recomputes with the same arguments, so it compares a function to itself. What
//! this establishes is the layout; in-circuit/off-circuit agreement comes from
//! `prove` succeeding, since `verify_proof_incircuit` is separate code
//! constrained to match those inputs, and the pairing from `verify`.
//!
//! The second verification uses a *reloaded* key — in the ledger the key is
//! serialized and parsed back before anyone verifies with it. `VerifierKey`'s
//! byte-level properties belong to `transient-crypto`, which can exercise them
//! with no circuit or SRS; only the end of the chain is checked here. Those
//! delegated properties are currently tested **nowhere**: that crate's
//! `proofs.rs` has no `mod tests`.

use midnight_zkir_v3::ir_instructions::verify_proof::{
    accumulator_pi_len, verify_proof_offcircuit,
};
use serialize::{Deserializable, Serializable};
use transient_crypto::curve::Fr;
use transient_crypto::proofs::VerifierKey;

use crate::e2e_harness::{
    BINDING_INPUT, outer_preimage, outer_prove, outer_verify, pinned_fixture, test_rng,
};

#[actix_rt::test]
#[ignore = "outer verifier circuit needs a high-k SRS not available in CI"]
async fn accumulator_at_recorded_offset_matches_offcircuit() {
    let mut rng = test_rng();

    let fixture = pinned_fixture().await;
    let inner_proof = fixture.correct_proof(&mut rng);
    let inner_pis = fixture.inner_pis();

    // One `verify_proof`, so one accumulator, one slot past the binding input.
    let acc_len = accumulator_pi_len();
    let offsets = fixture.ir.accumulator_offsets();
    assert_eq!(
        offsets,
        vec![1],
        "a single verify_proof puts its accumulator right after the binding input"
    );
    let offset = offsets[0];

    let preimage = outer_preimage(inner_proof.clone());
    let (outer_proof, outer_pis) =
        outer_prove(&fixture.ir, fixture.pk.clone(), &preimage, &mut rng).await;

    // The statement vector is exactly [binding input, accumulator].
    assert_eq!(
        outer_pis.len(),
        1 + acc_len,
        "statement vector should be the binding input followed by one accumulator"
    );
    assert_eq!(
        outer_pis[0],
        Fr::from(BINDING_INPUT),
        "slot 0 is the binding input, so the accumulator offset is not off by one"
    );

    // The accumulator region holds what off-circuit preparation independently
    // computes for this (vk, instance, proof).
    let expected: Vec<Fr> = verify_proof_offcircuit(&fixture.vk_blob, &inner_pis, &inner_proof)
        .expect("off-circuit preparation")
        .into_iter()
        .map(Fr)
        .collect();
    assert_eq!(
        expected.len(),
        acc_len,
        "off-circuit accumulator should be acc_len field elements"
    );
    assert_eq!(
        &outer_pis[offset..offset + acc_len],
        expected.as_slice(),
        "the accumulator at the recorded offset must match off-circuit preparation"
    );

    // The verifier, reading its own recorded offsets out of the VerifierKey,
    // agrees: it finds the accumulator there and its pairing check passes.
    outer_verify(&fixture.vk, &outer_proof, outer_pis.clone());

    // And so does a key that has been through the wire. Had the offsets been
    // lost the accumulator would not be found; had they shifted, it would not
    // pair.
    let mut bytes = Vec::new();
    Serializable::serialize(&fixture.vk, &mut bytes).expect("serialize vk");
    let reloaded: VerifierKey =
        Deserializable::deserialize(&mut &bytes[..], 0).expect("deserialize vk");
    outer_verify(&reloaded, &outer_proof, outer_pis);
}
