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

//! Merkle Patricia Tries.

use crate as storage;
use std::cmp::Ordering;
use std::fmt::Debug;
use std::hash::Hash;
use std::io::{Read, Write};
use std::iter::{empty, once};
use std::ops::Deref;

use crate::DefaultDB;
use crate::Storable;
use crate::arena::{ArenaKey, Sp};
use crate::dag_memo::DagMemo;
use crate::db::DB;
use crate::storable::{Loader, SizeAnn};
use derive_where::derive_where;
use serialize::{self, Deserializable, Serializable, Tagged, tag_enforcement_test};
pub use storage_core::monoid::{Monoid, Semigroup};

scoped_tls::scoped_thread_local!(
    /// While set, the trie invariants on the current thread accept a
    /// [`Node::Extension`] with a path shorter than 255 nibbles whose child is
    /// another [`Node::Extension`].
    ///
    /// `Node::insert` has produced such tries when splitting a 255-nibble
    /// extension whose child is itself an extension, and they exist in chain
    /// state. This lets clients load that state as-is, preserving its hash.
    ///
    /// <div class="warning">
    ///
    /// **Client-side mitigation only. NEVER set this in a node, or in any code
    /// that validates data for consensus.** A node that deserializes under it
    /// accepts wire data that every other node rejects, and splits from the
    /// network.
    ///
    /// </div>
    pub static ALLOW_SHORT_EXTENSION_CHAINS: ()
);

/// A Merkle Patricia Trie
#[derive_where(Debug, Eq, Clone, PartialEq; V, A)]
#[derive(Storable)]
#[storable(db = D)]
pub struct MerklePatriciaTrie<
    V: Storable<D>,
    D: DB = DefaultDB,
    A: Storable<D> + Annotation<V> = SizeAnn,
>(
    #[cfg(feature = "public-internal-structure")] pub Sp<Node<V, D, A>, D>,
    #[cfg(not(feature = "public-internal-structure"))] pub(crate) Sp<Node<V, D, A>, D>,
);

impl<V: Storable<D> + Tagged, D: DB, A: Storable<D> + Annotation<V> + Tagged> Tagged
    for MerklePatriciaTrie<V, D, A>
{
    fn tag() -> std::borrow::Cow<'static, str> {
        format!("mpt({},{})", V::tag(), A::tag()).into()
    }
    fn tag_unique_factor() -> String {
        <Node<V, D, A>>::tag_unique_factor()
    }
}
tag_enforcement_test!(MerklePatriciaTrie<()>);

impl<V: Storable<D>, D: DB, A: Storable<D> + Annotation<V>> Default
    for MerklePatriciaTrie<V, D, A>
{
    fn default() -> Self {
        Self::new()
    }
}

impl<V: Storable<D>, D: DB, A: Storable<D> + Annotation<V>> MerklePatriciaTrie<V, D, A> {
    /// Construct an empty trie
    pub fn new() -> Self {
        MerklePatriciaTrie(Sp::new(Node::Empty))
    }

    /// Check that the trie is in canonical form, returning a description of the
    /// violated rule otherwise. A well-formed trie is *canonical*: its structure
    /// is uniquely determined by its key-value contents (see the type-level
    /// docs).
    ///
    /// Every rule is local to a node and its children, so deserialization
    /// enforces this by checking each decoded node. This checks a trie built
    /// in memory, visiting each distinct node once.
    pub fn canonicity(&self) -> Result<(), std::io::Error> {
        let mut seen = std::collections::HashSet::new();
        let mut stack = vec![&self.0];
        while let Some(sp) = stack.pop() {
            if seen.insert(sp.hash()) {
                Storable::<D>::check_invariant(&**sp).map_err(|e| {
                    std::io::Error::new(std::io::ErrorKind::InvalidData, format!("canonicity: {e}"))
                })?;
                stack.extend(sp.non_empty_children());
            }
        }
        Ok(())
    }

    /// Whether [`Self::canonicity`] holds.
    pub fn is_canonical(&self) -> bool {
        self.canonicity().is_ok()
    }

    /// Insert a value into the trie
    pub fn insert(&self, path: &[u8], value: V) -> Self {
        MerklePatriciaTrie(Node::<V, D, A>::insert(&self.0, path, value).0)
    }

    /// Lookup a value in the trie
    pub fn lookup(&self, path: &[u8]) -> Option<&V> {
        Node::<V, D, A>::lookup(&self.0, path)
    }

    /// Prunes all paths which are lexicographically less than the `target_path`.
    /// Returns the updated tree, and a vector of the removed leaves.
    pub(crate) fn prune(
        &self,
        target_path: &[u8],
        // Returns "is this node empty?" so we can collapse parts of the tree as we go
    ) -> (Self, Vec<Sp<V, D>>) {
        let (node, pruned) = Node::<V, D, A>::prune(&self.0, target_path);
        (MerklePatriciaTrie(node), pruned)
    }

    /// Lookup a value in the trie
    pub fn lookup_sp(&self, path: &[u8]) -> Option<Sp<V, D>> {
        Node::<V, D, A>::lookup_sp(&self.0, path)
    }

    /// Given a path, find the nearest predecessor to that path
    pub(crate) fn find_predecessor<'a>(&'a self, path: &[u8]) -> Option<(Vec<u8>, &'a V)> {
        let mut best_predecessor = None;
        Node::<V, D, A>::find_predecessor_recursive(
            &self.0,
            path,
            &mut std::vec::Vec::new(),
            &mut best_predecessor,
        );
        best_predecessor
    }

    /// Remove a value from the trie
    pub fn remove(&self, path: &[u8]) -> Self {
        MerklePatriciaTrie(Node::<V, D, A>::remove(&self.0, path).0)
    }

    /// Consume internal pointers, returning only the leaves left dangling by this.
    /// Used for custom `Drop` implementations.
    pub fn into_inner_for_drop(self) -> impl Iterator<Item = V> {
        Sp::into_inner(self.0)
            .into_iter()
            .flat_map(Node::into_inner_for_drop)
    }

    /// Combines `leaf` over every entry, in [`Self::iter`] order, counting an
    /// entry once per path it is reached by. Each distinct node is visited
    /// once, so a trie with shared subtrees folds in time linear in its
    /// distinct nodes.
    pub fn fold<R: Monoid + Clone>(
        &self,
        memo: &mut DagMemo<R, D>,
        mut leaf: impl FnMut(&mut DagMemo<R, D>, &Sp<V, D>) -> R,
    ) -> R {
        Node::fold(&self.0, memo, &mut leaf, &|_, r| r)
    }

    /// Each distinct value with the number of entries holding it, saturating,
    /// in an unspecified but deterministic order. Linear in the trie's distinct nodes, however
    /// many entries they stand for.
    pub fn distinct_values(&self) -> std::vec::Vec<(Sp<V, D>, u128)> {
        Self::distinct_values_across([(self, 1)])
    }

    /// As [`Self::distinct_values`] over several tries, each counted `weight`
    /// times. Linear in the distinct nodes of all of them together, so tries
    /// sharing subtrees cost no more than one holding them all.
    pub fn distinct_values_across<'a>(
        tries: impl IntoIterator<Item = (&'a Self, u128)>,
    ) -> std::vec::Vec<(Sp<V, D>, u128)>
    where
        Self: 'a,
    {
        type Key<D> = crate::arena::ArenaHash<<D as DB>::Hasher>;
        let mut paths = std::collections::HashMap::<Key<D>, u128>::new();
        // Reverse post-order visits every node after all of its parents.
        let mut order = std::vec::Vec::new();
        let mut seen = std::collections::HashSet::<Key<D>>::new();
        for (trie, weight) in tries.into_iter().filter(|(_, weight)| *weight > 0) {
            let count = paths.entry(trie.0.hash()).or_insert(0);
            *count = count.saturating_add(weight);
            let mut stack = vec![(trie.0.clone(), false)];
            while let Some((node, expanded)) = stack.pop() {
                if expanded {
                    order.push(node);
                } else if seen.insert(node.hash()) {
                    stack.push((node.clone(), true));
                    stack.extend(
                        node.non_empty_children()
                            .map(|child| (child.clone(), false)),
                    );
                }
            }
        }
        let mut index = std::collections::HashMap::<Key<D>, usize>::new();
        let mut values = std::vec::Vec::<(Sp<V, D>, u128)>::new();
        for node in order.iter().rev() {
            let n = paths[&node.hash()];
            for child in node.non_empty_children() {
                let count = paths.entry(child.hash()).or_insert(0);
                *count = count.saturating_add(n);
            }
            if let Node::Leaf { value, .. } | Node::MidBranchLeaf { value, .. } = node.deref() {
                let i = *index.entry(value.hash()).or_insert_with(|| {
                    values.push((value.clone(), 0));
                    values.len() - 1
                });
                values[i].1 = values[i].1.saturating_add(n);
            }
        }
        values
    }

    /// As [`Self::fold`], with `under(path, r)` applied to the result `r` of
    /// each subtree reached from its parent by the nibbles `path`.
    pub(crate) fn fold_paths<R: Monoid + Clone>(
        &self,
        memo: &mut DagMemo<R, D>,
        mut leaf: impl FnMut(&mut DagMemo<R, D>, &Sp<V, D>) -> R,
        under: &impl Fn(&[u8], R) -> R,
    ) -> R {
        Node::fold(&self.0, memo, &mut leaf, under)
    }

    /// Generate iterator over leaves, `(path, &value)`
    pub fn iter(&self) -> MPTIter<'_, V, D> {
        MPTIter(Node::<V, D, A>::leaves(&self.0, vec![]))
    }

    /// Get the number of leaves in a trie
    pub fn size(&self) -> usize {
        Node::<V, D, A>::size(&self.0)
    }

    /// Return true if the trie is empty, false otherwise
    pub fn is_empty(&self) -> bool {
        matches!(self.0.deref(), Node::Empty)
    }

    /// Retrieve the annotation on the root of the trie
    pub fn ann(&self) -> A {
        Node::<V, D, A>::ann(&self.0)
    }
}

