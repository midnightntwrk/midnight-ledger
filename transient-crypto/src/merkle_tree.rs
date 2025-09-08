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

//! Sparse, fixed-depth Merkle trees.

use crate::curve::Fr;
use crate::hash::{degrade_to_transient, transient_hash};
use crate::repr::FieldRepr;
use base_crypto::hash::{HashOutput, persistent_hash};
use base_crypto::repr::{BinaryHashRepr, MemWrite};
use derive_where::derive_where;
use fake::Dummy;
#[cfg(feature = "proptest")]
use proptest::arbitrary::Arbitrary;
#[cfg(feature = "proptest")]
use proptest_derive::Arbitrary;
use rand::Rng;
use rand::distributions::{Distribution, Standard};
use serde::{Deserialize, Deserializer, Serialize, Serializer, ser::SerializeTuple};
#[cfg(feature = "proptest")]
use serialize::NoStrategy;
use serialize::{Deserializable, Serializable, Tagged, tag_enforcement_test};
use std::error::Error;
use std::fmt::{self, Debug, Display, Formatter};
use std::hash::Hash;
#[cfg(feature = "proptest")]
use std::marker::PhantomData;
use std::ops::Deref;
use storage::DefaultDB;
use storage::Storable;
use storage::arena::{ArenaKey, Sp};
use storage::db::DB;
#[cfg(test)]
use storage::db::InMemoryDB;
use storage::storable::Loader;

/// A `Storable` wrapper around `HashOutput`
#[derive(PartialEq, Eq, PartialOrd, Hash, Clone, Debug, Ord, Serializable)]
pub struct MerkleTreeHash(HashOutput);

/// A path in a Merkle tree
#[derive(Debug, Clone, FieldRepr, Serializable)]
pub struct MerklePath<T> {
    /// The leaf this path is pointing at. This is the contained element, not its hash!
    pub leaf: T,
    /// How to reach the tree root from this leaf.
    pub path: Vec<MerklePathEntry>,
}

/// The domain separator used in leaf commitments.
pub const LEAF_HASH_DOMAIN_SEP: &[u8] = b"mdn:lh";

/// An index into a Merkle tree which could not be resolved, as there is no item
/// there, or the path was deliberately collapsed.
#[derive(Debug, Clone)]
pub struct InvalidIndex(pub u64);

impl Display for InvalidIndex {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "invalid index into sparse merkle tree: {}", self.0)
    }
}

impl Error for InvalidIndex {}

/// An index into a Merkle tree which could not be resolved, as there is no item
/// there, or the path was deliberately collapsed.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum InvalidUpdate {
    /// The given index, for the given tree height, was collapsed, but needed
    /// for an update.
    CollapsedIndex(u128, u8),
    /// The given index, for the given tree height, was stubbed, but needed to
    /// produce an update.
    StubUpdate(u128, u8),
    /// The update range end is before the range start.
    EndBeforeStart(u64, u64),
    /// The update range end is not in the tree bounds.
    EndOutOfTree(u64),
    /// The update contained a different number of segments than expected.
    WrongNumberOfSegments(usize, usize),
    /// Attempted to build an update over a not-fully rehashed tree
    NotFullyRehashed,
    /// An update path didn't make sense (e.g. too many entries)
    BadUpdatePath,
}

impl Display for InvalidUpdate {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        use InvalidUpdate::*;
        match self {
            CollapsedIndex(idx, height) => write!(
                f,
                "attempted update on collapsed sub-tree at {idx}/{height}"
            ),
            StubUpdate(idx, height) => {
                write!(f, "attempted update on updated sub-tree at {idx}/{height}")
            }
            EndBeforeStart(start, end) => write!(
                f,
                "attempted update with end ({end}) after before ({start})"
            ),
            EndOutOfTree(end) => write!(f, "attempted update with end ({end}) outside of the tree"),
            WrongNumberOfSegments(..) => write!(f, "attempted update with mismatch segment count"),
            NotFullyRehashed => write!(f, "attempted update without the tree being fully rehashed"),
            BadUpdatePath => write!(
                f,
                "attempted to apply an update path that wasn't compatible with the tree"
            ),
        }
    }
}

impl Error for InvalidUpdate {}

/// The hash of any given leaf.
pub fn leaf_hash<T: BinaryHashRepr + ?Sized>(value: &T) -> HashOutput {
    let mut data = Vec::with_capacity(value.binary_len() + LEAF_HASH_DOMAIN_SEP.len());
    data.extend(LEAF_HASH_DOMAIN_SEP);
    value.binary_repr(&mut data);
    persistent_hash(&data)
}

impl<T: BinaryHashRepr> MerklePath<T> {
    /// The tree root that matches a Merkle path.
    pub fn root(&self) -> MerkleTreeDigest {
        MerkleTreeDigest(self.path.iter().fold(
            degrade_to_transient(leaf_hash(&self.leaf)),
            |acc, entry| {
                if entry.goes_left {
                    transient_hash(&[acc, entry.sibling.0])
                } else {
                    transient_hash(&[entry.sibling.0, acc])
                }
            },
        ))
    }
}

/// One entry in the Merkle path.
#[derive(Debug, Clone, FieldRepr, Serializable)]
pub struct MerklePathEntry {
    /// The hash of the sibling element.
    pub sibling: MerkleTreeDigest,
    /// Whether the path went left at this branch.
    pub goes_left: bool,
}

