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

//! `field_size` must take time linear in the number of distinct nodes, even
//! when a value's children are shared and so stand for exponentially many
//! subtrees.

use base_crypto::fab::AlignedValue;
use base_crypto::hash::HashOutput;
use midnight_onchain_state::state::StateValue;
use std::time::Duration;
use storage::db::InMemoryDB;
use storage::test::assert_completes_within;
use transient_crypto::merkle_tree::MerkleTree;
use transient_crypto::repr::FieldRepr;

type SV = StateValue<InMemoryDB>;

const LIMIT: Duration = Duration::from_secs(10);

#[test]
fn shared_chain() {
    let mut value = SV::Null;
    let mut expected: u128 = 1;
    for depth in 1..=12 {
        value = StateValue::Array(vec![value; 16].into());
        expected = 1 + 16 * expected;
        let v = value.clone();
        let size = assert_completes_within(LIMIT, move || v.field_size());
        assert_eq!(size as u128, expected, "depth {depth}");
    }
}

// Two distinct values per level, each referencing both values of the level
// below: deduplicating within each collection alone still leaves 2^depth
// calls.
#[test]
fn shared_diamond() {
    let mut a = SV::Null;
    let mut b = SV::from(AlignedValue::from(1u8));
    let mut expected_a = a.field_size() as u128;
    let mut expected_b = b.field_size() as u128;
    for depth in 1..=40 {
        (a, b) = (
            StateValue::Array(vec![a.clone(), b.clone()].into()),
            StateValue::Array(vec![b, a].into()),
        );
        (expected_a, expected_b) = (1 + expected_a + expected_b, 1 + expected_a + expected_b);
        let v = a.clone();
        let size = assert_completes_within(LIMIT, move || v.field_size());
        assert_eq!(size as u128, expected_a, "depth {depth}");
    }
}

// Checked values, which transcripts carry, may only hold blank trees; this
// covers unchecked values, such as contract state.
#[test]
fn shared_merkle_tree() {
    let leaf = HashOutput([7; 32]);
    let one: SV = StateValue::BoundedMerkleTree(MerkleTree::shared_full(1, leaf, ()));
    let per_leaf = (one.field_size() as u128 - 1) / 2;
    for height in 1..=32u8 {
        let value: SV = StateValue::BoundedMerkleTree(MerkleTree::shared_full(height, leaf, ()));
        let size = assert_completes_within(LIMIT, move || value.field_size());
        assert_eq!(size as u128, 1 + (per_leaf << height), "height {height}");
    }
}

/// The tree walk `field_size` replaced.
fn naive(value: &SV) -> u128 {
    1 + match value {
        StateValue::Null => 0,
        StateValue::Cell(v) => v.field_size() as u128,
        StateValue::Map(m) => m
            .iter()
            .map(|kv| kv.0.field_size() as u128 + naive(&kv.1))
            .sum(),
        StateValue::Array(arr) => arr.iter().map(|v| naive(&v)).sum(),
        StateValue::BoundedMerkleTree(t) => t.iter().map(|(_, h)| 1 + h.field_size() as u128).sum(),
        _ => unreachable!(),
    }
}

mod properties {
    use super::*;
    use proptest::prelude::*;
    use proptest::sample::Index;

    // Each value draws its entries from those before it, so values share
    // children arbitrarily.
    fn dag() -> impl Strategy<Value = SV> {
        let node = (0u8..5, prop::collection::vec(any::<Index>(), 0..4), 0u8..4);
        (any::<SV>(), prop::collection::vec(node, 1..10)).prop_map(|(seed, nodes)| {
            let mut values = vec![seed];
            for (kind, picks, n) in nodes {
                let picked: Vec<SV> = picks.iter().map(|i| i.get(&values).clone()).collect();
                let value = match kind {
                    0 => StateValue::Array(picked.into()),
                    1 => StateValue::Map(
                        picked
                            .into_iter()
                            .enumerate()
                            .fold(storage::storage::HashMap::new(), |m, (i, v)| {
                                m.insert(AlignedValue::from(i as u8), v)
                            }),
                    ),
                    2 => StateValue::BoundedMerkleTree(MerkleTree::shared_full(
                        n,
                        HashOutput([n; 32]),
                        (),
                    )),
                    3 => StateValue::from(AlignedValue::from(n)),
                    _ => StateValue::Null,
                };
                values.push(value);
            }
            values.pop().unwrap()
        })
    }

    proptest! {
        #[test]
        fn field_size_matches_naive(value in prop_oneof![any::<SV>(), dag()]) {
            prop_assert_eq!(value.field_size() as u128, naive(&value));
        }
    }
}
