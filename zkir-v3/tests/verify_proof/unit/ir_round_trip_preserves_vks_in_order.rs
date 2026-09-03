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

//! `vk_hash` and the `verify_proof_vks` side-table survive JSON and the binary
//! form, intact and in order.
//!
//! `IrSource::load` is covered separately by
//! `load_accepts_ir_carrying_a_side_table`, which currently fails.

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

    // Without this the equality checks above would also pass if a round-trip
    // reordered the side-table.
    let mut reordered = ir.clone();
    reordered.verify_proof_vks.reverse();
    assert_ne!(reordered, ir, "equality must distinguish side-table order");
}

fn instructions() -> String {
    bind_and_verify(&[vk_hash(&VK_BLOB_A), vk_hash(&VK_BLOB_B)])
}
