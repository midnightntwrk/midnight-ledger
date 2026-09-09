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

//! `ProofVersioned` is the transaction proof envelope. Fixed fixtures pin each
//! variant discriminant and both directions of its untagged encoding; tagged
//! round-trips cover the transaction representation. `V4` carries accumulators,
//! while `V2` and `V3` retain the legacy proof format.

use midnight_ledger_v10::structure::ProofVersioned;
use serialize::{Deserializable, Serializable, tagged_deserialize, tagged_serialize};
use transient_crypto::curve::outer::POINT_BYTES;
use transient_crypto::proofs::{DeferredAccumulator, Proof};

/// The proof bytes used by every `V4` fixture.
const V4_PROOF_BYTES: [u8; 2] = [0xab, 0xcd];

fn identity_point() -> Vec<u8> {
    let mut p = vec![0u8; POINT_BYTES];
    p[0] = 0xc0;
    p
}

/// An accumulator of two identity points, via the wire form the decoder accepts.
fn accumulator() -> DeferredAccumulator {
    let mut bytes = identity_point();
    bytes.extend(identity_point());
    DeferredAccumulator::deserialize(&mut &bytes[..], 0)
        .expect("two identity points are a well-formed accumulator")
}

fn old_proof(bytes: &[u8]) -> transient_crypto_old::proofs::Proof {
    transient_crypto_old::proofs::Proof(bytes.to_vec())
}

/// The expected encoding of `V4` carrying `n` accumulators.
///
/// `3` is the variant, `8` is `V4_PROOF_BYTES.len() << 2`, then the proof bytes,
/// then `n << 2` for the accumulator vector, then `n` accumulators of
/// `2 * POINT_BYTES` each.
fn v4_bytes(n: usize) -> Vec<u8> {
    let mut out = vec![3, (V4_PROOF_BYTES.len() << 2) as u8];
    out.extend(V4_PROOF_BYTES);
    out.push((n << 2) as u8);
    for _ in 0..n {
        out.extend(identity_point());
        out.extend(identity_point());
    }
    out
}

fn v4(n: usize) -> ProofVersioned {
    ProofVersioned::V4(Proof {
        bytes: V4_PROOF_BYTES.to_vec(),
        accumulators: std::iter::repeat_with(accumulator).take(n).collect(),
    })
}

#[test]
fn proof_versioned_encodes_every_variant_to_fixed_bytes() {
    let cases: Vec<(&str, ProofVersioned, Vec<u8>)> = vec![
        (
            "V2",
            ProofVersioned::V2(old_proof(&[0xde, 0xad, 0xbe, 0xef])),
            vec![1, 16, 0xde, 0xad, 0xbe, 0xef],
        ),
        (
            "V3",
            ProofVersioned::V3(old_proof(&[0xc0, 0xff, 0xee])),
            vec![2, 12, 0xc0, 0xff, 0xee],
        ),
        ("V4, no accumulators", v4(0), v4_bytes(0)),
        ("V4, one accumulator", v4(1), v4_bytes(1)),
        ("V4, two accumulators", v4(2), v4_bytes(2)),
    ];

    for (label, value, expected) in cases {
        // Writer: the value encodes to exactly these bytes.
        let mut written = Vec::new();
        Serializable::serialize(&value, &mut written).expect("serialize");
        assert_eq!(written, expected, "{label}: encoding must be exact");
        assert_eq!(
            written.len(),
            value.serialized_size(),
            "{label}: serialized_size must agree with what serialize wrote"
        );

        // Reader: the same bytes decode back to the value. Asserted separately,
        // because a decoder that normalised a field would pass one and fail the
        // other, and both matter.
        let read: ProofVersioned = Deserializable::deserialize(&mut &expected[..], 0)
            .unwrap_or_else(|e| panic!("{label}: expected bytes must decode: {e}"));
        assert_eq!(read, value, "{label}: decoding must reproduce the value");

        // And through the tagged envelope, which is how a transaction carries it.
        let mut tagged = Vec::new();
        tagged_serialize(&value, &mut tagged).expect("tagged serialize");
        let back: ProofVersioned = tagged_deserialize(&tagged[..]).expect("tagged deserialize");
        assert_eq!(back, value, "{label}: tagged round-trip");
    }
}

#[test]
fn proof_versioned_refuses_unknown_discriminants() {
    // `0` is called out separately in the reader as an invalid *old*
    // discriminant, so it is checked separately here.
    for byte in [0u8, 4, 255] {
        let bytes = vec![byte];
        assert!(
            <ProofVersioned as Deserializable>::deserialize(&mut &bytes[..], 0).is_err(),
            "discriminant {byte} must be refused rather than guessed at"
        );
    }
}
