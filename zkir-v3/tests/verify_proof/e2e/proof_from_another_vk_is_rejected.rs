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

//! An inner proof made under a verifying key other than the one the circuit was
//! compiled against is rejected.
//!
//! The complement of `valid_proof_of_other_statement_is_rejected`: that holds
//! the key fixed and varies the statement, this holds the statement fixed and
//! varies the key.
//!
//! Wrong size — the same relation in a larger domain — is a genuine proof of the
//! statement the instruction names, and the same length as a correct one, so
//! only the pairing can refuse it. A substituted relation does not fit this
//! key's transcript shape and dies in preparation.

use crate::e2e_harness::{
    Rejection, RsaSignatureRelation, expect_rejected, inner_setup_at, pinned_fixture, prove_inner,
    scalar_inner_proof, test_rng,
};

/// A domain larger than the RSA relation needs, giving the same circuit a
/// different verifying key.
const OVERSIZED_K: u8 = 13;

#[actix_rt::test]
#[ignore = "outer verifier circuit needs a high-k SRS not available in CI"]
async fn proof_from_another_vk_is_rejected() {
    let mut rng = test_rng();

    // The key the outer circuit is compiled against, and a correct proof under
    // it — kept only as the length reference below.
    let fixture = pinned_fixture().await;
    let correct_proof = fixture.correct_proof(&mut rng);

    // ---- A valid proof of the same statement, under a bigger domain ----
    let (oversized_srs, oversized_pk, oversized_vk) =
        inner_setup_at::<RsaSignatureRelation>("rsa, oversized domain", OVERSIZED_K).await;
    assert_ne!(
        fixture.vk_blob, oversized_vk,
        "setting the same relation up at a different k must give a different VK"
    );
    let wrong_size_proof = prove_inner(
        &fixture.instance,
        fixture.signature.clone(),
        &oversized_pk,
        &oversized_srs,
        &mut rng,
    );

    // ---- A proof of a different relation entirely ----------------------
    let substituted_proof = scalar_inner_proof(&mut rng).await.proof;

    // ---- Wrong size ----------------------------------------------------
    assert_eq!(
        wrong_size_proof.len(),
        correct_proof.len(),
        "the oversized-domain proof is the same length as a correct one, so no \
         length or structural check could tell them apart — only the key does"
    );
    let stage = expect_rejected(
        &fixture.ir,
        fixture.pk.clone(),
        &fixture.vk,
        wrong_size_proof,
        &mut rng,
    )
    .await;
    assert_eq!(
        stage,
        Rejection::AtVerify,
        "a valid proof of the right statement under the wrong key is well-formed, \
         so only the deferred pairing can refuse it"
    );

    // ---- Substituted circuit -------------------------------------------
    let stage = expect_rejected(
        &fixture.ir,
        fixture.pk.clone(),
        &fixture.vk,
        substituted_proof,
        &mut rng,
    )
    .await;
    assert_eq!(
        stage,
        Rejection::AtProve,
        "a proof of a different relation does not fit this key's transcript shape, \
         so preparation refuses it"
    );
}
