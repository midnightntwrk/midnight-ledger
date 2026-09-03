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

//! A blob that hashes to its instruction's `vk_hash` but is not a verifying key
//! is reported by `check()`, naming the unreadable key.
//!
//! FAILING: `IrSource::k()` aborts on the same circuit. `Zkir::k` returns a bare
//! `u8` with no error path, so `optimal_k` unwraps the synthesis result inside
//! `cost_model` and the process dies. Fixing it is a trait signature change, so
//! the assertion is only that it does not panic — it should hold however that is
//! resolved. Deployer-side tooling only; nothing node-side calls `k()`.

use transient_crypto::proofs::Zkir;

use crate::unit_harness::{
    VK_BLOB_A, bind_and_verify_one, expect_check_err, ir_with_vks, preimage, vk_hash,
};

#[test]
fn unsynthesizable_circuit_is_reported_gracefully() {
    let ir = ir_with_vks(
        &bind_and_verify_one(&vk_hash(&VK_BLOB_A)),
        vec![VK_BLOB_A.to_vec()],
    );

    // The graceful path, for contrast: same circuit, same blob.
    let err = expect_check_err(&ir, preimage(1));
    assert!(
        err.contains("verifying key"),
        "check() should name the unreadable key: {err}"
    );

    let k = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| ir.k()));
    assert!(
        k.is_ok(),
        "k() must report an unsynthesizable circuit, not abort the process"
    );
}
