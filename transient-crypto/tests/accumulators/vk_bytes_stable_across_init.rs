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

//! A `VerifierKey` serializes to identical bytes whether it has been
//! initialized or not, and a wire round-trip is byte-exact.
//!
//! Initialization is lazy and happens the first time a key is used, so if it
//! changed the serialization a key's hash would depend on whether anyone had
//! verified with it yet.
//!
//! Three states, each able to break on its own: fresh, after `init`, and after
//! a reload followed by `init` — the last re-derives the bytes from the parsed
//! structure rather than passing the originals through.

use serialize::{Deserializable, Serializable};

use midnight_transient_crypto::proofs::VerifierKey;

use crate::harness::{passing_accumulator, proof_exposing, test_rng};

fn bytes_of(vk: &VerifierKey) -> Vec<u8> {
    let mut out = Vec::new();
    Serializable::serialize(vk, &mut out).expect("serialize vk");
    out
}

#[test]
fn vk_bytes_stable_across_init() {
    let mut rng = test_rng();
    let (vk, _proof, _statement) = proof_exposing(&passing_accumulator(), &[0], &mut rng);

    let fresh = bytes_of(&vk);
    vk.init().expect("init vk");
    assert_eq!(
        fresh,
        bytes_of(&vk),
        "serialization must not change when a key is initialized"
    );

    let reloaded: VerifierKey =
        Deserializable::deserialize(&mut &fresh[..], 0).expect("deserialize vk");
    assert_eq!(
        fresh,
        bytes_of(&reloaded),
        "a round-trip must be byte-exact"
    );

    reloaded.init().expect("init reloaded vk");
    assert_eq!(
        fresh,
        bytes_of(&reloaded),
        "re-serializing from the parsed structure must reproduce the bytes"
    );
}
