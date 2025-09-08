// This file is part of midnight-ledger.
// Copyright (C) 2025 Midnight Foundation
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
use crate::db::DB;
use crate::storable::{Loader, SizeAnn};
use derive_where::derive_where;
use serialize::{self, Deserializable, Serializable, Tagged, tag_enforcement_test};

/// A Merkle Patricia Trie
#[derive_where(Debug, Eq, Clone, PartialEq; V, A)]
#[derive(Storable)]
#[storable(db = D)]
#[storable(db = D, invariant = MerklePatriciaTrie::invariant)]
pub struct MerklePatriciaTrie<
    V: Storable<D>,
    D: DB = DefaultDB,
    A: Storable<D> + Annotation<V> = SizeAnn,
>(pub(crate) Sp<Node<V, D, A>, D>);

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

    fn invariant(&self) -> Result<(), std::io::Error> {
        fn err<V: Storable<D>, D: DB, A: Storable<D> + Annotation<V>>(
            ann: &A,
            true_val: A,
        ) -> Result<A, std::io::Error> {
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "MPT annotation isn't correctly calculated: Annotation {:?}, correct calculation: {:?}",
                    ann, true_val
                ),
            ))
        }

        fn sum_ann<V: Storable<D>, D: DB, A: Storable<D> + Annotation<V>>(
            node: &Node<V, D, A>,
        ) -> Result<A, std::io::Error> {
            match node {
                Node::Empty => Ok(A::empty()),
                Node::Leaf { ann, value } => {
                    let true_val = A::from_value(&value);
                    if ann != &A::from_value(&value) {
                        return err(ann, true_val);
                    };
                    Ok(true_val)
                }
                Node::Branch { ann, children } => {
                    let true_val = children.iter().try_fold(A::empty(), |acc, x| {
                        Ok::<A, std::io::Error>(acc.append(&sum_ann(&x.deref().clone())?))
                    })?;
                    if ann != &true_val {
                        return err(ann, true_val);
                    }
                    Ok(true_val)
                }
                Node::Extension { ann, child, .. } => {
                    let true_val = sum_ann(&child.deref().clone())?;
                    if ann != &true_val {
                        return err(ann, true_val);
                    }
                    Ok(true_val)
                }
                Node::MidBranchLeaf { ann, value, child } => {
                    let true_val = sum_ann(&child.deref().clone())?.append(&A::from_value(&*value));
                    if ann != &true_val {
                        return err(ann, true_val);
                    }
                    Ok(true_val)
                }
            }
        }

        let _ = sum_ann(&self.0.deref())?;
        Ok(())
    }

    /// Insert a value into the trie
    pub fn insert(&self, path: &[u8], value: V) -> Self {
        MerklePatriciaTrie(self.0.insert(path, value).0)
    }

    /// Lookup a value in the trie
    pub fn lookup(&self, path: &[u8]) -> Option<&V> {
        self.0.lookup(path)
    }

    /// Prunes all paths which are lexicographically less than the `target_path`.
    /// Returns the updated tree, and a vector of the removed leaves.
    pub(crate) fn prune(
        &self,
        target_path: &[u8],
        // Returns "is this node empty?" so we can collapse parts of the tree as we go
    ) -> (Self, Vec<Sp<V, D>>) {
        let (node, pruned) = self.0.prune(target_path);
        (MerklePatriciaTrie(node), pruned)
    }

    /// Lookup a value in the trie
    pub fn lookup_sp(&self, path: &[u8]) -> Option<Sp<V, D>> {
        self.0.lookup_sp(path)
    }

    /// Given a path, find the nearest predecessor to that path
    pub(crate) fn find_predecessor<'a>(&'a self, path: &[u8]) -> Option<(Vec<u8>, &'a V)> {
        let mut best_predecessor = None;
        self.0
            .find_predecessor_recursive(path, &mut std::vec::Vec::new(), &mut best_predecessor);
        best_predecessor
    }

    /// Remove a value from the trie
    pub fn remove(&self, path: &[u8]) -> Self {
        MerklePatriciaTrie(self.0.remove(path).0)
    }

    /// Consume internal pointers, returning only the leaves left dangling by this.
    /// Used for custom `Drop` implementations.
    pub fn into_inner_for_drop(self) -> impl Iterator<Item = V> {
        Sp::into_inner(self.0)
            .into_iter()
            .flat_map(Node::into_inner_for_drop)
    }

    /// Generate iterator over leaves, `(path, &value)`
    pub fn iter(&self) -> MPTIter<V, D> {
        MPTIter(self.0.leaves(&[]).into_iter())
    }

    /// Get the number of leaves in a trie
    pub fn size(&self) -> usize {
        self.0.size()
    }

    /// Return true if the trie is empty, false otherwise
    pub fn is_empty(&self) -> bool {
        matches!(self.0.deref(), Node::Empty)
    }

    /// Retrieve the annotation on the root of the trie
    pub fn ann(&self) -> A {
        self.0.ann()
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
pub struct MPTIter<T: Storable<D> + 'static, D: DB>(
    std::vec::IntoIter<(std::vec::Vec<u8>, Sp<T, D>)>,
);

