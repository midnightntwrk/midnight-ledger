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

//! A proof carrying one accumulator and a proof carrying two both verify.
//!
//! `verify` runs the PLONK check, then rebuilds each block the proof carries
//! and pairs it. Two blocks exercise the loop rather than a single-element
//! special case, and each must be paired on its own — a second block that were
//! ignored, or the first paired twice, would both pass a one-block test.

use crate::harness::{acc_len, passing_accumulator, proof_carrying, test_rng};
use midnight_transient_crypto::proofs::PARAMS_VERIFIER;

#[test]
fn accumulators_verify_one_and_multiple() {
    let mut rng = test_rng();
    let acc = passing_accumulator();
    assert_eq!(
        acc.len(),
        acc_len(),
        "encoding should be one accumulator wide"
    );

    // One accumulator, and nothing else in the public inputs.
    let (vk, proof, pis) = proof_carrying(std::slice::from_ref(&acc), &[], &mut rng);
    assert_eq!(proof.accumulators.len(), 1);
    vk.verify(&PARAMS_VERIFIER, &proof, pis.into_iter())
        .expect("one accumulator must verify");

    // Two, back to back.
    let (vk, proof, pis) = proof_carrying(&[acc.clone(), acc], &[], &mut rng);
    assert_eq!(proof.accumulators.len(), 2);
    vk.verify(&PARAMS_VERIFIER, &proof, pis.into_iter())
        .expect("two accumulators must both verify");
}
