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

//! A round-trip preserves each instruction's `vk_hash` and the
//! `verify_proof_vks` side-table, intact and in order.
//!
//! `verify_proof_text_format_roundtrips` covers the hash-only form, where the
//! side-table is empty and nothing can be lost. This covers a circuit carrying
//! its resolved VKs, across every direction that exists:
//!
//!   - `tagged_serialize` / `tagged_deserialize` — the `.bzkir` format,
//!   - `IrSource::load` — the compiler-emitted text,
//!   - `Serialize` / `serde_json::from_str` — no production caller, but public.
//!
//! `load` is *not* the inverse of `Serialize`: it expects
//! `"version": {"major":3,"minor":0}` and rewrites it to the bare minor number
//! before delegating to `Deserialize`, which is what `Serialize` emits directly.

use midnight_zkir_v3::IrSource;
use serialize::{tagged_deserialize, tagged_serialize};

use crate::unit_harness::{VK_BLOB_A, VK_BLOB_B, bind_and_verify, ir_with_vks, vk_hash, vk_hashes};

#[test]
fn ir_round_trip_preserves_vks_in_order() {
    let blobs = vec![VK_BLOB_A.to_vec(), VK_BLOB_B.to_vec()];
    let ir = ir_with_vks(&instructions(), blobs.clone());
    let hashes = vk_hashes(&ir);
    assert_eq!(ir.verify_proof_vks, blobs);
    assert_eq!(hashes.len(), 2);

    // JSON, via the derived impls.
    let json = serde_json::to_string(&ir).expect("serializes");
    assert!(json.contains("verify_proof_vks"), "{json}");
    let back: IrSource = serde_json::from_str(&json).expect("parses back");
    assert_eq!(back, ir);
    assert_eq!(vk_hashes(&back), hashes);

    // Binary.
    let mut bytes = Vec::new();
    tagged_serialize(&ir, &mut bytes).expect("serializes");
    let back: IrSource = tagged_deserialize(&bytes[..]).expect("parses back");
    assert_eq!(back, ir);
    assert_eq!(vk_hashes(&back), hashes);

    // Canonical text carrying the side-table inline, rather than assigned after
    // parsing — otherwise `load`'s own handling of it goes untested.
    let back = IrSource::load(text_with_vks().as_bytes()).expect("parses");
    assert_eq!(back, ir);
    assert_eq!(back.verify_proof_vks, blobs);
    assert_eq!(vk_hashes(&back), hashes);

    // Without this the equality checks above would also pass if a round-trip
    // reordered the side-table.
    let mut reordered = ir.clone();
    reordered.verify_proof_vks.reverse();
    assert_ne!(reordered, ir, "equality must distinguish side-table order");
}

fn instructions() -> String {
    bind_and_verify(&[vk_hash(&VK_BLOB_A), vk_hash(&VK_BLOB_B)])
}

/// The same circuit with the side-table in the text. `Vec<Vec<u8>>` is an array
/// of byte arrays in JSON.
fn text_with_vks() -> String {
    let bytes = |b: &[u8]| {
        format!(
            "[{}]",
            b.iter()
                .map(|b| b.to_string())
                .collect::<Vec<_>>()
                .join(",")
        )
    };
    format!(
        r#"{{
           "version": {{ "major": 3, "minor": 0 }},
           "inputs": [],
           "outputs": [],
           "do_communications_commitment": false,
           "verify_proof_vks": [{a}, {b}],
           "instructions": [{instructions}]
        }}"#,
        a = bytes(&VK_BLOB_A),
        b = bytes(&VK_BLOB_B),
        instructions = instructions(),
    )
}
