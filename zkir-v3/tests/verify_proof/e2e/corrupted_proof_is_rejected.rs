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

//! A valid inner proof with one byte flipped is rejected — and where depends on
//! which byte.
//!
//! Corrupting a leading commitment point breaks BLS12-381 decoding, so
//! preparation refuses it outright. Corrupting an evaluation scalar leaves
//! every field element decodable, so preparation completes and only the
//! deferred pairing rejects it. For most of the proof's length nothing fails
//! fast.

use crate::e2e_harness::{Rejection, expect_rejected, pinned_fixture, test_rng};

#[actix_rt::test]
#[ignore = "outer verifier circuit needs a high-k SRS not available in CI"]
async fn corrupted_proof_is_rejected() {
    let mut rng = test_rng();

    let fixture = pinned_fixture().await;
    let proof = fixture.correct_proof(&mut rng);

    // ---- Structural: a leading commitment point ------------------------
    // The proof opens with group elements; breaking one makes it undecodable.
    let mut structural = proof.clone();
    structural[0] ^= 0x01;
    let stage = expect_rejected(
        &fixture.ir,
        fixture.pk.clone(),
        &fixture.vk,
        structural,
        &mut rng,
    )
    .await;
    assert_eq!(
        stage,
        Rejection::AtProve,
        "corrupting a commitment point breaks decoding, so the off-circuit \
         preparation must reject it before any accumulator is produced"
    );

    // ---- Semantic: an evaluation scalar mid-proof ----------------------
    // The midpoint lands among the evaluations, which are plain field elements:
    // the flipped byte still decodes, so nothing fails until the pairing.
    let mut semantic = proof.clone();
    let mid = proof.len() / 2;
    semantic[mid] ^= 0x01;
    let stage = expect_rejected(
        &fixture.ir,
        fixture.pk.clone(),
        &fixture.vk,
        semantic,
        &mut rng,
    )
    .await;
    assert_eq!(
        stage,
        Rejection::AtVerify,
        "corrupting an evaluation scalar leaves a decodable proof, so it \
         survives preparation and is caught only by the deferred pairing check"
    );
}
