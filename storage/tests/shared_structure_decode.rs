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

//! Decoding must take time linear in the encoding, even when the encoded
//! tries share subtrees and so stand for exponentially many entries.

use midnight_storage::storage::{Array, Map};
use midnight_storage::test::assert_completes_within;
use serialize::{tagged_deserialize, tagged_serialize};
use std::time::Duration;

// A linear-time decode takes milliseconds; a walk over every entry takes
// years.
const LIMIT: Duration = Duration::from_secs(10);

fn encode<T: serialize::Serializable + serialize::Tagged>(value: &T) -> Vec<u8> {
    let mut bytes = Vec::new();
    tagged_serialize(value, &mut bytes).unwrap();
    bytes
}

#[test]
fn shared_array() {
    let bytes = encode(&Array::<u64>::shared_dense(12, 7));
    let len = assert_completes_within(LIMIT, move || {
        tagged_deserialize::<Array<u64>>(&mut &bytes[..]).map(|arr| arr.len())
    });
    assert_eq!(len.unwrap(), 16usize.pow(12));
}

#[test]
fn shared_map() {
    let prefix = [0u8; 64 - 12];
    let bytes = encode(&Map::<[u8; 32], u64>::shared_full(&prefix, 12, 7));
    let size = assert_completes_within(LIMIT, move || {
        tagged_deserialize::<Map<[u8; 32], u64>>(&mut &bytes[..]).map(|map| map.size())
    });
    assert_eq!(size.unwrap(), 16usize.pow(12));
}

#[test]
fn shared_map_size_exceeding_u64_is_rejected() {
    let prefix = [0u8; 64 - 17];
    let bytes = encode(&Map::<[u8; 32], u64>::shared_full(&prefix, 17, 7));
    let res = assert_completes_within(LIMIT, move || {
        tagged_deserialize::<Map<[u8; 32], u64>>(&mut &bytes[..]).map(|map| map.size())
    });
    assert!(res.is_err(), "decoded with size {res:?}");
}

#[test]
fn shared_array_matches_honest_construction() {
    for k in 1..=3 {
        let honest: Array<u64> = std::iter::repeat_n(7, 16usize.pow(k)).collect();
        assert_eq!(encode(&Array::<u64>::shared_dense(k, 7)), encode(&honest));
    }
}

#[test]
fn shared_map_matches_honest_construction() {
    for free in 1..=2u32 {
        let mut honest = Map::<[u8; 32], u64>::new();
        for i in 0..16u32.pow(free) {
            let mut key = [0u8; 32];
            key[28..].copy_from_slice(&i.to_be_bytes());
            honest = honest.insert(key, 7);
        }
        let prefix = vec![0u8; 64 - free as usize];
        assert_eq!(
            encode(&Map::<[u8; 32], u64>::shared_full(&prefix, free, 7)),
            encode(&honest)
        );
    }
}
