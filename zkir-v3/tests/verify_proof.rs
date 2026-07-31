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

//! Shows the ZKIR text format of the `verify_proof` instruction.
//!
//! The instruction carries only the **hash** of the inner circuit's verifying
//! key, rendered as a `0x`-prefixed hex string:
//!
//! - `vk_hash`: hash of the decider-tagged, self-contained `MidnightVK` blob
//!   (the blob's leading byte is the decider tag, `0x00` = the Standard
//!   decider). The full VK is resolved out-of-band and carried in the IR's
//!   `verify_proof_vks` side-table; the canonical text stores only the hash.
//! - `instance`: the inner proof's public inputs, as ordinary `Native`
//!   operands (variable references or `0x`-hex immediates).
//!
//! The inner proof is *not* in the instruction — it is a prover-supplied
//! witness (`ProofPreimage::proof_witnesses`). The `vk_hash` here is fake —
//! this test exercises only the text format and round-trip, not verification.

use midnight_zkir_v3::IrSource;

/// Canonical, hash-only `verify_proof` IR: the instruction stores just the VK
/// hash; the inner statement is the single public input `%v_0`. Neither the
/// full VK nor the proof appears in the text — both are supplied out-of-band.
const VERIFY_PROOF_IR: &str = r#"{
   "version": { "major": 3, "minor": 0 },
   "inputs": [
      { "name": "%v_0", "type": "Scalar<BLS12-381>" }
   ],
   "outputs": [],
   "do_communications_commitment": false,
   "instructions": [
       {
           "op": "verify_proof",
           "vk_hash": "0x00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff",
           "instance": ["%v_0"]
       }
   ]
}"#;

#[test]
fn verify_proof_text_format_roundtrips() {
    let ir = IrSource::load(VERIFY_PROOF_IR.as_bytes()).expect("verify_proof IR must parse");

    // A hash-only IR carries no VK bytes.
    assert!(
        ir.verify_proof_vks.is_empty(),
        "hash-only IR should not carry VK blobs"
    );

    // Re-serialize so the exact canonical instruction shape is visible with
    // `cargo test -- --nocapture`.
    let json = serde_json::to_string_pretty(&ir).expect("IrSource serializes");
    println!("{json}");

    // The instruction is tagged `verify_proof`, the VK hash survives the
    // round-trip as a `0x` hex string, and the empty VK side-table is omitted.
    assert!(json.contains("verify_proof"), "op tag missing:\n{json}");
    assert!(
        json.contains("0x00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff"),
        "vk_hash hex missing:\n{json}"
    );
    assert!(json.contains("%v_0"), "instance operand missing:\n{json}");
    assert!(
        !json.contains("verify_proof_vks"),
        "empty VK side-table should be omitted:\n{json}"
    );
}
