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

//! The proof binds the accumulators it carries, so swapping or reordering them
//! is refused by PLONK before any pairing runs. Otherwise a prover could replace
//! a failing accumulator with one that pairs.

use midnight_transient_crypto::proofs::{PARAMS_VERIFIER, VerifierKey};

use crate::harness::{
    deferred, failing_accumulator, passing_accumulator, proof_carrying, test_rng,
};

#[test]
fn carried_accumulators_are_bound_by_the_proof() {
    let mut rng = test_rng();
    let pass = passing_accumulator();
    let fail = failing_accumulator();

    // Proven over a failing accumulator, carrying a passing one instead.
    let (swap_vk, mut swapped, swap_stmt) =
        proof_carrying(std::slice::from_ref(&fail), &[], &mut rng);
    swapped.accumulators = vec![deferred(&pass)];

    // Proven over `[pass, fail]`, carrying `[fail, pass]`.
    let (order_vk, mut reordered, order_stmt) =
        proof_carrying(&[pass.clone(), fail.clone()], &[], &mut rng);
    reordered.accumulators.reverse();

    for (label, vk, proof, stmt) in [
        ("swapped", &swap_vk, &swapped, &swap_stmt),
        ("reordered", &order_vk, &reordered, &order_stmt),
    ] {
        let err = vk
            .verify(&PARAMS_VERIFIER, proof, stmt.clone().into_iter())
            .expect_err("tampered accumulators must be refused");
        assert!(
            format!("{err:#}").contains("Invalid outer proof"),
            "{label}: expected PLONK to refuse it, got: {err:#}"
        );

        let err = VerifierKey::batch_verify(
            &PARAMS_VERIFIER,
            [(vk, proof, stmt.clone().into_iter())].into_iter(),
        )
        .expect_err("tampered accumulators must be refused in a batch");
        assert!(
            format!("{err:#}").contains("Invalid proof"),
            "{label}: expected PLONK to refuse it, got: {err:#}"
        );
    }
}
