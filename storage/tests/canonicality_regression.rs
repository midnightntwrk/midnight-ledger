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

//! Named regression tests for the specific non-canonical MPT encodings the
//! untrusted deserializer (`Arena::deserialize_sp`) must reject.
//!
//! Each test builds one concrete non-canonical encoding of a container and
//! asserts the single correct behaviour: the untrusted deserializer REJECTS it.
//! The general fuzz oracles for this class live in `prop_structural.rs`; this
//! file pins the individual known vectors with descriptive names and documents,
//! in each test, the concrete damage the vector causes if it is *not* rejected.

#![cfg(feature = "public-internal-structure")]

use midnight_storage::arena::{Arena, Sp};
use midnight_storage::merkle_patricia_trie::{Annotation, MerklePatriciaTrie, Node};
use midnight_storage::storable::SizeAnn;
use midnight_storage::storage::{Array, HashMap as StoreHashMap, Map};
use midnight_storage::{DefaultDB, Storable, Storage};
use serialize::Serializable;
use std::marker::PhantomData;
use std::ops::Deref;

type D = DefaultDB;

fn arena() -> Arena<D> {
    Storage::<D>::new(64, D::default()).arena
}

fn ser_sp<T: Storable<D>>(v: T) -> Vec<u8> {
    let mut b = Vec::new();
    Sp::serialize(&Sp::new(v), &mut b).expect("serialize");
    b
}

/// The shared oracle for this file: a crafted non-canonical encoding must be a
/// genuinely distinct wire form, the canonical form must still deserialize, and
/// the untrusted deserializer must REJECT the non-canonical form.
fn assert_rejected<T: Storable<D> + Clone>(canonical: T, noncanonical: T, what: &str) {
    let a = arena();
    let canon_bytes = ser_sp(canonical);
    let noncanon_bytes = ser_sp(noncanonical);
    assert_ne!(
        canon_bytes, noncanon_bytes,
        "{what}: the crafted encoding must be a genuinely distinct wire form"
    );
    assert!(
        a.deserialize_sp::<T, _>(&mut &canon_bytes[..], 0).is_ok(),
        "{what}: the canonical encoding must still deserialize"
    );
    assert!(
        a.deserialize_sp::<T, _>(&mut &noncanon_bytes[..], 0).is_err(),
        "{what}: the untrusted deserializer must REJECT the non-canonical encoding"
    );
}

/// Wrap a trie root in an empty-path `Extension` carrying the correct
/// (subtree-size) annotation -- a distinct encoding of the same logical trie
/// that the public builder never emits. Sound (passes `check_invariant`) but
/// non-canonical.
fn wrap_empty_extension<V, A>(
    mpt: &MerklePatriciaTrie<V, D, A>,
    size: u64,
) -> MerklePatriciaTrie<V, D, A>
where
    V: Storable<D>,
    A: Storable<D> + Annotation<V>,
    SizeAnn: Into<A>,
{
    let wrapped = Node::Extension {
        ann: SizeAnn(size).into(),
        compressed_path: Vec::new(),
        child: mpt.0.clone(),
    };
    MerklePatriciaTrie(Sp::new(wrapped))
}

/// An all-empty `Branch` root: a logically-empty trie whose root is not `Empty`.
/// Violates the ">= 2 non-empty children" rule.
fn all_empty_branch_root() -> Sp<MerklePatriciaTrie<u64>> {
    let children: Box<[Sp<Node<u64>>; 16]> = Box::new(std::array::from_fn(|_| Sp::new(Node::Empty)));
    Sp::new(MerklePatriciaTrie(Sp::new(Node::Branch {
        ann: SizeAnn(0),
        children,
    })))
}

/// Canonical nibble path for a key, mirroring the crate-private `to_nibbles`.
fn nibbles_of<T: Serializable>(v: &T) -> Vec<u8> {
    let mut bytes = Vec::new();
    v.serialize(&mut bytes).unwrap();
    let mut n = Vec::with_capacity(bytes.len() * 2);
    for b in bytes {
        n.push(b >> 4);
        n.push(b & 0x0f);
    }
    n
}

// ===========================================================================
// Sound-but-non-canonical SHAPE: empty-path `Extension` wrap.
//
// If accepted, a container has >= 2 distinct wire encodings of one logical
// value. That breaks content-addressing: structural equality and `Hash` differ
// for logically-identical containers (defeating dedup / set-membership), and it
// is the substrate for transaction-hash malleability.
// ===========================================================================
#[test]
fn empty_path_extension_rejected_mpt() {
    let mut canon: MerklePatriciaTrie<u64> = MerklePatriciaTrie::new();
    for k in 0u8..6 {
        canon = canon.insert(&[k, k + 1, k + 2], k as u64 * 10);
    }
    let size = canon.size() as u64;
    let wrapped = wrap_empty_extension(&canon, size);
    assert_rejected(canon, wrapped, "MPT empty-path Extension wrap");
}

