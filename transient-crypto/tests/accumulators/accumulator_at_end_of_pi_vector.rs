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

//! An accumulator occupying the final `acc_len` slots of the statement is read
//! and verified — `offset + acc_len == len` is in range, not one past it.
//!
//! The boundary `extract_accumulators` slices at. `accumulators_verify_one_and_multiple`
//! happens to place one at the end too, but only as a by-product of the
//! accumulator being the whole statement; here there is real content before it,
//! so an off-by-one would land on that content rather than fall off the end and
//! error.

use midnight_curves::Fq;

use midnight_transient_crypto::proofs::PARAMS_VERIFIER;

use crate::harness::{acc_len, passing_accumulator, proof_exposing, test_rng};

#[test]
fn accumulator_at_end_of_pi_vector() {
    let mut rng = test_rng();

    let leading: Vec<Fq> = (0..3).map(|i| Fq::from(i as u64 + 100)).collect();
    let mut pis = leading.clone();
    pis.extend(passing_accumulator());

    let offset = leading.len();
    assert_eq!(
        offset + acc_len(),
        pis.len(),
        "the accumulator must end exactly at the end of the statement"
    );

    let (vk, proof, statement) = proof_exposing(&pis, &[offset], &mut rng);
    vk.verify(&PARAMS_VERIFIER, &proof, statement.into_iter())
        .expect("an accumulator at the very end must verify");
}
