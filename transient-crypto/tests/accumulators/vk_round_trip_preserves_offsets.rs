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

//! A `VerifierKey` carrying accumulator offsets survives serialization and
//! still verifies through the reloaded copy.
//!
//! Offsets are the part `verify_proof` adds to the key, and the ledger never
//! verifies with the key keygen returned — it stores it and parses it back
//! first. `accumulator_offsets` is private, so a successful pairing through the
//! reloaded key is how the offsets are observed: had they been lost, the
//! accumulator would not be found; had they shifted, it would not pair.

use serialize::{Deserializable, Serializable};

use midnight_transient_crypto::proofs::{PARAMS_VERIFIER, VerifierKey};

use crate::harness::{acc_len, passing_accumulator, proof_exposing, test_rng};

#[test]
fn vk_round_trip_preserves_offsets() {
    let mut rng = test_rng();

    // A leading filler slot, so the offset is non-zero and a key that lost its
    // offsets would look at the wrong place rather than accidentally be right.
    let mut pis = vec![midnight_curves::Fq::from(7u64)];
    pis.extend(passing_accumulator());
    let (vk, proof, statement) = proof_exposing(&pis, &[1], &mut rng);

    let mut bytes = Vec::new();
    Serializable::serialize(&vk, &mut bytes).expect("serialize vk");
    let reloaded: VerifierKey =
        Deserializable::deserialize(&mut &bytes[..], 0).expect("deserialize vk");

    reloaded
        .verify(&PARAMS_VERIFIER, &proof, statement.into_iter())
        .expect("a reloaded key must find the accumulator at its recorded offset");

    assert_eq!(
        pis.len(),
        1 + acc_len(),
        "statement should be the filler slot followed by one accumulator"
    );
}
