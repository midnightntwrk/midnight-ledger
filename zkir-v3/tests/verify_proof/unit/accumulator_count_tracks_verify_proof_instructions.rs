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

//! `accumulator_count()` is one per `VerifyProof` instruction and nothing else.
//!
//! It counts instructions, not distinct keys. And it is witness-independent: a
//! guarded-off `verify_proof` still occupies its block, which is what lets
//! `prove` compute the split at keygen, before any witness exists.

use midnight_zkir_v3::IrSource;

use crate::unit_harness::{
    VK_BLOB_A, VK_BLOB_B, bind_and_verify, bind_and_verify_one, ir, vk_hash,
};

/// One `inner_proof`/`verify_proof` pair under an explicit guard operand.
fn guarded_pair(i: usize, guard: &str, blob: &[u8]) -> String {
    format!(
        r#"{{ "op": "inner_proof", "guard": "{guard}", "output": "%p_{i}" }},
           {{
               "op": "verify_proof",
               "guard": "{guard}",
               "vk_hash": "0x{hash}",
               "instance": ["0x7b"],
               "proof": "%p_{i}"
           }}"#,
        hash = vk_hash(blob),
    )
}

/// The count only reads the instruction list, so the side-table can stay empty.
fn count(instructions: &str) -> usize {
    ir(instructions).accumulator_count()
}

#[test]
fn accumulator_count_tracks_verify_proof_instructions() {
    // Nothing to verify.
    assert_eq!(
        count(r#"{ "op": "impact", "guard": "0x01", "inputs": ["0x2a"] }"#),
        0
    );

    // An `inner_proof` on its own binds a witness but verifies nothing, so it
    // contributes no accumulator.
    assert_eq!(
        count(r#"{ "op": "inner_proof", "guard": "0x01", "output": "%p_0" }"#),
        0
    );

    // One, and two over distinct keys.
    assert_eq!(count(&bind_and_verify_one(&vk_hash(&VK_BLOB_A))), 1);
    assert_eq!(
        count(&bind_and_verify(&[
            vk_hash(&VK_BLOB_A),
            vk_hash(&VK_BLOB_B)
        ])),
        2
    );

    // Two instructions resolving to *one* key. The side-table would hold a
    // single entry, but each instruction verifies its own proof and exposes its
    // own accumulator.
    assert_eq!(
        count(&bind_and_verify(&[
            vk_hash(&VK_BLOB_A),
            vk_hash(&VK_BLOB_A)
        ])),
        2,
        "the count is per instruction, not per distinct key"
    );

    // Unrelated instructions between the pairs change nothing.
    let interleaved = format!(
        "{},\n{},\n{}",
        guarded_pair(0, "0x01", &VK_BLOB_A),
        r#"{ "op": "impact", "guard": "0x01", "inputs": ["0x2a", "0x2b"] }"#,
        guarded_pair(1, "0x01", &VK_BLOB_B),
    );
    assert_eq!(
        count(&interleaved),
        2,
        "an Impact contributes no accumulator"
    );

    // Witness-independence: the guard is an operand the count must never read.
    // A constant-off, a constant-on and a variable guard all count the same.
    for guard in ["0x01", "0x00", "%v_0"] {
        assert_eq!(
            count(&guarded_pair(0, guard, &VK_BLOB_A)),
            1,
            "guard {guard}: a guarded-off verify_proof still exposes the trivial accumulator"
        );
    }

    // And a mixed circuit, where only one of the two is guarded off.
    let mixed = format!(
        "{},\n{}",
        guarded_pair(0, "0x01", &VK_BLOB_A),
        guarded_pair(1, "0x00", &VK_BLOB_B),
    );
    assert_eq!(
        count(&mixed),
        2,
        "the split must not depend on which branch the prover takes"
    );

    // The count is a property of the instruction list alone: attaching the
    // resolved keys does not change it.
    let ir_hash_only: IrSource = ir(&bind_and_verify_one(&vk_hash(&VK_BLOB_A)));
    let mut ir_with_keys = ir_hash_only.clone();
    ir_with_keys.verify_proof_vks = vec![VK_BLOB_A.to_vec()];
    assert_eq!(
        ir_hash_only.accumulator_count(),
        ir_with_keys.accumulator_count(),
        "resolving the side-table must not change the exposed shape"
    );
}
