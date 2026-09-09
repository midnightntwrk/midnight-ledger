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

//! `Proof` deserialization must validate both accumulator points.
//!
//! The type-level malformed-encoding test corrupts both points. This preserves
//! `lhs` and corrupts `rhs`, through the enclosing `Proof`.

use serialize::{Deserializable, Serializable};

use midnight_transient_crypto::curve::outer::POINT_BYTES;
use midnight_transient_crypto::proofs::Proof;

use crate::harness::{deferred, passing_accumulator};

#[test]
fn both_accumulator_points_are_checked() {
    let proof = Proof {
        bytes: b"stand-in for a plonk proof".to_vec(),
        accumulators: vec![deferred(&passing_accumulator())],
    };
    let mut good = Vec::new();
    Serializable::serialize(&proof, &mut good).expect("serialize proof");

    // Ensure the fixture itself is valid.
    <Proof as Deserializable>::deserialize(&mut &good[..], 0).expect("the fixture must parse");

    // The final `POINT_BYTES` encode `rhs`; `lhs` remains valid.
    let mut bad_rhs = good.clone();
    let rhs = bad_rhs.len() - POINT_BYTES;
    bad_rhs[rhs..].fill(0xff);

    let err = <Proof as Deserializable>::deserialize(&mut &bad_rhs[..], 0)
        .expect_err("a corrupt second point must be refused");
    assert!(
        format!("{err:#}").contains("accumulator point is not on the curve")
            || format!("{err:#}").contains("prime-order subgroup"),
        "expected the second point to be checked too, got: {err:#}"
    );
}
