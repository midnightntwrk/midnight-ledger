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

//! Decoding checks array indices with a summary per distinct trie node; these
//! tests compare it against a naive per-key check.

use crate::db::InMemoryDB;
use crate::storage::*;
use crate::test::round_trip;
use proptest::prelude::*;

type D = InMemoryDB;

/// A naive check over every key of the array.
fn naive_invariant(arr: &Array<u64, D>) -> bool {
    fn nibbles_to_index(raw_nibbles: &[u8]) -> Option<u64> {
        if raw_nibbles.first() == Some(&0) || raw_nibbles.len() > 16 {
            return None;
        }
        Some(
            raw_nibbles
                .iter()
                .fold(0u64, |acc, n| (acc << 4) | *n as u64),
        )
    }
    let len = arr.len() as u64;
    arr.0
        .iter()
        .all(|(k, _)| nibbles_to_index(&k).is_some_and(|n| n < len))
}

// Valid arrays, some built from shared subtrees, with keys inserted straight
// into their tries: some extend the array, others leave gaps or have leading
// zeroes or too many nibbles.
fn array() -> impl Strategy<Value = Array<u64, D>> {
    let keys = prop::collection::vec(prop::collection::vec(0u8..16, 0..18), 0..4);
    (0u32..3, 0usize..40, 0u64..3, keys).prop_map(|(log16_len, len, v, keys)| {
        let base = match log16_len {
            0 => (0..len).map(|i| i as u64 % 3).collect(),
            k => Array::<u64, D>::shared_dense(k, v),
        };
        let mpt = keys
            .iter()
            .fold((*base.0).clone(), |mpt, k| mpt.insert(k, v));
        Array(Sp::new(mpt))
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2048))]

    #[test]
    fn decode_agrees_with_naive_check(arr in array()) {
        let decoded = round_trip::<_, D>(&arr);
        prop_assert_eq!(decoded.is_ok(), naive_invariant(&arr), "{:?}", decoded.err());
    }
}
