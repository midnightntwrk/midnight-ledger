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

//! `accumulator_offsets()` places each accumulator past everything that occupies
//! public inputs ahead of it: the binding input, the communications commitment
//! if present, and any `Impact`'s inputs.

use midnight_zkir_v3::IrSource;
use midnight_zkir_v3::ir_instructions::verify_proof::accumulator_pi_len;

use crate::unit_harness::{VK_BLOB_A, VK_BLOB_B, bind_and_verify, ir_with_inputs, vk_hash};

#[test]
fn accumulator_offsets_account_for_preceding_inputs() {
    let acc_len = accumulator_pi_len();

    // Binding input (1) + two Impact inputs.
    assert_eq!(
        impact_then_two_verifications(false, 2).accumulator_offsets(),
        vec![3, 3 + acc_len]
    );

    // The communications commitment takes one more slot ahead of them.
    assert_eq!(
        impact_then_two_verifications(true, 2).accumulator_offsets(),
        vec![4, 4 + acc_len]
    );

    // The Impact contributes its input count, not a constant.
    assert_eq!(
        impact_then_two_verifications(false, 5).accumulator_offsets(),
        vec![6, 6 + acc_len]
    );

    // `inner_proof` publishes nothing, so with no Impact they start at slot 1.
    assert_eq!(
        impact_then_two_verifications(false, 0).accumulator_offsets(),
        vec![1, 1 + acc_len]
    );
}

/// An `Impact` declaring `impact_inputs` public inputs, then two
/// `verify_proof`s. Nothing resolves the VK hashes here.
fn impact_then_two_verifications(
    do_communications_commitment: bool,
    impact_inputs: usize,
) -> IrSource {
    let impact_operands = vec!["\"%v_0\""; impact_inputs].join(", ");
    let instructions = format!(
        r#"{{ "op": "impact", "guard": "%v_0", "inputs": [{impact_operands}] }},
           {verifications}"#,
        verifications = bind_and_verify(&[vk_hash(&VK_BLOB_A), vk_hash(&VK_BLOB_B)]),
    );
    ir_with_inputs(
        r#"{ "name": "%v_0", "type": "Scalar<BLS12-381>" }"#,
        do_communications_commitment,
        &instructions,
    )
}
