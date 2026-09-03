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

//! A guarded-off `verify_proof` exposes the trivial accumulator whatever bytes
//! the prover supplied, and the outer proof still verifies.
//!
//! Reachable because the guards are separate operands: `inner_proof` on consumes
//! a witness, `verify_proof` off discards it. Two witnesses through one circuit —
//! a genuine proof and bytes that are not one — must be indistinguishable in the
//! output.

use midnight_zkir_v3::IrSource;
use midnight_zkir_v3::ir_instructions::decidable::trivial_accumulator_pis;
use transient_crypto::curve::Fr;

use crate::e2e_harness::{
    InnerProof, instance_json, outer_ir_with, outer_keygen, outer_preimage_with, outer_prove,
    outer_verify, scalar_inner_proof, test_rng, vk_hash_hex,
};

/// `inner_proof` and `verify_proof` taking their guards from separate inputs,
/// so one circuit covers every combination.
fn split_guard_ir(inner: &InnerProof) -> IrSource {
    let instructions = format!(
        r#"{{ "op": "inner_proof", "guard": "%g_i", "output": "%p_0" }},
           {{ "op": "verify_proof", "guard": "%g_v", "vk_hash": "0x{hash}",
              "instance": [{instance}], "proof": "%p_0" }}"#,
        hash = vk_hash_hex(&inner.vk_blob),
        instance = instance_json(&inner.pis),
    );
    outer_ir_with(
        r#"{ "name": "%g_i", "type": "Scalar<BLS12-381>" },
           { "name": "%g_v", "type": "Scalar<BLS12-381>" }"#,
        false,
        &instructions,
        vec![inner.vk_blob.clone()],
    )
}

#[actix_rt::test]
#[ignore = "outer verifier circuit needs a high-k SRS not available in CI"]
async fn guarded_off_verify_proof_is_witness_independent() {
    let mut rng = test_rng();
    let inner = scalar_inner_proof(&mut rng).await;
    let ir = split_guard_ir(&inner);
    let (pk, vk) = outer_keygen(&ir, "split guards, verify_proof off").await;

    let trivial: Vec<Fr> = trivial_accumulator_pis().into_iter().map(Fr).collect();

    // Bytes that are not a proof, and the same length as one so the difference
    // cannot be dismissed as a size check somewhere.
    let garbage = vec![0xABu8; inner.proof.len()];
    assert_ne!(garbage, inner.proof, "the two witnesses must differ");

    let mut exposed = Vec::new();
    for (label, witness) in [
        ("a genuine inner proof", inner.proof.clone()),
        ("bytes that are not a proof", garbage),
    ] {
        println!("--- guarded off, witness: {label} ---");

        // `%g_i` on, so the witness is consumed; `%g_v` off, so it is discarded.
        let preimage =
            outer_preimage_with(vec![witness], vec![Fr::from(1u64), Fr::from(0u64)], vec![]);
        let (proof, pis) = outer_prove(&ir, pk.clone(), &preimage, &mut rng).await;

        assert_eq!(
            proof.accumulators.len(),
            1,
            "{label}: a guarded-off verify_proof still occupies its block"
        );
        assert_eq!(
            proof.accumulators[0], trivial,
            "{label}: the exposed accumulator must be the trivial one"
        );

        // The pairing holds by construction, so the outer proof verifies.
        outer_verify(&vk, &proof, pis);
        exposed.push(proof.accumulators[0].clone());
    }

    // The crux: the witness had no influence on anything the verifier sees.
    assert_eq!(
        exposed[0], exposed[1],
        "a genuine proof and garbage must be indistinguishable once guarded off"
    );
}
