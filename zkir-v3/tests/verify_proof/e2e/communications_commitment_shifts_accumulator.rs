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

//! With `do_communications_commitment` set, the commitment takes the slot after
//! the binding input and the accumulator lands one further along — in the
//! circuit's real output, not just in `accumulator_offsets()`.
//!
//! `accumulator_offsets_account_for_preceding_inputs` covers the arithmetic,
//! but that function only *predicts* where the circuit will publish. The
//! circuit lays its public inputs out in separate code, and every other e2e
//! case here runs with the flag `false` — so this branch of the prediction has
//! never been checked against reality. A disagreement would put the accumulator
//! one slot off, which is exactly the kind of error the pure-function test
//! cannot see.

use midnight_zkir_v3::ir_instructions::verify_proof::{
    accumulator_pi_len, verify_proof_offcircuit,
};
use transient_crypto::curve::Fr;

use crate::e2e_harness::{
    BINDING_INPUT, instance_json, outer_ir_with, outer_keygen, outer_preimage_committing,
    outer_prove, outer_verify, scalar_inner_proof, test_rng, vk_hash_hex,
};

/// The opening the preimage commits with. Arbitrary; the commitment itself is
/// derived from it, and is what must land in slot 1.
const OPENING: u64 = 22;

#[actix_rt::test]
#[ignore = "outer verifier circuit needs a high-k SRS not available in CI"]
async fn communications_commitment_shifts_accumulator() {
    let mut rng = test_rng();

    let inner = scalar_inner_proof(&mut rng).await;
    let instructions = format!(
        r#"{{ "op": "inner_proof", "output": "%p_0" }},
           {{
               "op": "verify_proof",
               "vk_hash": "0x{hash}",
               "instance": [{instance}],
               "proof": "%p_0"
           }}"#,
        hash = vk_hash_hex(&inner.vk_blob),
        instance = instance_json(&inner.pis),
    );
    let outer_ir = outer_ir_with("", true, &instructions, vec![inner.vk_blob.clone()]);

    // Predicted: binding input, then the commitment, then the accumulator.
    let acc_len = accumulator_pi_len();
    let offsets = outer_ir.accumulator_offsets();
    assert_eq!(
        offsets,
        vec![2],
        "the commitment must push the accumulator one slot past the binding input"
    );

    let (outer_pk, outer_vk) = outer_keygen(&outer_ir, "1 verify_proof, commitment").await;

    let preimage = outer_preimage_committing(vec![inner.proof.clone()], Fr::from(OPENING));
    let commitment = preimage
        .communications_commitment
        .expect("harness set the commitment")
        .0;
    let (outer_proof, outer_pis) = outer_prove(&outer_ir, outer_pk, &preimage, &mut rng).await;

    // Reality: the layout the circuit actually published.
    assert_eq!(
        outer_pis.len(),
        2 + acc_len,
        "statement vector should be binding input, commitment, accumulator"
    );
    assert_eq!(
        outer_pis[0],
        Fr::from(BINDING_INPUT),
        "slot 0 is still the binding input"
    );
    assert_eq!(
        outer_pis[1], commitment,
        "slot 1 is the commitment — it is what displaces the accumulator"
    );

    let expected: Vec<Fr> = verify_proof_offcircuit(&inner.vk_blob, &inner.pis, &inner.proof)
        .expect("off-circuit preparation")
        .into_iter()
        .map(Fr)
        .collect();
    assert_eq!(
        &outer_pis[offsets[0]..offsets[0] + acc_len],
        expected.as_slice(),
        "the accumulator must sit at the shifted offset the IR predicted"
    );

    outer_verify(&outer_vk, &outer_proof, outer_pis);
}
