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

//! Reusing one inner proof must not expose the same accumulator twice, or the
//! reuse is visible on-chain.
//!
//! FAILING: it is identical, both across two transactions of one circuit and
//! across two *structurally different* circuits with different verifying keys.
//! The accumulator is a deterministic function of `(vk_blob, instance, inner
//! proof, guard)` — none of which is a property of the outer circuit — so
//! nothing about the enclosing proof or contract perturbs it.
//!
//! Re-proving the inner statement does move it, asserted as the control, and is
//! currently the only thing that separates two uses.
//!
//! If inner proofs are single-use by design this is intended, and the assertion
//! should be inverted to pin the determinism instead.

use midnight_curves::Fq;
use midnight_zkir_v3::IrSource;
use midnight_zkir_v3::ir_instructions::decider::accumulator_pis;
use midnight_zkir_v3::ir_instructions::verify_proof::verify_proof_offcircuit;
use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;
use transient_crypto::curve::Fr;

use crate::e2e_harness::{
    instance_json, outer_ir_for, outer_ir_with, outer_keygen, outer_preimage, outer_prove,
    outer_verify, scalar_inner_proof, test_rng, vk_hash_hex,
};

/// A second outer circuit naming the same inner proof, but structurally
/// different: an extra `Impact` publishing two values the first never had, so
/// it compiles to a different verifying key.
fn other_circuit(vk_blob: &[u8], pis: &[Fq]) -> IrSource {
    let instructions = format!(
        r#"{{ "op": "inner_proof", "guard": "0x01", "output": "%p_0" }},
           {{ "op": "impact", "guard": "0x01", "inputs": ["0x2a", "0x2b"] }},
           {{ "op": "verify_proof", "guard": "0x01", "vk_hash": "0x{hash}",
              "instance": [{instance}], "proof": "%p_0" }}"#,
        hash = vk_hash_hex(vk_blob),
        instance = instance_json(pis),
    );
    outer_ir_with("", false, &instructions, vec![vk_blob.to_vec()])
}

#[actix_rt::test]
#[ignore = "outer verifier circuit needs a high-k SRS not available in CI"]
async fn accumulators_differ_when_an_inner_proof_is_reused() {
    let mut rng = test_rng();
    let inner = scalar_inner_proof(&mut rng).await;

    // Control first, so it runs whatever the assertions below do: a freshly
    // proven inner statement *does* move the accumulator. Without this, the
    // failure could be dismissed as "nothing ever changes".
    let mut rng2 = ChaCha20Rng::from_seed([99; 32]);
    let reproven = scalar_inner_proof(&mut rng2).await;
    assert_ne!(
        reproven.proof, inner.proof,
        "the control needs genuinely different proof bytes"
    );
    let from_original = accumulator_pis(
        &verify_proof_offcircuit(&inner.vk_blob, &inner.pis, &inner.proof, true)
            .expect("preparation"),
    );
    let from_reproven = accumulator_pis(
        &verify_proof_offcircuit(&reproven.vk_blob, &reproven.pis, &reproven.proof, true)
            .expect("preparation"),
    );
    assert_ne!(
        from_original, from_reproven,
        "re-proving the inner statement must move the accumulator"
    );

    // Two transactions over one circuit.
    let ir_a = outer_ir_for(&inner.vk_blob, &inner.pis);
    let (pk_a, vk_a) = outer_keygen(&ir_a, "circuit A, two transactions").await;
    let mut a_blocks = Vec::new();
    for tx in 0..2 {
        let (proof, pis) = outer_prove(
            &ir_a,
            pk_a.clone(),
            &outer_preimage(inner.proof.clone()),
            &mut rng,
        )
        .await;
        outer_verify(&vk_a, &proof, pis);
        println!("circuit A, transaction {tx}");
        a_blocks.push(proof.accumulators[0].clone());
    }

    // A different circuit, different verifying key, same inner proof.
    let ir_b = other_circuit(&inner.vk_blob, &inner.pis);
    let (pk_b, vk_b) = outer_keygen(&ir_b, "circuit B, extra Impact").await;
    let mut preimage_b = outer_preimage(inner.proof.clone());
    preimage_b.public_transcript_inputs = vec![Fr::from(0x2au64), Fr::from(0x2bu64)];
    let (proof_b, pis_b) = outer_prove(&ir_b, pk_b, &preimage_b, &mut rng).await;
    outer_verify(&vk_b, &proof_b, pis_b);
    let b_block = proof_b.accumulators[0].clone();

    let same_circuit = a_blocks[0] == a_blocks[1];
    let across_circuits = a_blocks[0] == b_block;
    println!("same circuit, two transactions -> identical? {same_circuit}");
    println!("different circuits             -> identical? {across_circuits}");

    assert!(
        !same_circuit && !across_circuits,
        "reusing one inner proof exposed the same accumulator (same circuit: {same_circuit}, \
         across different circuits: {across_circuits}). It is a deterministic function of \
         (vk_blob, instance, inner proof, guard), none of which belongs to the outer circuit, \
         so reuse is linkable on-chain — including across unrelated contracts. Blind it, or \
         invert this test if inner proofs are single-use by design.",
    );
}
