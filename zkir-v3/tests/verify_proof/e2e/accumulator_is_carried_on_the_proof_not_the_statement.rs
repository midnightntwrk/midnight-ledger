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

//! The accumulator is carried on the proof, matches what off-circuit preparation
//! computes, and is kept out of the caller's statement.
//!
//! Pins the split: one `verify_proof` must yield exactly one block and a
//! one-element statement. The second verification uses a reloaded key, which is
//! how the ledger uses it.

use midnight_zkir_v3::ir_instructions::decider::accumulator_pis;
use midnight_zkir_v3::ir_instructions::verify_proof::verify_proof_offcircuit;
use serialize::{Deserializable, Serializable};
use transient_crypto::curve::Fr;
use transient_crypto::proofs::{VerifierKey, accumulator_pi_len};

use crate::e2e_harness::{
    BINDING_INPUT, outer_preimage, outer_prove, outer_verify, pinned_fixture, test_rng,
};

#[actix_rt::test]
#[ignore = "outer verifier circuit needs a high-k SRS not available in CI"]
async fn accumulator_is_carried_on_the_proof_not_the_statement() {
    let mut rng = test_rng();

    let fixture = pinned_fixture().await;
    let inner_proof = fixture.correct_proof(&mut rng);
    let inner_pis = fixture.inner_pis();

    // One `verify_proof`, so one accumulator block.
    let acc_len = accumulator_pi_len();
    assert_eq!(
        fixture.ir.accumulator_count(),
        1,
        "a single verify_proof exposes a single accumulator"
    );

    let preimage = outer_preimage(inner_proof.clone());
    let (outer_proof, outer_pis) =
        outer_prove(&fixture.ir, fixture.pk.clone(), &preimage, &mut rng).await;

    // The accumulator went onto the proof, not into the caller's statement.
    assert_eq!(
        outer_proof.accumulators.len(),
        1,
        "one verify_proof must carry exactly one accumulator block"
    );
    assert_eq!(
        outer_proof.accumulators[0].len(),
        acc_len,
        "a block is one accumulator wide"
    );
    assert_eq!(
        outer_pis,
        vec![Fr::from(BINDING_INPUT)],
        "the statement is the binding input alone; the accumulator is not in it"
    );

    // The block holds what off-circuit preparation independently computes for
    // this (vk, instance, proof).
    let expected: Vec<Fr> = accumulator_pis(
        &verify_proof_offcircuit(&fixture.vk_blob, &inner_pis, &inner_proof, true)
            .expect("off-circuit preparation"),
    )
    .into_iter()
    .map(Fr)
    .collect();
    assert_eq!(
        expected.len(),
        acc_len,
        "off-circuit accumulator should be acc_len field elements"
    );
    assert_eq!(
        outer_proof.accumulators[0], expected,
        "the carried accumulator must match off-circuit preparation"
    );

    // The verifier re-assembles blocks-then-statement, finds the accumulator,
    // and its pairing check passes.
    outer_verify(&fixture.vk, &outer_proof, outer_pis.clone());

    // And so does a key that has been through the wire.
    let mut bytes = Vec::new();
    Serializable::serialize(&fixture.vk, &mut bytes).expect("serialize vk");
    let reloaded: VerifierKey =
        Deserializable::deserialize(&mut &bytes[..], 0).expect("deserialize vk");
    outer_verify(&reloaded, &outer_proof, outer_pis);
}