/// A part describing a specific tree insertion, together with intermediate hashes.
/// This allows replaying this insertion, even against collapsed trees.
/// The intermediate hashes may be missing, in case the tree was not fully
/// rehashed, in which case its success depends on the non-rehashed parts not
/// being collapsed in the target for insertion.
#[derive(Debug, Clone, PartialEq, Eq, Serializable, Storable)]
#[tag = "tree-insertion-path[v1]"]
#[storable(base)]
pub struct TreeInsertionPath<A>
where
    A: Serializable + Deserializable + Clone + Sync + Send + 'static,
{
    /// The leaf that was ultimately inserted
    pub leaf: (HashOutput, A),
    /// The path itself, from the leaf up
    pub path: Vec<TreeInsertionPathEntry>,
}
tag_enforcement_test!(TreeInsertionPath<()>);

/// An item in [`TreeInsertionPath`].
#[derive(Debug, Clone, PartialEq, Eq, Serializable)]
#[tag = "tree-insertion-path-entry[v1]"]
pub struct TreeInsertionPathEntry {
    /// The hash of the element along the path (*not* the sibling!), if available.
    pub hash: Option<MerkleTreeDigest>,
    /// Whether the path went left at this branch.
    pub goes_left: bool,
}
tag_enforcement_test!(TreeInsertionPathEntry);

/// The hash of a Merkle tree node.
#[derive(
    Copy,
    Clone,
    Hash,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    FieldRepr,
    Serializable,
    Serialize,
    Dummy,
    Storable,
)]
#[storable(base)]
#[cfg_attr(feature = "proptest", derive(Arbitrary))]
#[tag = "merkle-tree-digest[v1]"]
pub struct MerkleTreeDigest(pub Fr);
tag_enforcement_test!(MerkleTreeDigest);

impl rand::distributions::Distribution<MerkleTreeDigest> for rand::distributions::Standard {
    fn sample<R: rand::Rng + ?Sized>(&self, rng: &mut R) -> MerkleTreeDigest {
        MerkleTreeDigest(rng.r#gen())
    }
}

impl From<Fr> for MerkleTreeDigest {
    fn from(field: Fr) -> MerkleTreeDigest {
        MerkleTreeDigest(field)
    }
}

impl From<MerkleTreeDigest> for Fr {
    fn from(digest: MerkleTreeDigest) -> Fr {
        digest.0
    }
}
impl Debug for MerkleTreeDigest {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

/// A concise update skipping for a range of the tree, in collapsed form.
#[derive(Clone, Hash, PartialEq, Eq, PartialOrd, Ord, Serializable)]
#[tag = "merkle-tree-collapsed-update[v1]"]
pub struct MerkleTreeCollapsedUpdate {
    /// The first index covered by the update range.
    pub start: u64,
    /// The last index covered by the update range.
    pub end: u64,
    hashes: Vec<MerkleTreeDigest>,
}
tag_enforcement_test!(MerkleTreeCollapsedUpdate);

impl Debug for MerkleTreeCollapsedUpdate {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.debug_struct("MerkleTreeCollapsedUpdate")
            .field("start", &self.start)
            .field("end", &self.end)
            .finish()
    }
}

impl MerkleTreeCollapsedUpdate {
    fn step_sizes(mut a: u64, b: u64) -> Vec<u8> {
        // A hash of height 0 can step from x to x+1
        // A hash of height h can step from x to x+(2^h) IF 2^h | x
        // We want to step from a to b in the fewest number of steps
        // Ex: 010110 -> 011101
        //   - 010110 -> 011000 (+ 10)
        //   - 011000 -> 011100 (+ 100)
        //   - 011100 -> 011101 (+ 1)
        // Two stages:
        // 1. Get the MSB that differs flipped, by adding increasing powers of
        //    two when possible
        // 2. Cascade down, decreasing powers of two until equal to b.
        let msb_diff = (b ^ a).ilog2() as u8;
        let mut res = Vec::with_capacity(u8::max(1, msb_diff) as usize * 2 - 1);
        // Stage 1: Flip increasing
        for bit in 0..msb_diff {
            let shifted = 1 << bit as u64;
            if (a & shifted) != 0 {
                a += shifted;
                res.push(bit);
            }
        }
        // Stage 1.5: Flip msb
        if a & (1 << msb_diff) == 0 {
            res.push(msb_diff);
        }
        // Stage 2: Flip decreasing
        for bit in (0..msb_diff).rev() {
            if (b & (1 << bit as u64)) != 0 {
                res.push(bit);
            }
        }
        res
    }

    fn partial_index<A: Storable<D>, D: DB>(
        tree: &MerkleTree<A, D>,
        idx: u128,
        height: u8,
    ) -> Result<MerkleTreeDigest, InvalidUpdate> {
        let hdiff = tree.height() - height;
        let mut curr = tree.0.deref();
        for i in 0..hdiff {
            let go_left = (idx & (1u128 << (tree.height() - i - 1))) == 0;
            match curr {
                Leaf { .. } => unreachable!(),
                Node { left, right, .. } => curr = if go_left { left } else { right },
                Collapsed { .. } => return Err(InvalidUpdate::CollapsedIndex(idx, height)),
                Stub { .. } => return Err(InvalidUpdate::StubUpdate(idx, height)),
            }
        }
        Ok(MerkleTreeDigest(
            curr.root().ok_or(InvalidUpdate::NotFullyRehashed)?,
        ))
    }

