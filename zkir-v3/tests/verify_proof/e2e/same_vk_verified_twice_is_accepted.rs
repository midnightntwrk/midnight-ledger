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

//! Two `verify_proof` instructions resolving to the *same* key, each with its
//! own inner proof, both verify.
//!
//! The complement of `two_proofs_with_distinct_vks_are_accepted`. Resolution
//! indexes by digest, so the side-table holds one entry; the "every key is used"
//! check counts distinct hashes, so one entry serving two instructions must not
//! read as surplus, nor two instructions naming one hash as a duplicate.
//!
//! The two proofs are of different statements, so a circuit that collapsed them
//! would still have to satisfy both to pair. The single-scalar relation is the
//! fixture: nothing here depends on the inner relation, and two RSA
//! verifications reach k=20, where keygen alone exceeds two minutes.

use crate::e2e_harness::{
    outer_preimage_all, outer_prove, outer_setup_all, outer_verify, scalar_inner_proofs, test_rng,
};

#[actix_rt::test]
#[ignore = "outer verifier circuit needs a high-k SRS not available in CI"]
async fn same_vk_verified_twice_is_accepted() {
    let mut rng = test_rng();

    // Two distinct statements, proven under one key.
    let inner = scalar_inner_proofs(&[123, 456], &mut rng).await;
    assert_eq!(
        inner[0].vk_blob, inner[1].vk_blob,
        "both proofs must be under the same key, or this is the other test"
    );
    assert_ne!(
        inner[0].pis, inner[1].pis,
        "the two statements must differ, or verifying one twice would pass"
    );

    // One blob, two instructions: `outer_ir_for_all` collapses the side-table to
    // the single entry both resolve to.
    let entries: Vec<_> = inner.iter().map(|i| i.entry()).collect();
    let (outer_ir, outer_pk, outer_vk) = outer_setup_all(&entries).await;
    assert_eq!(
        outer_ir.verify_proof_vks.len(),
        1,
        "the side-table must hold one entry for a key used twice"
    );
    assert_eq!(
        outer_ir.accumulator_count(),
        2,
        "two instructions still expose two accumulators"
    );

    let preimage = outer_preimage_all(inner.iter().map(|i| i.proof.clone()).collect());
    let (outer_proof, outer_pis) = outer_prove(&outer_ir, outer_pk, &preimage, &mut rng).await;
    outer_verify(&outer_vk, &outer_proof, outer_pis);
}
