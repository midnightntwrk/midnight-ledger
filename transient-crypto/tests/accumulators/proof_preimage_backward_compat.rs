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

//! A `ProofPreimage` written before `proof_witnesses` existed must still load,
//! defaulting to no witnesses.
//!
//! The settled design is that old payloads default to empty. `proof_witnesses`
//! sits in the middle of the struct — after `public_transcript_outputs`, before
//! `binding_input` — and the encoding is a plain concatenation of fields in
//! declaration order with no framing, so an old payload is byte-for-byte the
//! current one minus that field's single length byte.
//!
//! Which is what makes this worth pinning: with nothing to mark the field's
//! absence, a reader that expects it takes `binding_input`'s bytes as a
//! witness-count prefix, and every field after the gap shifts. What that costs
//! depends on the bytes — the values here yield a large count and run out of
//! input, but a `binding_input` whose encoding starts with a small byte would
//! parse a plausible witness list and carry on misaligned. The per-field
//! assertions below are there for that second case, which is the dangerous one.

use std::borrow::Cow;

use serialize::{Deserializable, Serializable};

use midnight_transient_crypto::curve::Fr;
use midnight_transient_crypto::proofs::{KeyLocation, ProofPreimage};

fn ser(value: &impl Serializable) -> Vec<u8> {
    let mut bytes = Vec::new();
    Serializable::serialize(value, &mut bytes).expect("serialize");
    bytes
}

#[test]
fn proof_preimage_backward_compat() {
    // Distinct values throughout, so a shifted read is visible rather than
    // landing on a matching zero.
    let expected = ProofPreimage {
        inputs: vec![Fr::from(11u64), Fr::from(12u64)],
        private_transcript: vec![Fr::from(21u64)],
        public_transcript_inputs: vec![Fr::from(31u64)],
        public_transcript_outputs: vec![Fr::from(41u64)],
        proof_witnesses: vec![],
        binding_input: Fr::from(99u64),
        communications_commitment: Some((Fr::from(51u64), Fr::from(61u64))),
        key_location: KeyLocation(Cow::Borrowed("builtin")),
    };

    // The pre-`proof_witnesses` form: every field in order, with that one
    // omitted entirely.
    let mut old = Vec::new();
    old.extend(ser(&expected.inputs));
    old.extend(ser(&expected.private_transcript));
    old.extend(ser(&expected.public_transcript_inputs));
    old.extend(ser(&expected.public_transcript_outputs));
    old.extend(ser(&expected.binding_input));
    old.extend(ser(&expected.communications_commitment));
    old.extend(ser(&expected.key_location));

    // Sanity: the current form is the same bytes plus the empty-vector byte.
    let current = ser(&expected);
    assert_eq!(
        current.len(),
        old.len() + 1,
        "the only difference should be the empty `proof_witnesses` length byte"
    );

    let loaded: ProofPreimage =
        Deserializable::deserialize(&mut &old[..], 0).expect("an old preimage must deserialize");

    assert_eq!(
        loaded.proof_witnesses,
        Vec::<Vec<u8>>::new(),
        "an old preimage must default to no proof witnesses"
    );

    // The fields after the gap are where a misparse would surface.
    assert_eq!(
        loaded.binding_input, expected.binding_input,
        "binding_input"
    );
    assert_eq!(
        loaded.communications_commitment, expected.communications_commitment,
        "communications_commitment"
    );
    assert_eq!(loaded.key_location, expected.key_location, "key_location");
    assert_eq!(loaded, expected, "every field must survive");
}