    /// Creates a new collapsed update slice, starting from a given index, and
    /// ending (inclusively) at a given index.
    ///
    /// The source tree should either not be collapsed, or collapse exactly
    /// between these indices, and not further. It should also not include stub
    /// entries (that is, be a sparse part of the tree).
    pub fn new<A: Storable<D>, D: DB>(
        tree: &MerkleTree<A, D>,
        start: u64,
        end: u64,
    ) -> Result<Self, InvalidUpdate> {
        if end < start {
            return Err(InvalidUpdate::EndBeforeStart(start, end));
        }
        if (end as u128) >= (1u128 << tree.height()) {
            return Err(InvalidUpdate::EndOutOfTree(end));
        }
        let step_sizes = Self::step_sizes(start, end + 1);
        let mut hashes = Vec::with_capacity(step_sizes.len());
        let mut curr = start as u128;
        for step in step_sizes.into_iter() {
            hashes.push(Self::partial_index(tree, curr, step)?);
            curr += 1u128 << step as u128;
        }
        Ok(MerkleTreeCollapsedUpdate { start, end, hashes })
    }
}

/// A Merkle tree, represented sparsely.
///
/// Unless otherwise specified, operations are O(height), excepting
/// serialization. All operations are safe, unless parts of the tree are
/// collapsed.
///
/// The tree is indexed as if it were an array of length `2^height`: data leaves
/// only occur at the end of paths of length `height` bits.
#[derive_where(Clone, PartialOrd, PartialEq, Ord, Eq; A)]
#[derive(Storable)]
#[storable(db = D)]
#[tag = "merkle-tree[v1]"]
pub struct MerkleTree<A: Storable<D>, D: DB = DefaultDB>(Sp<MerkleTreeNode<A, D>, D>);
tag_enforcement_test!(MerkleTree<(), InMemoryDB>);

impl<A: Storable<D>, D: DB> Hash for MerkleTree<A, D> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        Hash::hash(&ArenaKey::from(self.0.hash()), state)
    }
}

impl<A: Debug + Storable<D>, D: DB> Debug for MerkleTree<A, D> {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.write_fmt(format_args!("MerkleTree(root = {:?}) ", self.root()))?;
        self.0.fmt(f)
    }
}

struct SerdeMerkleTreeMap<'a, A: Storable<D>, D: DB>(&'a MerkleTree<A, D>);

impl<A: Serialize + Storable<D>, D: DB> Serialize for MerkleTree<A, D> {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        let mut pair = ser.serialize_tuple(2)?;
        pair.serialize_element(&self.height())?;
        pair.serialize_element(&SerdeMerkleTreeMap(self))?;
        pair.end()
    }
}

impl<A: Serialize + Storable<D>, D: DB> Serialize for SerdeMerkleTreeMap<'_, A, D> {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.collect_map(self.0.iter_aux())
    }
}

impl<'de, A: Deserialize<'de> + Storable<D>, D: DB> Deserialize<'de> for MerkleTree<A, D> {
    fn deserialize<D2: Deserializer<'de>>(de: D2) -> Result<Self, D2::Error> {
        let (height, data): (u8, std::collections::HashMap<u64, (HashOutput, A)>) =
            Deserialize::deserialize(de)?;
        Ok(data
            .into_iter()
            .fold(MerkleTree::blank(height), |mt, (k, (v, a))| {
                MerkleTree::update_hash(&mt, k, v, a)
            }))
    }
}

/// Inner Merkle tree node type
#[derive_where(Clone, Hash, Ord, PartialOrd, Eq, PartialEq; A)]
#[derive(Storable)]
#[storable(db = D, invariant = MerkleTreeNode::invariant)]
#[tag = "merkle-tree-node[v1]"]
pub enum MerkleTreeNode<A: Storable<D>, D: DB> {
    /// Leaf node
    Leaf {
        /// Stored hash
        hash: HashOutput,
        /// Auxiliary data
        aux: A,
    },
    /// Collapsed
    Collapsed {
        /// Intermediate hash
        hash: Fr,
        /// Height of collapsed section
        height: u8,
    },
    /// Stub
    Stub {
        /// Height
        height: u8,
    },
    /// Branching node
    Node {
        /// Intermediate hash
        hash: Option<Sp<Fr, D>>,
        /// Left node
        #[storable(child)]
        left: Sp<MerkleTreeNode<A, D>, D>,
        /// Right node
        #[storable(child)]
        right: Sp<MerkleTreeNode<A, D>, D>,
        /// Height
        height: u8,
    },
}
tag_enforcement_test!(MerkleTreeNode<(), InMemoryDB>);

impl<Faker, D: DB> fake::Dummy<Faker> for MerkleTree<(), D> {
    fn dummy(_config: &Faker) -> Self {
        // TODO: make random trees!
        MerkleTree::<(), D>::blank(32)
            .update(0, &Fr::from(42u64), ())
            .update(0, &Fr::from(41u64), ())
            .update(3, &Fr::from(43u64), ())
            .update(62, &Fr::from(12u64), ())
    }

    fn dummy_with_rng<R: rand::Rng + ?Sized>(config: &Faker, _rng: &mut R) -> Self {
        Self::dummy(config)
    }
}

struct DebugCollapsed;

impl Debug for DebugCollapsed {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "<collapsed>")
    }
}

struct DebugEntry<'a, A>(HashOutput, &'a A);

impl<A: Debug> Debug for DebugEntry<'_, A> {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "({:?}, {:?})", &self.0, &self.1)
    }
}

impl<A: Debug + Storable<D>, D: DB> Debug for MerkleTreeNode<A, D> {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        use LeafOrCollapsed::*;
        let mut map = f.debug_map();
        let mut current_collapsed_range = None;
        for leaf in self.leaves().into_iter() {
            match (leaf, current_collapsed_range) {
                (Leaf { index, hash, aux }, None) => {
                    map.entry(&index, &DebugEntry(hash, aux));
                }
                (Leaf { index, hash, aux }, Some((start, end))) => {
                    map.entry(&(start..=end), &DebugCollapsed);
                    current_collapsed_range = None;
                    map.entry(&index, &DebugEntry(hash, aux));
                }
                (Collapsed { start, end }, Some((start2, end2))) if end2 + 1 == start => {
                    current_collapsed_range = Some((start2, end));
                }
                (Collapsed { start, end }, Some((start2, end2))) => {
                    map.entry(&(start2..=end2), &DebugCollapsed);
                    current_collapsed_range = Some((start, end));
                }
                (Collapsed { start, end }, None) => {
                    current_collapsed_range = Some((start, end));
                }
            }
        }
        if let Some((start, end)) = current_collapsed_range {
            map.entry(&(start..=end), &DebugCollapsed);
        }
        map.finish()
    }
}

