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

//! A `Proof` carries its accumulator blocks across serialization, intact and in
//! order.
//!
//! Order matters because the blocks are positional, one per `verify_proof`: a
//! round-trip that reordered them would still yield the right number of
//! well-formed blocks that still pair, against the wrong instructions.

use serialize::{Deserializable, Serializable, tagged_deserialize, tagged_serialize};

use midnight_transient_crypto::proofs::Proof;

use crate::harness::{failing_accumulator, fr_vec, passing_accumulator};

/// The `bytes` field stands in for a PLONK proof; nothing here reads it.
const PROOF_BYTES: &[u8] = b"not a real plonk proof, but it must survive too";

fn round_trip(proof: &Proof) -> Proof {
    let mut bytes = Vec::new();
    Serializable::serialize(proof, &mut bytes).expect("serialize proof");
    assert_eq!(
        bytes.len(),
        proof.serialized_size(),
        "serialized_size must agree with what serialize wrote"
    );
    Deserializable::deserialize(&mut &bytes[..], 0).expect("deserialize proof")
}

#[test]
fn proof_round_trip_preserves_accumulators() {
    // Two distinct, well-formed encodings, so a swap is detectable.
    let a = fr_vec(&passing_accumulator());
    let b = fr_vec(&failing_accumulator());
    assert_ne!(
        a, b,
        "the two fixtures must differ for order to be testable"
    );

    for blocks in [vec![], vec![a.clone()], vec![a.clone(), b.clone()]] {
        let proof = Proof {
            bytes: PROOF_BYTES.to_vec(),
            accumulators: blocks.clone(),
        };

        let back = round_trip(&proof);
        assert_eq!(
            back,
            proof,
            "{} block(s): round-trip must be exact",
            blocks.len()
        );
        assert_eq!(back.accumulators, blocks, "blocks must come back verbatim");
        assert_eq!(back.bytes, PROOF_BYTES, "the plonk bytes must survive too");

        // And through the tagged envelope, which is how the ledger writes it.
        let mut tagged = Vec::new();
        tagged_serialize(&proof, &mut tagged).expect("tagged serialize");
        let back: Proof = tagged_deserialize(&tagged[..]).expect("tagged deserialize");
        assert_eq!(back, proof, "{} block(s): tagged round-trip", blocks.len());
    }

    // Order is load-bearing: the blocks are positional, one per `verify_proof`.
    let forward = Proof {
        bytes: PROOF_BYTES.to_vec(),
        accumulators: vec![a.clone(), b.clone()],
    };
    let reversed = Proof {
        bytes: PROOF_BYTES.to_vec(),
        accumulators: vec![b, a],
    };
    assert_ne!(
        forward, reversed,
        "equality must distinguish block order, or the round-trip assertions above prove little"
    );
    assert_eq!(round_trip(&forward), forward, "order must survive the wire");
    assert_ne!(
        round_trip(&forward),
        reversed,
        "a round-trip must not reorder the blocks"
    );
}
