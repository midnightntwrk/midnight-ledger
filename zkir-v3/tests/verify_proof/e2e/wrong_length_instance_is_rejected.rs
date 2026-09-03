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

//! An `instance` of the wrong length for the key it names is caught, but not
//! until the deferred pairing.
//!
//! Worth knowing, but not asserted: nothing fails fast. `check()` accepts both
//! directions and the in-circuit pass agrees, because both use the same wrong
//! length, so proving succeeds too and the error only surfaces at the pairing.
//! That costs a keygen and a prove to discover, but it is caught.

use transient_crypto::proofs::Zkir;

use crate::e2e_harness::{
    Rejection, expect_rejected, outer_ir_for, outer_keygen, outer_preimage, rsa_inner_proof,
    test_rng,
};

#[actix_rt::test]
#[ignore = "outer verifier circuit needs a high-k SRS not available in CI"]
async fn wrong_length_instance_is_rejected() {
    let mut rng = test_rng();
    let inner = rsa_inner_proof(&mut rng).await;
    assert!(
        inner.pis.len() > 1,
        "truncating must not reduce to an empty instance"
    );

    // One direction is enough: the instance length is fixed at compile time, so
    // each direction is a separate circuit and a separate keygen, and both fail
    // identically at the same stage.
    let instance = inner.pis[..inner.pis.len() - 1].to_vec();
    let ir = outer_ir_for(&inner.vk_blob, &instance);

    let (pk, vk) = outer_keygen(&ir, "wrong-length instance, one short").await;
    let stage = expect_rejected(&ir, pk, &vk, inner.proof.clone(), &mut rng).await;
    assert_eq!(
        stage,
        Rejection::AtVerify,
        "both passes agree on the wrong length, so only the pairing can refuse it"
    );

    // Preparation prepares against whatever length the IR declares and hands
    // back a well-formed accumulator for it, so the mismatch survives to the
    // pairing above rather than being refused here.
    ir.check(&outer_preimage(inner.proof.clone()))
        .expect("check() does not police arity; the pairing is what refuses it");
}