enum LeafOrCollapsed<'a, A> {
    Leaf {
        index: u64,
        hash: HashOutput,
        aux: &'a A,
    },
    Collapsed {
        start: u64,
        end: u64,
    },
}

impl<A> LeafOrCollapsed<'_, A> {
    fn upgrade(self, shift: u64) -> Self {
        use LeafOrCollapsed::*;
        match self {
            Leaf { index, hash, aux } => Leaf {
                index: index + shift,
                hash,
                aux,
            },
            Collapsed { start, end } => Collapsed {
                start: start + shift,
                end: end + shift,
            },
        }
    }
}

impl<A: Storable<D>, D: DB> MerkleTreeNode<A, D> {
    fn invariant(&self) -> std::io::Result<()> {
        if let Node {
            left,
            right,
            height,
            hash,
        } = self
        {
            if *height == 0 || *height - 1 != left.height() || *height - 1 != right.height() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "MerkleTree inconsistent height on deserialization",
                ));
            }
            // NOTE: WARN: we *cannot* check the hash invariant on deserialization!
            // This is because deserialization is *severely* compute-time limited, and hashing is
            // compute-heavy.
            //
            // What does this mean? Well, it means that untrusted Merkle trees could be internally
            // invalid, which in turn means they may accept membership proofs for elements that do
            // not appear to be present, until they are updated and rehashed.
            //
            // The only places untrusted Merkle trees can appear as of the time of writing are in
            // Impact `push` operations, and contract initial deployments. Either case is within
            // language and dApp bounds, and it is therefore on the language to ensure that these
            // are computed sensibly.
            //
            //   if hash != transient_hash(&[left.root(), right.root()]) {
            //       // ...
            //   }

            // If we *do* have a computed hash, children need to as well!
            if hash.is_some() && (left.root().is_none() || right.root().is_none()) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "MerkleTree children not rehashed, but parent claiming to be",
                ));
            }
        }
        Ok(())
    }

    fn leaves(&self) -> Vec<LeafOrCollapsed<'_, A>> {
        match self {
            Leaf { hash, aux } => vec![LeafOrCollapsed::Leaf {
                index: 0,
                hash: *hash,
                aux,
            }],
            Stub { .. } => Vec::new(),
            Collapsed { .. } => vec![LeafOrCollapsed::Collapsed {
                start: 0,
                end: (1u64 << self.height()) - 1,
            }],
            Node { left, right, .. } => left
                .leaves()
                .into_iter()
                .chain(
                    right
                        .leaves()
                        .into_iter()
                        .map(|leaf| leaf.upgrade(1 << left.height())),
                )
                .collect(),
        }
    }

    fn new(height: u8) -> Self {
        Stub { height }
    }

    fn height(&self) -> u8 {
        match self {
            Leaf { .. } => 0,
            Stub { height, .. } => *height,
            Collapsed { height, .. } => *height,
            Node { height, .. } => *height,
        }
    }

    fn root(&self) -> Option<Fr> {
        match self {
            Leaf { hash, .. } => Some(degrade_to_transient(*hash)),
            Stub { .. } => Some(Fr::default()),
            Collapsed { hash, .. } => Some(*hash),
            Node { hash, .. } => hash.as_ref().map(|h| **h),
        }
    }

    #[allow(clippy::wrong_self_convention)]
    fn is_collapsed(&self) -> bool {
        matches!(self, Collapsed { .. })
    }

    fn update_from_evidence_internal(
        &self,
        leaf: (HashOutput, A),
        path: &[TreeInsertionPathEntry],
    ) -> Result<Self, InvalidUpdate> {
        if path.is_empty() {
            return Ok(Leaf {
                hash: leaf.0,
                aux: leaf.1,
            });
        }
        let entry = path.last().expect("non-empty");
        Ok(match self {
            Collapsed { height, .. } => Collapsed {
                hash: entry.hash.ok_or(InvalidUpdate::BadUpdatePath)?.0,
                height: *height,
            },
            Node {
                left,
                right,
                height,
                ..
            } => {
                if entry.goes_left {
                    Node {
                        hash: None,
                        left: Sp::new(
                            left.update_from_evidence_internal(leaf, &path[..path.len() - 1])?,
                        ),
                        right: right.clone(),
                        height: *height,
                    }
                } else {
                    Node {
                        hash: None,
                        left: left.clone(),
                        right: Sp::new(
                            right.update_from_evidence_internal(leaf, &path[..path.len() - 1])?,
                        ),
                        height: *height,
                    }
                }
            }
            Stub { .. } | Leaf { .. } => return Err(InvalidUpdate::BadUpdatePath),
        })
    }

    /// Retrieves the leaf hash value at a given index, if available.
    /// `index` *must* be within range of the tree height.
    pub fn index(&self, index: u64) -> Option<(HashOutput, &A)> {
        if self.is_collapsed() {
            panic!("Attempted to index into collapsed portion of Merkle tree!");
        }
        match self {
            Leaf { hash, aux, .. } => Some((*hash, aux)),
            Stub { .. } => None,
            Collapsed { .. } => unreachable!(),
            Node {
                left,
                right,
                height,
                ..
            } => {
                let cmp = 1 << (height - 1);
                if index < cmp {
                    left.index(index)
                } else {
                    right.index(index - cmp)
                }
            }
        }
    }
}

