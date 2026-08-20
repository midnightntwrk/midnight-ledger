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

//! A side-table blob that does not hash to its instruction's `vk_hash` is
//! refused rather than used positionally.
//!
//! `vk_hash` is what binds a circuit to the key it was compiled against, and
//! that is only worth anything if resolution checks it. The side-table here is
//! fully populated and the right length — only the *contents* are wrong, which
//! positional resolution would accept.

use sha2::Digest;

use crate::unit_harness::{
    VK_BLOB_A, VK_BLOB_B, bind_and_verify_one, expect_check_err, ir_with_vks, preimage, vk_hash,
};

#[test]
fn vk_hash_mismatch_is_rejected() {
    let correct = vk_hash(&VK_BLOB_A);

    // The blob was swapped for a different key.
    let ir = ir_with_vks(&bind_and_verify_one(&correct), vec![VK_BLOB_B.to_vec()]);
    let err = expect_check_err(&ir, preimage(1));
    assert!(
        err.contains("no verifying key") && err.contains(&correct),
        "got: {err}"
    );

    // The declared digest was altered instead.
    let mut altered = sha2::Sha256::digest(VK_BLOB_A).to_vec();
    altered[0] ^= 0x01;
    let altered = const_hex::encode(&altered);
    let ir = ir_with_vks(&bind_and_verify_one(&altered), vec![VK_BLOB_A.to_vec()]);
    let err = expect_check_err(&ir, preimage(1));
    assert!(
        err.contains("no verifying key") && err.contains(&altered),
        "got: {err}"
    );

    // Control: a matching pair resolves, leaving failure to key parsing.
    let ir = ir_with_vks(&bind_and_verify_one(&correct), vec![VK_BLOB_A.to_vec()]);
    let err = expect_check_err(&ir, preimage(1));
    assert!(
        err.contains("verifying key") && !err.contains("no verifying key"),
        "got: {err}"
    );
}