impl<V: Storable<D>, D: DB, A: Storable<D> + Annotation<V>> Hash for MerklePatriciaTrie<V, D, A> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        <Sp<Node<V, D, A>, D> as Hash>::hash(&self.0, state)
    }

    fn hash_slice<H: std::hash::Hasher>(data: &[Self], state: &mut H)
    where
        Self: Sized,
    {
        Sp::<Node<V, D, A>, D>::hash_slice(
            &data
                .iter()
                .map(|d| d.0.clone())
                .collect::<std::vec::Vec<Sp<Node<V, D, A>, D>>>(),
            state,
        )
    }
}

impl<V: Storable<D> + Ord, D: DB, A: Storable<D> + Ord + Annotation<V>> Ord
    for MerklePatriciaTrie<V, D, A>
{
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.cmp(&other.0)
    }
}

impl<V: Storable<D> + PartialOrd, D: DB, A: Storable<D> + PartialOrd + Annotation<V>> PartialOrd
    for MerklePatriciaTrie<V, D, A>
{
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.0.partial_cmp(&other.0)
    }
}

/// Iterator over (path, value) pairs in `MerklePatriciaTrie`
pub struct MPTIter<'a, T: Storable<D> + 'static, D: DB>(
    Box<dyn Iterator<Item = (std::vec::Vec<u8>, Sp<T, D>)> + 'a>,
);

impl<T: Storable<D>, D: DB> Iterator for MPTIter<'_, T, D> {
    type Item = (std::vec::Vec<u8>, Sp<T, D>);

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next()
    }
}

#[derive(Debug, Default)]
#[derive_where(Clone, Hash, PartialEq, Eq; T, A)]
#[derive_where(PartialOrd; T: PartialOrd, A: PartialOrd)]
#[derive_where(Ord; T: Ord, A: Ord)]
#[allow(clippy::type_complexity)]
#[cfg(feature = "public-internal-structure")]
pub enum Node<T: Storable<D> + 'static, D: DB = DefaultDB, A: Storable<D> + Annotation<T> = SizeAnn>
{
    #[default]
    Empty,
    Leaf {
        ann: A,
        value: Sp<T, D>,
    },
    Branch {
        ann: A,
        children: Box<[Sp<Node<T, D, A>, D>; 16]>,
    },
    Extension {
        ann: A,
        compressed_path: std::vec::Vec<u8>, // with a length no longer than 255
        child: Sp<Node<T, D, A>, D>,
    },
    MidBranchLeaf {
        ann: A,
        value: Sp<T, D>,
        child: Sp<Node<T, D, A>, D>, // should only be an `Extension` or `Branch`
    },
}

// The MPT node
#[derive(Debug, Default)]
#[derive_where(Clone, Hash, PartialEq, Eq; T, A)]
#[derive_where(PartialOrd; T: PartialOrd, A: PartialOrd)]
#[derive_where(Ord; T: Ord, A: Ord)]
#[allow(clippy::type_complexity)]
#[cfg(not(feature = "public-internal-structure"))]
pub enum Node<
    T: Storable<D> + 'static,
    D: DB = DefaultDB,
    A: Storable<D> + Annotation<T> = SizeAnn,
> {
    #[default]
    Empty,
    Leaf {
        ann: A,
        value: Sp<T, D>,
    },
    Branch {
        ann: A,
        children: Box<[Sp<Node<T, D, A>, D>; 16]>,
    },
    Extension {
        ann: A,
        compressed_path: std::vec::Vec<u8>, // with a length no longer than 255
        child: Sp<Node<T, D, A>, D>,
    },
    MidBranchLeaf {
        ann: A,
        value: Sp<T, D>,
        child: Sp<Node<T, D, A>, D>, // should only be an `Extension` or `Branch`
    },
}

impl<T: Storable<D> + Tagged + 'static, D: DB, A: Storable<D> + Annotation<T> + Tagged> Tagged
    for Node<T, D, A>
{
    fn tag() -> std::borrow::Cow<'static, str> {
        format!("mpt-node({},{})", T::tag(), A::tag()).into()
    }
    fn tag_unique_factor() -> String {
        let a = A::tag();
        let t = T::tag();
        format!(
            "[(),({a},{t}),({a},array(mpt-node({a},{t}),16)),({a},vec(u8),mpt-node({a},{t})),({a},{t},mpt-node({a},{t}))]"
        )
    }
}
tag_enforcement_test!(Node<(), DefaultDB, SizeAnn>);

fn compress_nibbles(nibbles: &[u8]) -> std::vec::Vec<u8> {
    let mut compressed: std::vec::Vec<u8> = vec![0; (nibbles.len() / 2) + (nibbles.len() % 2)];
    for i in 0..nibbles.len() {
        if i % 2 == 0 {
            compressed[i / 2] |= nibbles[i] << 4;
        } else {
            compressed[i / 2] |= nibbles[i];
        }
    }

    compressed
}

fn expand_nibbles(compressed: &[u8], len: usize) -> std::vec::Vec<u8> {
    let mut nibbles = std::vec::Vec::new();
    for i in 0..len {
        if i % 2 == 0 {
            nibbles.push((compressed[i / 2] & 0xf0) >> 4);
        } else {
            nibbles.push(compressed[i / 2] & 0x0f);
        }
    }

    nibbles
}

impl<T: Storable<D>, D: DB, A: Storable<D> + Annotation<T>> Node<T, D, A> {
    fn fold<R: Monoid + Clone>(
        sp: &Sp<Self, D>,
        memo: &mut DagMemo<R, D>,
        leaf: &mut impl FnMut(&mut DagMemo<R, D>, &Sp<T, D>) -> R,
        under: &impl Fn(&[u8], R) -> R,
    ) -> R {
        memo.get_or_compute(sp, |memo, node| match node {
            Node::Empty => R::empty(),
            Node::Leaf { value, .. } => leaf(memo, value),
            Node::Extension {
                compressed_path,
                child,
                ..
            } => under(compressed_path, Self::fold(child, memo, leaf, under)),
            Node::Branch { children, .. } => children
                .iter()
                .enumerate()
                .filter(|(_, child)| !matches!(***child, Node::Empty))
                .fold(R::empty(), |acc, (i, child)| {
                    acc.append(&under(&[i as u8], Self::fold(child, memo, leaf, under)))
                }),
            Node::MidBranchLeaf { value, child, .. } => {
                Self::fold(child, memo, leaf, under).append(&leaf(memo, value))
            }
        })
    }

