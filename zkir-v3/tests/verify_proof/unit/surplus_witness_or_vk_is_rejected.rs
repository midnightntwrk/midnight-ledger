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

//! Witnesses or keys the circuit never consumes are refused by the count
//! checks, not ignored.
//!
//! Silently ignoring either is the dangerous behaviour: an unbound witness is a
//! prover asserting something the circuit never checks, and an unreferenced key
//! is material with no bearing on what is verified. A blob listed *twice* is a
//! separate case, in `duplicate_vk_in_side_table_is_rejected`.

use transient_crypto::proofs::Zkir;

use crate::unit_harness::{BIND_ONE, VK_BLOB_A, expect_check_err, ir, ir_with_vks, preimage};

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

    // Surplus alongside a binding that does consume one.
    let err = expect_check_err(&ir(BIND_ONE), preimage(2));
    assert!(err.contains("proof witnesses"), "got: {err}");

    // Controls: nothing surplus passes outright.
    ir("").check(&preimage(0)).expect("empty circuit");
    ir(BIND_ONE)
        .check(&preimage(1))
        .expect("one binding, one witness");
}
