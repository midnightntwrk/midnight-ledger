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

//! Decoding checks canonicity node by node; these tests compare it against a
//! naive whole-trie check, on arbitrary and often malformed tries.

use crate::db::InMemoryDB;
use crate::merkle_patricia_trie::*;
use crate::storage::Map;
use proptest::prelude::*;
use proptest::sample::Index;
use serialize::{Deserializable, Serializable};
use std::collections::HashSet;

type D = InMemoryDB;
type N = Node<u64, D, SizeAnn>;

#[derive(Debug, Clone)]
enum Spec {
    Empty,
    Leaf(u64, u64),
    Branch(Vec<Option<Index>>, u64),
    Extension(Vec<u8>, Index, u64),
    MidBranchLeaf(u64, Index, u64),
}

// Annotations are usually correct and sometimes off by a little, paths
// sometimes empty, overlong or holding a non-nibble.
fn spec() -> impl Strategy<Value = Spec> {
    let skew = prop_oneof![8 => Just(0u64), 1 => 1u64..3];
    let path = prop_oneof![
        prop::collection::vec(0u8..16, 0..4),
        prop::collection::vec(0u8..17, 1..4),
        Just(vec![1u8; 255]),
        Just(vec![1u8; 256]),
    ];
    prop_oneof![
        Just(Spec::Empty),
        (0u64..3, skew.clone()).prop_map(|(v, s)| Spec::Leaf(v, s)),
        (
            prop::collection::vec(prop::option::weighted(0.25, any::<Index>()), 16),
            skew.clone()
        )
            .prop_map(|(c, s)| Spec::Branch(c, s)),
        (path, any::<Index>(), skew.clone()).prop_map(|(p, c, s)| Spec::Extension(p, c, s)),
        (0u64..3, any::<Index>(), skew).prop_map(|(v, c, s)| Spec::MidBranchLeaf(v, c, s)),
    ]
}

fn size(node: &Sp<N, D>) -> u64 {
    N::ann(node).0
}

fn build(specs: &[Spec]) -> Sp<N, D> {
    let mut nodes: Vec<Sp<N, D>> = vec![Sp::new(Node::Empty)];
    for spec in specs {
        let pick = |i: &Index| i.get(&nodes).clone();
        let node = match spec {
            Spec::Empty => Node::Empty,
            Spec::Leaf(v, s) => Node::Leaf {
                ann: SizeAnn(1 + s),
                value: Sp::new(*v),
            },
            Spec::Branch(children, s) => {
                let children: [Sp<N, D>; 16] = std::array::from_fn(|i| {
                    children[i]
                        .as_ref()
                        .map_or_else(|| Sp::new(Node::Empty), pick)
                });
                Node::Branch {
                    ann: SizeAnn(children.iter().map(size).sum::<u64>() + s),
                    children: Box::new(children),
                }
            }
            Spec::Extension(path, child, s) => {
                let child = pick(child);
                Node::Extension {
                    ann: SizeAnn(size(&child) + s),
                    compressed_path: path.clone(),
                    child,
                }
            }
            Spec::MidBranchLeaf(v, child, s) => {
                let child = pick(child);
                Node::MidBranchLeaf {
                    ann: SizeAnn(size(&child) + 1 + s),
                    value: Sp::new(*v),
                    child,
                }
            }
        };
        nodes.push(Sp::new(node));
    }
    nodes.pop().unwrap()
}

/// A naive per-node check, in which a size overflowing `u64` is rejected
/// rather than wrapping or panicking.
fn naive_node_check(node: &N) -> bool {
    match node {
        Node::Empty | Node::Leaf { .. } => true,
        Node::Branch { ann, children } => {
            children
                .iter()
                .filter(|c| !matches!(***c, Node::Empty))
                .count()
                >= 2
                && children
                    .iter()
                    .try_fold(0u64, |acc, c| acc.checked_add(size(c)))
                    == Some(ann.0)
        }
        Node::Extension {
            ann,
            compressed_path,
            child,
        } => {
            !(matches!(**child, Node::Extension { .. }) && compressed_path.len() != 255)
                && compressed_path.len() <= 255
                && ann.0 == size(child)
                && compressed_path.iter().all(|b| *b <= 0x0f)
        }
        Node::MidBranchLeaf { ann, child, .. } => {
            matches!(**child, Node::Branch { .. } | Node::Extension { .. })
                && size(child).checked_add(1) == Some(ann.0)
        }
    }
}

