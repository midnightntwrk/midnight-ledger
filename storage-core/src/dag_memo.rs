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

//! Memoization of functions over the storage DAG.
//!
//! Encodings deduplicate nodes by hash, so a value's tree can be exponentially
//! larger than its distinct nodes. A recursive function that caches its result
//! per distinct node through a [`DagMemo`] does work linear in the latter.

use crate::Storable;
use crate::arena::{ArenaHash, Sp};
use crate::db::DB;
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::marker::PhantomData;

/// Caches a function's result per distinct node.
///
/// Results must depend only on the node's subtree, never on where it is
/// reached from. Nodes are keyed by type as well as hash, as two types may
/// share an encoding. The function is not part of the key: use one memo per
/// function, or a second fold reads the first's results.
pub struct DagMemo<R, D: DB> {
    results: HashMap<(TypeId, ArenaHash<D::Hasher>), R>,
}

impl<R, D: DB> Default for DagMemo<R, D> {
    fn default() -> Self {
        DagMemo {
            results: HashMap::new(),
        }
    }
}

impl<R: Clone, D: DB> DagMemo<R, D> {
    /// Creates an empty memo.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns `f`'s result for `sp`, running `f` only if no result is
    /// cached for it. `f` recurses into children through the memo it is passed.
    pub fn get_or_compute<T: Storable<D>>(
        &mut self,
        sp: &Sp<T, D>,
        f: impl FnOnce(&mut Self, &T) -> R,
    ) -> R {
        let key = (TypeId::of::<T>(), sp.hash());
        if let Some(res) = self.results.get(&key) {
            return res.clone();
        }
        let res = f(self, sp);
        self.results.insert(key, res.clone());
        res
    }

    /// The number of distinct nodes computed so far.
    pub fn len(&self) -> usize {
        self.results.len()
    }

    /// Whether no node has been computed yet.
    pub fn is_empty(&self) -> bool {
        self.results.is_empty()
    }
}

/// State per invariant check, usually a [`DagMemo`], shared by every check
/// made during a single decode, so checks that walk shared children walk each once.
pub struct InvariantMemos<D: DB> {
    states: HashMap<TypeId, Box<dyn Any>>,
    _db: PhantomData<D>,
}

impl<D: DB> Default for InvariantMemos<D> {
    fn default() -> Self {
        InvariantMemos {
            states: HashMap::new(),
            _db: PhantomData,
        }
    }
}

impl<D: DB> InvariantMemos<D> {
    /// Creates an empty set of memos.
    pub fn new() -> Self {
        Self::default()
    }

    /// The state of type `S` for the check named by the marker type `C`.
    pub fn get<C: 'static, S: Default + 'static>(&mut self) -> &mut S {
        self.states
            .entry(TypeId::of::<(C, S)>())
            .or_insert_with(|| Box::new(S::default()))
            .downcast_mut()
            .expect("keyed by its state's type")
    }

