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

//! Constructors for collections whose tries share subtrees, so that a
//! collection of `16^k` entries is built from `O(k)` distinct nodes.
//!
//! An honest insert-built collection of identical values shares subtrees in
//! the same way, but takes `O(16^k)` inserts to build.

use super::{Array, Map};
use crate::arena::Sp;
use crate::db::DB;
use crate::merkle_patricia_trie::{MerklePatriciaTrie, Node};
use crate::storable::{SizeAnn, Storable};
use serialize::Serializable;
use std::marker::PhantomData;

type N<V, D> = Node<V, D, SizeAnn>;

fn branch<V: Storable<D>, D: DB>(children: [Sp<N<V, D>, D>; 16]) -> N<V, D> {
    let ann = SizeAnn(
        children
            .iter()
            .fold(0u64, |acc, child| acc.wrapping_add(Node::ann(child).0)),
    );
    Node::Branch {
        ann,
        children: Box::new(children),
    }
}

/// Every nibble path of length `0..=depth` holds `value`.
fn all_paths_up_to<V: Storable<D>, D: DB>(depth: u32, value: &Sp<V, D>) -> Sp<N<V, D>, D> {
    let mut node = Sp::new(Node::Leaf {
        ann: SizeAnn(1),
        value: value.clone(),
    });
    for _ in 0..depth {
        let child = Sp::new(branch(std::array::from_fn(|_| node.clone())));
        node = Sp::new(Node::MidBranchLeaf {
            ann: SizeAnn(Node::ann(&child).0.wrapping_add(1)),
            value: value.clone(),
            child,
        });
    }
    node
}

/// Every nibble path of length exactly `depth` holds `value`.
fn all_paths_of<V: Storable<D>, D: DB>(depth: u32, value: &Sp<V, D>) -> Sp<N<V, D>, D> {
    let mut node = Sp::new(Node::Leaf {
        ann: SizeAnn(1),
        value: value.clone(),
    });
    for _ in 0..depth {
        node = Sp::new(branch(std::array::from_fn(|_| node.clone())));
    }
    node
}

impl<V: Storable<D>, D: DB> Array<V, D> {
    /// An array of `16^log16_len` copies of `value`, built from
    /// `O(log16_len)` distinct nodes.
    pub fn shared_dense(log16_len: u32, value: V) -> Self {
        Self::shared_dense_scaled(log16_len, 16, value)
    }

    /// An array of `c * 16^(k-1)` copies of `value`, built from `O(k)`
    /// distinct nodes. Below `2^64` while `k < 16`, or `k == 16` and `c < 16`.
    pub fn shared_dense_scaled(k: u32, c: u32, value: V) -> Self {
        assert!(k >= 1 && (2..=16).contains(&c) && (k >= 2 || c == 16));
        let value = Sp::new(value);
        // Array indices are nibble strings without leading zeroes: index 0 is
        // the root's own value, and below each nonzero first nibble lie all
        // paths of length `0..k`, one nibble shorter from `c` on.
        let long = all_paths_up_to(k - 1, &value);
        let short = (c < 16).then(|| all_paths_up_to(k - 2, &value));
        let children: [Sp<N<V, D>, D>; 16] = std::array::from_fn(|i| match i {
            0 => Sp::new(Node::Empty),
            i if (i as u32) < c => long.clone(),
            _ => short.clone().expect("c < 16"),
        });
        let child = Sp::new(branch(children));
        let root = Node::MidBranchLeaf {
            ann: SizeAnn(Node::ann(&child).0.wrapping_add(1)),
            value,
            child,
        };
        Array(Sp::new(MerklePatriciaTrie(Sp::new(root))))
    }
}

impl<K: Serializable, V: Storable<D>, D: DB> Map<K, V, D> {
    /// A map with `value` at every key whose nibble path is `prefix` followed
    /// by `free` arbitrary nibbles, built from `O(free)` distinct nodes.
    ///
    /// The caller ensures every such path decodes as a `K`. The recorded size
    /// wraps at `2^64`, as an adversarial encoding's would.
    pub fn shared_full(prefix: &[u8], free: u32, value: V) -> Self {
        assert!(prefix.len() <= 255 && prefix.iter().all(|n| *n <= 0x0f));
        let mut node = all_paths_of(free, &Sp::new(value));
        if !prefix.is_empty() {
            node = Sp::new(Node::Extension {
                ann: Node::ann(&node),
                compressed_path: prefix.to_vec(),
                child: node,
            });
        }
        Map {
            mpt: Sp::new(MerklePatriciaTrie(node)),
            key_type: PhantomData,
        }
    }
}
