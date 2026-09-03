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

//! A `Collapsed` decider must discharge the accumulator its inner proof carried,
//! not just the one verifying it produced.
//!
//! Nothing here is malformed. A proof of `BOGUS` prepared against `GOOD` yields a
//! well-formed collapsed accumulator that does not pair, and the recursive proof
//! carrying it proves successfully — the deferred model working as designed.
//!
//! The control is the sharper half: under `DeciderKind::None` the same recursive
//! proof verifies, because `None` never folds the carried accumulator in. The tag
//! on a `verify_proof_vks` entry is therefore security-critical.

use midnight_curves::Fq;

use midnight_zkir_v3::ir_instructions::decidable::{DeciderKind, serialize_vk};
use midnight_zkir_v3::ir_instructions::verify_proof::verify_proof_offcircuit;

use crate::e2e_harness::{
    RecursiveRelation, Rejection, expect_rejected, outer_ir_for, outer_keygen, outer_preimage,
    outer_prove, outer_verify, prove_recursive, scalar_inner_proofs, test_rng,
};

/// The statement the recursive circuit names.
const GOOD: u64 = 123;
/// A statement the same key can prove, but not the one that is named.
const BOGUS: u64 = 456;

#[actix_rt::test]
#[ignore = "two levels of in-circuit verification need a high-k SRS not available in CI"]
async fn collapsed_decider_catches_a_bad_carried_accumulator() {
    let mut rng = test_rng();

    // Two statements under one key: the one the recursive circuit names, and a
    // proof of something else entirely.
    let inner = scalar_inner_proofs(&[GOOD, BOGUS], &mut rng).await;
    let (good, bogus) = (&inner[0], &inner[1]);
    assert_eq!(
        good.vk_blob, bogus.vk_blob,
        "both proofs must be under one key"
    );

    // The bogus proof prepared against the named statement. This succeeds:
    // preparation does not check the pairing, it defers it.
    let deferred_bad = verify_proof_offcircuit(&good.vk_blob, &good.pis, &bogus.proof, true)
        .expect("preparation succeeds; only the deferred pairing could refuse this");

    // The recursive proof carrying that accumulator in its instance tail. It
    // proves — the inner proof's validity was deferred, not checked.
    let relation = RecursiveRelation {
        inner_vk: good.vk_blob.clone(),
    };
    let instance: Vec<Fq> = good.pis.iter().copied().chain(deferred_bad).collect();
    let (recursive_proof, recursive_vk) =
        prove_recursive(&relation, &instance, bogus.proof.clone(), &mut rng).await;

    // ---- Collapsed: the fold must carry the failure through ----
    let collapsed = serialize_vk(&recursive_vk, DeciderKind::Collapsed).expect("collapsed blob");
    let ir = outer_ir_for(&collapsed, &instance);
    let (pk, vk) = outer_keygen(&ir, "collapsed decider, bad carried accumulator").await;
    let stage = expect_rejected(&ir, pk, &vk, recursive_proof.clone(), &mut rng).await;
    assert_eq!(
        stage,
        Rejection::AtVerify,
        "nothing is malformed, so only the deferred pairing can refuse it"
    );

    // ---- Control: `None` over the *same* recursive proof is accepted ----
    // Its own accumulator pairs and the carried one is never folded in, so
    // nothing discharges it: the bogus proof two levels down goes unchecked.
    let none = serialize_vk(&recursive_vk, DeciderKind::None).expect("none blob");
    let ir_none = outer_ir_for(&none, &instance);
    let (pk_none, vk_none) = outer_keygen(&ir_none, "none decider, same recursive proof").await;
    let (proof, pis) = outer_prove(
        &ir_none,
        pk_none,
        &outer_preimage(recursive_proof),
        &mut rng,
    )
    .await;
    outer_verify(&vk_none, &proof, pis);
}
