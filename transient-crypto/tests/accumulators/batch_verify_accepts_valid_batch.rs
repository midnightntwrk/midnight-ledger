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

//! `batch_verify` accepts a batch mixing keys that carry accumulators with keys
//! that carry none, and checks the accumulators of the ones that do.
//!
//! The batch path is separate code from `verify`: it runs one batched PLONK
//! check and then walks each key's offsets against its own statement. Mixing
//! plain and accumulator-bearing keys is where a mismatch between the two lists
//! would show — a plain key contributes an empty offsets entry, which must still
//! line up with its statement.

use midnight_curves::Fq;

use midnight_transient_crypto::proofs::{PARAMS_VERIFIER, VerifierKey};

use crate::harness::{acc_len, passing_accumulator, proof_exposing, test_rng};

#[test]
fn batch_verify_accepts_valid_batch() {
    let mut rng = test_rng();

    // A key with no accumulators at all.
    let plain_pis: Vec<Fq> = (0..2).map(|i| Fq::from(i as u64 + 1)).collect();
    let (plain_vk, plain_proof, plain_stmt) = proof_exposing(&plain_pis, &[], &mut rng);

    // A key with one, and a key with two.
    let one = passing_accumulator();
    let (one_vk, one_proof, one_stmt) = proof_exposing(&one, &[0], &mut rng);

    let two: Vec<Fq> = one.iter().chain(one.iter()).copied().collect();
    let (two_vk, two_proof, two_stmt) = proof_exposing(&two, &[0, acc_len()], &mut rng);

    VerifierKey::batch_verify(
        &PARAMS_VERIFIER,
        [
            (&plain_vk, &plain_proof, plain_stmt.into_iter()),
            (&one_vk, &one_proof, one_stmt.into_iter()),
            (&two_vk, &two_proof, two_stmt.into_iter()),
        ]
        .into_iter(),
    )
    .expect("a batch of valid proofs must verify");
}
