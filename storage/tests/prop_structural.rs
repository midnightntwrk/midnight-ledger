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

//! Clean-room *structural* / parser-differential property tests for the
//! Merkle-Patricia trie.
//!
//! Iteration 1 only ever built "valid" values through the public constructor
//! API, and its adversarial oracle was merely panic-freedom. This file closes
//! both gaps by building candidate values DIRECTLY at the structural level
//! (via the crate's own `public-internal-structure` feature) -- including
//! shapes the public API cannot emit -- and, for anything the untrusted loader
//! `Arena::deserialize_sp` ACCEPTS, asserting the type's FULL semantic
//! invariants rather than "did not crash".
//!
//! Contracts under test (the code's own):
//!   * `Node::check_invariant` states a `Branch` must have >=2 non-empty
//!     children and that every node's annotation equals its subtree size.
//!   * `Node::size` is *computed from the stored annotation*, whereas
//!     `iter()`/`leaves()` walk the tree structurally -- so for any honest
//!     value they must agree.
//!   * A content-addressed structure must have exactly ONE serialization per
//!     logical content (otherwise hashing / equality / dedup / consensus split).
//!
//! Oracle `prop_structural_mutants_accepted_size_consistent`:
//! randomly assembles arbitrary (often denormalized) `Node` trees and, for
//! every one the loader accepts, asserts
//!   size() == iter().count()   and   is_empty() == (count == 0).
//! It needs no canonical reference, so it cannot false-positive. The untrusted
//! deserialize path enforces the trie's canonical form, so a denormalized node
//! with `size() != iter().count()` is rejected. Kept as a GENERAL fuzz oracle;
//! `canonicality_regression.rs` holds the specific known vectors as named
//! rejection tests.

#![cfg(feature = "public-internal-structure")]

use midnight_storage::arena::Sp;
use midnight_storage::merkle_patricia_trie::{MerklePatriciaTrie, Node};
use midnight_storage::storable::SizeAnn;
use midnight_storage::{DefaultDB, Storage};
use serialize::Serializable;

type N = Node<u64>;
type Mpt = MerklePatriciaTrie<u64>;

// --- tiny deterministic PRNG (SplitMix64) ---
struct Rng(u64);
impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed.wrapping_add(0x9E37_79B9_7F4A_7C15))
    }
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }
}

fn ser<T: Serializable>(t: &T) -> Vec<u8> {
    let mut b = Vec::new();
    t.serialize(&mut b).expect("serialize");
    b
}

fn fresh_arena() -> midnight_storage::arena::Arena<DefaultDB> {
    Storage::<DefaultDB>::new(64, DefaultDB::default()).arena
}

/// True (structural) leaf count -- what every annotation is supposed to equal,
/// computed WITHOUT trusting any stored annotation.
fn true_leaves(n: &N) -> u64 {
    match n {
        Node::Empty => 0,
        Node::Leaf { .. } => 1,
        Node::Branch { children, .. } => children.iter().map(|c| true_leaves(c)).sum(),
        Node::Extension { child, .. } => true_leaves(child),
        Node::MidBranchLeaf { child, .. } => true_leaves(child) + 1,
    }
}

fn empty_children() -> Box<[Sp<N>; 16]> {
    Box::new(std::array::from_fn(|_| Sp::new(Node::Empty)))
}

/// Honest annotation most of the time, deliberately wrong sometimes.
fn ann_for(rng: &mut Rng, true_size: u64) -> SizeAnn {
    if rng.below(3) == 0 {
        SizeAnn(true_size ^ (1 + rng.below(5)))
    } else {
        SizeAnn(true_size)
    }
}

