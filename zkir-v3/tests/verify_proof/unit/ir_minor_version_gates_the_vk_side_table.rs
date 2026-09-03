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

//! `verify_proof_vks` is readable only on a `V1`, and a `V0` claiming to carry
//! one is refused at the point of writing.
//!
//! The binary encoding has no framing, so the version is the only thing telling a
//! reader whether the trailing side-table is there.

use serialize::{Deserializable, Serializable};

use midnight_zkir_v3::IrSource;
use midnight_zkir_v3::ir::IrMinorVersion;

use crate::unit_harness::{BIND_ONE, VK_BLOB_A, ir, ir_with_vks};

fn write(ir: &IrSource) -> Result<Vec<u8>, std::io::Error> {
    let mut bytes = Vec::new();
    Serializable::serialize(ir, &mut bytes)?;
    assert_eq!(
        bytes.len(),
        ir.serialized_size(),
        "serialized_size must agree with what serialize wrote"
    );
    Ok(bytes)
}

fn read(bytes: &[u8]) -> IrSource {
    Deserializable::deserialize(&mut { bytes }, 0).expect("deserialize IrSource")
}

#[test]
fn ir_minor_version_gates_the_vk_side_table() {
    // ---- V0, no side-table: the pre-existing shape, still readable ----
    let v0 = ir(BIND_ONE);
    assert_eq!(v0.version, IrMinorVersion::V0, "text IR parses as V0");
    assert!(v0.verify_proof_vks.is_empty());

    let back = read(&write(&v0).expect("a V0 with no side-table must write"));
    assert_eq!(back, v0, "a V0 must round-trip");
    assert!(
        back.verify_proof_vks.is_empty(),
        "a V0 payload must read back with an empty side-table, not consume what follows"
    );

    // ---- V1 carrying keys: round-trips intact ----
    let v1 = ir_with_vks(BIND_ONE, vec![VK_BLOB_A.to_vec()]);
    assert_eq!(v1.version, IrMinorVersion::V1);

    let bytes = write(&v1).expect("a V1 with a side-table must write");
    let back = read(&bytes);
    assert_eq!(back, v1, "a V1 must round-trip");
    assert_eq!(
        back.verify_proof_vks,
        vec![VK_BLOB_A.to_vec()],
        "the side-table must survive"
    );

    // The version really is what adds those bytes, rather than them riding along
    // on something else that happens to differ between the two values.
    let mut v1_empty = ir(BIND_ONE);
    v1_empty.version = IrMinorVersion::V1;
    assert!(
        write(&v1_empty)
            .expect("a V1 with no keys must write")
            .len()
            < bytes.len(),
        "a V1 carrying keys must be strictly larger than the same V1 without them"
    );

    // ---- V0 claiming a side-table: refused, not silently dropped ----
    let mut inconsistent = ir(BIND_ONE);
    inconsistent.verify_proof_vks = vec![VK_BLOB_A.to_vec()];
    assert_eq!(inconsistent.version, IrMinorVersion::V0);

    let err = write(&inconsistent)
        .expect_err("a V0 carrying a side-table has no encoding and must not be written");
    assert!(
        format!("{err}").contains("V1"),
        "the error should name the version required: {err}"
    );
}
