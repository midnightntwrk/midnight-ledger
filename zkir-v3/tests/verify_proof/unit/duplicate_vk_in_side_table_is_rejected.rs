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

//! The same VK blob listed twice in `verify_proof_vks` is refused, even when an
//! instruction does reference it.
//!
//! The side-table is indexed by digest, so a second copy of a blob can never be
//! the entry any instruction resolves to — it is surplus by construction, and
//! the index would silently collapse the pair. Distinct from
//! `surplus_witness_or_vk_is_rejected`, which is about keys and witnesses the
//! circuit never consumes at all: here the key *is* consumed, just listed twice.

use crate::unit_harness::{
    VK_BLOB_A, bind_and_verify_one, expect_check_err, ir_with_vks, preimage, vk_hash,
};

#[test]
fn duplicate_vk_in_side_table_is_rejected() {
    let duplicated = ir_with_vks(
        &bind_and_verify_one(&vk_hash(&VK_BLOB_A)),
        vec![VK_BLOB_A.to_vec(), VK_BLOB_A.to_vec()],
    );
    let err = expect_check_err(&duplicated, preimage(1));
    assert!(err.contains("duplicate verifying key"), "got: {err}");
}
