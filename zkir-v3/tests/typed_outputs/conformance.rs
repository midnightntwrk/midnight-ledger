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

use crate::common::assert_check_err_contains;

#[actix_rt::test]
async fn output_arity_mismatch() {
    // Signature declares 1 output; Output instruction supplies 2.
    // Expect the error to mention "Output" and the two arities.
    assert_check_err_contains(
        r#"{
           "version": { "major": 3, "minor": 0 },
           "inputs": [
              { "name": "%v_0", "type": "Scalar<BLS12-381>" }
           ],
           "outputs": ["Scalar<BLS12-381>"],
           "do_communications_commitment": false,
           "instructions": [
               { "op": "output", "vals": ["%v_0", "%v_0"] }
           ]
        }"#,
        vec![1.into()],
        &["Output", "1", "2"],
    );
}

#[actix_rt::test]
async fn output_operand_type_mismatch() {
    // Signature declares Point<Jubjub> at position 0; the operand resolves to
    // a Native value. Expect the error to identify the position and both type
    // names.
    assert_check_err_contains(
        r#"{
           "version": { "major": 3, "minor": 0 },
           "inputs": [
              { "name": "%v_0", "type": "Scalar<BLS12-381>" }
           ],
           "outputs": ["Point<Jubjub>"],
           "do_communications_commitment": false,
           "instructions": [
               { "op": "output", "vals": ["%v_0"] }
           ]
        }"#,
        vec![1.into()],
        &["Output position", "Native", "JubjubPoint"],
    );
}