impl<T: Storable<D>, D: DB> Iterator for MPTIter<T, D> {
    type Item = (std::vec::Vec<u8>, Sp<T, D>);

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next()
    }
}

/// Has a size property
pub trait HasSize
where
    Self: Sized + Clone,
{
    /// Getter for the size property
    fn get_size(&self) -> u64;
    /// Setter for the size property
    fn set_size(&self, x: u64) -> Self;
}

// The MPT node
#[derive(Debug, Default)]
#[derive_where(Clone, Hash, PartialEq, Eq; T, A)]
#[derive_where(PartialOrd; T: PartialOrd, A: PartialOrd)]
#[derive_where(Ord; T: Ord, A: Ord)]
pub(crate) enum Node<
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
        children: [Sp<Node<T, D, A>, D>; 16],
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

impl HasSize for SizeAnn {
    fn get_size(&self) -> u64 {
        self.0
    }

    fn set_size(&self, x: u64) -> Self {
        SizeAnn(x)
    }
}

impl Semigroup for SizeAnn {
    fn append(&self, other: &Self) -> Self {
        SizeAnn(self.0 + other.0)
    }
}

impl Monoid for SizeAnn {
    fn empty() -> Self {
        SizeAnn(0)
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

/// A `Semigroup` is a structure with an associative binary operator
///
/// Implementations are responsible for satisfying the law:
///
/// `∀ a b c. (a.append(b)).append(c) == a.append(b.append(c))`
pub trait Semigroup {
    /// The associative binary operator
    fn append(&self, other: &Self) -> Self;
}

/// A `Monoid` is a `Semigroup` with an identity element.
///
/// The identity element, `empty`, can be combined with any other value
/// using `Semigroup::append` without changing the other value.
///
/// For example, for numbers under addition, the identity element is `0`, since `n + 0 == n`.
///
/// Implementations are responsible for satisfying the laws:
///
/// Right identity: `∀ a. a.append(Self::empty()) == a`
/// Left identity: `∀ a. Self::empty().append(a) == a`
pub trait Monoid: Semigroup {
    /// Returns the identity element
    fn empty() -> Self;
}

impl Semigroup for () {
    fn append(&self, _: &Self) -> Self {
        ()
    }
}

impl Monoid for () {
    fn empty() -> Self {
        ()
    }
}

impl Semigroup for u64 {
    fn append(&self, other: &Self) -> Self {
        self.saturating_add(*other)
    }
}

impl Monoid for u64 {
    fn empty() -> Self {
        0
    }
}

impl Semigroup for u128 {
    fn append(&self, other: &Self) -> Self {
        self.saturating_add(*other)
    }
}

impl Monoid for u128 {
    fn empty() -> Self {
        0
    }
}

impl Semigroup for i64 {
    fn append(&self, other: &Self) -> Self {
        self.saturating_add(*other)
    }
}

impl Monoid for i64 {
    fn empty() -> Self {
        0
    }
}

impl Semigroup for i128 {
    fn append(&self, other: &Self) -> Self {
        self.saturating_add(*other)
    }
}

impl Monoid for i128 {
    fn empty() -> Self {
        0
    }
}

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

impl<T: Storable<D>, D: DB, A: Storable<D> + Annotation<T>> Sp<Node<T, D, A>, D> {
    fn lookup_sp(&self, path: &[u8]) -> Option<Sp<T, D>> {
        self.lookup_with(path, Clone::clone)
    }

    fn lookup<'a>(&'a self, path: &[u8]) -> Option<&'a T> {
        self.lookup_with::<&T>(path, |sp| sp.deref())
    }

