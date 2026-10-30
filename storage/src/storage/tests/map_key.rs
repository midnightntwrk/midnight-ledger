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

//! Decoding checks `Map` keys per trie node; these tests compare it against a
//! naive per-key check.

use crate::db::InMemoryDB;
use crate::delta_tracking::ChildRef;
use crate::storage::*;
use crate::test::round_trip;
use proptest::prelude::*;

type D = InMemoryDB;

/// A naive check: every path decodes as a `K`.
fn naive_check<K: Deserializable, V: Storable<D>>(mpt: &MerklePatriciaTrie<V, D>) -> bool {
    mpt.iter().all(|(path, _)| from_nibbles::<K>(&path).is_ok())
}

fn insert_paths(mpt: MerklePatriciaTrie<u64, D>, paths: &[Vec<u8>]) -> MerklePatriciaTrie<u64, D> {
    paths.iter().fold(mpt, |mpt, p| mpt.insert(p, 0))
}

// Honest and shared maps, with raw paths inserted that may not decode: odd
// lengths, truncations and extensions of valid keys.
fn shape_map<K>(
    keys: impl Strategy<Value = K> + Clone,
    width: usize,
) -> impl Strategy<Value = Map<K, u64, D>>
where
    K: Serializable + Deserializable + std::fmt::Debug + Clone + 'static,
{
    let raw = prop::collection::vec(prop::collection::vec(0u8..16, 0..2 * width + 3), 0..4);
    (prop::collection::vec(keys, 0..20), 0u32..3, raw).prop_map(move |(keys, free, raw)| {
        let base = match free {
            0 => MerklePatriciaTrie::new(),
            f => (*Map::<[u8; 2], u64, D>::shared_full(&vec![0; 4 - f as usize], f, 1).mpt).clone(),
        };
        let honest = keys
            .iter()
            .fold(base, |mpt, k| mpt.insert(&to_nibbles(k), 1));
        Map {
            mpt: Sp::new(insert_paths(honest, &raw)),
            key_type: PhantomData,
        }
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1024))]

    #[test]
    fn shape_keys_agree_with_naive_check_u64(map in shape_map(any::<u64>(), 9)) {
        prop_assert_eq!(round_trip::<_, D>(&map).is_ok(), naive_check::<u64, u64>(&map.mpt));
    }

    #[test]
    fn shape_keys_agree_with_naive_check_pair(map in shape_map(any::<[u8; 2]>(), 2)) {
        prop_assert_eq!(round_trip::<_, D>(&map).is_ok(), naive_check::<[u8; 2], u64>(&map.mpt));
    }
}

type Rc0 = Map<ArenaKey<<D as DB>::Hasher>, ChildRef<D>, D>;
type Big = [u8; crate::storable::SMALL_OBJECT_LIMIT];

/// The nodes an `Rc0`'s `ChildRef`s point to, which must stay allocated.
#[derive(Debug)]
struct Referents(Vec<Sp<u64, D>>, Vec<Sp<Big, D>>);

impl Referents {
    fn key(&mut self, v: u64, big: bool) -> ArenaKey<<D as DB>::Hasher> {
        if big {
            let sp = Sp::new([v as u8; crate::storable::SMALL_OBJECT_LIMIT]);
            let key = sp.as_child();
            self.1.push(sp);
            key
        } else {
            let sp = Sp::new(v);
            let key = sp.as_child();
            self.0.push(sp);
            key
        }
    }
}

// Keys of in-lined and referenced nodes, stored under their own key, under
// another key, or under a raw path.
fn rc_0() -> impl Strategy<Value = (Rc0, Referents)> {
    let entry = (
        0u64..4,
        any::<bool>(),
        0u8..4,
        prop::collection::vec(0u8..16, 0..6),
    );
    prop::collection::vec(entry, 0..8).prop_map(|entries| {
        let mut referents = Referents(vec![], vec![]);
        let mut mpt = MerklePatriciaTrie::new();
        for (v, big, how, raw) in entries {
            let k = referents.key(v, big);
            mpt = match how {
                0 | 1 => mpt.insert(&to_nibbles(&k), ChildRef::new(k.clone())),
                2 => mpt.insert(&to_nibbles(&k), ChildRef::new(referents.key(v + 1, big))),
                _ => mpt.insert(&raw, ChildRef::new(k.clone())),
            };
        }
        let map = Map {
            mpt: Sp::new(mpt),
            key_type: PhantomData,
        };
        (map, referents)
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1024))]

    #[test]
    fn self_keyed_maps_accept_exactly_pinned_decodable_keys((map, _referents) in rc_0()) {
        let expected = map.mpt.iter().all(|(path, v)| {
            from_nibbles::<ArenaKey<<D as DB>::Hasher>>(&path).is_ok()
                && to_nibbles(&v.child) == path
        });
        prop_assert_eq!(round_trip::<_, D>(&map).is_ok(), expected);
    }

    #[test]
    fn self_keyed_maps_reject_shared_subtrees(
        (map, _referents) in rc_0(),
        i in any::<prop::sample::Index>(),
    ) {
        // Graft the trie under two different branches: every leaf then has two
        // paths, at most one of which matches its key.
        prop_assume!(!matches!(*map.mpt.0, Node::Empty));
        let mut children: [Sp<Node<ChildRef<D>, D>, D>; 16] =
            std::array::from_fn(|_| Sp::new(Node::Empty));
        let (a, b) = (i.index(16), (i.index(16) + 1) % 16);
        children[a] = map.mpt.0.clone();
        children[b] = map.mpt.0.clone();
        let size = 2 * Node::ann(&map.mpt.0).0;
        let grafted: Rc0 = Map {
            mpt: Sp::new(MerklePatriciaTrie(Sp::new(Node::Branch {
                ann: SizeAnn(size),
                children: Box::new(children),
            }))),
            key_type: PhantomData,
        };
        prop_assert!(round_trip::<_, D>(&grafted).is_err());
    }
}

#[test]
fn rc_0_keys_cover_both_kinds() {
    let mut referents = Referents(vec![], vec![]);
    assert!(matches!(referents.key(0, false), ArenaKey::Direct(_)));
    assert!(matches!(referents.key(0, true), ArenaKey::Ref(_)));
}