    fn into_inner_for_drop(self) -> impl Iterator<Item = T> {
        let res: Box<dyn Iterator<Item = T>> = match self {
            Node::Empty => Box::new(empty()),
            Node::Leaf { value, .. } => Box::new(Sp::into_inner(value).into_iter()),
            Node::Branch { children, .. } => Box::new(
                children
                    .into_iter()
                    .flat_map(Sp::into_inner)
                    .flat_map(Node::into_inner_for_drop),
            ),
            Node::Extension { child, .. } => Box::new(
                Sp::into_inner(child)
                    .into_iter()
                    .flat_map(Node::into_inner_for_drop),
            ),
            Node::MidBranchLeaf { value, child, .. } => Box::new(
                Sp::into_inner(value).into_iter().chain(
                    Sp::into_inner(child)
                        .into_iter()
                        .flat_map(Node::into_inner_for_drop),
                ),
            ),
        };
        res
    }
}

fn extension<T: Storable<D>, D: DB, A: Storable<D> + Annotation<T>>(
    mut path: Vec<u8>,
    child: Sp<Node<T, D, A>, D>,
) -> Sp<Node<T, D, A>, D> {
    let mut cur = child;
    while let Node::Extension {
        compressed_path,
        child,
        ..
    } = &*cur
        && !path.len().is_multiple_of(255)
    {
        path.extend(compressed_path);
        cur = child.clone();
    }
    for working_path in path.chunks(255).rev() {
        cur = Sp::new(Node::Extension {
            ann: Node::<T, D, A>::ann(&cur),
            compressed_path: working_path.to_vec(),
            child: cur.clone(),
        });
    }
    cur
}

pub trait HasSize
where
    Self: Sized + Clone,
{
    /// Getter for the size property
    fn get_size(&self) -> u64;
    /// Setter for the size property
    fn set_size(&self, x: u64) -> Self;
}

impl HasSize for SizeAnn {
    fn get_size(&self) -> u64 {
        self.0
    }

    fn set_size(&self, x: u64) -> Self {
        SizeAnn(x)
    }
}

/// A type that knows how to build itself from some `T`
pub trait Annotation<T>:
    Monoid + Serializable + Deserializable + HasSize + Debug + PartialEq
{
    /// Build `Self` from some `T`
    fn from_value(value: &T) -> Self;
}

impl<T> Annotation<T> for SizeAnn {
    fn from_value(_value: &T) -> Self {
        SizeAnn(1)
    }
}

impl<T: Storable<D>, D: DB, A: Storable<D> + Annotation<T>> Node<T, D, A> {
    fn lookup_sp(sp: &Sp<Node<T, D, A>, D>, path: &[u8]) -> Option<Sp<T, D>> {
        Self::lookup_with(sp, path, Clone::clone)
    }

