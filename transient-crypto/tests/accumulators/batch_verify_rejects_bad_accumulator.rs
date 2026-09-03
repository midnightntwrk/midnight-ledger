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

//! One proof whose accumulator does not pair rejects the whole batch.
//!
//! The batched PLONK check cannot see it: the block is a well-formed part of a
//! genuinely valid proof, so only the deferred pairing can refuse it.

use midnight_transient_crypto::proofs::{PARAMS_VERIFIER, VerifierKey};

use crate::harness::{failing_accumulator, passing_accumulator, proof_carrying, test_rng};

#[test]
fn batch_verify_rejects_bad_accumulator() {
    let mut rng = test_rng();

    let (good_vk, good_proof, good_stmt) = proof_carrying(&[passing_accumulator()], &[], &mut rng);
    let (bad_vk, bad_proof, bad_stmt) = proof_carrying(&[failing_accumulator()], &[], &mut rng);

    // Control: each is individually as expected.
    good_vk
        .verify(&PARAMS_VERIFIER, &good_proof, good_stmt.clone().into_iter())
        .expect("the good proof must verify on its own");
    bad_vk
        .verify(&PARAMS_VERIFIER, &bad_proof, bad_stmt.clone().into_iter())
        .expect_err("the bad accumulator must not pair on its own");

    let err = VerifierKey::batch_verify(
        &PARAMS_VERIFIER,
        [
            (&good_vk, &good_proof, good_stmt.into_iter()),
            (&bad_vk, &bad_proof, bad_stmt.into_iter()),
        ]
        .into_iter(),
    )
    .expect_err("one failing accumulator must reject the whole batch");
    assert!(
        format!("{err:#}").contains("pairing"),
        "expected a pairing failure, got: {err:#}"
    );
}
