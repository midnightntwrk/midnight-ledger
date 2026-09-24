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

//! `verify` pairs every accumulator a proof carries, not just the first or last.

use crate::harness::{failing_accumulator, passing_accumulator, proof_carrying, test_rng};
use midnight_transient_crypto::proofs::PARAMS_VERIFIER;

#[test]
fn each_accumulator_block_is_paired() {
    let mut rng = test_rng();
    let acc = passing_accumulator();

    // One.
    let (vk, proof, pis) = proof_carrying(std::slice::from_ref(&acc), &[], &mut rng);
    assert_eq!(proof.accumulators.len(), 1);
    vk.verify(&PARAMS_VERIFIER, &proof, pis.into_iter())
        .expect("one accumulator must verify");

    // Two.
    let (vk, proof, pis) = proof_carrying(&[acc.clone(), acc.clone()], &[], &mut rng);
    assert_eq!(proof.accumulators.len(), 2);
    vk.verify(&PARAMS_VERIFIER, &proof, pis.into_iter())
        .expect("two accumulators must both verify");

    // Passing blocks cannot show that every one is paired; a failing block must
    // be caught in either position.
    let bad = failing_accumulator();
    for (label, blocks) in [
        ("failing second", vec![acc.clone(), bad.clone()]),
        ("failing first", vec![bad, acc]),
    ] {
        let (vk, proof, pis) = proof_carrying(&blocks, &[], &mut rng);
        let err = vk
            .verify(&PARAMS_VERIFIER, &proof, pis.into_iter())
            .expect_err("a block that does not pair must reject, wherever it sits");
        assert!(
            format!("{err:#}").contains("pairing"),
            "{label}: expected a pairing failure, got: {err:#}"
        );
    }
}
