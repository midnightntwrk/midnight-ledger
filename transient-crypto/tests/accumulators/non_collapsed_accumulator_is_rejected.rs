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

//! An accumulator region that is not in collapsed form is refused.
//!
//! A collapsed accumulator has, on each side, exactly one variable base with
//! scalar **1** — `Msm::collapse` sets `scalars = vec![ONE]`, and
//! `Accumulator::trivial` documents the same invariant. Reconstruction depends
//! on it: the encoding is not self-describing (`AssignedAccumulator::from_public_input`
//! is `unimplemented!` upstream, since inner MSM sizes cannot be recovered from
//! the flat form), so `reconstruct_accumulator` *assumes* one point plus one
//! scalar per side and reads the scalar verbatim.
//!
//! Nothing checks that scalar. A region carrying any other value is taken as a
//! valid accumulator, and whether it then pairs is incidental — a scalar of
//! zero makes its side evaluate to the identity whatever the base is, so the
//! pairing passes vacuously and a region attesting to nothing is accepted.
//!
//! The invariant is cheap to enforce where it is already assumed, and doing so
//! also turns a genuinely non-collapsed accumulator into a structural error
//! rather than an incidental pairing failure: a wider encoding sliced to
//! `acc_len` yields a scalar taken from the middle of a point, which is not 1.

use group::Group;
use midnight_circuits::types::Instantiable;
use midnight_circuits::verifier::{Accumulator, AssignedAccumulator, Msm, SelfEmulation};
use midnight_curves::Fq;
use std::collections::BTreeMap;

use midnight_transient_crypto::curve::Fr;
use midnight_transient_crypto::proofs::{PARAMS_VERIFIER, S, VerifierKey};

use crate::harness::{raw_proof_exposing, test_rng};

/// Puts `acc`'s encoding in a proof's public inputs and verifies it.
fn verify_region(acc: &Accumulator<S>) -> Result<(), String> {
    let enc = <AssignedAccumulator<S> as Instantiable<Fq>>::as_public_input(acc);
    let mut rng = test_rng();
    let (raw, proof) = raw_proof_exposing(&enc, &mut rng);
    let statement: Vec<Fr> = enc.iter().map(|f| Fr(*f)).collect();
    VerifierKey::from_vk_with_accumulator_offsets(raw, &[0])
        .verify(&PARAMS_VERIFIER, &proof, statement.into_iter())
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
fn non_collapsed_accumulator_is_rejected() {
    let id = <S as SelfEmulation>::C::identity();
    let g = <S as SelfEmulation>::C::generator();

    // Control: collapsed form, scalar 1 on both sides.
    verify_region(&Accumulator::new(side(id, 1), side(id, 1)))
        .expect("a collapsed accumulator must verify");

    // Scalars other than 1 are not collapsed form. Both sides still evaluate to
    // the identity here, so the pairing cannot be what refuses them.
    for (lhs, rhs) in [(2, 3), (0, 0)] {
        let err = verify_region(&Accumulator::new(side(id, lhs), side(id, rhs)))
            .expect_err("scalars ({lhs}, {rhs}) are not collapsed form and must be refused");
        assert!(
            err.contains("accumulator"),
            "expected a structural error, got: {err}"
        );
    }

    // The one that matters: a zero scalar zeroes its side whatever the base is,
    // so this pairs vacuously while attesting to nothing.
    let err = verify_region(&Accumulator::new(side(g, 0), side(g, 0)))
        .expect_err("a zero-scalar region attests to nothing and must be refused");
    assert!(
        err.contains("accumulator"),
        "expected a structural error, got: {err}"
    );
}
