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

//! A `ProofPreimage` carries its inner-proof witnesses across serialization, and
//! the fields after them survive too.
//!
//! `proof_witnesses` sits mid-struct in a bare concatenation, so its length
//! prefix is the only thing marking where it ends — misjudge it and every later
//! field reads from the wrong offset. Hence the individual tail assertions.
//!
//! This catches the writer and reader disagreeing, not a misunderstanding they
//! share; the `proof-preimage[v2]` tag bump covers the old form.

use std::borrow::Cow;

use serialize::{Deserializable, Serializable, tagged_deserialize, tagged_serialize};

use midnight_transient_crypto::curve::Fr;
use midnight_transient_crypto::proofs::{InnerProofWitness, KeyLocation, ProofPreimage};

/// Distinctive values in every field after `proof_witnesses`, so a shifted read
/// cannot coincidentally land on the right ones.
const BINDING: u64 = 0xB1D;
const COMM: u64 = 0xC0;
const OPENING: u64 = 0x0E;

fn preimage(witnesses: Vec<InnerProofWitness>) -> ProofPreimage {
    ProofPreimage {
        inputs: vec![Fr::from(1u64), Fr::from(2u64)],
        private_transcript: vec![Fr::from(3u64)],
        public_transcript_inputs: vec![Fr::from(4u64)],
        public_transcript_outputs: vec![Fr::from(5u64)],
        proof_witnesses: witnesses,
        binding_input: Fr::from(BINDING),
        communications_commitment: Some((Fr::from(COMM), Fr::from(OPENING))),
        key_location: KeyLocation(Cow::Borrowed("builtin")),
    }
}

fn round_trip(p: &ProofPreimage) -> ProofPreimage {
    let mut bytes = Vec::new();
    Serializable::serialize(p, &mut bytes).expect("serialize preimage");
    assert_eq!(
        bytes.len(),
        p.serialized_size(),
        "serialized_size must agree with what serialize wrote"
    );
    Deserializable::deserialize(&mut &bytes[..], 0).expect("deserialize preimage")
}

/// Every field after `proof_witnesses`, checked individually: these are what a
/// misread of the witness vector's length would shift.
fn assert_tail_intact(p: &ProofPreimage, label: &str) {
    assert_eq!(p.binding_input, Fr::from(BINDING), "{label}: binding input");
    assert_eq!(
        p.communications_commitment,
        Some((Fr::from(COMM), Fr::from(OPENING))),
        "{label}: communications commitment"
    );
    assert_eq!(
        p.key_location,
        KeyLocation(Cow::Borrowed("builtin")),
        "{label}: key location"
    );
}

#[test]
fn proof_preimage_round_trip_preserves_witnesses() {
    let a = InnerProofWitness::Direct(vec![0xAA; 32]);
    let b = InnerProofWitness::Direct(vec![0xBB; 96]);
    let empty = InnerProofWitness::Direct(Vec::new());

    let cases: Vec<(&str, Vec<InnerProofWitness>)> = vec![
        ("none", vec![]),
        ("one", vec![a.clone()]),
        ("two, differing lengths", vec![a.clone(), b.clone()]),
        ("an empty blob", vec![empty]),
    ];

    for (label, witnesses) in cases {
        let p = preimage(witnesses.clone());

        let back = round_trip(&p);
        assert_eq!(back, p, "{label}: round-trip must be exact");
        assert_eq!(
            back.proof_witnesses, witnesses,
            "{label}: witnesses must come back verbatim"
        );
        assert_tail_intact(&back, label);

        // And through the tagged envelope, which is how it is stored and sent.
        let mut tagged = Vec::new();
        tagged_serialize(&p, &mut tagged).expect("tagged serialize");
        let back: ProofPreimage = tagged_deserialize(&tagged[..]).expect("tagged deserialize");
        assert_eq!(back, p, "{label}: tagged round-trip");
        assert_tail_intact(&back, label);
    }

    // Order is load-bearing: witnesses are consumed positionally, one per active
    // `InnerProof`, so a round-trip that reordered them would still produce the
    // right count of well-formed blobs — feeding them to the wrong instructions.
    let forward = preimage(vec![a.clone(), b.clone()]);
    let reversed = preimage(vec![b, a]);
    assert_ne!(
        forward, reversed,
        "equality must distinguish witness order, or the assertions above prove little"
    );
    assert_eq!(round_trip(&forward), forward, "order must survive the wire");
    assert_ne!(
        round_trip(&forward),
        reversed,
        "a round-trip must not reorder the witnesses"
    );
}
