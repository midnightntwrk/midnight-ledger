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

//! An inner proof built with ZKIR's Blake2b transcript, instead of the Poseidon
//! one the in-circuit verifier requires, is rejected.
//!
//! Caught at verify, not prove: reading a Blake2b proof with a Poseidon
//! transcript does not error, it yields a well-formed but meaningless
//! accumulator that proving commits to happily. Nothing fails fast on this
//! mistake, which is why it is worth pinning.

use midnight_zk_stdlib::prove;

use crate::e2e_harness::{
    Rejection, RsaSignatureRelation, expect_rejected, pinned_fixture, test_rng,
};

#[actix_rt::test]
#[ignore = "outer verifier circuit needs a high-k SRS not available in CI"]
async fn blake2b_transcript_proof_is_rejected() {
    let mut rng = test_rng();

    let fixture = pinned_fixture().await;

    // Everything is right except the transcript type parameter, which is the
    // only difference from `harness::prove_inner`.
    let proof = prove::<RsaSignatureRelation, blake2b_simd::State>(
        fixture.inner_srs.as_ref(),
        &fixture.inner_pk,
        &RsaSignatureRelation,
        &fixture.instance,
        fixture.signature.clone(),
        &mut rng,
    )
    .expect("inner prove (blake2b)");

    let stage = expect_rejected(
        &fixture.ir,
        fixture.pk.clone(),
        &fixture.vk,
        proof,
        &mut rng,
    )
    .await;

    assert_eq!(
        stage,
        Rejection::AtVerify,
        "a Blake2b inner proof reads as a well-formed but meaningless \
         accumulator, so it is the deferred pairing check that rejects it"
    );
}