    fn lookup_with<'a, S>(&'a self, path: &[u8], f: impl FnOnce(&'a Sp<T, D>) -> S) -> Option<S> {
        match self.deref() {
            Node::Empty => None,
            Node::Leaf { value, .. } if path.is_empty() => Some(f(value)),
            // If the path isn't empty
            Node::Leaf { .. } => None,
            Node::Branch { children, .. } => {
                if path.is_empty() {
                    return None;
                }
                let index: usize = path[0].into();
                children[index].lookup_with(&path[1..], f)
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
                child.lookup_with(&path[compressed_path.len()..], f)
            }
            Node::MidBranchLeaf { value, child, .. } => {
                if path.is_empty() {
                    Some(f(value))
                } else {
                    child.lookup_with(path, f)
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
        &self,
        target_path: &[u8],
        // Returns "is this node empty?" so we can collapse parts of the tree as we go
    ) -> (Self, Vec<Sp<T, D>>) {
        if target_path.is_empty() {
            return (self.clone(), vec![]);
        }

        match &**self {
            Node::Empty => (Sp::new(Node::Empty), vec![]),
            Node::Leaf { value, .. } => {
                // We've not matched exactly, but we've ended up at a leaf, which means the leaf is less than the cutoff, so we need to remove it
                (Sp::new(Node::Empty), vec![value.clone()])
            }
            Node::Branch { children, ann, .. } => {
                // All children indexed prior to the head of `path_remaining` are guaranteed to be earlier than the cutoff
                let path_head = target_path[0];
                if path_head >= 16 {
                    panic!("Invalid path nibble: {}", path_head);
                }
                let mut pruned = (0..path_head as usize)
                    .flat_map(|i| children[i].iter())
                    .collect::<Vec<_>>();
                let mut children = children.clone();
                for child in children.iter_mut().take(path_head as usize) {
                    *child = Sp::new(Node::Empty);
                }

                // The child at the `path_head` contains the exact point referenced by the `original_target_path`.
                // As such, we need to `prune` it.
                let (child, pruned_2) = children[path_head as usize].prune(&target_path[1..]);
                children[path_head as usize] = child;
                pruned.extend(pruned_2);

                // If ALL of a branch's children are empty, that makes the branch also empty.
                // If only one is left filled, this branch becomes an extension.
                let no_filled = children.iter().filter(|c| !c.is_empty()).count();

                let ann = if pruned.is_empty() {
                    ann.clone()
                } else {
                    children
                        .iter()
                        .fold(A::empty(), |acc, x| acc.append(&x.ann()))
                };

                match no_filled {
                    0 => (Sp::new(Node::Empty), pruned),
                    1 => {
                        let (path_head, child) = children
                            .into_iter()
                            .enumerate()
                            .find(|(_, child)| !child.is_empty())
                            .expect("Exactly one non-empty child must exist in branch");
                        match &*child {
                            Node::Extension {
                                compressed_path,
                                child,
                                ..
                            } => (
                                Sp::new(Node::Extension {
                                    ann: ann.clone(),
                                    compressed_path: once(path_head as u8)
                                        .chain(compressed_path.iter().copied())
                                        .collect(),
                                    child: child.clone(),
                                }),
                                pruned,
                            ),
                            _ => (
                                Sp::new(Node::Extension {
                                    compressed_path: vec![path_head as u8],
                                    child: child,
                                    ann,
                                }),
                                pruned,
                            ),
                        }
                    }
                    _ => (Sp::new(Node::Branch { children, ann }), pruned),
                }
            }
            Node::Extension {
                compressed_path,
                child,
                ann,
            } => {
                let relevant_target_path =
                    &target_path[..usize::min(target_path.len(), compressed_path.len())];
                match compressed_path[..].cmp(relevant_target_path) {
                    // The extension is entirely smaller than the path to prune, and is removed
                    // entirely.
                    Ordering::Less => (Sp::new(Node::Empty), child.iter().collect()),
                    // The extension matches the path to prune, and we recurse into the child
                    Ordering::Equal => {
                        let (child, pruned) = child.prune(&target_path[compressed_path.len()..]);
                        match &*child {
                            Node::Empty => (Sp::new(Node::Empty), pruned),
                            Node::Extension {
                                compressed_path: cpath2,
                                child,
                                ..
                            } => (
                                Sp::new(Node::Extension {
                                    ann: child.ann(),
                                    compressed_path: compressed_path
                                        .iter()
                                        .chain(cpath2.iter())
                                        .copied()
                                        .collect(),
                                    child: child.clone(),
                                }),
                                pruned,
                            ),
                            _ => (
                                Sp::new(Node::Extension {
                                    ann: if pruned.is_empty() {
                                        ann.clone()
                                    } else {
                                        child.ann()
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
                    Ordering::Greater => (self.clone(), vec![]),
                }
            }
            Node::MidBranchLeaf { value, child, .. } => {
                // Because we know `path_remaining` isn't empty, we know we this node's value is
                // definitely being removed, but we don't know if its child needs to be removed yet.
                // As such, we need to `prune` it.
                let (child, pruned) = child.prune(target_path);
                (child, once(value.clone()).chain(pruned).collect())
            }
        }
    }

    // Apply a function to all values in a node
    pub(crate) fn iter(&self) -> impl Iterator<Item = Sp<T, D>> + '_ {
        let res: Box<dyn Iterator<Item = Sp<T, D>>> = match self.deref() {
            Node::Empty => Box::new(empty()),
            Node::Leaf { value, .. } => Box::new(once(value.clone())),
            Node::Branch { children, .. } => Box::new(children.iter().flat_map(|c| c.iter())),
            Node::Extension { child, .. } => Box::new(child.iter()),
            Node::MidBranchLeaf { value, child, .. } => {
                Box::new(once(value.clone()).chain(child.iter()))
            }
        };
        res
    }

    pub(crate) fn is_empty(&self) -> bool {
        matches!(self.deref(), Node::Empty)
    }

    fn find_predecessor_recursive<'a>(
        &'a self,
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

        match self.deref() {
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
                if !matching_child.is_empty() {
                    let mut new_best_pred = None;
                    Self::with_pushed_nibble(explored_path, path_head, |temp_path| {
                        matching_child.find_predecessor_recursive(
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
                        children[i as usize].find_largest_key_in_subtree(temp_path)
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
                        child.find_predecessor_recursive(
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
                    {
                        if let Some((key, val)) = self.find_largest_key_in_subtree(explored_path) {
                            Self::update_best_pred(best_predecessor, key, val)
                        }
                    }
                }
            }
            Node::MidBranchLeaf { value, child, .. } => {
                // Only if we don't have an exact match should we consider either this branch's value
                // or its child (again, to be pedantic with the naming, to discuss with @Thomas probably)
                // It's better to check the child first since, if we find something, it'll be a better
                // predecessor candidate than this node's value
                let mut new_best_pred = None;
                child.find_predecessor_recursive(
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
            .map_or(true, |(bp_path, _)| candidate_path > *bp_path)
        {
            *best_predecessor = Some((candidate_path, candidate_value));
        }
    }

    pub(crate) fn find_largest_key_in_subtree<'a>(
        &'a self,
        current_path_to_node: &mut Vec<u8>,
    ) -> Option<(Vec<u8>, &'a T)> {
        match self.deref() {
            Node::Empty => None,
            Node::Leaf { value, .. } => Some((current_path_to_node.to_vec(), value.deref())),
            Node::Branch { children, .. } => (0..16).rev().find_map(|i| {
                Self::with_pushed_nibble(current_path_to_node, i as u8, |p| {
                    children[i].find_largest_key_in_subtree(p)
                })
            }),
            Node::Extension {
                compressed_path,
                child,
                ..
            } => Self::with_extended_suffix(current_path_to_node, compressed_path, |temp_path| {
                child.find_largest_key_in_subtree(temp_path)
            }),
            Node::MidBranchLeaf { value, child, .. } => {
                let largest_in_child = child.find_largest_key_in_subtree(current_path_to_node);
                if largest_in_child.is_some() {
                    return largest_in_child;
                }

                Some((current_path_to_node.to_vec(), value.deref()))
            }
        }
    }

    /// Return the annotation of the `Node`
    pub fn ann(&self) -> A {
        match &**self {
            Node::Empty => A::empty(),
            Node::Leaf { ann, .. }
            | Node::Branch { ann, .. }
            | Node::Extension { ann, .. }
            | Node::MidBranchLeaf { ann, .. } => (*ann).clone(),
        }
    }

    // Inserts a value at a given path.
    // Returns the updated tree, and the existing value at that path, if applicable.
    fn insert(&self, path: &[u8], value: T) -> (Self, Option<Sp<T, D>>) {
        if path.is_empty() {
            let value_sp = self.arena.alloc(value.clone());
            let (node, existing_val) = match self.deref() {
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
                    let new_ann = self.ann().append(&Annotation::<T>::from_value(&value));
                    (
                        Node::MidBranchLeaf {
                            ann: new_ann,
                            value: value_sp,
                            child: self.clone(),
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
                        ann: child.ann().append(&Annotation::<T>::from_value(&value)),
                        value: value_sp,
                        child: child.clone(),
                    },
                    Some(old_val.clone()),
                ),
            };
            return (self.arena.alloc(node), existing_val);
        }

        match self.deref() {
            Node::Empty => {
                let value_sp = self.arena.alloc(value.clone());
                let mut child = self.arena.alloc(Node::Leaf {
                    ann: Annotation::<T>::from_value(&value),
                    value: value_sp,
                });

                for working_path in path.chunks(255).rev() {
                    child = self.arena.alloc(Node::Extension {
                        ann: child.ann(),
                        compressed_path: working_path.to_vec(),
                        child: child.clone(),
                    });
                }
                (child, None)
            }
            Node::Leaf {
                ann: existing_ann,
                value: self_value,
                ..
            } => {
                let mut child = self.arena.alloc(Node::Leaf {
                    ann: A::from_value(&value),
                    value: self.arena.alloc(value.clone()),
                });
                for chunk in path.chunks(255).rev() {
                    child = self.arena.alloc(Node::Extension {
                        ann: child.ann(),
                        compressed_path: chunk.to_vec(),
                        child,
                    });
                }

                let branch_ann = A::from_value(&value).append(&existing_ann);
                (
                    self.arena.alloc(Node::MidBranchLeaf {
                        ann: branch_ann,
                        value: self_value.clone(),
                        child,
                    }),
                    None,
                )
            }
            Node::Branch { children, .. } => {
                let index: usize = path[0].into();
                let mut new_children = children.clone();
                let (new_child, existing) = new_children[index].insert(&path[1..], value);
                new_children[index] = new_child;

                let new_ann = new_children
                    .iter()
                    .fold(A::empty(), |ann, c| ann.append(&c.ann()));

                (
                    self.arena.alloc(Node::Branch {
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
                    let (new_child, existing) = child.insert(&path[index..], value);
                    return (
                        self.arena.alloc(Node::Extension {
                            ann: new_child.ann(),
                            compressed_path: compressed_path.clone(),
                            child: new_child,
                        }),
                        existing,
                    );
                } else {
                    // if path splits on final nibble old child node doesn't need an extension,
                    // otherwise it does
                    let remaining = if index == compressed_path.len() - 1 {
                        child.clone()
                    } else {
                        self.arena.alloc(Node::Extension {
                            ann: child.ann(),
                            compressed_path: compressed_path[(index + 1)..].to_vec(),
                            child: child.clone(),
                        })
                    };

                    let compressed_path_index: usize = compressed_path[index].into();
                    let mut children: [Sp<Node<T, D, A>, D>; 16] =
                        core::array::from_fn(|_| self.arena.alloc(Node::Empty));
                    children[compressed_path_index] = remaining;

                    let initial_ann = children
                        .iter()
                        .map(|c| c.ann())
                        .fold(A::empty(), |acc, child_ann| acc.append(&child_ann));

                    let branch = self.arena.alloc(Node::Branch {
                        ann: initial_ann,
                        children,
                    });

                    let (final_branch, existing) = branch.insert(&path[index..], value);

                    // if path split on first nibble no extension required, otherwise it is
                    if index == 0 {
                        (final_branch, existing)
                    } else {
                        (
                            self.arena.alloc(Node::Extension {
                                ann: final_branch.ann(),
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
                let (new_child, existing) = child.insert(path, value);
                let new_ann = A::from_value(&leaf_value).append(&new_child.ann());

                (
                    self.arena.alloc(Node::MidBranchLeaf {
                        ann: new_ann,
                        value: leaf_value.clone(),
                        child: new_child,
                    }),
                    existing,
                )
            }
        }
    }

    fn size(&self) -> usize {
        match self.deref() {
            Node::Empty => 0,
            Node::Leaf { .. } => 1,
            Node::Extension { ann, .. }
            | Node::Branch { ann, .. }
            | Node::MidBranchLeaf { ann, .. } => ann.clone().get_size() as usize,
        }
    }

    fn leaves(&self, current_path: &[u8]) -> std::vec::Vec<(std::vec::Vec<u8>, Sp<T, D>)> {
        match self.deref() {
            Node::Empty => std::vec::Vec::new(),
            Node::Leaf { value, .. } => vec![(current_path.to_vec(), value.clone())],
            Node::Extension {
                compressed_path,
                child,
                ..
            } => {
                let mut new_path = current_path.to_vec();
                new_path.append(&mut compressed_path.clone());
                child.leaves(new_path.as_slice())
            }
            Node::Branch { children, .. } => {
                let mut leaves = std::vec::Vec::new();
                for (i, child) in children.iter().enumerate() {
                    let mut new_path = current_path.to_vec();
                    new_path.push(i as u8);
                    leaves.extend(child.leaves(new_path.as_slice()));
                }
                leaves
            }
            Node::MidBranchLeaf { value, child, .. } => {
                let mut leaves = child.leaves(current_path);
                leaves.push((current_path.to_vec(), value.clone()));
                leaves
            }
        }
    }

    /// Removes a value from a path, doing nothing if no value was present.
    /// Returns the updated node, and the value removed if applicable.
    pub fn remove(&self, path: &[u8]) -> (Self, Option<Sp<T, D>>) {
        match self.deref() {
            Node::Empty => (self.arena.alloc(Node::Empty), None),
            Node::Leaf { value, ann } => {
                if path.is_empty() {
                    return (self.arena.alloc(Node::Empty), Some(value.clone()));
                }

                (
                    self.arena.alloc(Node::Leaf {
                        ann: ann.clone(),
                        value: value.clone(),
                    }),
                    None,
                )
            }
            Node::Branch { children, .. } => {
                let mut new_children = children.clone();
                let index: usize = path[0].into();
                let (new_child, removed) = new_children[index].remove(&path[1..]);
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
                            (
                                self.arena.alloc(Node::Extension {
                                    ann: child.ann(),
                                    compressed_path: new_compressed_path,
                                    child,
                                }),
                                removed,
                            )
                        }
                        _ => (
                            self.arena.alloc(Node::Extension {
                                ann: only_child.ann(),
                                compressed_path: vec![only_child_index as u8],
                                child: only_child.clone(),
                            }),
                            removed,
                        ),
                    }
                } else {
                    (
                        self.arena.alloc(Node::Branch {
                            ann: new_children
                                .iter()
                                .fold(A::empty(), |acc, x| acc.append(&x.ann())),
                            children: new_children,
                        }),
                        removed,
                    )
                }
            }
            Node::Extension {
                ann,
                compressed_path,
                child,
                ..
            } => {
                for i in 0..compressed_path.len() {
                    if compressed_path[i] != path[i] {
                        return (
                            self.arena.alloc(Node::Extension {
                                ann: ann.clone(),
                                compressed_path: compressed_path.clone(),
                                child: child.clone(),
                            }),
                            None,
                        );
                    }
                }

                let (new_child, removed) = child.remove(&path[compressed_path.len()..]);
                let new_ann = new_child.ann();
                match new_child.deref() {
                    Node::Empty => (self.arena.alloc(Node::Empty), removed),
                    Node::Extension {
                        compressed_path: p,
                        child: c,
                        ..
                    } => {
                        let mut new_compressed_path = compressed_path.clone();
                        new_compressed_path.append(&mut p.clone());

                        let mut child = c.clone();
                        for path_chunk in new_compressed_path.chunks(255).rev() {
                            child = if path_chunk.is_empty() {
                                child
                            } else {
                                self.arena.alloc(Node::Extension {
                                    ann: new_ann.clone(),
                                    compressed_path: path_chunk.to_vec(),
                                    child: child.clone(),
                                })
                            };
                        }
                        (child, removed)
                    }
                    _ => (
                        self.arena.alloc(Node::Extension {
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
                    let (child, removed) = child.remove(path);
                    let new_ann = child.ann().append(&Annotation::<T>::from_value(&value));
                    match child.deref() {
                        Node::Empty => (
                            self.arena.alloc(Node::Leaf {
                                ann: new_ann,
                                value: value.clone(),
                            }),
                            removed,
                        ),
                        _ => (
                            self.arena.alloc(Node::MidBranchLeaf {
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
            Node::Leaf { value, .. } => vec![value.root.clone()],
            Node::Branch { children, .. } => children.iter().map(|sp| sp.root.clone()).collect(),
            Node::Extension { child, .. } => vec![child.root.clone()],
            Node::MidBranchLeaf { child, value, .. } => {
                vec![value.root.clone(), child.root.clone()]
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
                A::serialize(&ann, writer)?;
            }
            Node::Branch { ann, .. } => {
                u8::serialize(&2, writer)?;
                A::serialize(&ann, writer)?;
            }
            Node::Extension {
                ann,
                compressed_path,
                ..
            } => {
                u8::serialize(&3, writer)?;
                A::serialize(&ann, writer)?;
                let compressed = compress_nibbles(compressed_path);
                u8::serialize(&(compressed_path.len() as u8), writer)?;
                std::vec::Vec::<u8>::serialize(&compressed, writer)?;
            }
            Node::MidBranchLeaf { ann, .. } => {
                u8::serialize(&4, writer)?;
                A::serialize(&ann, writer)?;
            }
        }

        Ok(())
    }

    fn check_invariant(&self) -> Result<(), std::io::Error> {
        match self {
            Node::Empty | Node::Leaf { .. } => {}
            Node::Branch { ann, children } => {
                let non_empty_children = children
                    .iter()
                    .filter(|child| matches!(***child, Node::Empty))
                    .count();
                if non_empty_children < 2 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "Fewer than 2 non-empty children in Node::Branch".to_string(),
                    ));
                }
                if ann.get_size()
                    != children
                        .iter()
                        .map(|child| child.size() as u64)
                        .sum::<u64>()
                {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "Recorded branch size doesn't match sum of children",
                    ));
                }
            }
            Node::Extension {
                ann,
                compressed_path,
                child,
            } => {
                if matches!(child.deref(), Node::Extension { .. }) && compressed_path.len() != 255 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "Node::Extension path must be of length 255 when having another Node::Extension child",
                    ));
                }
                if compressed_path.len() > 255 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "Node::Extension path may not be longer than 255",
                    ));
                }
                if ann.get_size() != child.size() as u64 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "Recorded extension size doesn't match child size",
                    ));
                }
                if compressed_path.iter().any(|b| *b > 0x0f) {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "Node::Extension path must consist of nibbles",
                    ));
                }
            }
            Node::MidBranchLeaf { ann, child, .. } => {
                match child.deref() {
                    Node::Branch { .. } | Node::Extension { .. } => {}
                    _ => {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "Node::MidBranchLeaf may only have Node::Branch or Node::Extension children",
                        ));
                    }
                }
                if ann.get_size() != child.size() as u64 + 1 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "Recorded mid-branch-leaf size isn't one greater than child size",
                    ));
                }
            }
        }
        Ok(())
    }

    #[inline(always)]
    fn from_binary_repr<R: Read>(
        reader: &mut R,
        child_hashes: &mut impl Iterator<Item = ArenaKey<D::Hasher>>,
        loader: &impl Loader<D>,
    ) -> Result<Node<T, D, A>, std::io::Error> {
        let disc = u8::deserialize(reader, 0)?;

        match disc {
            0 => Ok(Node::Empty),
            1 => {
                let ann = A::deserialize(reader, 0)?;
                Ok(Node::Leaf {
                    ann,
                    value: loader.get_next(child_hashes)?,
                })
            }
            2 => {
                let ann = A::deserialize(reader, 0)?;
                let mut children: [Sp<Node<T, D, A>, D>; 16] =
                    core::array::from_fn(|_| loader.alloc(Node::Empty));

                #[allow(clippy::needless_range_loop)]
                for child in children.iter_mut() {
                    *child = loader.get_next(child_hashes)?;
                }

                Ok(Node::Branch { ann, children })
            }
            3 => {
                let ann = A::deserialize(reader, 0)?;
                let len = u8::deserialize(reader, 0)?;
                let path =
                    expand_nibbles(&std::vec::Vec::<u8>::deserialize(reader, 0)?, len as usize);
                let child: Sp<Node<T, D, A>, D> = loader.get_next(child_hashes)?;
                Ok(Node::Extension {
                    ann,
                    compressed_path: path,
                    child,
                })
            }
            4 => {
                let ann = A::deserialize(reader, 0)?;
                let value: Sp<T, D> = loader.get_next(child_hashes)?;
                let child: Sp<Node<T, D, A>, D> = loader.get_next(child_hashes)?;
                Ok(Node::MidBranchLeaf { ann, value, child })
            }
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Unrecognised discriminant",
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use sha2::Sha256;

    use crate::{
        Storage,
        db::InMemoryDB,
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
    fn deduplicate() {
        // Isolate our storage, since we're checking arena size.
        struct Tag;
        type D = WrappedDB<DefaultDB, Tag>;
        let _ = set_default_storage::<D>(Storage::default);
        let mut mpt = MerklePatriciaTrie::<u64, D>::new();
        mpt = mpt.insert(&([1, 2, 3]), 100);
        mpt = mpt.insert(&([1, 2, 2]), 100);
        assert_eq!(mpt.lookup(&([1, 2, 3])), Some(&100));
        assert_eq!(mpt.lookup(&([1, 2, 2])), Some(&100));
        assert_eq!(mpt.size(), 2);
        dbg!(&mpt.0.arena);
        assert_eq!(mpt.0.arena.size(), 5);
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
            let mut mpt: MerklePatriciaTrie<u64, D> = MerklePatriciaTrie::new();
            mpt = mpt.insert(&([1, 2, 3]), 100);
            mpt = mpt.insert(&([1, 2, 4]), 104);
            mpt = mpt.insert(&([2, 2, 4]), 105);
            assert_eq!(arena.size(), 11);
            assert_eq!(mpt.lookup(&([2, 2, 4])), Some(&105));
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
}
