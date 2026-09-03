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

//! A block that cannot be read is refused gracefully: one of the wrong width,
//! and one whose fields are not a point encoding.
//!
//! `verify` runs PLONK on the concatenation before looking at the blocks, so the
//! wrong-width case moves the block/statement boundary rather than adding or
//! dropping a field element. A control shows a readable block that does not pair
//! fails at the pairing instead.

use midnight_curves::Fq;

use midnight_transient_crypto::proofs::{PARAMS_VERIFIER, Proof, VerifierKey};

use crate::harness::{
    acc_len, failing_accumulator, fr_vec, passing_accumulator, proof_carrying, raw_proof_exposing,
    test_rng,
};

#[test]
fn bad_accumulator_block_is_rejected() {
    let mut rng = test_rng();
    let acc = passing_accumulator();

    // ---- A block of the wrong width ----
    // The public-input vector is a correct one; only the split between block and
    // statement is moved, so PLONK still passes and the block is one short.
    let (raw_vk, bytes) = raw_proof_exposing(&acc, &mut rng);
    let proof = Proof {
        bytes,
        accumulators: vec![fr_vec(&acc[..acc_len() - 1])],
    };
    let err = VerifierKey::from(raw_vk)
        .verify(
            &PARAMS_VERIFIER,
            &proof,
            fr_vec(&acc[acc_len() - 1..]).into_iter(),
        )
        .expect_err("a block that is not one accumulator wide must not be read");
    assert!(
        format!("{err:#}").contains("accumulator block has length"),
        "expected a block-width error, got: {err:#}"
    );

    // ---- Right width, but not a point encoding ----
    // Every field element is valid; they just do not decode to a curve point.
    let junk: Vec<Fq> = (0..acc_len()).map(|i| Fq::from(i as u64 + 1)).collect();
    let (vk, proof, stmt) = proof_carrying(&[junk], &[], &mut rng);
    let err = vk
        .verify(&PARAMS_VERIFIER, &proof, stmt.into_iter())
        .expect_err("non-point fields must not decode as an accumulator");
    assert!(
        format!("{err:#}").contains("malformed accumulator"),
        "expected a malformed-accumulator error, got: {err:#}"
    );

    // Control: a well-formed block is read, and refused only by the pairing —
    // so the errors above are about readability, not validity.
    let (vk, proof, stmt) = proof_carrying(&[failing_accumulator()], &[], &mut rng);
    let err = vk
        .verify(&PARAMS_VERIFIER, &proof, stmt.into_iter())
        .expect_err("an accumulator that does not pair must be refused");
    assert!(
        format!("{err:#}").contains("pairing"),
        "expected a pairing failure, got: {err:#}"
    );
}
