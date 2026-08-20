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

//! Two `verify_proof` instructions over distinct inner VKs in one circuit.
//!
//! `VK_NAME` is a single constant (`"inner_vk"`) labelling the fixed bases and
//! the assigned VK of *every* `verify_proof`, so two distinct keys both carrying
//! it is where a collision would show up.
//!
//! Placement is not re-checked here — `accumulator_at_recorded_offset_matches_offcircuit`
//! pins that, and a misplaced accumulator would fail the pairing below anyway.

use crate::e2e_harness::{
    outer_preimage_all, outer_prove, outer_setup_all, outer_verify, rsa_inner_proof,
    scalar_inner_proof, test_rng,
};

#[actix_rt::test]
#[ignore = "outer verifier circuit needs a high-k SRS not available in CI"]
async fn two_proofs_with_distinct_vks_are_accepted() {
    let mut rng = test_rng();

    // The RSA fixture (22 public inputs) and a single-scalar circuit (1), so the
    // two inner keys differ in architecture as well as content.
    let rsa = rsa_inner_proof(&mut rng).await;
    let scalar = scalar_inner_proof(&mut rng).await;
    assert_ne!(
        rsa.vk_blob, scalar.vk_blob,
        "the two inner verifying keys must actually differ"
    );

    // One `verify_proof` per inner proof.
    let (outer_ir, outer_pk, outer_vk) = outer_setup_all(&[rsa.entry(), scalar.entry()]).await;

    let preimage = outer_preimage_all(vec![rsa.proof, scalar.proof]);
    let (outer_proof, outer_pis) = outer_prove(&outer_ir, outer_pk, &preimage, &mut rng).await;

    // A single `verify` runs the Plonk check and the deferred pairing on *both*
    // accumulators.
    outer_verify(&outer_vk, &outer_proof, outer_pis);
}