impl<A: Storable<D>, D: DB> MerkleTreeNode<A, D> {
    fn partial_insert(
        &self,
        idx: u128,
        height: u8,
        digest: MerkleTreeDigest,
    ) -> Result<Sp<Self, D>, InvalidUpdate> {
        let hdiff = self.height() - height;
        #[allow(clippy::type_complexity)]
        let mut stack: Vec<(Sp<MerkleTreeNode<A, D>, D>, bool)> =
            Vec::with_capacity(hdiff as usize);
        let theight = self.height();
        let mut curr: Sp<MerkleTreeNode<A, D>, D> = Sp::new(self.clone());
        for i in 0..hdiff {
            let go_left = idx & (1u128 << (theight - i - 1)) == 0;
            match curr.clone().deref() {
                Leaf { .. } => unreachable!(),
                Node { left, right, .. } => {
                    curr = if go_left { left.clone() } else { right.clone() };
                    stack.push((if go_left { right.clone() } else { left.clone() }, go_left));
                }
                Stub { height } => {
                    curr = Sp::new(Stub { height: height - 1 });
                    stack.push((Sp::new(Stub { height: height - 1 }), go_left));
                }
                Collapsed { .. } => return Err(InvalidUpdate::CollapsedIndex(idx, height)),
            }
        }
        Ok(stack.into_iter().rev().enumerate().fold(
            Sp::new(Collapsed {
                hash: digest.0,
                height,
            }),
            |acc, (i, (sibling, goes_left))| {
                let (left, right) = if goes_left {
                    (acc, sibling)
                } else {
                    (sibling, acc)
                };
                Sp::new(Node {
                    left,
                    right,
                    height: height + 1 + i as u8,
                    hash: None,
                })
            },
        ))
    }

    fn rehash(&self) -> Self {
        match self {
            Leaf { .. } | Collapsed { .. } | Stub { .. } | Node { hash: Some(_), .. } => {
                self.clone()
            }
            Node {
                hash: None,
                left,
                right,
                height,
            } => {
                let left = Sp::new(left.rehash());
                let right = Sp::new(right.rehash());
                let hash = Some(Sp::new(transient_hash(&[
                    left.root().expect("rehashed tree must have root"),
                    right.root().expect("rehashed tree must have root"),
                ])));
                Node {
                    hash,
                    left,
                    right,
                    height: *height,
                }
            }
        }
    }

    fn collapse(&self, start: u64, end: u64) -> Sp<Self, D> {
        match self {
            Leaf { hash, .. } => {
                return Sp::new(Collapsed {
                    hash: degrade_to_transient(*hash),
                    height: 0,
                });
            }
            Collapsed { .. } => return Sp::new(self.clone()),
            _ => {}
        }
        let h = self.height();
        if start == 0 && end == (1 << h) - 1 {
            return Sp::new(Collapsed {
                hash: self.rehash().root().expect("rehashed tree must have root"),
                height: h,
            });
        }
        let self2 = if let Stub { height } = self {
            let new = Sp::new(MerkleTreeNode::<A, D>::new(*height - 1));
            Some(Node {
                hash: Some(Sp::new(Fr::default())),
                left: new.clone(),
                right: new,
                height: *height,
            })
        } else {
            None
        };
        if let Node {
            left,
            right,
            height,
            hash,
            ..
        } = self2.as_ref().unwrap_or(self)
        {
            let cmp = 1 << (h - 1);
            let left = if start < cmp {
                left.collapse(start, u64::min(end, cmp - 1))
            } else {
                left.clone()
            };
            let right = if end >= cmp {
                right.collapse(start.saturating_sub(cmp), end - cmp)
            } else {
                right.clone()
            };
            if left.is_collapsed() && right.is_collapsed() {
                return Sp::new(Collapsed {
                    hash: self.rehash().root().expect("rehashed node must have root"),
                    height: *height,
                });
            }
            Sp::new(Node {
                left,
                right,
                // NOTE: Collapsing leaves the hash invariant!
                hash: hash.clone(),
                height: *height,
            })
        } else {
            unreachable!()
        }
    }

    /// Inserts a hash value at a specific index, returning the resulting tree.
    /// `index` *must* be within range of the tree height.
    pub fn update_hash(&self, index: u64, new_leaf: HashOutput, new_aux: A) -> Sp<Self, D> {
        let h = self.height();
        if self.is_collapsed() {
            panic!("Attempted to insert into collapsed portion of Merkle tree!");
        }
        if self.height() == 0 {
            return Sp::new(Leaf {
                hash: new_leaf,
                aux: new_aux,
            });
        }
        let self2 = if let Stub { height } = self {
            let new = Sp::new(MerkleTreeNode::new(*height - 1));
            Some(Node {
                hash: None,
                left: new.clone(),
                right: new,
                height: *height,
            })
        } else {
            None
        };
        if let Node {
            left,
            right,
            height,
            ..
        } = self2.as_ref().unwrap_or(self)
        {
            let cmp = 1 << (h - 1);
            // Here `index < cmp` is the same as `index & cmp == 0`, i.e. we're
            // checking if the height `h` bit in the path is set or not.
            let (left, right) = if index < cmp {
                (left.update_hash(index, new_leaf, new_aux), right.clone())
            } else {
                (
                    left.clone(),
                    right.update_hash(index - cmp, new_leaf, new_aux),
                )
            };
            Sp::new(Node {
                left,
                right,
                hash: None,
                height: *height,
            })
        } else {
            unreachable!()
        }
    }
}

