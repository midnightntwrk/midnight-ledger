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

//! An accumulator region that cannot be read is refused gracefully, not by
//! panicking or by indexing out of bounds.
//!
//! Two ways to be unreadable. An offset that runs past the end of the statement
//! has nothing to slice, and field elements that are not a valid point encoding
//! decode to nothing. Both are reached with a *genuine* proof — the PLONK check
//! passes and only extraction fails — because a doctored statement would be
//! refused by PLONK first and never reach this code.

use midnight_curves::Fq;

use midnight_transient_crypto::proofs::{PARAMS_VERIFIER, VerifierKey};

use crate::harness::{
    acc_len, failing_accumulator, passing_accumulator, raw_proof_exposing, test_rng,
};
use midnight_transient_crypto::curve::Fr;

#[test]
fn bad_accumulator_in_public_inputs_is_rejected() {
    let mut rng = test_rng();
    let acc = passing_accumulator();

    // ---- Offset past the end of the statement ----
    let (raw_vk, proof) = raw_proof_exposing(&acc, &mut rng);
    let statement: Vec<Fr> = acc.iter().map(|f| Fr(*f)).collect();
    let vk = VerifierKey::from_vk_with_accumulator_offsets(raw_vk, &[acc.len()]);
    let err = vk
        .verify(&PARAMS_VERIFIER, &proof, statement.clone().into_iter())
        .expect_err("an offset past the end must not be read");
    assert!(
        format!("{err:#}").contains("out of range"),
        "expected an out-of-range error, got: {err:#}"
    );

    // Also when the region starts inside the statement but runs off the end.
    let (raw_vk, proof2) = raw_proof_exposing(&acc, &mut rng);
    let vk = VerifierKey::from_vk_with_accumulator_offsets(raw_vk, &[1]);
    let err = vk
        .verify(&PARAMS_VERIFIER, &proof2, statement.into_iter())
        .expect_err("a region overrunning the statement must not be read");
    assert!(
        format!("{err:#}").contains("out of range"),
        "expected an out-of-range error, got: {err:#}"
    );

    // ---- Right width, but not a point encoding ----
    // Every field element is valid; they just do not decode to a curve point.
    let junk: Vec<Fq> = (0..acc_len()).map(|i| Fq::from(i as u64 + 1)).collect();
    let (raw_vk, proof) = raw_proof_exposing(&junk, &mut rng);
    let statement: Vec<Fr> = junk.iter().map(|f| Fr(*f)).collect();
    let vk = VerifierKey::from_vk_with_accumulator_offsets(raw_vk, &[0]);
    let err = vk
        .verify(&PARAMS_VERIFIER, &proof, statement.into_iter())
        .expect_err("non-point fields must not decode as an accumulator");
    assert!(
        format!("{err:#}").contains("malformed accumulator"),
        "expected a malformed-accumulator error, got: {err:#}"
    );

    // Control: a well-formed region at a valid offset is read, and refused only
    // by the pairing — so the errors above are about readability, not validity.
    let bad = failing_accumulator();
    let (raw_vk, proof) = raw_proof_exposing(&bad, &mut rng);
    let statement: Vec<Fr> = bad.iter().map(|f| Fr(*f)).collect();
    let vk = VerifierKey::from_vk_with_accumulator_offsets(raw_vk, &[0]);
    let err = vk
        .verify(&PARAMS_VERIFIER, &proof, statement.into_iter())
        .expect_err("an accumulator that does not pair must be refused");
    assert!(
        format!("{err:#}").contains("pairing"),
        "expected a pairing failure, got: {err:#}"
    );
}
