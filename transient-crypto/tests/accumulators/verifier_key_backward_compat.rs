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

//! A `VerifierKey` written before the accumulator-offset field must still load,
//! defaulting to no offsets.
//!
//! The stored form is the `MidnightVK` bytes followed by an offset count and
//! the offsets. A key written before that field existed simply ends after the
//! `MidnightVK`, and `force_init` reads the count with `read_exact`, so it hits
//! end-of-input rather than defaulting.
//!
//! Deserialization itself succeeds — the payload is kept as opaque bytes — so
//! the break only appears the first time the key is used. A key with no
//! accumulators has nothing to defer, so there is no reason it should not
//! verify exactly as before.

use midnight_curves::Fq;
use midnight_proofs::utils::SerdeFormat;
use midnight_zk_stdlib::setup_vk;
use serialize::{Deserializable, Serializable};

use midnight_transient_crypto::proofs::{PARAMS_VERIFIER, Proof, TranscriptHash, VerifierKey};

use crate::harness::{ExposeAll, MIN_SRS_K, srs, test_rng};
use midnight_transient_crypto::curve::Fr;
use midnight_zk_stdlib::{optimal_k, prove, setup_pk};

#[test]
fn verifier_key_backward_compat() {
    let mut rng = test_rng();

    // A key and a proof, both entirely ordinary — no accumulators involved.
    let pis: Vec<Fq> = (0..2).map(|i| Fq::from(i as u64 + 1)).collect();
    let relation = ExposeAll(pis.len());
    let k = (optimal_k(&relation) as u8).max(MIN_SRS_K);
    let params = srs(k);
    let vk = setup_vk(params.as_ref(), &relation);
    let pk = setup_pk(&relation, &vk);
    let proof = Proof(
        prove::<ExposeAll, TranscriptHash>(params.as_ref(), &pk, &relation, &pis, (), &mut rng)
            .expect("prove"),
    );
    let statement: Vec<Fr> = pis.iter().map(|f| Fr(*f)).collect();

    // The pre-offsets on-disk form: the MidnightVK bytes and nothing more,
    // behind the same length-prefix wrapper the current writer uses.
    let mut inner = Vec::new();
    vk.write(&mut inner, SerdeFormat::Processed)
        .expect("serialize MidnightVK");
    let mut old = Vec::new();
    Serializable::serialize(&(inner.len() as u64), &mut old).expect("length prefix");
    old.extend_from_slice(&inner);

    let loaded: VerifierKey =
        Deserializable::deserialize(&mut &old[..], 0).expect("an old key must deserialize");

    loaded
        .init()
        .expect("an old key must initialize, defaulting to no offsets");

    loaded
        .verify(&PARAMS_VERIFIER, &proof, statement.into_iter())
        .expect("an old key must still verify what it always did");
}
