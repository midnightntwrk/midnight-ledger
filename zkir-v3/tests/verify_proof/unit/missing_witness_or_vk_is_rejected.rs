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

//! Fewer `inner_proofs` or `verify_proof_vks` than the circuit needs bails
//! during preprocessing rather than indexing out of bounds.
//!
//! The control matters: every assertion here is on an `Err`, so "it errored"
//! proves nothing on its own. With both shortfalls made good the same fixture
//! gets strictly further — to parsing a key — which is what ties each error to
//! its claimed cause.

use crate::unit_harness::{
    VK_BLOB_A, VK_BLOB_B, bind_and_verify, expect_check_err, ir_with_vks, preimage, vk_hash,
};

#[test]
fn missing_witness_or_vk_is_rejected() {
    let both = vec![VK_BLOB_A.to_vec(), VK_BLOB_B.to_vec()];
    let instructions = bind_and_verify(&[vk_hash(&VK_BLOB_A), vk_hash(&VK_BLOB_B)]);

    // Too few witnesses: the i-th binding bails mid-loop, naming its index.
    let ir = ir_with_vks(&instructions, both.clone());
    let err = expect_check_err(&ir, preimage(1));
    assert!(
        err.contains("Not enough proof witnesses") && err.contains("index 1"),
        "got: {err}"
    );

    let err = expect_check_err(&ir, preimage(0));
    assert!(
        err.contains("Not enough proof witnesses") && err.contains("index 0"),
        "got: {err}"
    );

    // Too few VKs: resolution refuses before any instruction runs.
    let ir = ir_with_vks(&instructions, vec![VK_BLOB_A.to_vec()]);
    let err = expect_check_err(&ir, preimage(2));
    assert!(
        err.contains("no verifying key") && err.contains("vk_hash"),
        "got: {err}"
    );

    // Control: complete inputs fail later, at reading the key.
    let ir = ir_with_vks(&instructions, both);
    let err = expect_check_err(&ir, preimage(2));
    assert!(
        err.contains("verifying key") && !err.contains("Not enough proof witnesses"),
        "got: {err}"
    );
}
