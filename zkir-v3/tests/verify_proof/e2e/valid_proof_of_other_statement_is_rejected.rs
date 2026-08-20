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

//! A valid inner proof of a *different* statement is rejected.
//!
//! Nothing is malformed — correct relation, key and transcript, a genuine
//! signature — so only the statement binding can catch it. Both preparation
//! passes agree with each other and the outer proof is internally consistent;
//! the deferred pairing is what fails. A caller that only proved and never
//! verified would not notice.

use crate::e2e_harness::{
    Rejection, expect_rejected, message, pinned_fixture, prove_inner, rsa_key, sign, test_rng,
};

#[actix_rt::test]
#[ignore = "outer verifier circuit needs a high-k SRS not available in CI"]
async fn valid_proof_of_other_statement_is_rejected() {
    let mut rng = test_rng();

    // The circuit is pinned to `fixture.instance`.
    let fixture = pinned_fixture().await;

    // A *different* message under the same key, honestly signed and honestly
    // proven. The proof is perfectly valid — just not for the pinned statement.
    let (modulus, d) = rsa_key();
    let msg_b = message(&modulus, 1);
    assert_ne!(fixture.instance.1, msg_b, "the two messages must differ");
    let instance_b = (modulus.clone(), msg_b.clone());
    let sig_b = sign(&msg_b, &d, &modulus);
    let proof_b = prove_inner(
        &instance_b,
        sig_b,
        &fixture.inner_pk,
        &fixture.inner_srs,
        &mut rng,
    );

    let stage = expect_rejected(
        &fixture.ir,
        fixture.pk.clone(),
        &fixture.vk,
        proof_b,
        &mut rng,
    )
    .await;
    assert_eq!(
        stage,
        Rejection::AtVerify,
        "a well-formed proof of the wrong statement must survive proving and be \
         caught by the deferred pairing check at verify"
    );
}
