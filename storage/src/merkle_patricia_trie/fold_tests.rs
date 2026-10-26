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

use super::*;
use crate::db::InMemoryDB;
use crate::storage::{Array, HashMap, HashSet, Map, MultiSet};
use proptest::prelude::*;
use std::collections::HashSet as StdHashSet;

type D = InMemoryDB;
type Mpt = MerklePatriciaTrie<u64, D>;

#[derive(Clone, Debug, PartialEq)]
struct Concat<T>(Vec<T>);

impl<T: Clone> Semigroup for Concat<T> {
    fn append(&self, other: &Self) -> Self {
        Concat([&self.0[..], &other.0[..]].concat())
    }
}

impl<T: Clone> Monoid for Concat<T> {
    fn empty() -> Self {
        Concat(Vec::new())
    }
}

fn nibbles(max_len: usize) -> impl Strategy<Value = Vec<u8>> + Clone {
    prop::collection::vec(0u8..16, 0..=max_len)
}

fn insert_all(mpt: Mpt, entries: &[(Vec<u8>, u64)]) -> Mpt {
    entries.iter().fold(mpt, |mpt, (k, v)| mpt.insert(k, *v))
}

// Random tries, and tries with shared subtrees with random entries inserted
// on top, which leaves some subtrees shared and others not.
fn trie() -> impl Strategy<Value = Mpt> {
    let entries = prop::collection::vec((nibbles(5), 0u64..3), 0..40);
    prop_oneof![
        entries.clone().prop_map(|e| insert_all(Mpt::new(), &e)),
        (0usize..2, 1u32..4, 0u64..3, entries).prop_map(|(prefix, free, v, e)| {
            let shared = Map::<[u8; 2], u64, D>::shared_full(&vec![0; prefix], free, v);
            insert_all((*shared.mpt).clone(), &e)
        }),
    ]
}

// Branches skip their empty children rather than visiting them.
fn distinct_nodes(mpt: &Mpt) -> usize {
    let mut seen = StdHashSet::new();
    let mut frontier = vec![mpt.0.clone()];
    while let Some(node) = frontier.pop() {
        if !seen.insert(node.hash()) {
            continue;
        }
        match &*node {
            Node::Empty | Node::Leaf { .. } => {}
            Node::Branch { children, .. } => frontier.extend(
                children
                    .iter()
                    .filter(|child| !matches!(***child, Node::Empty))
                    .cloned(),
            ),
            Node::Extension { child, .. } | Node::MidBranchLeaf { child, .. } => {
                frontier.push(child.clone())
            }
        }
    }
    seen.len()
}

proptest! {
    #[test]
    fn fold_matches_iter_and_visits_each_node_once(mpt in trie()) {
        let mut memo = DagMemo::new();
        let values = mpt.fold(&mut memo, |_, v| Concat(vec![**v]));
        prop_assert_eq!(values.0, mpt.iter().map(|(_, v)| *v).collect::<Vec<_>>());
        prop_assert_eq!(memo.len(), distinct_nodes(&mpt));
    }

    #[test]
    fn distinct_values_count_entries(mpt in trie()) {
        let mut expected = std::collections::BTreeMap::<u64, u128>::new();
        for (_, v) in mpt.iter() {
            *expected.entry(*v).or_default() += 1;
        }
        let mut actual = std::collections::BTreeMap::<u64, u128>::new();
        for (v, n) in mpt.distinct_values() {
            prop_assert!(actual.insert(*v, n).is_none(), "value listed twice");
        }
        prop_assert_eq!(actual, expected);
    }

    #[test]
    fn fold_paths_reconstructs_iter_paths(mpt in trie()) {
        let paths = mpt.fold_paths(
            &mut DagMemo::new(),
            |_, _| Concat(vec![Vec::new()]),
            &|seg: &[u8], r: Concat<Vec<u8>>| {
                Concat(r.0.into_iter().map(|p| [seg, &p[..]].concat()).collect())
            },
        );
        prop_assert_eq!(paths.0, mpt.iter().map(|(p, _)| p).collect::<Vec<_>>());
    }

    #[test]
    fn collection_folds_match_naive(
        values in prop::collection::vec(0u64..4, 0..40),
        log16_len in 1u32..3,
        shared_value in 0u64..4,
    ) {
        let sum = |memo: &mut DagMemo<u64, D>, v: &Sp<u64, D>| { let _ = memo; **v };

        let arr: Array<u64, D> = values.iter().copied().collect();
        prop_assert_eq!(arr.fold(&mut DagMemo::new(), sum), values.iter().sum::<u64>());
        let shared = Array::<u64, D>::shared_dense(log16_len, shared_value);
        prop_assert_eq!(
            shared.fold(&mut DagMemo::new(), |_, _| 1u64),
            16u64.pow(log16_len)
        );

        let map = values.iter().enumerate().fold(Map::<u64, u64, D>::new(), |m, (i, v)| m.insert(i as u64, *v));
        prop_assert_eq!(
            map.fold(&mut DagMemo::new(), |_, v| Concat(vec![**v])).0,
            map.iter().map(|(_, v)| *v).collect::<Vec<_>>()
        );

        let hmap = values.iter().enumerate().fold(HashMap::<u64, u64, D>::new(), |m, (i, v)| m.insert(i as u64, *v));
        prop_assert_eq!(
            hmap.fold(&mut DagMemo::new(), |_, k, v| Concat(vec![(**k, **v)])).0,
            hmap.iter().map(|kv| (*kv.0, *kv.1)).collect::<Vec<_>>()
        );

        let hset = values.iter().fold(HashSet::<u64, D>::new(), |s, v| s.insert(*v));
        prop_assert_eq!(
            hset.fold(&mut DagMemo::new(), |_, v| Concat(vec![**v])).0,
            hset.iter().map(|v| **v).collect::<Vec<_>>()
        );

        let mset = values.iter().fold(MultiSet::<u64, D>::new(), |s, v| s.insert(*v));
        prop_assert_eq!(mset.fold(&mut DagMemo::new(), sum), values.iter().sum::<u64>());
    }
}