use MerkleTreeNode::*;

/// An iterator over Merkle tree leaf indices and hashes.
pub struct MerkleTreeIter(std::vec::IntoIter<(u64, HashOutput)>);

impl Iterator for MerkleTreeIter {
    type Item = (u64, HashOutput);

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next()
    }
}

/// An iterator over Merkle tree leaf indices, hashes, and auxiliary data
pub struct MerkleTreeIterAux<A>(std::vec::IntoIter<(u64, (HashOutput, A))>);

impl<A> Iterator for MerkleTreeIterAux<A> {
    type Item = (u64, (HashOutput, A));

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next()
    }
}

impl<A: Storable<D>, D: DB> MerkleTree<A, D> {
    /// Create an empty Merkle tree with a given height. Must be O(1).
    pub fn blank(height: u8) -> Self {
        MerkleTree(Sp::new(Stub { height }))
    }

    /// Inserts a hash value at a specific index, returning the resulting tree.
    /// `index` *must* be within range of the tree height.
    pub fn update_hash(&self, index: u64, new_leaf: HashOutput, aux: A) -> Self {
        MerkleTree(self.0.update_hash(index, new_leaf, aux))
    }

    /// Inserts a value into a specific index of the tree.
    ///
    /// # Panics
    ///
    /// May panic if this index was previously in a range passed to
    /// [`collapse`](crate::merkle_tree::MerkleTree::collapse).
    pub fn update<T: BinaryHashRepr + ?Sized>(&self, index: u64, value: &T, aux: A) -> Self
    where
        Self: Sized,
    {
        self.update_hash(index, crate::merkle_tree::leaf_hash(value), aux)
    }

    /// Collapses the tree between `start` and `end` (inclusive) into their
    /// hashes. This prevents future `update`s to this portion of the tree.
    pub fn collapse(&self, start: u64, end: u64) -> Self {
        MerkleTree(self.0.collapse(start, end))
    }

    fn partial_insert(
        self,
        idx: u128,
        height: u8,
        digest: MerkleTreeDigest,
    ) -> Result<Self, InvalidUpdate> {
        Ok(MerkleTree(self.0.partial_insert(idx, height, digest)?))
    }

    /// Apply a collapsed update to the current tree. This update should *not*
    /// touch any collapsed part of the current tree, and should be well-formed.
    pub fn apply_collapsed_update(
        &self,
        update: &MerkleTreeCollapsedUpdate,
    ) -> Result<Self, InvalidUpdate> {
        let segments = MerkleTreeCollapsedUpdate::step_sizes(update.start, update.end + 1);
        if segments.len() != update.hashes.len() {
            return Err(InvalidUpdate::WrongNumberOfSegments(
                segments.len(),
                update.hashes.len(),
            ));
        }
        let mut curr_idx = update.start as u128;
        let mut curr = self.clone();
        for (segment, hash) in segments.into_iter().zip(update.hashes.iter()) {
            curr = curr.partial_insert(curr_idx, segment, *hash)?;
            curr_idx += 1u128 << segment as u128;
        }
        Ok(curr)
    }
}

impl<A: Storable<D>, D: DB> MerkleTree<A, D> {
    /// Retrieves the height of this tree. Must be O(1).
    pub fn height(&self) -> u8 {
        self.0.height()
    }

    /// Retrieves the Merkle root of this tree. Must be O(1).
    ///
    /// This returns `Some` iff the underlying tree has been rehashed.
    pub fn root(&self) -> Option<MerkleTreeDigest> {
        self.0.root().map(MerkleTreeDigest)
    }

    /// Rehashes the Merkle tree, computing the new root and intermediate hashes.
    /// This is a separate operation as it amortizes costs across sequential
    /// insertions to `O(n + h)` instead of `O(nh)`.
    pub fn rehash(&self) -> Self {
        MerkleTree(Sp::new(self.0.rehash()))
    }

    /// Retrieves the leaf hash value at a given index, if available.
    /// `index` *must* be within range of the tree height.
    pub fn index(&self, index: u64) -> Option<(HashOutput, &A)> {
        self.0.index(index)
    }