    fn lookup<'a>(sp: &'a Sp<Node<T, D, A>, D>, path: &[u8]) -> Option<&'a T> {
        Self::lookup_with::<&T>(sp, path, |sp| sp.deref())
    }

    fn lookup_with<'a, S>(
        sp: &'a Sp<Node<T, D, A>, D>,
        path: &[u8],
        f: impl FnOnce(&'a Sp<T, D>) -> S,
    ) -> Option<S> {
        match sp.deref() {
            Node::Empty => None,
            Node::Leaf { value, .. } if path.is_empty() => Some(f(value)),
            // If the path isn't empty
            Node::Leaf { .. } => None,
            Node::Branch { children, .. } => {
                if path.is_empty() {
                    return None;
                }
                let index: usize = path[0].into();
                Self::lookup_with(&children[index], &path[1..], f)
            }
            Node::Extension {
                compressed_path,
                child,
                ..
            } => {
                if path.len() < compressed_path.len() {
                    return None;
                }
                for i in 0..compressed_path.len() {
                    if compressed_path[i] != path[i] {
                        return None;
                    }
                }
                Self::lookup_with(child, &path[compressed_path.len()..], f)
            }
            Node::MidBranchLeaf { value, child, .. } => {
                if path.is_empty() {
                    Some(f(value))
                } else {
                    Self::lookup_with(child, path, f)
                }
            }
        }
    }

    // There are several places in `find_predecessor_recursive` and `find_largest_key_in_subtree`
    // where we need to locally modify the `explored_path`. These two helpers serve to avoid any nasty
    // bugs by centralising the pushing/popping/extending/truncating
    pub(crate) fn with_pushed_nibble<R>(
        path: &mut Vec<u8>,
        nibble: u8,
        f: impl FnOnce(&mut Vec<u8>) -> R,
    ) -> R {
        path.push(nibble);
        let res = f(path);
        path.pop();
        res
    }

    pub(crate) fn with_extended_suffix<R>(
        path: &mut Vec<u8>,
        temporary_suffix: &[u8],
        f: impl FnOnce(&mut Vec<u8>) -> R,
    ) -> R {
        let old_len = path.len();
        path.extend_from_slice(temporary_suffix);
        let res = f(path);
        path.truncate(old_len);
        res
    }

    /// Prunes all paths which are lexicographically less than the `target_path`.
    /// Returns the updated tree, and a vector of the removed leaves.
    ///
    /// # Panics
    ///
    /// If any values in `target_path` are not `u4` nibbles, i.e. larger than
    /// 15.
    pub fn prune(
        sp: &Sp<Node<T, D, A>, D>,
        target_path: &[u8],
        // Returns "is this node empty?" so we can collapse parts of the tree as we go
    ) -> (Sp<Self, D>, Vec<Sp<T, D>>) {
        if target_path.is_empty() {
            return (sp.clone(), vec![]);
        }

        match &**sp {
            Node::Empty => (Sp::new(Node::Empty), vec![]),
            Node::Leaf { value, .. } => {
                // We've not matched exactly, but we've ended up at a leaf, which means the leaf is less than the cutoff, so we need to remove it
                (Sp::new(Node::Empty), vec![value.clone()])
            }
            Node::Branch {
                children, ann: an, ..
            } => {
                // All children indexed prior to the head of `path_remaining` are guaranteed to be earlier than the cutoff
                let path_head = target_path[0];
                if path_head >= 16 {
                    panic!("Invalid path nibble: {}", path_head);
                }
                let mut pruned = (0..path_head as usize)
                    .flat_map(|i| Self::iter(&children[i]))
                    .collect::<Vec<_>>();
                let mut children = children.clone();
                for child in children.iter_mut().take(path_head as usize) {
                    *child = Sp::new(Node::Empty);
                }

                // The child at the `path_head` contains the exact point referenced by the `original_target_path`.
                // As such, we need to `prune` it.
                let (child, pruned_2) =
                    Self::prune(&children[path_head as usize], &target_path[1..]);
                children[path_head as usize] = child;
                pruned.extend(pruned_2);

                // If ALL of a branch's children are empty, that makes the branch also empty.
                // If only one is left filled, this branch becomes an extension.
                let no_filled = children.iter().filter(|c| !Self::is_empty(c)).count();

                let an = if pruned.is_empty() {
                    an.clone()
                } else {
                    children
                        .iter()
                        .fold(A::empty(), |acc, x| acc.append(&Self::ann(x)))
                };

                match no_filled {
                    0 => (Sp::new(Node::Empty), pruned),
                    1 => {
                        let (path_head, child) = children
                            .into_iter()
                            .enumerate()
                            .find(|(_, child)| !Self::is_empty(child))
                            .expect("Exactly one non-empty child must exist in branch");
                        match &*child {
                            Node::Extension {
                                compressed_path,
                                child,
                                ..
                            } => (
                                extension(
                                    once(path_head as u8)
                                        .chain(compressed_path.iter().copied())
                                        .collect(),
                                    child.clone(),
                                ),
                                pruned,
                            ),
                            _ => (extension(vec![path_head as u8], child), pruned),
                        }
                    }
                    _ => (Sp::new(Node::Branch { children, ann: an }), pruned),
                }
            }
            Node::Extension {
                compressed_path,
                child,
                ann: an,
            } => {
                let relevant_target_path =
                    &target_path[..usize::min(target_path.len(), compressed_path.len())];
                match compressed_path[..].cmp(relevant_target_path) {
                    // The extension is entirely smaller than the path to prune, and is removed
                    // entirely.
                    Ordering::Less => (Sp::new(Node::Empty), Self::iter(child).collect()),
                    // The extension matches the path to prune, and we recurse into the child
                    Ordering::Equal => {
                        let (child, pruned) =
                            Self::prune(child, &target_path[compressed_path.len()..]);
                        match &*child {
                            Node::Empty => (Sp::new(Node::Empty), pruned),
                            Node::Extension {
                                compressed_path: cpath2,
                                child,
                                ..
                            } => (
                                extension(
                                    compressed_path
                                        .iter()
                                        .chain(cpath2.iter())
                                        .copied()
                                        .collect(),
                                    child.clone(),
                                ),
                                pruned,
                            ),
                            _ => (
                                Sp::new(Node::Extension {
                                    ann: if pruned.is_empty() {
                                        an.clone()
                                    } else {
                                        Self::ann(&child)
                                    },
                                    compressed_path: compressed_path.clone(),
                                    child: child.clone(),
                                }),
                                pruned,
                            ),
                        }
                    }
                    // The extension is entirely greater than the path to prune, and is kept
                    // entirely.
                    Ordering::Greater => (sp.clone(), vec![]),
                }
            }
            Node::MidBranchLeaf { value, child, .. } => {
                // Because we know `path_remaining` isn't empty, we know we this node's value is
                // definitely being removed, but we don't know if its child needs to be removed yet.
                // As such, we need to `prune` it.
                let (child, pruned) = Self::prune(child, target_path);
                (child, once(value.clone()).chain(pruned).collect())
            }
        }
    }

    // Apply a function to all values in a node
    pub(crate) fn iter(sp: &Sp<Node<T, D, A>, D>) -> impl Iterator<Item = Sp<T, D>> + '_ {
        let res: Box<dyn Iterator<Item = Sp<T, D>>> = match sp.deref() {
            Node::Empty => Box::new(empty()),
            Node::Leaf { value, .. } => Box::new(once(value.clone())),
            Node::Branch { children, .. } => Box::new(children.iter().flat_map(|c| Self::iter(c))),
            Node::Extension { child, .. } => Box::new(Self::iter(child)),
            Node::MidBranchLeaf { value, child, .. } => {
                Box::new(once(value.clone()).chain(Self::iter(child)))
            }
        };
        res
    }

    pub(crate) fn is_empty(sp: &Sp<Node<T, D, A>, D>) -> bool {
        matches!(sp.deref(), Node::Empty)
    }

    fn find_predecessor_recursive<'a>(
        sp: &'a Sp<Node<T, D, A>, D>,
        original_target_path: &[u8],
        explored_path: &mut Vec<u8>,
        best_predecessor: &mut Option<(Vec<u8>, &'a T)>,
    ) {
        // How deep are we currently?
        let current_depth = explored_path.len();
        // Calculate how much of `original_target_path` is left based on how deep we've explored.
        let path_remaining = if current_depth <= original_target_path.len() {
            &original_target_path[current_depth..]
        } else {
            &[]
        };

        // If `path_remaining` is empty, it means we've matched exactly the target, so
        // there's nothing to do.
        if path_remaining.is_empty() {
            return;
        }

        match sp.deref() {
            Node::Empty => (),
            Node::Leaf { value, .. } => {
                // We've not matched exactly, but we've ended up at a leaf, which means the leaf is our new `best_predecessor`
                Self::update_best_pred(best_predecessor, explored_path.to_vec(), value.deref())
            }
            Node::Branch { children, .. } => {
                // All children indexed prior to the head of `path_remaining` are guaranteed to be earlier than the target
                let path_head = path_remaining[0];

                // Explore the child at exactly `path_head` first. If we check the siblings first,
                // we must still check the child at the `path_head` since it might contain a better candidate.
                let matching_child = &children[path_head as usize];
                if !Self::is_empty(matching_child) {
                    let mut new_best_pred = None;
                    Self::with_pushed_nibble(explored_path, path_head, |temp_path| {
                        Self::find_predecessor_recursive(
                            matching_child,
                            original_target_path,
                            temp_path,
                            &mut new_best_pred,
                        )
                    });

                    if new_best_pred.is_some() {
                        *best_predecessor = new_best_pred;
                        return;
                    }
                }

                // Check the siblings that are guaranteed to be earlier than the target
                for i in (0..path_head).rev() {
                    let largest = Self::with_pushed_nibble(explored_path, i, |temp_path| {
                        Self::find_largest_key_in_subtree(&children[i as usize], temp_path)
                    });
                    if let Some((key, val)) = largest {
                        Self::update_best_pred(best_predecessor, key, val);
                        break; // Since we're iterating downwards, no smaller child can possibly be the best predecessor
                    }
                }
            }
            Node::Extension {
                compressed_path,
                child,
                ..
            } => {
                // The number of nibbles of `compressed_path` that match our `path_remaining`
                let match_len = compressed_path
                    .iter()
                    .zip(path_remaining.iter())
                    .take_while(|(a, b)| a == b)
                    .count();

                // The compressed path is an exact prefix of `path_remaining`
                // so we can just stuff the compressed path onto the end of our `explored_path`,
                // Because it's an exact match, this means that somewhere in this extension's child,
                // the cutoff is reached, so we need to recurse on the child.
                if match_len == compressed_path.len() {
                    Self::with_extended_suffix(explored_path, compressed_path, |temp_path| {
                        Self::find_predecessor_recursive(
                            child,
                            original_target_path,
                            temp_path,
                            best_predecessor,
                        )
                    });
                } else {
                    let diverging_compressed_nibble = compressed_path[match_len];
                    let diverging_remaining_nibble = path_remaining[match_len];

                    // We've got a partial match with the compressed path (so the `compressed_path` and our `path_remaining` diverge in the middle)
                    if match_len < compressed_path.len() && match_len < path_remaining.len()
                        // For the nibbles at the position where the divergence occurs, if the `compressed_path`'s diverging nibble is
                        // less than our `remaining_path`'s diverging nibble, then all keys in this node are potential candidates.
                        // As such, we don't need to recurse on the child, we can just take the largest key.
                        //
                        // Obviously it follows that the `else` case here just means all keys in this node are greater than the target
                        // and thus there aren't any candidates.
                        && diverging_compressed_nibble < diverging_remaining_nibble
                            && let Some((key, val)) = Self::find_largest_key_in_subtree(sp, explored_path)
                    {
                        Self::update_best_pred(best_predecessor, key, val)
                    }
                }
            }
            Node::MidBranchLeaf { value, child, .. } => {
                // Only if we don't have an exact match should we consider either this branch's value
                // or its child (again, to be pedantic with the naming, to discuss with @Thomas probably)
                // It's better to check the child first since, if we find something, it'll be a better
                // predecessor candidate than this node's value
                let mut new_best_pred = None;
                Self::find_predecessor_recursive(
                    child,
                    original_target_path,
                    explored_path,
                    &mut new_best_pred,
                );

                if new_best_pred.is_some() {
                    *best_predecessor = new_best_pred;
                    return;
                }

                // Otherwise, this node's value is, in fact, our best candidate
                Self::update_best_pred(best_predecessor, explored_path.to_vec(), value.deref())
            }
        }
    }

    pub(crate) fn update_best_pred<'a>(
        best_predecessor: &mut Option<(Vec<u8>, &'a T)>,
        candidate_path: Vec<u8>,
        candidate_value: &'a T,
    ) {
        if best_predecessor
            .as_ref()
            .is_none_or(|(bp_path, _)| candidate_path > *bp_path)
        {
            *best_predecessor = Some((candidate_path, candidate_value));
        }
    }

    pub(crate) fn find_largest_key_in_subtree<'a>(
        sp: &'a Sp<Node<T, D, A>, D>,
        current_path_to_node: &mut Vec<u8>,
    ) -> Option<(Vec<u8>, &'a T)> {
        match sp.deref() {
            Node::Empty => None,
            Node::Leaf { value, .. } => Some((current_path_to_node.to_vec(), value.deref())),
            Node::Branch { children, .. } => (0..16).rev().find_map(|i| {
                Self::with_pushed_nibble(current_path_to_node, i as u8, |p| {
                    Self::find_largest_key_in_subtree(&children[i], p)
                })
            }),
            Node::Extension {
                compressed_path,
                child,
                ..
            } => Self::with_extended_suffix(current_path_to_node, compressed_path, |temp_path| {
                Self::find_largest_key_in_subtree(child, temp_path)
            }),
            Node::MidBranchLeaf { value, child, .. } => {
                let largest_in_child =
                    Self::find_largest_key_in_subtree(child, current_path_to_node);
                if largest_in_child.is_some() {
                    return largest_in_child;
                }

                Some((current_path_to_node.to_vec(), value.deref()))
            }
        }
    }

    /// The node's children, skipping empty ones.
    pub(crate) fn non_empty_children(&self) -> impl Iterator<Item = &Sp<Node<T, D, A>, D>> {
        let children: &[Sp<Node<T, D, A>, D>] = match self {
            Node::Empty | Node::Leaf { .. } => &[],
            Node::Branch { children, .. } => &children[..],
            Node::Extension { child, .. } | Node::MidBranchLeaf { child, .. } => {
                std::slice::from_ref(child)
            }
        };
        children
            .iter()
            .filter(|child| !matches!(***child, Node::Empty))
    }

    /// Return the annotation of the `Node`
    pub fn ann(sp: &Sp<Node<T, D, A>, D>) -> A {
        match &**sp {
            Node::Empty => A::empty(),
            Node::Leaf { ann, .. }
            | Node::Branch { ann, .. }
            | Node::Extension { ann, .. }
            | Node::MidBranchLeaf { ann, .. } => (*ann).clone(),
        }
    }

    // Inserts a value at a given path.
    // Returns the updated tree, and the existing value at that path, if applicable.
    fn insert(sp: &Sp<Self, D>, path: &[u8], value: T) -> (Sp<Self, D>, Option<Sp<T, D>>) {
        if path.is_empty() {
            let value_sp = sp.arena.alloc(value.clone());
            let (node, existing_val) = match sp.deref() {
                Node::Empty => (
                    Node::Leaf {
                        ann: Annotation::<T>::from_value(&value),
                        value: value_sp,
                    },
                    None,
                ),
                Node::Leaf { value: old_val, .. } => (
                    Node::Leaf {
                        ann: Annotation::<T>::from_value(&value),
                        value: value_sp,
                    },
                    Some(old_val.clone()),
                ),
                Node::Branch { .. } | Node::Extension { .. } => {
                    let new_ann = Self::ann(sp).append(&Annotation::<T>::from_value(&value));
                    (
                        Node::MidBranchLeaf {
                            ann: new_ann,
                            value: value_sp,
                            child: sp.clone(),
                        },
                        None,
                    )
                }
                Node::MidBranchLeaf {
                    value: old_val,
                    child,
                    ..
                } => (
                    Node::MidBranchLeaf {
                        ann: Self::ann(child).append(&Annotation::<T>::from_value(&value)),
                        value: value_sp,
                        child: child.clone(),
                    },
                    Some(old_val.clone()),
                ),
            };
            return (sp.arena.alloc(node), existing_val);
        }

        match sp.deref() {
            Node::Empty => {
                let value_sp = sp.arena.alloc(value.clone());
                let child = Sp::new(Node::Leaf {
                    ann: Annotation::<T>::from_value(&value),
                    value: value_sp,
                });
                let res = extension(path.to_vec(), child);
                (res, None)
            }
            Node::Leaf {
                ann: existing_ann,
                value: self_value,
                ..
            } => {
                let child = Sp::new(Node::Leaf {
                    ann: A::from_value(&value),
                    value: sp.arena.alloc(value.clone()),
                });
                let ext = extension(path.to_vec(), child);

                let branch_ann = A::from_value(&value).append(existing_ann);
                (
                    sp.arena.alloc(Node::MidBranchLeaf {
                        ann: branch_ann,
                        value: self_value.clone(),
                        child: ext,
                    }),
                    None,
                )
            }
            Node::Branch { children, .. } => {
                let index: usize = path[0].into();
                let mut new_children = children.clone();
                let (new_child, existing) = Self::insert(&new_children[index], &path[1..], value);
                new_children[index] = new_child;

                let new_ann = new_children
                    .iter()
                    .fold(A::empty(), |an, c| an.append(&Self::ann(c)));

                (
                    sp.arena.alloc(Node::Branch {
                        ann: new_ann,
                        children: new_children,
                    }),
                    existing,
                )
            }
            Node::Extension {
                compressed_path,
                child,
                ..
            } => {
                let working_path: std::vec::Vec<u8> =
                    path.chunks(255).next().expect("path is not empty").to_vec();

                let index = compressed_path
                    .iter()
                    .zip(working_path)
                    .take_while(|(a, b)| **a == *b)
                    .count();

                // complete path match, insert at the child node
                if index == compressed_path.len() {
                    let (new_child, existing) = Self::insert(child, &path[index..], value);
                    (
                        sp.arena.alloc(Node::Extension {
                            ann: Self::ann(&new_child),
                            compressed_path: compressed_path.clone(),
                            child: new_child,
                        }),
                        existing,
                    )
                } else {
                    // if path splits on final nibble old child node doesn't need an extension,
                    // otherwise it does
                    let remaining = if index == compressed_path.len() - 1 {
                        child.clone()
                    } else {
                        sp.arena.alloc(Node::Extension {
                            ann: Self::ann(child),
                            compressed_path: compressed_path[(index + 1)..].to_vec(),
                            child: child.clone(),
                        })
                    };

                    let compressed_path_index: usize = compressed_path[index].into();
                    let mut children: [Sp<Node<T, D, A>, D>; 16] =
                        core::array::from_fn(|_| sp.arena.alloc(Node::Empty));
                    children[compressed_path_index] = remaining;

                    let initial_ann = children
                        .iter()
                        .map(|c| Self::ann(c))
                        .fold(A::empty(), |acc, child_ann| acc.append(&child_ann));

                    let branch = sp.arena.alloc(Node::Branch {
                        ann: initial_ann,
                        children: Box::new(children),
                    });

                    let (final_branch, existing) = Self::insert(&branch, &path[index..], value);

                    // if path split on first nibble no extension required, otherwise it is
                    if index == 0 {
                        (final_branch, existing)
                    } else {
                        (
                            sp.arena.alloc(Node::Extension {
                                ann: Self::ann(&final_branch),
                                compressed_path: compressed_path[0..index].to_vec(),
                                child: final_branch,
                            }),
                            existing,
                        )
                    }
                }
            }
            Node::MidBranchLeaf {
                child,
                value: leaf_value,
                ..
            } => {
                let (new_child, existing) = Self::insert(child, path, value);
                let new_ann = A::from_value(leaf_value).append(&Self::ann(&new_child));

                (
                    sp.arena.alloc(Node::MidBranchLeaf {
                        ann: new_ann,
                        value: leaf_value.clone(),
                        child: new_child,
                    }),
                    existing,
                )
            }
        }
    }

    fn size(sp: &Sp<Node<T, D, A>, D>) -> usize {
        match sp.deref() {
            Node::Empty => 0,
            Node::Leaf { .. } => 1,
            Node::Extension { ann, .. }
            | Node::Branch { ann, .. }
            | Node::MidBranchLeaf { ann, .. } => ann.clone().get_size() as usize,
        }
    }

    fn leaves<'a>(
        sp: &'a Sp<Node<T, D, A>, D>,
        current_path: Vec<u8>,
    ) -> Box<dyn Iterator<Item = (std::vec::Vec<u8>, Sp<T, D>)> + 'a> {
        match sp.deref() {
            Node::Empty => Box::new(empty()),
            Node::Leaf { value, .. } => Box::new([(current_path, value.clone())].into_iter()),
            Node::Extension {
                compressed_path,
                child,
                ..
            } => {
                let mut new_path = current_path.to_vec();
                new_path.append(&mut compressed_path.clone());
                Self::leaves(child, new_path)
            }
            Node::Branch { children, .. } => {
                Box::new(children.iter().enumerate().flat_map(move |(i, child)| {
                    let mut new_path = current_path.clone();
                    new_path.push(i as u8);
                    Self::leaves(child, new_path)
                }))
            }
            Node::MidBranchLeaf { value, child, .. } => Box::new(
                Self::leaves(child, current_path.clone())
                    .chain(once((current_path, value.clone()))),
            ),
        }
    }

    /// Removes a value from a path, doing nothing if no value was present.
    /// Returns the updated node, and the value removed if applicable.
    pub fn remove(sp: &Sp<Self, D>, path: &[u8]) -> (Sp<Self, D>, Option<Sp<T, D>>) {
        match sp.deref() {
            Node::Empty => (sp.arena.alloc(Node::Empty), None),
            Node::Leaf { value, ann } => {
                if path.is_empty() {
                    return (sp.arena.alloc(Node::Empty), Some(value.clone()));
                }

                (
                    sp.arena.alloc(Node::Leaf {
                        ann: ann.clone(),
                        value: value.clone(),
                    }),
                    None,
                )
            }
            Node::Branch { children, .. } => {
                let mut new_children = children.clone();
                let index: usize = path[0].into();
                let (new_child, removed) = Self::remove(&new_children[index], &path[1..]);
                new_children[index] = new_child;

                // Remove branch if only one child remaining
                if new_children
                    .iter()
                    .map(|v| match **v {
                        Node::Empty => 0,
                        _ => 1,
                    })
                    .sum::<usize>()
                    == 1
                {
                    let (only_child_index, only_child) = new_children
                        .iter()
                        .enumerate()
                        .find(|(_i, v)| !matches!(***v, Node::Empty))
                        .unwrap();

                    match (**only_child).clone() {
                        Node::Extension {
                            mut compressed_path,
                            child,
                            ..
                        } => {
                            let mut new_compressed_path = vec![only_child_index as u8];
                            new_compressed_path.append(&mut compressed_path);
                            (extension(new_compressed_path, child), removed)
                        }
                        _ => (
                            extension(vec![only_child_index as u8], only_child.clone()),
                            removed,
                        ),
                    }
                } else {
                    (
                        sp.arena.alloc(Node::Branch {
                            ann: new_children
                                .iter()
                                .fold(A::empty(), |acc, x| acc.append(&Self::ann(x))),
                            children: new_children,
                        }),
                        removed,
                    )
                }
            }
            Node::Extension {
                compressed_path,
                child,
                ..
            } => {
                if path.len() < compressed_path.len() {
                    return (sp.clone(), None);
                }
                for i in 0..compressed_path.len() {
                    if compressed_path[i] != path[i] {
                        return (sp.clone(), None);
                    }
                }

                let (new_child, removed) = Self::remove(child, &path[compressed_path.len()..]);
                let new_ann = Self::ann(&new_child);
                match new_child.deref() {
                    Node::Empty => (sp.arena.alloc(Node::Empty), removed),
                    Node::Extension {
                        compressed_path: p,
                        child: c,
                        ..
                    } => {
                        let mut new_compressed_path = compressed_path.clone();
                        new_compressed_path.append(&mut p.clone());

                        let child = extension(new_compressed_path, c.clone());
                        (child, removed)
                    }
                    _ => (
                        sp.arena.alloc(Node::Extension {
                            ann: new_ann,
                            compressed_path: compressed_path.clone(),
                            child: new_child,
                        }),
                        removed,
                    ),
                }
            }
            Node::MidBranchLeaf { child, value, .. } => {
                if path.is_empty() {
                    (child.clone(), Some(value.clone()))
                } else {
                    let (child, removed) = Self::remove(child, path);
                    let new_ann = Self::ann(&child).append(&Annotation::<T>::from_value(value));
                    match child.deref() {
                        Node::Empty => (
                            sp.arena.alloc(Node::Leaf {
                                ann: new_ann,
                                value: value.clone(),
                            }),
                            removed,
                        ),
                        _ => (
                            sp.arena.alloc(Node::MidBranchLeaf {
                                ann: new_ann,
                                value: value.clone(),
                                child,
                            }),
                            removed,
                        ),
                    }
                }
            }
        }
    }
}

