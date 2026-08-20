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

//! Bytes that are not a proof at all fail gracefully during off-circuit
//! preparation — an `Err`, not a panic or a hang.
//!
//! Reached through `check`, so there is no keygen and no proving; this lives in
//! `e2e/` only because it needs a genuine verifying key.
//!
//! Trailing bytes are the exception, recorded at the end: a padded proof is
//! accepted, because the transcript reader stops once it has what it needs. The
//! padding is inert — the resulting accumulator is byte-identical.

use midnight_zkir_v3::ir_instructions::verify_proof::verify_proof_offcircuit;
use transient_crypto::proofs::Zkir;

use crate::e2e_harness::{outer_ir_for, outer_preimage, rsa_inner_proof, test_rng};

#[actix_rt::test]
#[ignore = "needs an SRS to build a genuine inner verifying key"]
async fn malformed_proof_witness_is_rejected() {
    let mut rng = test_rng();

    // The control, and the source of the truncated case.
    let inner = rsa_inner_proof(&mut rng).await;
    let ir = outer_ir_for(&inner.vk_blob, &inner.pis);

    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("empty", Vec::new()),
        ("short nonsense", b"not a proof".to_vec()),
        ("right length, wrong bytes", vec![0xff; inner.proof.len()]),
        (
            "truncated real proof",
            inner.proof[..inner.proof.len() / 2].to_vec(),
        ),
    ];

    for (label, witness) in cases {
        // An `Err` — a panic would fail the test outright.
        match ir.check(&outer_preimage(witness)) {
            Ok(_) => panic!("{label}: malformed bytes were accepted as a proof witness"),
            Err(e) => println!("{label}: rejected -- {e:#}"),
        }
    }

    // Control: the same path accepts the real proof.
    ir.check(&outer_preimage(inner.proof.clone()))
        .expect("a genuine inner proof must pass off-circuit preparation");

    // Trailing bytes are accepted, so a proof is not canonically encoded. That
    // is inert rather than merely unchecked: the accumulator is unchanged, and
    // it is the only thing preparation derives from the proof.
    let mut padded = inner.proof.clone();
    padded.extend_from_slice(b"trailing junk");
    ir.check(&outer_preimage(padded.clone()))
        .expect("trailing bytes are ignored by the transcript reader");

    let from_real = verify_proof_offcircuit(&inner.vk_blob, &inner.pis, &inner.proof)
        .expect("preparation of the real proof");
    let from_padded = verify_proof_offcircuit(&inner.vk_blob, &inner.pis, &padded)
        .expect("preparation of the padded proof");
    assert_eq!(
        from_real, from_padded,
        "trailing bytes must not perturb the accumulator, or they would not be inert"
    );
}