    /// Iterate over the leaves and leaf indices of the tree.
    pub fn iter(&self) -> MerkleTreeIter {
        MerkleTreeIter(
            self.0
                .leaves()
                .into_iter()
                .filter_map(|leaf| match leaf {
                    LeafOrCollapsed::Leaf { index, hash, .. } => Some((index, hash)),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .into_iter(),
        )
    }

    /// Iterate over the leaves and leaf indices of the tree, includes aux data
    pub fn iter_aux(&self) -> MerkleTreeIterAux<A> {
        MerkleTreeIterAux(
            self.0
                .leaves()
                .into_iter()
                .filter_map(|leaf| match leaf {
                    LeafOrCollapsed::Leaf { index, hash, aux } => {
                        Some((index, (hash, aux.clone())))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
                .into_iter(),
        )
    }

    /// Generate an iterator over leaves, including indices, hashes, and auxiliary data
    /// Does a linear search for a given leaf.
    ///
    /// `O(2^height)` worst-case behavior, this should only be used for small trees.
    ///
    /// May panic if the Merkle tree has not been rehashed.
    pub fn find_path_for_leaf<T: BinaryHashRepr>(&self, leaf: T) -> Option<MerklePath<T>> {
        let hash = leaf_hash(&leaf);
        for (index, hash2) in self.0.leaves().into_iter().filter_map(|leaf| match leaf {
            LeafOrCollapsed::Leaf { index, hash, .. } => Some((index, hash)),
            _ => None,
        }) {
            if hash == hash2 {
                return self.path_for_leaf(index, leaf).ok();
            }
        }
        None
    }

    /// Attempts to replay a piece of insertion evidence against this tree.
    /// Note that this requires the insertion to be from a trusted source, as
    /// hashes may not be checkable.
    pub fn update_from_evidence(
        &self,
        insertion: TreeInsertionPath<A>,
    ) -> Result<Self, InvalidUpdate>
    where
        A: Serializable + Deserializable + Clone + Sync + Send + 'static,
    {
        Ok(MerkleTree(Sp::new(self.0.update_from_evidence_internal(
            insertion.leaf,
            &insertion.path,
        )?)))
    }

    /// Produces insertion evidence for a specific index; this index must be
    /// present and not collapsed.
    pub fn insertion_evidence(&self, index: u64) -> Result<TreeInsertionPath<A>, InvalidIndex>
    where
        A: Serializable + Deserializable + Clone + Sync + Send + 'static,
    {
        if self.height() == 0 {
            return Err(InvalidIndex(index));
        }
        let (path, lefts) = self.path_for_index_internal(index, false)?;
        let leaf = self.index(index).ok_or(InvalidIndex(index))?;
        Ok(TreeInsertionPath {
            leaf: (leaf.0, leaf.1.clone()),
            path: path
                .into_iter()
                .zip(lefts)
                .map(|(hash, goes_left)| {
                    Ok(TreeInsertionPathEntry {
                        hash: hash.map(MerkleTreeDigest),
                        goes_left,
                    })
                })
                .rev()
                .collect::<Result<_, _>>()?,
        })
    }

    /// Given a leaf at a specific index, produces a [`MerklePath`] for it.
    ///
    /// May panic if the Merkle tree has not been rehashed.
    pub fn path_for_leaf<T: BinaryHashRepr>(
        &self,
        index: u64,
        leaf: T,
    ) -> Result<MerklePath<T>, InvalidIndex> {
        if self.height() == 0 {
            return if index == 0 {
                Ok(MerklePath {
                    leaf,
                    path: Vec::new(),
                })
            } else {
                Err(InvalidIndex(index))
            };
        }
        let (path, lefts) = self.path_for_index_internal(index, true)?;
        Ok(MerklePath {
            leaf,
            path: path
                .into_iter()
                .zip(lefts)
                .map(|(sibling, goes_left)| {
                    Ok(MerklePathEntry {
                        sibling: MerkleTreeDigest(sibling.ok_or(InvalidIndex(index))?),
                        goes_left,
                    })
                })
                .rev()
                .collect::<Result<_, _>>()?,
        })
    }

    fn path_for_index_internal(
        &self,
        index: u64,
        siblings: bool,
    ) -> Result<(Vec<Option<Fr>>, Vec<bool>), InvalidIndex> {
        let mut at = self.0.deref();
        let mut i = index;
        assert!(
            at.height() >= 1,
            "height-0 trees should have been caught earlier"
        );
        let mut path = Vec::with_capacity(at.height() as usize);
        let mut goes_left = Vec::with_capacity(at.height() as usize);
        while at.height() > 1 {
            let cmp = 1 << (at.height() - 1);
            let nxt = match at {
                Leaf { .. } => unreachable!(),
                Stub { .. } => return Err(InvalidIndex(index)),
                Collapsed { .. } => return Err(InvalidIndex(index)),
                Node { left, right, .. } => {
                    if i < cmp {
                        path.push(if siblings { right.root() } else { at.root() });
                        goes_left.push(true);
                        left
                    } else {
                        path.push(if siblings { left.root() } else { at.root() });
                        goes_left.push(false);
                        i -= cmp;
                        right
                    }
                }
            };
            at = nxt;
        }
        goes_left.push(i == 0);
        let path_elem = match (siblings, i, at) {
            (_, _, Stub { .. }) => return Err(InvalidIndex(index)),
            (false, _, at) => at.root(),
            (true, 0, Node { right, .. }) => right.root(),
            (true, 1, Node { left, .. }) => left.root(),
            _ => unreachable!(),
        };
        path.push(path_elem);
        Ok((path, goes_left))
    }
}

#[cfg(feature = "proptest")]
impl<A: Debug + Storable<D>, D: DB> Arbitrary for MerkleTree<A, D>
where
    Standard: Distribution<A>,
{
    type Parameters = ();
    type Strategy = NoStrategy<MerkleTree<A, D>>;

    fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
        NoStrategy(PhantomData)
    }
}

impl<A: Storable<D>, D: DB> Distribution<MerkleTree<A, D>> for Standard
where
    Standard: Distribution<u8> + Distribution<Fr> + Distribution<A>,
{
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> MerkleTree<A, D> {
        let height: u8 = rng.gen_range(1..17);
        let mut mt = MerkleTree::blank(height);

        for i in 0..height {
            mt = mt.update(i.into(), &rng.r#gen::<Fr>(), rng.r#gen());
        }

        mt.rehash()
    }
}

#[cfg(test)]
mod tests {
    use sha2::Sha256;

    use storage::db::InMemoryDB;

    use super::*;

    fn new_mt<A: Storable<InMemoryDB>>(height: u8) -> MerkleTree<A, InMemoryDB<Sha256>> {
        MerkleTree::<A, InMemoryDB<Sha256>>::blank(height)
    }

    #[test]
    fn test_membership() {
        let tree = new_mt::<()>(32)
            .update(0, &Fr::from(42u64), ())
            .update(0, &Fr::from(41u64), ())
            .update(3, &Fr::from(43u64), ())
            .update(62, &Fr::from(12u64), ())
            .rehash();
        assert_eq!(
            tree.path_for_leaf(0, Fr::from(41u64)).unwrap().root(),
            tree.root().unwrap()
        );
        assert_eq!(
            tree.path_for_leaf(3, Fr::from(43u64)).unwrap().root(),
            tree.root().unwrap()
        );
        assert_eq!(
            tree.path_for_leaf(62, Fr::from(12u64)).unwrap().root(),
            tree.root().unwrap()
        );
    }

    #[test]
    fn test_collapse_good() {
        let tree = new_mt::<()>(32)
            .update(0, &Fr::from(42u64), ())
            .update(0, &Fr::from(41u64), ())
            .update(3, &Fr::from(43u64), ())
            .update(62, &Fr::from(12u64), ())
            .collapse(0, 61)
            .rehash();
        assert_eq!(
            tree.path_for_leaf(62, Fr::from(12u64)).unwrap().root(),
            tree.root().unwrap()
        );
    }

    #[test]
    fn test_collapse_bad_proof() {
        let tree = new_mt::<()>(32)
            .update(0, &Fr::from(42u64), ())
            .update(0, &Fr::from(41u64), ())
            .update(3, &Fr::from(43u64), ())
            .update(62, &Fr::from(12u64), ())
            .collapse(0, 61)
            .rehash();
        assert!(tree.path_for_leaf(3, Fr::from(43u64)).is_err());
    }

    #[test]
    #[should_panic = "Attempted to insert into collapsed portion of Merkle tree!"]
    fn test_collapse_bad_update() {
        let _tree = new_mt::<()>(32)
            .update(0, &Fr::from(42u64), ())
            .update(0, &Fr::from(41u64), ())
            .update(3, &Fr::from(43u64), ())
            .update(62, &Fr::from(12u64), ())
            .collapse(0, 61)
            .update(61, &Fr::from(0xdeadbeefu64), ());
    }

    #[test]
    fn test_incremental_collapse() {
        let tree = new_mt::<()>(3)
            .update(0, &Fr::from(42u64), ())
            .collapse(0, 0)
            .update(1, &Fr::from(42u64), ())
            .collapse(1, 1)
            .update(2, &Fr::from(42u64), ())
            .collapse(2, 2)
            .update(3, &Fr::from(42u64), ())
            .update(4, &Fr::from(42u64), ())
            .collapse(4, 4);
        let tree2 = new_mt::<()>(3)
            .update(0, &Fr::from(42u64), ())
            .update(1, &Fr::from(42u64), ())
            .update(2, &Fr::from(42u64), ())
            .update(3, &Fr::from(42u64), ())
            .update(4, &Fr::from(42u64), ())
            .collapse(0, 2)
            .collapse(4, 4);
        assert_eq!(tree, tree2);
    }

    #[test]
    fn test_collapsed_update() {
        let t = new_mt::<()>(6)
            .update(0, &Fr::from(42u64), ())
            .update(1, &Fr::from(42u64), ());
        let t2 = (2..=32)
            .fold(t.clone(), |t, i| t.update(i, &Fr::from(42u64), ()))
            .rehash();
        let upd1 = MerkleTreeCollapsedUpdate::new(&t2, 2, 2).unwrap();
        let upd2 = MerkleTreeCollapsedUpdate::new(&t2, 3, 31).unwrap();
        let t3 = t.update(32, &Fr::from(42u64), ());
        let t4 = t3
            .apply_collapsed_update(&upd1)
            .unwrap()
            .apply_collapsed_update(&upd2)
            .unwrap()
            .rehash();
        assert_eq!(t4.root(), t2.root());
    }

    #[test]
    fn test_insertion_evidence() {
        let t = (0..=32)
            .fold(new_mt::<()>(6), |t, i| t.update(i, &Fr::from(42u64), ()))
            .rehash();
        let t2 = t.update(12, &Fr::from(43u64), ()).rehash();
        let evidence = t2.insertion_evidence(12).unwrap();
        assert_eq!(
            t.update_from_evidence(evidence.clone()).unwrap().rehash(),
            t2
        );
        assert_eq!(
            t.collapse(0, 32)
                .update_from_evidence(evidence)
                .unwrap()
                .rehash()
                .root(),
            t2.root()
        );
        // test *not* rehashing the tree first
        let t3 = (33..=64).fold(t.update(12, &Fr::from(43u64), ()).rehash(), |t, i| {
            t.update(i, &Fr::from(42u64), ())
        });
        let evidence = t3.insertion_evidence(12).unwrap();
        dbg!(&evidence);
        // We should still be able to insert into collapsed `t`!
        assert_eq!(
            t.collapse(0, 32)
                .update_from_evidence(evidence)
                .unwrap()
                .rehash()
                .root(),
            t2.root()
        );
    }

    #[test]
    fn test_singleton_collapsed_update() {
        let t = new_mt::<()>(6).update(0, &Fr::from(42u64), ()).rehash();
        let upd = MerkleTreeCollapsedUpdate::new(&t, 0, 0).unwrap();
        let t2 = new_mt::<()>(6)
            .apply_collapsed_update(&upd)
            .unwrap()
            .rehash();
        assert_eq!(t.root(), t2.root());
    }

    #[test]
    fn test_tiny_trees() {
        let t = new_mt::<()>(1)
            .update(0, &Fr::from(42u64), ())
            .update(1, &Fr::from(42u64), ());
        t.path_for_leaf(0, Fr::from(42u64)).unwrap();
        let t = new_mt::<()>(0).update(0, &Fr::from(42u64), ());
        t.path_for_leaf(0, Fr::from(42u64)).unwrap();
    }

    #[test]
    fn test_aux_data() {
        let t = new_mt::<u8>(32).update(0, &Fr::from(42u64), 10);
        for (_index, (_hash, aux)) in t.iter_aux() {
            assert_eq!(aux, 10);
        }
    }
}