impl<T: Storable<D> + 'static, D: DB, A: Storable<D> + Annotation<T>> Storable<D>
    for Node<T, D, A>
{
    fn children(&self) -> std::vec::Vec<ArenaKey<D::Hasher>> {
        match self {
            Node::Empty => std::vec::Vec::new(),
            Node::Leaf { value, .. } => vec![value.as_child()],
            Node::Branch { children, .. } => children.iter().map(Sp::as_child).collect(),
            Node::Extension { child, .. } => vec![child.as_child()],
            Node::MidBranchLeaf { child, value, .. } => {
                vec![value.as_child(), child.as_child()]
            }
        }
    }

    fn to_binary_repr<W: Write>(&self, writer: &mut W) -> Result<(), std::io::Error> {
        match self {
            Node::Empty => {
                u8::serialize(&0, writer)?;
            }
            Node::Leaf { ann, .. } => {
                u8::serialize(&1, writer)?;
                A::serialize(ann, writer)?;
            }
            Node::Branch { ann, .. } => {
                u8::serialize(&2, writer)?;
                A::serialize(ann, writer)?;
            }
            Node::Extension {
                ann,
                compressed_path,
                ..
            } => {
                u8::serialize(&3, writer)?;
                A::serialize(ann, writer)?;
                let compressed = compress_nibbles(compressed_path);
                u8::serialize(&(compressed_path.len() as u8), writer)?;
                std::vec::Vec::<u8>::serialize(&compressed, writer)?;
            }
            Node::MidBranchLeaf { ann, .. } => {
                u8::serialize(&4, writer)?;
                A::serialize(ann, writer)?;
            }
        }

        Ok(())
    }

    /// Checks this node's canonicity rules against its children, which were
    /// checked when decoded, so a decode checks each distinct node once.
    fn check_invariant(&self) -> Result<(), std::io::Error> {
        fn err(msg: &str) -> Result<(), std::io::Error> {
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                msg.to_string(),
            ))
        }
        fn size<T: Storable<D>, D: DB, A: Storable<D> + Annotation<T>>(
            node: &Sp<Node<T, D, A>, D>,
        ) -> u64 {
            Node::<T, D, A>::ann(node).get_size()
        }
        let (ann, true_ann, true_size) = match self {
            Node::Empty => return Ok(()),
            Node::Leaf { ann, value } => (ann, A::from_value(value), Some(1)),
            Node::Branch { ann, children } => {
                let non_empty_children = children
                    .iter()
                    .filter(|child| !matches!(***child, Node::Empty))
                    .count();
                if non_empty_children < 2 {
                    return err("Fewer than 2 non-empty children in Node::Branch");
                }
                let true_ann = children
                    .iter()
                    .fold(A::empty(), |acc, child| acc.append(&Self::ann(child)));
                let true_size = children
                    .iter()
                    .try_fold(0u64, |acc, child| acc.checked_add(size(child)));
                (ann, true_ann, true_size)
            }
            Node::Extension {
                ann,
                compressed_path,
                child,
            } => {
                if compressed_path.is_empty() {
                    return err("Node::Extension path must be non-empty");
                }
                if compressed_path.len() > 255 {
                    return err("Node::Extension path may not be longer than 255");
                }
                if compressed_path.iter().any(|b| *b > 0x0f) {
                    return err("Node::Extension path must consist of nibbles");
                }
                match child.deref() {
                    Node::Empty => return err("Node::Extension child must not be Node::Empty"),
                    Node::Extension { .. }
                        if compressed_path.len() != 255 && !ALLOW_SHORT_EXTENSION_CHAINS.is_set() =>
                    {
                        return err(
                            "Node::Extension path must be of length 255 when having another Node::Extension child",
                        );
                    }
                    _ => {}
                }
                (ann, Self::ann(child), Some(size(child)))
            }
            Node::MidBranchLeaf { ann, value, child } => {
                if !matches!(child.deref(), Node::Branch { .. } | Node::Extension { .. }) {
                    return err(
                        "Node::MidBranchLeaf may only have Node::Branch or Node::Extension children",
                    );
                }
                let true_ann = Self::ann(child).append(&A::from_value(value));
                (ann, true_ann, size(child).checked_add(1))
            }
        };
        if true_size.is_none() {
            return err("MPT size overflows u64");
        }
        if Some(ann.get_size()) != true_size || *ann != true_ann {
            return err("MPT annotation isn't correctly calculated");
        }
        Ok(())
    }

    #[inline(always)]
    fn from_binary_repr<R: Read>(
        reader: &mut R,
        child_hashes: &mut impl Iterator<Item = ArenaKey<D::Hasher>>,
        loader: &impl Loader<D>,
    ) -> Result<Node<T, D, A>, std::io::Error> {
        // Thread the loader's recursion depth through, rather than restarting
        // from zero: resetting it at every node boundary would let a nested
        // structure evade `midnight-serialize`'s recursion limit entirely.
        let depth = loader.get_recursion_depth();
        let disc = u8::deserialize(reader, depth)?;

        let node = match disc {
            0 => Node::Empty,
            1 => {
                let ann = A::deserialize(reader, depth)?;
                Node::Leaf {
                    ann,
                    value: loader.get_next(child_hashes)?,
                }
            }
            2 => {
                let ann = A::deserialize(reader, depth)?;
                let Ok(children) = (0..16)
                    .map(|_| loader.get_next(child_hashes))
                    .collect::<Result<Vec<_>, _>>()?
                    .try_into()
                else {
                    unreachable!("iterator must be of expected length")
                };

                Node::Branch {
                    ann,
                    children: Box::new(children),
                }
            }
            3 => {
                let ann = A::deserialize(reader, depth)?;
                let len = u8::deserialize(reader, depth)? as usize;
                let data = <std::vec::Vec<u8>>::deserialize(reader, depth)?;
                if data.len() != len.div_ceil(2) {
                    return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("inferred nibble byte length {} did not match actual data length of {}", len.div_ceil(2), data.len())));
                };
                let path = expand_nibbles(&data, len);
                let child: Sp<Node<T, D, A>, D> = loader.get_next(child_hashes)?;
                Node::Extension {
                    ann,
                    compressed_path: path,
                    child,
                }
            }
            4 => {
                let ann = A::deserialize(reader, depth)?;
                let value: Sp<T, D> = loader.get_next(child_hashes)?;
                let child: Sp<Node<T, D, A>, D> = loader.get_next(child_hashes)?;
                Node::MidBranchLeaf { ann, value, child }
            }
            _ => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Unrecognised discriminant",
                ));
            },
        };

        // Enforce structural node invariants when loading from an untrusted
        // source. `do_check` is a no-op for trusted back-end loaders and runs
        // `check_invariant` for wire loaders, rejecting malformed node shapes
        // (e.g. a `MidBranchLeaf` whose child is not a `Branch`/`Extension`)
        // that would otherwise let the size annotation diverge from the set of
        // canonically-indexed entries reachable by `get`.
        loader.do_check(node)
    }
}