/// A naive whole-trie canonicity check.
fn naive_canonicity(node: &N) -> bool {
    fn structure(node: &N) -> bool {
        match node {
            Node::Empty | Node::Leaf { .. } => true,
            Node::Branch { children, .. } => {
                children
                    .iter()
                    .filter(|c| !matches!(***c, Node::Empty))
                    .count()
                    >= 2
                    && children.iter().all(|c| structure(c))
            }
            Node::Extension {
                compressed_path,
                child,
                ..
            } => {
                !compressed_path.is_empty()
                    && compressed_path.len() <= 255
                    && compressed_path.iter().all(|b| *b <= 0x0f)
                    && !matches!(**child, Node::Empty)
                    && !(matches!(**child, Node::Extension { .. }) && compressed_path.len() != 255)
                    && structure(child)
            }
            Node::MidBranchLeaf { child, .. } => {
                matches!(**child, Node::Branch { .. } | Node::Extension { .. }) && structure(child)
            }
        }
    }
    fn sum_ann(node: &N) -> Option<u64> {
        let (ann, true_val) = match node {
            Node::Empty => return Some(0),
            Node::Leaf { ann, .. } => (ann, Some(1)),
            Node::Branch { ann, children } => (
                ann,
                children
                    .iter()
                    .try_fold(0u64, |acc, c| acc.checked_add(sum_ann(c)?)),
            ),
            Node::Extension { ann, child, .. } => (ann, sum_ann(child)),
            Node::MidBranchLeaf { ann, child, .. } => (ann, sum_ann(child)?.checked_add(1)),
        };
        true_val.filter(|v| *v == ann.0)
    }
    structure(node) && sum_ann(node).is_some()
}

fn naive_decode_accepts(root: &Sp<N, D>) -> bool {
    let mut seen = HashSet::new();
    let mut frontier = vec![root.clone()];
    while let Some(node) = frontier.pop() {
        if !seen.insert(node.hash()) {
            continue;
        }
        if !naive_node_check(&node) {
            return false;
        }
        match &*node {
            Node::Empty | Node::Leaf { .. } => {}
            Node::Branch { children, .. } => frontier.extend(children.iter().cloned()),
            Node::Extension { child, .. } | Node::MidBranchLeaf { child, .. } => {
                frontier.push(child.clone())
            }
        }
    }
    naive_canonicity(root)
}

// Non-nibbles and paths longer than 255 have no wire encoding: serializing
// such a trie yields a different trie.
fn representable(specs: &[Spec]) -> bool {
    specs.iter().all(|spec| match spec {
        Spec::Extension(path, ..) => path.len() <= 255 && path.iter().all(|b| *b <= 0x0f),
        _ => true,
    })
}

// Honest tries, some over shared subtrees, which are valid and larger than
// the random graphs above usually manage to be while staying valid.
fn honest() -> impl Strategy<Value = Sp<N, D>> {
    let entries = prop::collection::vec((prop::collection::vec(0u8..16, 0..6), 0u64..3), 0..40);
    (0usize..2, 0u32..4, 0u64..3, entries).prop_map(|(prefix, free, v, entries)| {
        let base = match free {
            0 => MerklePatriciaTrie::new(),
            _ => (*Map::<[u8; 2], u64, D>::shared_full(&vec![0; prefix], free, v).mpt).clone(),
        };
        entries.iter().fold(base, |mpt, (k, v)| mpt.insert(k, *v)).0
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2048))]

    #[test]
    fn checks_agree_with_naive_checks(
        (root, representable) in prop_oneof![
            prop::collection::vec(spec(), 1..12).prop_map(|specs| (build(&specs), representable(&specs))),
            honest().prop_map(|root| (root, true)),
        ],
    ) {
        let expected = naive_decode_accepts(&root);
        let mpt = Sp::new(MerklePatriciaTrie::<u64, D>(root));
        prop_assert_eq!(mpt.canonicity().is_ok(), expected);
        if representable {
            let mut bytes = Vec::new();
            mpt.serialize(&mut bytes).unwrap();
            let decoded = Sp::<MerklePatriciaTrie<u64, D>, D>::deserialize(&mut &bytes[..], 0);
            prop_assert_eq!(decoded.is_ok(), expected, "{:?}", decoded.err());
        }
    }
}
