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

//! v1 conversion rejects inner proofs rather than discarding them.

use std::borrow::Cow;

use midnight_zkir::ir_v1::preimage_to_v1;
use transient_crypto::curve::Fr;
use transient_crypto::proofs::{InnerProofWitness, KeyLocation, ProofPreimage};

fn preimage(inner_proofs: Vec<InnerProofWitness>) -> ProofPreimage {
    ProofPreimage {
        binding_input: Fr::from(7u64),
        communications_commitment: None,
        inputs: vec![Fr::from(1u64)],
        private_transcript: vec![Fr::from(2u64)],
        public_transcript_inputs: vec![Fr::from(3u64)],
        public_transcript_outputs: vec![Fr::from(4u64)],
        inner_proofs,
        key_location: KeyLocation(Cow::Borrowed("builtin")),
    }
}

#[test]
fn preimage_to_v1_refuses_inner_proofs() {
    // Empty witnesses must be rejected too.
    for (label, witnesses) in [
        ("one empty witness", vec![InnerProofWitness::Direct(vec![])]),
        (
            "one real witness",
            vec![InnerProofWitness::Direct(b"proof bytes".to_vec())],
        ),
        (
            "two witnesses",
            vec![
                InnerProofWitness::Direct(vec![]),
                InnerProofWitness::Direct(b"proof bytes".to_vec()),
            ],
        ),
    ] {
        let err = preimage_to_v1(&preimage(witnesses)).expect_err(&format!(
            "{label}: v1 must refuse inner proofs, not drop them"
        ));
        assert!(
            err.to_string().contains("inner proofs"),
            "{label}: expected a refusal naming inner proofs, got: {err}"
        );
    }

    // The ordinary conversion path remains available.
    let converted = preimage_to_v1(&preimage(vec![])).expect("a preimage with no inner proofs");
    assert_eq!(converted.inputs.len(), 1, "inputs must survive");
    assert_eq!(
        converted.private_transcript.len(),
        1,
        "the private transcript must survive"
    );
    assert_eq!(
        converted.public_transcript_inputs.len(),
        1,
        "the public transcript inputs must survive"
    );
    assert_eq!(
        converted.public_transcript_outputs.len(),
        1,
        "the public transcript outputs must survive"
    );
    assert_eq!(
        converted.key_location.0, "builtin",
        "the key location must survive"
    );
}