/// Structural generator: arbitrary, often-denormalized `Node` trees with
/// nibble-only paths.
fn gen_node(rng: &mut Rng, depth: u32) -> N {
    let choice = if depth >= 4 { rng.below(2) } else { rng.below(6) };
    match choice {
        0 => Node::Empty,
        1 => Node::Leaf {
            ann: ann_for(rng, 1),
            value: Sp::new(rng.next_u64()),
        },
        2 => {
            // Branch with k in 0..=3 non-empty children (k < 2 is denormalized).
            let mut children = empty_children();
            let k = rng.below(4);
            let mut used = [false; 16];
            for _ in 0..k {
                let mut idx = rng.below(16) as usize;
                while used[idx] {
                    idx = (idx + 1) % 16;
                }
                used[idx] = true;
                children[idx] = Sp::new(gen_node(rng, depth + 1));
            }
            let true_size: u64 = children.iter().map(|c| true_leaves(c)).sum();
            Node::Branch {
                ann: ann_for(rng, true_size),
                children,
            }
        }
        3 => {
            // Extension, path length 0..=3 (0 == empty path, denormalized).
            let plen = rng.below(4);
            let path: Vec<u8> = (0..plen).map(|_| rng.below(16) as u8).collect();
            let child = gen_node(rng, depth + 1);
            let true_size = true_leaves(&child);
            Node::Extension {
                ann: ann_for(rng, true_size),
                compressed_path: path,
                child: Sp::new(child),
            }
        }
        4 => {
            // MidBranchLeaf; child may be any node (Leaf/Empty is denormalized).
            let child = gen_node(rng, depth + 1);
            let true_size = true_leaves(&child) + 1;
            Node::MidBranchLeaf {
                ann: ann_for(rng, true_size),
                value: Sp::new(rng.next_u64()),
                child: Sp::new(child),
            }
        }
        _ => Node::Leaf {
            ann: ann_for(rng, 1),
            value: Sp::new(rng.next_u64()),
        },
    }
}

// ===========================================================================
// Oracle (A): accepted structural mutants must be size- and emptiness-consistent.
// (No canonical reference -> no possible false positive.)
// ===========================================================================

/// Classification of a single deserialize attempt in oracle (A).
enum Class {
    Rejected,
    Good,
    Bad(String),
}

#[test]
fn prop_structural_mutants_accepted_size_consistent() {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));

    let (mut accepted, mut rejected, mut panicked) = (0u64, 0u64, 0u64);
    let mut violations: Vec<String> = Vec::new();

    for seed in 0..5000u64 {
        let mut rng = Rng::new(seed.wrapping_mul(0x2545_F491_4F6C_DD1D) ^ 0xB16B);
        let root = gen_node(&mut rng, 0);
        let mpt: Mpt = MerklePatriciaTrie(Sp::new(root.clone()));
        let buf = ser(&mpt);

        let arena = fresh_arena();
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            match arena.deserialize_sp::<Mpt, _>(&mut &buf[..], 0) {
                Err(_) => Class::Rejected,
                Ok(sp) => {
                    let count = sp.iter().count() as u64;
                    let size = sp.size() as u64;
                    if size != count {
                        Class::Bad(format!(
                            "size-consistency: size()={size} but iter() yields {count} leaves"
                        ))
                    } else if sp.is_empty() != (count == 0) {
                        Class::Bad(format!(
                            "emptiness-consistency: is_empty()={} but count={count}",
                            sp.is_empty()
                        ))
                    } else {
                        Class::Good
                    }
                }
            }
        }));

        match outcome {
            Err(_) => {
                panicked += 1;
                if violations.len() < 8 {
                    violations.push(format!("seed {seed}: PANIC on {root:?}"));
                }
            }
            Ok(Class::Rejected) => rejected += 1,
            Ok(Class::Good) => accepted += 1,
            Ok(Class::Bad(why)) => {
                accepted += 1;
                if violations.len() < 8 {
                    violations.push(format!("seed {seed}: {why}\n    tree = {root:?}"));
                }
            }
        }
    }

    std::panic::set_hook(prev);
    eprintln!(
        "[A structural mutants] accepted={accepted} rejected={rejected} panicked={panicked} \
         violations(sampled)={}",
        violations.len()
    );
    assert!(
        violations.is_empty(),
        "deserialize_sp ACCEPTED structurally-invalid tries (accept must imply invariants):\n{}",
        violations.join("\n")
    );
}

