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

//! Two `inner_proof` instructions binding the same name are rejected.
//!
//! The proof map is keyed by name, so the second binding overwrites the first —
//! but both advance the witness cursor, so the end-of-loop count check sees
//! every witness consumed and passes. The result is a witness slot a prover
//! fills and nothing checks.
//!
//! The shadowed proof feeds no constraint, so it cannot make a false statement
//! true. It is still a circuit no compiler should emit.

use transient_crypto::proofs::Zkir;

use crate::unit_harness::{expect_check_err, ir, preimage};

const BIND_TWICE: &str = r#"{ "op": "inner_proof", "output": "%p_0" },
                            { "op": "inner_proof", "output": "%p_0" }"#;

#[test]
fn duplicate_inner_proof_binding_is_rejected() {
    let err = expect_check_err(&ir(BIND_TWICE), preimage(2));
    assert!(
        err.contains("%p_0") || err.to_lowercase().contains("bound"),
        "the error should name the rebound proof: {err}"
    );

    // The count check still governs independently: one witness for two bindings
    // is short either way.
    let err = expect_check_err(&ir(BIND_TWICE), preimage(1));
    assert!(
        err.contains("Not enough proof witnesses") || err.contains("%p_0"),
        "got: {err}"
    );

    // Control: a single binding with its witness is fine.
    ir(r#"{ "op": "inner_proof", "output": "%p_0" }"#)
        .check(&preimage(1))
        .expect("one binding, one witness");
}