#[cfg(test)]
mod tests {
    mod canonicity;
    mod fold;

    use sha2::Sha256;

    use crate::{
        Storage,
        db::InMemoryDB,
        storable::SMALL_OBJECT_LIMIT,
        storage::{WrappedDB, default_storage, set_default_storage},
    };

    use super::*;
    use serialize::{Deserializable, Serializable};

    #[test]
    fn insert_lookup() {
        dbg!("start");
        let mut mpt = MerklePatriciaTrie::<u64>::new();
        dbg!("new tree");
        mpt = mpt.insert(&([1, 2, 3]), 100);
        dbg!("inserted 100 at [1, 2, 3]");
        mpt = mpt.insert(&([1, 2, 4]), 104);
        dbg!("inserted 104 at [1, 2, 4]");
        mpt = mpt.insert(&([2, 2, 4]), 105);
        dbg!("inserted 105 at [2, 2, 4]");
        assert_eq!(mpt.lookup(&([1, 2, 3])), Some(&100));
        assert_eq!(mpt.lookup(&([1, 2, 4])), Some(&104));
        assert_eq!(mpt.lookup(&([2, 2, 4])), Some(&105));
    }

    #[test]
    fn remove() {
        let mut mpt = MerklePatriciaTrie::<u64>::new();
        mpt = mpt.insert(&([1, 2, 3]), 100);
        mpt = mpt.insert(&([1, 3, 3]), 102);
        mpt = mpt.remove(&([1, 2, 3]));
        assert_eq!(mpt.size(), 1);
        assert_eq!(mpt.lookup(&([1, 3, 3])), Some(&102));
        assert_eq!(mpt.lookup(&([1, 2, 3])), None);
    }

