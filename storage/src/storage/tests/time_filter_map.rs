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

//! Decoding checks a `TimeFilterMap`'s set against its time map by counting
//! each distinct container once; these tests compare it against a naive
//! full reconstruction.

use crate::db::InMemoryDB;
use crate::storage::*;
use crate::test::{assert_completes_within, round_trip};
use proptest::prelude::*;

type D = InMemoryDB;
type Tfm = TimeFilterMap<Identity<u64>, D>;

fn naive_invariant(tfm: &Tfm) -> bool {
    let reconstructed = tfm
        .time_map
        .iter()
        .flat_map(|e| Container::<D>::iter_items(*e.1.deref()))
        .fold(MultiSet::new(), |s, i| s.insert(i));
    reconstructed == tfm.set
}

// Honest maps, some over a shared time map, with the set then perturbed.
fn tfm() -> impl Strategy<Value = Tfm> {
    let upserts = prop::collection::vec((0u64..300, 0u64..3), 0..12);
    (upserts, 0u32..3, 0u64..3, 0u8..4, 0u64..3).prop_map(|(upserts, free, v, how, w)| {
        let base = match free {
            0 => TimeFilterMap::new(),
            f => {
                let time_map = Map::shared_full(&[0; 16][..16 - f as usize], f, Identity(v));
                let set = (0..16u32.pow(f)).fold(MultiSet::new(), |s, _| s.insert(v));
                TimeFilterMap { time_map, set }
            }
        };
        let tfm = upserts
            .iter()
            .fold(base, |t, (ts, v)| t.insert(Timestamp::from_secs(*ts), *v));
        match how {
            0 => TimeFilterMap {
                set: tfm.set.insert(w),
                ..tfm
            },
            1 => TimeFilterMap {
                set: tfm.set.remove(&w),
                ..tfm
            },
            _ => tfm,
        }
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn decode_agrees_with_naive_check(tfm in tfm()) {
        prop_assert_eq!(round_trip::<_, D>(&tfm).is_ok(), naive_invariant(&tfm));
    }
}

#[test]
fn shared_time_map_decodes_quickly() {
    let tfm: Tfm = TimeFilterMap {
        time_map: Map::shared_full(&[0; 4], 12, Identity(7)),
        set: MultiSet::new().insert(7),
    };
    let mut bytes = Vec::new();
    Sp::new(tfm).serialize(&mut bytes).unwrap();
    // The set holds one 7 where the time map holds 16^12 of them.
    assert_completes_within(std::time::Duration::from_secs(10), move || {
        assert!(Sp::<Tfm, D>::deserialize(&mut &bytes[..], 0).is_err())
    });
}
