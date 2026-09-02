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

//! `inner_proof` consumes from `ProofPreimage::proof_witnesses` only when its
//! guard is on, so a caller never has to pad the vector for a branch it did not
//! take.

use std::borrow::Cow;

use midnight_zkir_v3::IrSource;
use transient_crypto::curve::Fr;
use transient_crypto::proofs::{InnerProofWitness, KeyLocation, ProofPreimage, Zkir};

fn preimage(guards: [u64; 2], proof_witnesses: Vec<InnerProofWitness>) -> ProofPreimage {
    ProofPreimage {
        binding_input: Fr::from(7u64),
        communications_commitment: None,
        inputs: guards.into_iter().map(Fr::from).collect(),
        private_transcript: vec![],
        public_transcript_inputs: vec![],
        public_transcript_outputs: vec![],
        proof_witnesses,
        key_location: KeyLocation(Cow::Borrowed("builtin")),
    }
}

fn load(instructions: &str) -> IrSource {
    let ir_json = format!(
        r#"{{
           "version": {{ "major": 3, "minor": 0 }},
           "inputs": [
              {{ "name": "%g_0", "type": "Scalar<BLS12-381>" }},
              {{ "name": "%g_1", "type": "Scalar<BLS12-381>" }}
           ],
           "outputs": [],
           "do_communications_commitment": false,
           "instructions": [{instructions}]
        }}"#
    );
    IrSource::load(ir_json.as_bytes()).expect("IR must parse")
}

#[test]
fn proof_witnesses_track_the_guards() {
    let ir = load(
        r#"{ "op": "inner_proof", "guard": "%g_0", "output": "%p_0" },
           { "op": "inner_proof", "guard": "%g_1", "output": "%p_1" }"#,
    );
    let proof = || vec![InnerProofWitness::Direct(vec![1u8, 2, 3])];

    // The single witness belongs to the second, active instruction: the
    // guarded-off first consumed nothing.
    ir.check(&preimage([0, 1], proof()))
        .expect("one witness, for the active instruction");

    // Two active instructions need two witnesses...
    assert!(ir.check(&preimage([1, 1], proof())).is_err());

    // ...and none needs none: an unconsumed witness is rejected, so the vector
    // cannot quietly carry a proof for a branch that was not taken.
    assert!(ir.check(&preimage([0, 0], proof())).is_err());
}
