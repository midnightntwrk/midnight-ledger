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

//! An accumulator block that is not in collapsed form is refused.
//!
//! A collapsed accumulator has, on each side, exactly one variable base with
//! scalar **1** — `Msm::collapse` sets `scalars = vec![ONE]`, and
//! `Accumulator::trivial` documents the same invariant. Reconstruction depends
//! on it: the encoding is not self-describing (`AssignedAccumulator::from_public_input`
//! is `unimplemented!` upstream, since inner MSM sizes cannot be recovered from
//! the flat form), so `reconstruct_accumulator` *assumes* one point plus one
//! scalar per side and reads the scalar verbatim.
//!
//! Nothing checks that scalar. A block carrying any other value is taken as a
//! valid accumulator, and whether it then pairs is incidental — a scalar of
//! zero makes its side evaluate to the identity whatever the base is, so the
//! pairing passes vacuously and a block attesting to nothing is accepted.
//!
//! The invariant is cheap to enforce where it is already assumed. Carrying the
//! blocks on the proof rather than at offsets in the statement narrowed the
//! surface — a block is now length-checked, so a *wider* encoding sliced down
//! can no longer get in — but it did nothing about the scalar itself, which is
//! the half a prover chooses.
//!
//! KNOWN GAP, hence `#[ignore]`: `reconstruct_accumulator` still reads the
//! scalar verbatim and nothing rejects a value other than 1, so a non-collapsed
//! block is accepted. Unchanged from before this suite was ported. Drop the
//! `#[ignore]` once reconstruction enforces the invariant it already assumes.

use group::Group;
use midnight_circuits::types::Instantiable;
use midnight_circuits::verifier::{Accumulator, AssignedAccumulator, Msm, SelfEmulation};
use midnight_curves::Fq;
use std::collections::BTreeMap;

use midnight_transient_crypto::proofs::{InnerSelfEmulation as S, PARAMS_VERIFIER};

use crate::harness::{proof_carrying, test_rng};

/// Puts `acc`'s encoding on a proof as its single accumulator block, and
/// verifies it.
fn verify_block(acc: &Accumulator<S>) -> Result<(), String> {
    let enc = <AssignedAccumulator<S> as Instantiable<Fq>>::as_public_input(acc);
    let mut rng = test_rng();
    let (vk, proof, stmt) = proof_carrying(&[enc], &[], &mut rng);
    vk.verify(&PARAMS_VERIFIER, &proof, stmt.into_iter())
        .map_err(|e| format!("{e:#}"))
}

fn side(base: <S as SelfEmulation>::C, scalar: u64) -> Msm<S> {
    Msm::new(
        &[base],
        &[<S as SelfEmulation>::F::from(scalar)],
        &BTreeMap::new(),
    )
}

#[test]
#[ignore = "KNOWN GAP: a non-collapsed accumulator block is accepted, its scalar unchecked"]
fn non_collapsed_accumulator_is_rejected() {
    let id = <S as SelfEmulation>::C::identity();
    let g = <S as SelfEmulation>::C::generator();

    // Control: collapsed form, scalar 1 on both sides.
    verify_block(&Accumulator::new(side(id, 1), side(id, 1)))
        .expect("a collapsed accumulator must verify");

    // Scalars other than 1 are not collapsed form. Both sides still evaluate to
    // the identity here, so the pairing cannot be what refuses them.
    for (lhs, rhs) in [(2, 3), (0, 0)] {
        let err = verify_block(&Accumulator::new(side(id, lhs), side(id, rhs)))
            .expect_err("scalars ({lhs}, {rhs}) are not collapsed form and must be refused");
        assert!(
            err.contains("accumulator"),
            "expected a structural error, got: {err}"
        );
    }

    // The one that matters: a zero scalar zeroes its side whatever the base is,
    // so this pairs vacuously while attesting to nothing.
    let err = verify_block(&Accumulator::new(side(g, 0), side(g, 0)))
        .expect_err("a zero-scalar block attests to nothing and must be refused");
    assert!(
        err.contains("accumulator"),
        "expected a structural error, got: {err}"
    );
}
