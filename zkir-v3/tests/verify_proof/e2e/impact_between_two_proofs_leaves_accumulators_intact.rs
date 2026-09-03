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

//! An `Impact` between two `verify_proof`s leaves both accumulators untouched
//! and stays out of them, guard on or off.
//!
//! Carrying accumulators on the proof rather than at offsets in the statement
//! made the two independent by construction, and this is what pins that they
//! really are. The blocks come off the head of the public-input vector, so the
//! statement handed back is the binding input followed by the `Impact`'s slots
//! and nothing else — an `Impact` between the verifications must not appear in
//! a block, and must not change how many blocks there are.
//!
//! The guard is a circuit input, so both states run against one circuit and one
//! keygen — they cannot disagree about layout for any reason but the branch
//! taken. Guarded on, each declared input publishes its resolved value and
//! consumes a transcript entry; guarded off, each publishes a *zero* and
//! consumes none. The statement is the same width either way, so they agree
//! only if the off branch really pads.
//!
//! Skips are asserted too: one entry per `Impact`, none per `verify_proof`.
//! Those indices are now read against a statement the accumulators have already
//! been stripped from, so an entry per verification would not merely be
//! redundant — it would point the verifier at the wrong slot.

use midnight_zkir_v3::IrSource;
use midnight_zkir_v3::ir_instructions::verify_proof::verify_proof_offcircuit;
use transient_crypto::curve::Fr;
use transient_crypto::proofs::accumulator_pi_len;

use crate::e2e_harness::{
    BINDING_INPUT, InnerProof, instance_json, outer_ir_with, outer_keygen, outer_preimage_with,
    outer_prove_with_skips, outer_verify, rsa_inner_proof, scalar_inner_proof, test_rng,
    vk_hash_hex,
};

/// How many inputs the interleaved `Impact` declares. More than one, so a branch
/// padding a fixed single slot would also be caught.
const IMPACT_INPUTS: usize = 3;

/// The value each declared input discloses when the guard is on.
const DISCLOSED: u64 = 42;

#[actix_rt::test]
#[ignore = "outer verifier circuit needs a high-k SRS not available in CI"]
async fn impact_between_two_proofs_leaves_accumulators_intact() {
    let mut rng = test_rng();

    let inner = [
        rsa_inner_proof(&mut rng).await,
        scalar_inner_proof(&mut rng).await,
    ];
    let ir = interleaved_ir(&inner[0], &inner[1]);

    // Counted once: the guard is a witness, so the block count cannot depend on
    // it, and neither can the interleaved `Impact`.
    let acc_len = accumulator_pi_len();
    assert_eq!(
        ir.accumulator_count(),
        2,
        "two verify_proof instructions expose two accumulators, Impact or not"
    );

    let (pk, vk) = outer_keygen(&ir, "2 verify_proof, 1 interleaved impact").await;

    for guard in [true, false] {
        println!("--- guard {} ---", if guard { "on" } else { "off" });

        // Guarded on, each input publishes `DISCLOSED` and consumes a matching
        // transcript entry; guarded off, each publishes zero and consumes none.
        let (inputs, transcript, expected_middle) = if guard {
            (
                vec![Fr::from(1u64)],
                vec![Fr::from(DISCLOSED); IMPACT_INPUTS],
                Fr::from(DISCLOSED),
            )
        } else {
            (vec![Fr::from(0u64)], vec![], Fr::from(0u64))
        };

        let preimage = outer_preimage_with(
            inner.iter().map(|i| i.proof.clone()).collect(),
            inputs,
            transcript,
        );
        let (proof, pis, skips) =
            outer_prove_with_skips(&ir, pk.clone(), &preimage, &mut rng).await;

        // One entry, for the one `Impact`. Two `verify_proof`s in the same
        // circuit must not add any.
        let expected_skips = if guard {
            vec![None]
        } else {
            vec![Some(IMPACT_INPUTS)]
        };
        assert_eq!(
            skips, expected_skips,
            "guard {guard}: one skip entry per Impact, none per verify_proof"
        );

        // Two blocks on the proof, and a statement holding only the binding
        // input and the Impact's slots.
        assert_eq!(
            proof.accumulators.len(),
            2,
            "guard {guard}: both accumulators must be carried"
        );
        assert!(
            proof.accumulators.iter().all(|b| b.len() == acc_len),
            "guard {guard}: every block is one accumulator wide"
        );
        assert_eq!(
            pis.len(),
            1 + IMPACT_INPUTS,
            "guard {guard}: the statement is the binding input plus the Impact's slots"
        );
        assert_eq!(
            pis[0],
            Fr::from(BINDING_INPUT),
            "guard {guard}: no accumulator field leaked into the statement"
        );

        let middle = &pis[1..1 + IMPACT_INPUTS];
        assert!(
            middle.iter().all(|f| *f == expected_middle),
            "guard {guard}: the interleaved slots should all be {expected_middle:?}, got {middle:?}"
        );

        for (i, inner) in inner.iter().enumerate() {
            let want: Vec<Fr> =
                verify_proof_offcircuit(&inner.vk_blob, &inner.pis, &inner.proof, true)
                    .expect("off-circuit preparation")
                    .into_iter()
                    .map(Fr)
                    .collect();
            assert_eq!(
                proof.accumulators[i], want,
                "guard {guard}: accumulator {i} must match off-circuit preparation"
            );
        }

        outer_verify(&vk, &proof, pis);
    }
}

/// `inner_proof`, `inner_proof`, `verify_proof`, `impact`, `verify_proof` — the
/// `Impact` deliberately between the two verifications, which is the shape the
/// harness's `outer_ir_for_all` cannot emit. The `Impact`'s guard is the
/// circuit's single input, so one circuit serves both branches; the two
/// verifications are unguarded, since it is the `Impact` that varies here.
fn interleaved_ir(a: &InnerProof, b: &InnerProof) -> IrSource {
    let impact_operands = vec![format!(r#""0x{DISCLOSED:02x}""#); IMPACT_INPUTS].join(", ");
    let instructions = format!(
        r#"{{ "op": "inner_proof", "guard": "0x01", "output": "%p_0" }},
           {{ "op": "inner_proof", "guard": "0x01", "output": "%p_1" }},
           {{
               "op": "verify_proof",
               "guard": "0x01",
               "vk_hash": "0x{hash_a}",
               "instance": [{instance_a}],
               "proof": "%p_0"
           }},
           {{ "op": "impact", "guard": "%v_0", "inputs": [{impact_operands}] }},
           {{
               "op": "verify_proof",
               "guard": "0x01",
               "vk_hash": "0x{hash_b}",
               "instance": [{instance_b}],
               "proof": "%p_1"
           }}"#,
        hash_a = vk_hash_hex(&a.vk_blob),
        hash_b = vk_hash_hex(&b.vk_blob),
        instance_a = instance_json(&a.pis),
        instance_b = instance_json(&b.pis),
    );
    outer_ir_with(
        r#"{ "name": "%v_0", "type": "Scalar<BLS12-381>" }"#,
        false,
        &instructions,
        vec![a.vk_blob.clone(), b.vk_blob.clone()],
    )
}
