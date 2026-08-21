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
//! `verify` runs the PLONK check, then rebuilds each accumulator at its
//! recorded offset and pairs it. Two offsets exercise the loop rather than a
//! single-element special case, and the second sits past the first, so an
//! offset that were ignored or reused would pair the wrong region.

use crate::harness::{acc_len, passing_accumulator, proof_exposing, test_rng};
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

    // One accumulator, occupying the whole statement.
    let (vk, proof, pis) = proof_exposing(&acc, &[0], &mut rng);
    vk.verify(&PARAMS_VERIFIER, &proof, pis.into_iter())
        .expect("one accumulator must verify");

    // Two, back to back.
    let pair: Vec<_> = acc.iter().chain(acc.iter()).copied().collect();
    let (vk, proof, pis) = proof_exposing(&pair, &[0, acc_len()], &mut rng);
    vk.verify(&PARAMS_VERIFIER, &proof, pis.into_iter())
        .expect("two accumulators must both verify");
}
