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

//! A circuit that cannot be synthesized is reported, not fatal.
//!
//! The blob here hashes to the `vk_hash` naming it, so resolution is satisfied
//! and the bytes are handed on; only reading them as a key fails. `check()`
//! reports that gracefully and every entry point should agree.
//!
//! `IrSource::k()` does not: `Zkir::k()` returns `u8` with no error path and is
//! `optimal_k(self)`, which unwraps the synthesis result, so the process aborts.
//! `keygen_vk` calls `k()` first, which makes it reachable.
//!
//! Asserting only the absence of a panic is deliberate — giving `k()` an error
//! path is a signature change, and this should hold however that is resolved.
//!
//! KNOWN GAP, hence `#[ignore]`: `k()` still aborts. `Zkir::k` returns a bare
//! `u8`, so `optimal_k` has nowhere to report a synthesis failure and unwraps
//! inside `cost_model`. Unchanged from before this suite was ported. Drop the
//! `#[ignore]` once `k()` can fail.

use transient_crypto::proofs::Zkir;

use crate::unit_harness::{
    VK_BLOB_A, bind_and_verify_one, expect_check_err, ir_with_vks, preimage, vk_hash,
};

#[test]
#[ignore = "KNOWN GAP: IrSource::k() aborts on an unsynthesizable circuit instead of reporting"]
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
        "k() must report an unsynthesizable circuit, not panic"
    );
}