    /// The memo for the check named by the marker type `C`.
    pub fn memo<C: 'static, R: Clone + 'static>(&mut self) -> &mut DagMemo<R, D> {
        self.get::<C, DagMemo<R, D>>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arena::ArenaKey;
    use crate::db::InMemoryDB;
    use crate::storable::{Loader, SMALL_OBJECT_LIMIT};
    use crate::{self as storage, Storable};
    use derive_where::derive_where;
    use proptest::prelude::*;
    use proptest::sample::Index;
    use serialize::Tagged;
    use std::collections::HashSet;

    type D = InMemoryDB;

    // Too large to be inlined into its parent's encoding.
    #[derive(Storable)]
    #[derive_where(Clone, PartialEq, Eq)]
    #[tag = "test-big-tree"]
    #[storable(db = D)]
    struct BigTree<D: DB> {
        value: u64,
        left: Option<Sp<BigTree<D>, D>>,
        right: Option<Sp<BigTree<D>, D>>,
        pad: [u8; SMALL_OBJECT_LIMIT],
    }

    // Small enough to be inlined into its parent's encoding.
    #[derive(Storable)]
    #[derive_where(Clone, PartialEq, Eq)]
    #[tag = "test-small-tree"]
    #[storable(db = D)]
    struct SmallTree<D: DB> {
        value: u8,
        left: Option<Sp<SmallTree<D>, D>>,
        right: Option<Sp<SmallTree<D>, D>>,
    }

    trait Tree: Storable<D> {
        fn value(&self) -> u64;
        fn subtrees(&self) -> [Option<&Sp<Self, D>>; 2];
        fn build(value: u64, left: Option<Sp<Self, D>>, right: Option<Sp<Self, D>>) -> Self;
    }

    impl Tree for BigTree<D> {
        fn value(&self) -> u64 {
            self.value
        }
        fn subtrees(&self) -> [Option<&Sp<Self, D>>; 2] {
            [self.left.as_ref(), self.right.as_ref()]
        }
        fn build(value: u64, left: Option<Sp<Self, D>>, right: Option<Sp<Self, D>>) -> Self {
            BigTree {
                value,
                left,
                right,
                pad: [0; SMALL_OBJECT_LIMIT],
            }
        }
    }

    impl Tree for SmallTree<D> {
        fn value(&self) -> u64 {
            self.value as u64
        }
        fn subtrees(&self) -> [Option<&Sp<Self, D>>; 2] {
            [self.left.as_ref(), self.right.as_ref()]
        }
        fn build(value: u64, left: Option<Sp<Self, D>>, right: Option<Sp<Self, D>>) -> Self {
            SmallTree {
                value: value as u8,
                left,
                right,
            }
        }
    }

    type Shape = Vec<(u64, Option<Index>, Option<Index>)>;

    // Each node draws its children from the nodes before it, so sharing is
    // arbitrary. Few distinct values make separately built nodes coincide.
    fn shape() -> impl Strategy<Value = Shape> {
        prop::collection::vec(
            (0u64..3, any::<Option<Index>>(), any::<Option<Index>>()),
            1..16,
        )
    }

    fn build<T: Tree>(shape: &Shape) -> Sp<T, D> {
        let mut nodes: Vec<Sp<T, D>> = Vec::new();
        for (value, left, right) in shape {
            let pick = |i: &Option<Index>| {
                i.as_ref()
                    .filter(|_| !nodes.is_empty())
                    .map(|i| i.get(&nodes).clone())
            };
            nodes.push(Sp::new(T::build(*value, pick(left), pick(right))));
        }
        nodes.pop().unwrap()
    }

    fn distinct<T: Tree>(root: &Sp<T, D>) -> usize {
        let mut seen = HashSet::new();
        let mut frontier = vec![root.clone()];
        while let Some(node) = frontier.pop() {
            if seen.insert(node.hash()) {
                frontier.extend(node.subtrees().into_iter().flatten().cloned());
            }
        }
        seen.len()
    }

    // Position-sensitive, so a memo returning a sibling's result would show.
    fn naive<T: Tree>(node: &T) -> u64 {
        let [l, r] = node.subtrees().map(|c| c.map_or(1, |c| naive(&**c)));
        node.value()
            .wrapping_add(l.wrapping_mul(31))
            .wrapping_add(r.wrapping_mul(961))
    }

    fn memoized<T: Tree>(sp: &Sp<T, D>, memo: &mut DagMemo<u64, D>, calls: &mut usize) -> u64 {
        memo.get_or_compute(sp, |memo, node| {
            *calls += 1;
            let [l, r] = node
                .subtrees()
                .map(|c| c.map_or(1, |c| memoized(c, memo, calls)));
            node.value()
                .wrapping_add(l.wrapping_mul(31))
                .wrapping_add(r.wrapping_mul(961))
        })
    }

    fn check<T: Tree>(shape: &Shape) -> Result<(), TestCaseError> {
        let root = build::<T>(shape);
        let mut memo = DagMemo::new();
        let mut calls = 0;
        let res = memoized(&root, &mut memo, &mut calls);
        prop_assert_eq!(calls, distinct(&root));
        prop_assert_eq!(memo.len(), calls);
        prop_assert_eq!(res, naive(&*root));
        Ok(())
    }

    proptest! {
        #[test]
        fn computes_once_per_distinct_node_and_matches_naive(shape in shape()) {
            check::<BigTree<D>>(&shape)?;
            check::<SmallTree<D>>(&shape)?;
        }
    }

    #[derive(Storable)]
    #[derive_where(Clone, PartialEq, Eq)]
    #[tag = "test-small-tree-twin"]
    #[storable(db = D)]
    struct SmallTreeTwin<D: DB> {
        value: u8,
        left: Option<Sp<SmallTreeTwin<D>, D>>,
        right: Option<Sp<SmallTreeTwin<D>, D>>,
    }

    #[test]
    fn types_sharing_an_encoding_are_memoized_separately() {
        let a = Sp::<SmallTree<D>, D>::new(SmallTree::build(5, None, None));
        let b = Sp::<SmallTreeTwin<D>, D>::new(SmallTreeTwin {
            value: 5,
            left: None,
            right: None,
        });
        assert_eq!(
            a.hash(),
            b.hash(),
            "test needs types with a shared encoding"
        );
        let mut memo = DagMemo::<&str, D>::new();
        assert_eq!(memo.get_or_compute(&a, |_, _| "a"), "a");
        assert_eq!(memo.get_or_compute(&b, |_, _| "b"), "b");
    }
}