    #[test]
    fn remove_path_shorter_than_extension() {
        let mut mpt = MerklePatriciaTrie::<u64>::new();
        // Insert with a path long enough to create an Extension node.
        mpt = mpt.insert(&[1, 2, 3], 100);
        // Removing with a path shorter than the Extension's compressed_path
        // should return the trie unchanged, not panic.
        mpt = mpt.remove(&[1]);
        assert_eq!(mpt.lookup(&[1, 2, 3]), Some(&100));
        assert_eq!(mpt.size(), 1);
    }

    #[test]
    fn deduplicate() {
        // Isolate our storage, since we're checking arena size.
        struct Tag;
        type D = WrappedDB<DefaultDB, Tag>;
        let _ = set_default_storage::<D>(Storage::default);
        let mut mpt = MerklePatriciaTrie::<[u8; SMALL_OBJECT_LIMIT], D>::new();
        mpt = mpt.insert(&([1, 2, 3]), [100; SMALL_OBJECT_LIMIT]);
        mpt = mpt.insert(&([1, 2, 2]), [100; SMALL_OBJECT_LIMIT]);
        assert_eq!(mpt.lookup(&([1, 2, 3])), Some(&[100; SMALL_OBJECT_LIMIT]));
        assert_eq!(mpt.lookup(&([1, 2, 2])), Some(&[100; SMALL_OBJECT_LIMIT]));
        assert_eq!(mpt.size(), 2);
        dbg!(&mpt.0.arena);
        assert_eq!(mpt.0.arena.size(), 1);
    }