#[test]
fn empty_path_extension_rejected_map() {
    let mut canon: Map<u16, u64> = Map::new();
    for k in 0u16..6 {
        canon = canon.insert(k, k as u64 * 7);
    }
    let wrapped_mpt = wrap_empty_extension(&canon.mpt, canon.size() as u64);
    let wrapped: Map<u16, u64> = Map {
        mpt: Sp::new(wrapped_mpt),
        key_type: PhantomData,
    };
    assert_rejected(canon, wrapped, "Map empty-path Extension wrap");
}

#[test]
fn empty_path_extension_rejected_hashmap() {
    let mut canon: StoreHashMap<u16, u64> = StoreHashMap::new();
    for k in 0u16..6 {
        canon = canon.insert(k, k as u64 * 13);
    }
    // HashMap wraps an inner `Map<ArenaHash, (Sp<K>, Sp<V>)>`; wrap that map's root.
    let wrapped_mpt = wrap_empty_extension(&canon.0.mpt, canon.size() as u64);
    let wrapped: StoreHashMap<u16, u64> = StoreHashMap(Map {
        mpt: Sp::new(wrapped_mpt),
        key_type: PhantomData,
    });
    assert_rejected(canon, wrapped, "HashMap empty-path Extension wrap");
}

// ===========================================================================
// Rule-violating SHAPE: all-empty `Branch` root.
//
// If accepted, a logically-empty container reports `is_empty() == false` while
// `size() == 0`, and compares `!= Map::new()`. The codebase's emptiness idioms
// (`!container.is_empty()`, `container != X::new()`) then disagree with `size()`
// and reach the wrong conclusion (e.g. the `DustActions` "empty" guard).
// ===========================================================================
#[test]
fn all_empty_branch_root_rejected_map() {
    let canon: Map<u16, u64> = Map::new();
    let fake_empty: Map<u16, u64> = Map {
        mpt: all_empty_branch_root(),
        key_type: PhantomData,
    };
    assert_rejected(canon, fake_empty, "Map all-empty Branch root");
}

#[test]
fn all_empty_branch_root_rejected_array() {
    let canon: Array<u64> = Array::new();
    let fake_empty: Array<u64> = Array(all_empty_branch_root());
    assert_rejected(canon, fake_empty, "Array all-empty Branch root");
}

// ===========================================================================
// NON-INJECTIVE KEYS: a key's canonical encoding followed by trailing
// nibbles that fixed-width `from_nibbles` (`read_exact`) silently ignores.
//
// If accepted, one logical key is held twice: `size()`/`iter()` report two
// entries while `get(k)` finds one, and re-collecting the entries (what
// `Transaction::erase_proofs`/`erase_signatures` do via `.iter()...collect()`)
// collapses the duplicate -- so the validated representation and the applied
// representation disagree. This is the GHSA-vhp6-px6f-jv94 substrate.
// ===========================================================================

/// Build a `Map<u16,_>` holding a real entry for key `k` plus a second leaf at
/// `canonical_path(k) ++ [0x00, 0x00]`, which fixed-width `from_nibbles::<u16>`
/// decodes back to `k` (ignoring the trailing byte).
fn map_with_over_long_key_leaf() -> Map<u16, u64> {
    let p1 = nibbles_of(&1u16);
    let mut p2 = p1.clone();
    p2.extend_from_slice(&[0, 0]); // one extra (ignored) byte -> aliases key 1
    let mut mpt = MerklePatriciaTrie::<u64>::new();
    mpt = mpt.insert(&p1, 100);
    mpt = mpt.insert(&p2, 200);
    Map {
        mpt: Sp::new(mpt),
        key_type: PhantomData,
    }
}

#[test]
fn over_long_key_path_rejected_map() {
    let canon: Map<u16, u64> = Map::new().insert(1u16, 100);
    let aliased = map_with_over_long_key_leaf();
    assert_rejected(canon, aliased, "Map over-long (aliasing) key path");
}

#[test]
fn over_long_key_path_rejected_hashmap() {
    // `HashMap<u16,_>` is the type of `StandardTransaction.intents`.
    let canon: StoreHashMap<u16, u64> = StoreHashMap::new().insert(1, 100);

    // Canonical inner path of the single leaf (over the ArenaHash key).
    let (p, existing) = canon
        .0
        .mpt
        .iter()
        .next()
        .map(|(p, v)| (p, (*v).clone()))
        .expect("one leaf");
    let mut p2 = p.clone();
    p2.extend_from_slice(&[0, 0]); // extra byte ignored by ArenaHash's read_exact

    // Second leaf carrying key 1 again with a different value.
    let dup_value = (existing.0.clone(), Sp::new(200u64));
    let aliased_inner = canon.0.mpt.deref().insert(&p2, dup_value);
    let aliased: StoreHashMap<u16, u64> = StoreHashMap(Map {
        mpt: Sp::new(aliased_inner),
        key_type: PhantomData,
    });
    assert_rejected(canon, aliased, "HashMap over-long (aliasing) key path");
}
