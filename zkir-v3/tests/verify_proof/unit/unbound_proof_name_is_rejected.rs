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

//! A `verify_proof` whose `proof` operand names something no `inner_proof`
//! bound is rejected.
//!
//! The two namespaces are separate: `inner_proof` binds into the proof map,
//! every other instruction writes to the value map. So a dangling `%p_n`, and a
//! name that exists but holds a *value* rather than a proof, are the same
//! lookup miss — and neither may fall through to a default or an empty proof.

use crate::unit_harness::{VK_BLOB_A, expect_check_err, ir_with_vks, preimage, vk_hash};

#[test]
fn unbound_proof_name_is_rejected() {
    // Bound `%p_0`, but the verification names `%p_1`.
    let err = expect_check_err(&verifying("%p_1"), preimage(1));
    assert!(
        err.contains("not an inner proof") && err.contains("p_1"),
        "got: {err}"
    );

    // A name from the value namespace, which `inner_proof` never writes to.
    let err = expect_check_err(&verifying("%v_0"), preimage(1));
    assert!(
        err.contains("not an inner proof") && err.contains("v_0"),
        "got: {err}"
    );
}

/// One `inner_proof` binding `%p_0`, and one `verify_proof` reading `proof`.
fn verifying(proof: &str) -> midnight_zkir_v3::IrSource {
    let instructions = format!(
        r#"{{ "op": "inner_proof", "output": "%p_0" }},
           {{ "op": "verify_proof", "vk_hash": "0x{hash}", "instance": ["0x7b"], "proof": "{proof}" }}"#,
        hash = vk_hash(&VK_BLOB_A),
    );
    ir_with_vks(&instructions, vec![VK_BLOB_A.to_vec()])
}