    #[test]
    fn short_extension_chains_load_when_allowed() {
        fn key(diverge_at: usize) -> std::vec::Vec<u8> {
            let mut key = vec![0u8; 300];
            key[diverge_at] = 1;
            key
        }
        fn bytes(mpt: &MerklePatriciaTrie<u8>) -> std::vec::Vec<u8> {
            let mut bytes = std::vec::Vec::new();
            Serializable::serialize(mpt, &mut bytes).unwrap();
            bytes
        }
        // A 300-nibble key forms Extension(255) -> Extension(45) -> Leaf, and
        // diverging at nibble 10 splits the 255-nibble extension, leaving an
        // Extension(244) -> Extension(45) chain.
        let mpt = MerklePatriciaTrie::<u8>::new()
            .insert(&[0; 300], 1)
            .insert(&key(10), 2);
        let encoded = bytes(&mpt);

        assert!(
            <MerklePatriciaTrie<u8> as Deserializable>::deserialize(&mut &encoded[..], 0).is_err()
        );
        let loaded: MerklePatriciaTrie<u8> = ALLOW_SHORT_EXTENSION_CHAINS
            .set(&(), || Deserializable::deserialize(&mut &encoded[..], 0))
            .unwrap();
        assert!(!ALLOW_SHORT_EXTENSION_CHAINS.is_set());
        assert_eq!(bytes(&loaded), encoded);

        // The loaded trie must evolve exactly as the in-memory one does.
        assert_eq!(
            bytes(&loaded.insert(&key(100), 3)),
            bytes(&mpt.insert(&key(100), 3))
        );
        assert_eq!(
            bytes(&loaded.remove(&key(10))),
            bytes(&mpt.remove(&key(10)))
        );
    }

    #[test]
    fn mpt_arena_serialization() {
        let mut mpt = MerklePatriciaTrie::<u8>::new();
        mpt = mpt.insert(&([1, 2, 3]), 100);
        mpt = mpt.insert(&([1, 2, 4]), 104);
        mpt = mpt.insert(&([2, 2, 4]), 105);
        let mut bytes = std::vec::Vec::new();
        MerklePatriciaTrie::serialize(&mpt, &mut bytes).unwrap();
        assert_eq!(bytes.len(), MerklePatriciaTrie::<u8>::serialized_size(&mpt));
        let mpt: MerklePatriciaTrie<u8> =
            MerklePatriciaTrie::deserialize(&mut bytes.as_slice(), 0).unwrap();
        assert_eq!(mpt.lookup(&([1, 2, 3])), Some(&100));
        assert_eq!(mpt.lookup(&([1, 2, 4])), Some(&104));
        assert_eq!(mpt.lookup(&([2, 2, 4])), Some(&105));
    }

    #[test]
    fn nodes_stored() {
        // Isolate our storage, since we're checking arena size.
        struct Tag;
        type D = WrappedDB<DefaultDB, Tag>;
        let _ = set_default_storage::<D>(Storage::default);
        let arena = &default_storage::<D>().arena;
        {
            let mut mpt: MerklePatriciaTrie<[u8; SMALL_OBJECT_LIMIT], D> =
                MerklePatriciaTrie::new();
            mpt = mpt.insert(&([1, 2, 3]), [100; SMALL_OBJECT_LIMIT]);
            mpt = mpt.insert(&([1, 2, 4]), [104; SMALL_OBJECT_LIMIT]);
            mpt = mpt.insert(&([2, 2, 4]), [105; SMALL_OBJECT_LIMIT]);
            assert_eq!(arena.size(), 3);
            assert_eq!(mpt.lookup(&([2, 2, 4])), Some(&[105; SMALL_OBJECT_LIMIT]))
        }
        assert_eq!(arena.size(), 0);
    }

    #[test]
    fn long_extension_paths_serialization() {
        let mut mpt: MerklePatriciaTrie<u8, InMemoryDB<Sha256>> = MerklePatriciaTrie::new();
        mpt = mpt.insert(&(vec![2; 300]), 100);

        let mut bytes = std::vec::Vec::new();
        Serializable::serialize(&mpt, &mut bytes).unwrap();
        let deserialized_mpt: MerklePatriciaTrie<u8> =
            Deserializable::deserialize(&mut bytes.as_slice(), 0).unwrap();
        assert_eq!(deserialized_mpt, mpt);

        assert_eq!(
            mpt.iter()
                .map(|(k, _)| k)
                .collect::<std::vec::Vec<std::vec::Vec<u8>>>(),
            vec![vec![2; 300]]
        );
        assert_eq!(
            deserialized_mpt
                .iter()
                .map(|(k, _)| k)
                .collect::<std::vec::Vec<std::vec::Vec<u8>>>(),
            vec![vec![2; 300]]
        );
    }

    #[test]
    fn mpt_structure() {
        fn validate_long_path(
            mpt: &MerklePatriciaTrie<u8, InMemoryDB<Sha256>>,
            path_length: u64,
            validate_value: u8,
        ) {
            match mpt.0.deref() {
                Node::Extension {
                    compressed_path,
                    child,
                    ..
                } => {
                    assert_eq!(compressed_path.len() as u64, 255);
                    match child.deref() {
                        Node::Extension {
                            compressed_path,
                            child,
                            ..
                        } => {
                            assert_eq!(compressed_path.len() as u64, path_length - 255);
                            assert!(
                                matches!(child.deref(), Node::Leaf { ann: SizeAnn(1), value } if value.deref() == &validate_value)
                            );
                        }
                        _ => unreachable!(),
                    }
                }
                _ => unreachable!(),
            };
        }

        let mut mpt = MerklePatriciaTrie::<u8>::new();
        mpt = mpt.insert(&(vec![2; 300]), 100);

        let mut bytes = std::vec::Vec::new();
        Serializable::serialize(&mpt, &mut bytes).unwrap();
        let deserialized_mpt: MerklePatriciaTrie<u8> =
            Deserializable::deserialize(&mut bytes.as_slice(), 0).unwrap();
        assert_eq!(deserialized_mpt, mpt);

        validate_long_path(&mpt, 300, 100);
        validate_long_path(&deserialized_mpt, 300, 100);
    }

    #[test]
    fn extended_path_insertion() {
        let mut mpt = MerklePatriciaTrie::<u32>::new();
        mpt = mpt.insert(&([1, 2]), 12);
        mpt = mpt.insert(&([1, 2, 3, 4, 5]), 12345);
        mpt = mpt.insert(&([1, 2, 3, 4, 6]), 12346);
        mpt = mpt.insert(&([1, 2, 3, 5, 6]), 12356);
        mpt = mpt.insert(&([1]), 1);

        // Make sure we can look up specific values
        assert_eq!(mpt.lookup(&([1, 2])), Some(&12));
        assert_eq!(mpt.lookup(&([1, 2, 3, 4, 5])), Some(&12345));
        assert_eq!(mpt.lookup(&([1, 2, 3, 5, 6])), Some(&12356));
        assert_eq!(mpt.lookup(&([1])), Some(&1));

        // and make sure we get none for things not in the tree
        assert_eq!(mpt.lookup(&([4])), None);
        assert_eq!(mpt.lookup(&([1, 2, 4])), None);
        // In particular, 123 ends at a *branch*, which used to panic because branch assumed path was non-empty
        assert_eq!(mpt.lookup(&([1, 2, 3])), None);
        assert_eq!(mpt.lookup(&([1, 2, 3, 6])), None);
        // in particular, this test case used to panic with an out of bounds error because the path is empty
        assert_eq!(mpt.lookup(&([])), None);

        // Finally, make sure we can print the tree, which will force a traversal of the whole tree
        println!("{:?}", mpt);
    }

    #[test]
    fn test_canonicity() {
        let segment1 = [0u8; 200];
        let segment2 = [1u8; 200];
        let path1 = segment1
            .iter()
            .chain(segment1.iter())
            .chain(segment1.iter())
            .copied()
            .collect::<Vec<_>>();
        let path2 = segment1
            .iter()
            .chain(segment2.iter())
            .chain(segment2.iter())
            .copied()
            .collect::<Vec<_>>();

        let mpt1 = MerklePatriciaTrie::<()>::new().insert(&path1, ());
        let mpt2 = MerklePatriciaTrie::<()>::new()
            .insert(&path2, ())
            .insert(&path1, ())
            .remove(&path2);
        dbg!(&mpt1);
        dbg!(&mpt2);
        assert_eq!(mpt1.0.hash(), mpt2.0.hash());
    }
}
