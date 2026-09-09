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

//! Unconsumed witnesses and keys are rejected rather than ignored.
//!
//! Duplicate keys are covered by `duplicate_vk_in_side_table_is_rejected`.

use transient_crypto::proofs::Zkir;

use crate::unit_harness::{
    VK_BLOB_A, bind_and_verify_off, expect_check_err, ir, ir_with_vks, preimage, vk_hash,
};

#[test]
fn surplus_witness_or_vk_is_rejected() {
    // A witness no `inner_proof` binds.
    let err = expect_check_err(&ir(""), preimage(1));
    assert!(err.contains("proof witnesses"), "got: {err}");

    // A key no `verify_proof` references.
    let err = expect_check_err(&ir_with_vks("", vec![VK_BLOB_A.to_vec()]), preimage(0));
    assert!(
        err.contains("verify_proof_vks") && err.contains("used"),
        "got: {err}"
    );

    // A guarded-off pair lets this test use a stub verifying key.
    let paired = ir_with_vks(
        &bind_and_verify_off(&[vk_hash(&VK_BLOB_A)]),
        vec![VK_BLOB_A.to_vec()],
    );
    let err = expect_check_err(&paired, preimage(2));
    assert!(err.contains("proof witnesses"), "got: {err}");

    // Controls with no surplus material.
    ir("").check(&preimage(0)).expect("empty circuit");
    paired
        .check(&preimage(1))
        .expect("one binding, one witness");
}
