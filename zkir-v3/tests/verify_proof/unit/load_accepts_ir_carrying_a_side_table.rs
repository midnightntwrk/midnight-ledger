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

//! Text IR carrying `verify_proof_vks` inline must be loadable, and whatever
//! `load` returns must be serializable.
//!
//! FAILING, in two ways, and the second is the defect proper:
//!
//! - `load` accepts only `minor: 0..=0`, so text declaring the `V1` that a
//!   side-table requires is refused outright. Widening the arm to `0..=1` fixes
//!   this half — the version is `serde_repr` over `#[repr(u8)]`, so `1` maps
//!   straight to `IrMinorVersion::V1`.
//! - Text declaring `minor: 0` *with* a side-table loads happily into a `V0`
//!   carrying one, which is exactly what `serialize` refuses. So `load` produces
//!   values that cannot be written back out.
//!
//! The second case deliberately does not say which way to resolve it: rejecting
//! an inconsistent document at load, or inferring the version from its content,
//! are both defensible and the choice is not this test's to make. It asserts
//! only the invariant both satisfy — anything `load` returns can be serialized.

use serialize::tagged_serialize;

use midnight_zkir_v3::IrSource;
use midnight_zkir_v3::ir::IrMinorVersion;

use crate::unit_harness::{VK_BLOB_A, VK_BLOB_B, bind_and_verify, vk_hash};

/// The circuit with its side-table written inline, declaring `minor`.
/// `Vec<Vec<u8>>` is an array of byte arrays in JSON.
fn text_with_vks(minor: u8) -> String {
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
           "version": {{ "major": 3, "minor": {minor} }},
           "inputs": [],
           "outputs": [],
           "do_communications_commitment": false,
           "verify_proof_vks": [{a}, {b}],
           "instructions": [{instructions}]
        }}"#,
        a = bytes(&VK_BLOB_A),
        b = bytes(&VK_BLOB_B),
        instructions = bind_and_verify(&[vk_hash(&VK_BLOB_A), vk_hash(&VK_BLOB_B)]),
    )
}

fn serialized(ir: &IrSource) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    tagged_serialize(ir, &mut bytes)?;
    Ok(bytes)
}

#[test]
fn load_accepts_ir_carrying_a_side_table() {
    let blobs = vec![VK_BLOB_A.to_vec(), VK_BLOB_B.to_vec()];

    // A document declaring the version its contents require must load, and
    // round-trip. `load` refuses `minor: 1` outright today.
    let ir = IrSource::load(text_with_vks(1).as_bytes())
        .expect("`minor: 1` must load; widen `load`'s match to `0..=1`");
    assert_eq!(ir.version, IrMinorVersion::V1);
    assert_eq!(ir.verify_proof_vks, blobs, "the side-table must survive");
    serialized(&ir).expect("a V1 carrying a side-table must serialize");

    // A document whose declared version contradicts its contents must not load
    // into something unserializable. Either answer is fine; silently producing a
    // value `serialize` refuses is not.
    if let Ok(ir) = IrSource::load(text_with_vks(0).as_bytes()) {
        serialized(&ir).expect(
            "`load` accepted `minor: 0` with a side-table and produced a value \
             `serialize` refuses; reject the inconsistency at load, or infer the \
             version from the contents",
        );
    }
}
