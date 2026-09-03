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

//! `verify_proof` and the `inner_proof` feeding it must carry the same guard.
//!
//! The IR documents both with a `WARNING` that they must match, and nothing
//! enforces it. A guard is an ordinary operand, so an author writing two
//! different variables hands a prover independent runtime control of them.
//!
//! FAILING: the mismatch is accepted today. A witness is consumed and the
//! accumulator it would have produced is discarded for the trivial one, so a
//! prover fills a slot nothing checks.
//!
//! The check belongs here rather than in `compact`, so it is not contingent on
//! one compiler's behaviour: every consumer of an `IrSource` gets it, whichever
//! toolchain produced the IR or whether one did.
//!
//! Scope, stated honestly: this is an authoring guard, not consensus
//! validation. A node never sees an `IrSource` — it verifies against a stored
//! `VerifierKey`, and `preprocess` runs prover-side — so there is no hostile-IR
//! submission path, and an author who wants to ship an inconsistent circuit can
//! bypass any check here anyway. What it buys is an honest mistake caught
//! before the circuit its users depend on is deployed.

use std::borrow::Cow;

use midnight_zkir_v3::IrSource;
use midnight_zkir_v3::ir::IrMinorVersion;
use transient_crypto::curve::Fr;
use transient_crypto::proofs::{InnerProofWitness, KeyLocation, ProofPreimage, Zkir};

use crate::unit_harness::{BINDING_INPUT, VK_BLOB_A, ir_with_inputs, vk_hash};

/// The pair, each instruction taking its guard from its own circuit input.
fn circuit() -> IrSource {
    let instructions = format!(
        r#"{{ "op": "inner_proof", "guard": "%g_i", "output": "%p_0" }},
           {{ "op": "verify_proof", "guard": "%g_v", "vk_hash": "0x{hash}",
              "instance": ["0x7b"], "proof": "%p_0" }}"#,
        hash = vk_hash(&VK_BLOB_A),
    );
    let mut ir = ir_with_inputs(
        r#"{ "name": "%g_i", "type": "Scalar<BLS12-381>" },
           { "name": "%g_v", "type": "Scalar<BLS12-381>" }"#,
        false,
        &instructions,
    );
    ir.version = IrMinorVersion::V1;
    ir.verify_proof_vks = vec![VK_BLOB_A.to_vec()];
    ir
}

fn preimage(g_inner: u64, g_verify: u64, witnesses: usize) -> ProofPreimage {
    ProofPreimage {
        binding_input: Fr::from(BINDING_INPUT),
        communications_commitment: None,
        inputs: vec![Fr::from(g_inner), Fr::from(g_verify)],
        private_transcript: vec![],
        public_transcript_inputs: vec![],
        public_transcript_outputs: vec![],
        proof_witnesses: (0..witnesses)
            .map(|i| InnerProofWitness::Direct(vec![i as u8; 8]))
            .collect(),
        key_location: KeyLocation(Cow::Borrowed("builtin")),
    }
}

fn err(ir: &IrSource, p: ProofPreimage) -> String {
    format!("{:#}", ir.check(&p).expect_err("expected a rejection"))
}

#[test]
fn guard_mismatch_between_the_pair_is_rejected() {
    let ir = circuit();

    // Guards agreeing and off: nothing consumed, VK never read.
    ir.check(&preimage(0, 0, 0)).expect("both off");

    // Guards agreeing and on: the VK is read, and the filler blob fails to
    // parse — which is what shows the guarded-on path reaches verification.
    let e = err(&ir, preimage(1, 1, 1));
    assert!(
        e.contains("verifying key"),
        "guarded-on must reach the VK: {e}"
    );

    // The witness is genuinely consumed under a mismatch: withhold it and the
    // count check bites.
    let e = err(&ir, preimage(1, 0, 0));
    assert!(e.contains("proof witnesses"), "expected a count error: {e}");

    // The defect: a consumed witness that nothing verifies must not be accepted.
    ir.check(&preimage(1, 0, 1)).expect_err(
        "inner_proof on with verify_proof off consumes a witness nothing checks; \
         reject the mismatch in `IrSource::preprocess`, alongside the \
         proof-witness count check",
    );
}
