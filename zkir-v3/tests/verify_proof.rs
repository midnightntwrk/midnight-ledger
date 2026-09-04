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

//! Shows the ZKIR text format of the `inner_proof` / `verify_proof` pair.
//!
//! `inner_proof` binds the next prover-supplied inner proof
//! (`ProofPreimage::proof_witnesses`) to a name, and `verify_proof` takes that
//! name as its `proof` input. Both carry a `guard`, which should be the same
//! condition on the two: it decides whether a witness is consumed at all, and
//! whether the proof is actually verified. Alongside it, `verify_proof` takes:
//!
//! - `vk_hash`: hash of the decider-tagged, self-contained `MidnightVK` blob
//!   (the blob's leading byte is the decider tag, `0x00` = `DeciderKind::None`).
//!   The full VK is resolved out-of-band and carried in the IR's
//!   `verify_proof_vks` side-table, keyed by this hash; the canonical text
//!   stores only the hash.
//! - `instance`: the inner proof's public inputs, as ordinary `Native`
//!   operands (variable references or `0x`-hex immediates).
//!
//! A guard is an ordinary operand, so it can be a variable (witnessed) or a
//! `0x`-hex immediate (a circuit constant, in practice `0x01`). Guarded off, the
//! pair consumes no witness and exposes the trivial accumulator, which the outer
//! verifier's deferred pairing check accepts unconditionally.
//!
//! The `vk_hash` here is fake — this test exercises only the text format and
//! round-trip, not verification.

use midnight_zkir_v3::IrSource;
use midnight_zkir_v3::ir::IrMinorVersion;
use serialize::tagged_serialize;

/// Canonical, hash-only IR: `%p_0` is bound to the inner proof, the inner
/// statement is the single public input `%v_0`, and the instruction stores just
/// the VK hash. Neither the full VK nor the proof appears in the text — both
/// are supplied out-of-band.
///
/// The second `verify_proof` shows the other shape of the same operator: a
/// constant guard, always `0x01`, i.e. verify unconditionally.
const VERIFY_PROOF_IR: &str = r#"{
   "version": { "major": 3, "minor": 0 },
   "inputs": [
      { "name": "%v_0", "type": "Scalar<BLS12-381>" },
      { "name": "%g", "type": "Scalar<BLS12-381>" }
   ],
   "outputs": [],
   "do_communications_commitment": false,
   "instructions": [
       {
           "op": "inner_proof",
           "guard": "%g",
           "output": "%p_0"
       },
       {
           "op": "verify_proof",
           "guard": "%g",
           "vk_hash": "0x00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff",
           "instance": ["%v_0"],
           "proof": "%p_0"
       },
       {
           "op": "inner_proof",
           "guard": "0x01",
           "output": "%p_1"
       },
       {
           "op": "verify_proof",
           "guard": "0x01",
           "vk_hash": "0x00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff",
           "instance": ["%v_0"],
           "proof": "%p_1"
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

    // Both instructions survive the round-trip, the proof flows from one to the
    // other by name, the VK hash stays a `0x` hex string, and the empty VK
    // side-table is omitted.
    assert!(json.contains("inner_proof"), "op tag missing:\n{json}");
    assert!(json.contains("verify_proof"), "op tag missing:\n{json}");
    assert!(json.contains("%p_0"), "proof operand missing:\n{json}");
    assert!(
        json.contains("0x00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff"),
        "vk_hash hex missing:\n{json}"
    );
    assert!(json.contains("%v_0"), "instance operand missing:\n{json}");

    // Both guard forms survive as operands: a variable stays a variable, and a
    // constant round-trips as the canonical `0x`-hex immediate.
    assert!(
        json.contains("\"guard\": \"%g\""),
        "variable guard missing:\n{json}"
    );
    assert!(
        json.contains("\"guard\": \"0x01\""),
        "constant guard missing:\n{json}"
    );
    assert!(
        !json.contains("verify_proof_vks"),
        "empty VK side-table should be omitted:\n{json}"
    );
}

/// The canonical IR with a side-table written inline, declaring `minor`.
/// `Vec<Vec<u8>>` is an array of byte arrays in JSON. The blobs are arbitrary
/// here: nothing about them is checked until the VM resolves them by hash.
fn ir_with_side_table(minor: u8) -> String {
    VERIFY_PROOF_IR
        .replace("\"minor\": 0", &format!("\"minor\": {minor}"))
        .replace(
            "\"do_communications_commitment\": false,",
            "\"do_communications_commitment\": false,\n   \"verify_proof_vks\": [[0, 1, 2]],",
        )
}

/// A side-table exists only from `V1` on, so text carrying one must declare
/// `minor: 1`. The inconsistent document is refused at load.
#[test]
fn side_table_requires_minor_1() {
    let ir = IrSource::load(ir_with_side_table(1).as_bytes()).expect("`minor: 1` must load");
    assert_eq!(ir.version, IrMinorVersion::V1);
    assert_eq!(ir.verify_proof_vks, vec![vec![0u8, 1, 2]]);
    tagged_serialize(&ir, &mut Vec::new()).expect("a V1 carrying a side-table must serialize");

    assert!(
        IrSource::load(ir_with_side_table(0).as_bytes()).is_err(),
        "`minor: 0` must not accept a side-table"
    );
}
