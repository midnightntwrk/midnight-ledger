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

//! `verify` checks `accumulators ++ statement`, not their boundary. Production
//! callers must compute a fixed-length statement; otherwise deferred pairings
//! can be omitted. `each_accumulator_block_is_paired` covers the fixed split.

use midnight_curves::Fq;

use midnight_transient_crypto::proofs::{PARAMS_VERIFIER, Proof, VerifierKey};

use crate::harness::{deferred, fr_vec, passing_accumulator, raw_proof_exposing, test_rng};

#[test]
fn the_caller_statement_pins_the_accumulator_count() {
    let mut rng = test_rng();
    let acc = passing_accumulator();

    // One public-input vector, two accumulators wide, and one proof of it.
    // Every case below reuses these bytes, so the PLONK check is identical
    // throughout and only the split varies.
    let mut pis: Vec<Fq> = acc.clone();
    pis.extend_from_slice(&acc);
    let (raw_vk, bytes) = raw_proof_exposing(&pis, &mut rng);
    let vk = VerifierKey::from(raw_vk);

    // The honest split: both accumulators carried, nothing in the statement.
    let honest = Proof {
        bytes: bytes.clone(),
        accumulators: vec![deferred(&acc), deferred(&acc)],
    };
    vk.verify(&PARAMS_VERIFIER, &honest, [].into_iter())
        .expect("the honest split must verify");

    // Boundary moved by one accumulator. Accepted: the concatenation is
    // unchanged, so PLONK cannot tell, and one pairing that should have run
    // does not.
    let one_moved = Proof {
        bytes: bytes.clone(),
        accumulators: vec![deferred(&acc)],
    };
    assert!(
        vk.verify(&PARAMS_VERIFIER, &one_moved, fr_vec(&acc).into_iter())
            .is_ok(),
        "moving the boundary changes nothing PLONK can see; if this now fails, \
         `verify` has gained a split check and this test should assert that instead"
    );

    // The whole way: nothing carried, so the deferred-pairing loop is empty and
    // no inner proof is discharged at all.
    let none_carried = Proof {
        bytes,
        accumulators: vec![],
    };
    assert!(
        vk.verify(&PARAMS_VERIFIER, &none_carried, fr_vec(&pis).into_iter())
            .is_ok(),
        "with no accumulators carried, no deferred pairing runs; if this now \
         fails, `verify` has gained a split check and this test should assert that instead"
    );
}
