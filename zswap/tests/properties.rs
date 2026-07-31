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

//! Property tests for zswap offers.
//!
//! Uses the `proptest`-feature generators (activated here via
//! the self dev-dependency). Follows the non-shrinking `StdRng`+`Distribution`
//! pattern used by the ledger property tests.

use coin_structure::coin::ShieldedTokenType;
use midnight_zswap::error::MalformedOffer;
use midnight_zswap::{Delta, Offer};
use rand::{Rng, SeedableRng, rngs::StdRng};
use serialize::{tagged_deserialize, tagged_serialize};
use storage::db::InMemoryDB;

type Db = InMemoryDB;

/// **Serialization roundtrip.** A generated (proof-erased) offer survives a
/// tagged serialize/deserialize cycle unchanged. This is the byte-level control:
/// it must hold, and it is *not* the oracle that catches semantic bugs — the
/// `well_formed` property below is.
#[test]
fn prop_offer_serialization_roundtrips() {
    let mut rng = StdRng::seed_from_u64(0x2405A);
    let mut failures: Vec<String> = Vec::new();

    for case in 0..128u64 {
        let offer: Offer<(), Db> = rng.r#gen();

        let mut bytes = Vec::new();
        tagged_serialize(&offer, &mut bytes).expect("serialize");
        let back: Offer<(), Db> = tagged_deserialize(&mut &bytes[..]).expect("deserialize");

        if offer != back {
            failures.push(format!("case {case}: offer changed across serialization roundtrip"));
        }
    }

    assert!(
        failures.is_empty(),
        "offer serialization roundtrip failed in {} case(s):\n{}",
        failures.len(),
        failures.join("\n"),
    );
}

/// **Semantic normal-form oracle.** `Offer::<(), _>::well_formed` accepts an
/// offer iff it is in normal form (sorted inputs/outputs/transient, strictly
/// increasing unique deltas, all deltas non-zero).
///
/// Positive case: the generator produces normalized offers by construction, so
/// `well_formed` must accept. Negative case: we splice in a single zero-value
/// delta — a guaranteed normal-form violation regardless of the rest of the
/// offer — and `well_formed` must reject with `NotNormalized`. This is a
/// semantic oracle over the object graph, not a byte roundtrip.
#[test]
fn prop_offer_well_formed_iff_normalized() {
    let mut rng = StdRng::seed_from_u64(0xB0F0F);
    let mut failures: Vec<String> = Vec::new();
    let mut positives = 0u32;

    for case in 0..128u64 {
        let segment: u16 = rng.gen_range(0..=64u16);
        let offer: Offer<(), Db> = rng.r#gen();

        // Positive: normalized-by-construction must be accepted.
        match offer.well_formed(segment) {
            Ok(_) => positives += 1,
            Err(e) => failures.push(format!(
                "case {case} (positive): well_formed rejected a normalized offer: {e:?}"
            )),
        }

        // Negative: a zero-value delta is never normal form.
        let mut malformed = offer.clone();
        let bad_delta = Delta {
            token_type: rng.r#gen::<ShieldedTokenType>(),
            value: 0,
        };
        malformed.deltas = vec![bad_delta].into();
        match malformed.well_formed(segment) {
            Err(MalformedOffer::NotNormalized) => {}
            other => failures.push(format!(
                "case {case} (negative): well_formed did not reject a zero-value delta as \
                 NotNormalized: {other:?}"
            )),
        }
    }

    assert!(positives > 0, "generator never produced an accepted offer — property was vacuous");
    assert!(
        failures.is_empty(),
        "well_formed disagreed with the normal-form oracle in {} case(s):\n{}",
        failures.len(),
        failures.join("\n"),
    );
}
