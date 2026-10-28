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

//! Decoding checks that `HashMap` entries are stored under their keys'
//! hashes once per trie node, however many maps share it; these tests compare
//! it against a naive per-entry check.

use crate::db::InMemoryDB;
use crate::storage::*;
use crate::test::{assert_completes_within, round_trip};
use proptest::prelude::*;

type D = InMemoryDB;
type H = HashMap<u64, u64, D>;

/// A naive check: every path decodes as a hash, and is the hash of the
/// entry's key.
fn naive_check(map: &H) -> bool {
    map.0.mpt.iter().all(|(path, kv)| {
        from_nibbles::<ArenaHash<<D as DB>::Hasher>>(&path).is_ok_and(|h| h == H::gen_key(&kv.0))
    })
}

/// A map of entries stored under their key's hash, under another key's
/// hash, or under their own hash with the first nibble of another entry's,
/// which puts a misplaced entry beside an honest one below the root.
fn map() -> impl Strategy<Value = H> {
    prop::collection::vec((0u64..64, 0u8..3), 0..40).prop_map(|entries| {
        let mut mpt = MerklePatriciaTrie::<(Sp<u64, D>, Sp<u64, D>), D>::new();
        for (k, how) in entries {
            let mut path = to_nibbles(&H::gen_key(&if how == 1 { k + 1 } else { k }));
            if how == 2 {
                path[0] = to_nibbles(&H::gen_key(&(k / 2)))[0];
            }
            mpt = mpt.insert(&path, (Sp::new(k), Sp::new(k)));
        }
        HashMap(Map {
            mpt: Sp::new(mpt),
            key_type: PhantomData,
        })
    })
}

/// `map` with the subtree under the root's `from` child moved to its `to`
/// child, so the moved entries sit at the same depth under the wrong nibble.
fn grafted(map: &H, from: usize, to: usize) -> Option<H> {
    let Node::Branch { children, .. } = map.0.mpt.0.deref() else {
        return None;
    };
    let mut children = children.clone();
    if matches!(*children[from], Node::Empty) || from == to {
        return None;
    }
    children[to] = children[from].clone();
    let size = children.iter().map(|c| Node::ann(c).0).sum();
    let mpt = MerklePatriciaTrie(Sp::new(Node::Branch {
        ann: SizeAnn(size),
        children,
    }));
    Some(HashMap(Map {
        mpt: Sp::new(mpt),
        key_type: PhantomData,
    }))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1024))]

    #[test]
    fn entries_agree_with_naive_check(map in map()) {
        prop_assert_eq!(round_trip::<_, D>(&map).is_ok(), naive_check(&map));
    }

    // Decoded together, the maps share one memo, so a subtree checked in the
    // first must still be rejected where the second misplaces it.
    #[test]
    fn maps_sharing_subtrees_agree_with_naive_check(
        map in map(),
        extra in any::<u64>(),
        from in 0usize..16,
        to in 0usize..16,
    ) {
        let second = grafted(&map, from, to)
            .unwrap_or_else(|| HashMap(map.0.insert(H::gen_key(&extra), (Sp::new(extra), Sp::new(extra)))));
        let pair: Array<H, D> = vec![map.clone(), second.clone()].into();
        prop_assert_eq!(round_trip::<_, D>(&pair).is_ok(), naive_check(&map) && naive_check(&second));
    }
}

#[test]
fn maps_sharing_a_large_trie_decode_quickly() {
    let base: H = (0..2000u64).fold(HashMap::new(), |m, k| m.insert(k, k));
    let maps: Array<H, D> = (0..2000u64)
        .map(|i| base.insert(1_000_000 + i, i))
        .collect();
    let mut bytes = Vec::new();
    Sp::new(maps).serialize(&mut bytes).unwrap();
    assert_completes_within(std::time::Duration::from_secs(10), move || {
        assert!(Sp::<Array<H, D>, D>::deserialize(&mut &bytes[..], 0).is_ok());
    });
}
