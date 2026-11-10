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

// Regression test for the arena-level recursion bound: a chain of in-lined
// (`ArenaKey::Direct`) children must be bounded by the recursion limit, and not
// blow the stack.
//
// The `Storable` used here deliberately does *not* call any `Deserializable`
// implementation, so the only thing standing between the input and a stack
// overflow is `IrLoader`'s own depth accounting.

use midnight_storage_core::Storage;
use midnight_storage_core::arena::{Arena, ArenaKey, Sp, TopoSortedNode, TopoSortedNodes};
use midnight_storage_core::db::{DB, InMemoryDB};
use midnight_storage_core::storable::{Loader, Storable};
use serialize::Serializable;

type Db = InMemoryDB;

#[derive(Clone, Debug)]
struct Inline(Option<Sp<Inline, Db>>);

impl Storable<Db> for Inline {
    fn children(&self) -> Vec<ArenaKey<<Db as DB>::Hasher>> {
        self.0.iter().map(Sp::as_child).collect()
    }

    fn to_binary_repr<W: std::io::Write>(&self, writer: &mut W) -> Result<(), std::io::Error> {
        writer.write_all(&[self.0.is_some() as u8])
    }

    fn from_binary_repr<R: std::io::Read>(
        reader: &mut R,
        child_nodes: &mut impl Iterator<Item = ArenaKey<<Db as DB>::Hasher>>,
        loader: &impl Loader<Db>,
    ) -> Result<Self, std::io::Error> {
        let mut disc = [0u8; 1];
        reader.read_exact(&mut disc)?;
        match disc[0] {
            0 => Ok(Inline(None)),
            1 => Ok(Inline(Some(loader.get_next(child_nodes)?))),
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "bad discriminant",
            )),
        }
    }
}

fn chain(depth: usize) -> TopoSortedNodes {
    let mut nodes = vec![TopoSortedNode {
        child_indices: vec![],
        data: vec![0u8],
    }];
    for i in 0..depth {
        nodes.push(TopoSortedNode {
            child_indices: vec![i as u64],
            data: vec![1u8],
        });
    }
    TopoSortedNodes { nodes }
}

#[test]
fn deeply_nested_inline_chain_is_rejected() {
    std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(|| {
            let arena: Arena<Db> = Storage::<Db>::new(16, Db::default()).arena;
            let mut bytes = Vec::new();
            chain(50_000).serialize(&mut bytes).unwrap();
            let res = arena.deserialize_sp::<Inline, _>(&mut &bytes[..], 0);
            assert!(res.is_err(), "expected an error, got a value");
            println!("error: {}", res.err().unwrap());
        })
        .unwrap()
        .join()
        .unwrap();
}

/// The boundary itself, expressed relative to the limit so it holds for both
/// the debug (50) and release (250) values.
///
/// The root is decoded at depth 0, and descending to a child is rejected once
/// its depth would exceed the limit, so at most `RECURSION_LIMIT` nodes are
/// nested: `chain(depth)` has `depth + 1` nodes.
#[test]
fn recursion_limit_boundary_is_exact() {
    let arena: Arena<Db> = Storage::<Db>::new(16, Db::default()).arena;
    let limit = serialize::RECURSION_LIMIT as usize;

    let decode = |depth: usize| {
        let mut bytes = Vec::new();
        chain(depth).serialize(&mut bytes).unwrap();
        arena
            .deserialize_sp::<Inline, _>(&mut &bytes[..], 0)
            .is_ok()
    };

    assert!(decode(limit - 2), "one below the limit must decode");
    assert!(decode(limit - 1), "the deepest accepted chain must decode");
    assert!(!decode(limit), "one past the limit must be rejected");
    assert!(!decode(limit + 1), "two past the limit must be rejected");
}

thread_local! {
    /// The number of `Fan::from_binary_repr` calls currently on this thread's
    /// stack, and the most there have been since the last `Frame::reset`.
    static FRAMES: std::cell::Cell<(usize, usize)> = const { std::cell::Cell::new((0, 0)) };
}

struct Frame;

impl Frame {
    fn enter() -> Frame {
        FRAMES.with(|f| {
            let (active, max) = f.get();
            f.set((active + 1, max.max(active + 1)));
        });
        Frame
    }

    fn reset() {
        FRAMES.with(|f| f.set((0, 0)));
    }

    fn max() -> usize {
        FRAMES.with(|f| f.get().1)
    }
}

impl Drop for Frame {
    fn drop(&mut self) {
        FRAMES.with(|f| {
            let (active, max) = f.get();
            f.set((active - 1, max));
        });
    }
}

/// A node with up to 16 children and `pad` bytes of filler, so a test can
/// choose whether it is in-lined (small) or stored by reference (large).
#[derive(Clone, Debug)]
struct Fan {
    children: Vec<Sp<Fan, Db>>,
    pad: u16,
}

impl Storable<Db> for Fan {
    fn children(&self) -> Vec<ArenaKey<<Db as DB>::Hasher>> {
        self.children.iter().map(Sp::as_child).collect()
    }

    fn to_binary_repr<W: std::io::Write>(&self, writer: &mut W) -> Result<(), std::io::Error> {
        writer.write_all(&[self.children.len() as u8])?;
        writer.write_all(&self.pad.to_le_bytes())?;
        writer.write_all(&vec![0u8; self.pad as usize])
    }

    fn from_binary_repr<R: std::io::Read>(
        reader: &mut R,
        child_nodes: &mut impl Iterator<Item = ArenaKey<<Db as DB>::Hasher>>,
        loader: &impl Loader<Db>,
    ) -> Result<Self, std::io::Error> {
        let _frame = Frame::enter();
        let mut n = [0u8; 1];
        reader.read_exact(&mut n)?;
        if n[0] > 16 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "too many children",
            ));
        }
        let mut pad = [0u8; 2];
        reader.read_exact(&mut pad)?;
        let pad = u16::from_le_bytes(pad);
        reader.read_exact(&mut vec![0u8; pad as usize])?;
        let children = (0..n[0])
            .map(|_| loader.get_next(child_nodes))
            .collect::<Result<_, _>>()?;
        Ok(Fan { children, pad })
    }
}

/// Serializes a graph whose longest path is `chain_len` (plus the few levels
/// of a fan-out), but whose every node is *first* reached at a shallow depth.
///
/// The chain `n_0 <- n_1 <- ... <- n_len` is reached through a 16-ary fan over
/// `n_1, ..., n_len`, in order: so `n_i` is first visited from the fan, with
/// its only child `n_{i-1}` already memoized.
fn shared_chain(chain_len: usize, pad: u16) -> Vec<u8> {
    let arena: Arena<Db> = Storage::<Db>::new(16, Db::default()).arena;
    let fan = |children| arena.alloc(Fan { children, pad });
    let mut chain = vec![fan(vec![])];
    for i in 1..=chain_len {
        let next = fan(vec![chain[i - 1].clone()]);
        chain.push(next);
    }
    let mut level = chain[1..].to_vec();
    while level.len() > 1 {
        level = level.chunks(16).map(|c| fan(c.to_vec())).collect();
    }
    let mut bytes = Vec::new();
    level[0].serialize(&mut bytes).unwrap();
    bytes
}

/// Serializes a stack of diamonds whose deepest node is `height` levels below
/// the root.
///
/// Each diamond is a top node with two children, both leading to the top of
/// the diamond below, with the right-hand path one node longer:
///
/// ```text
///     top_i
///     /   \
///   a_i   b_i
///    |     |
///    |    c_i
///     \   /
///    top_{i-1}
/// ```
///
/// So `top_{i-1}` is first deserialized under `a_i`, and then reused from the
/// memo one level deeper, under `c_i`. Only the height check on memoized nodes
/// catches the extra depth along the longer side. Each diamond adds three
/// levels and doubles the number of paths; any remainder of `height` is made up
/// with single nodes on top.
fn diamonds(height: usize, pad: u16) -> Vec<u8> {
    let arena: Arena<Db> = Storage::<Db>::new(16, Db::default()).arena;
    let fan = |children, pad| arena.alloc(Fan { children, pad });
    let mut top = fan(vec![], pad);
    for _ in 0..height / 3 {
        // `a_i`, `b_i` and `c_i` differ in padding, so they are distinct nodes.
        let a = fan(vec![top.clone()], pad);
        let c = fan(vec![top], pad + 2);
        let b = fan(vec![c], pad + 1);
        top = fan(vec![a, b], pad);
    }
    for _ in 0..height % 3 {
        top = fan(vec![top], pad);
    }
    let mut bytes = Vec::new();
    top.serialize(&mut bytes).unwrap();
    bytes
}

// Both padding sizes, so that both the in-lined and the by-reference memo
// tables are exercised.
const PADS: [u16; 2] = [
    0,
    2 * midnight_storage_core::storable::SMALL_OBJECT_LIMIT as u16,
];

#[test]
fn shared_nodes_count_towards_the_recursion_limit() {
    let limit = serialize::RECURSION_LIMIT as usize;
    for pad in PADS {
        let arena: Arena<Db> = Storage::<Db>::new(16, Db::default()).arena;
        let bytes = shared_chain(4 * limit, pad);
        let res = arena.deserialize_sp::<Fan, _>(&mut &bytes[..], 0);
        assert!(
            res.is_err(),
            "pad {pad}: a graph deeper than the limit was accepted via shared nodes"
        );
        println!("pad {pad}: {}", res.err().unwrap());
    }
}

#[test]
fn shared_nodes_within_the_limit_still_decode() {
    let limit = serialize::RECURSION_LIMIT as usize;
    for pad in PADS {
        let arena: Arena<Db> = Storage::<Db>::new(16, Db::default()).arena;
        // The root is at depth 0, so the leaf is the `limit`th nested node.
        // Without memoization this is `2^(limit / 3)` work.
        let bytes = diamonds(limit - 1, pad);
        let res = arena.deserialize_sp::<Fan, _>(&mut &bytes[..], 0);
        assert!(res.is_ok(), "pad {pad}: {}", res.err().unwrap());

        let bytes = diamonds(limit, pad);
        let res = arena.deserialize_sp::<Fan, _>(&mut &bytes[..], 0);
        assert!(res.is_err(), "pad {pad}: one past the limit was accepted");
    }
}

const SMALL: u16 = 0;

/// The padding of the `i`th node of a chain, from the root.
type Layout = fn(usize) -> u16;
const LARGE: u16 = PADS[1];

/// Serializes a chain of `nodes` nodes, the `i`th from the root padded with
/// `pad(i)` bytes, so that it is in-lined or stored by reference.
fn padded_chain(nodes: usize, pad: impl Fn(usize) -> u16) -> Vec<u8> {
    let arena: Arena<Db> = Storage::<Db>::new(16, Db::default()).arena;
    let mut node = arena.alloc(Fan {
        children: vec![],
        pad: pad(nodes - 1),
    });
    for i in (0..nodes - 1).rev() {
        node = arena.alloc(Fan {
            children: vec![node],
            pad: pad(i),
        });
    }
    let mut bytes = Vec::new();
    node.serialize(&mut bytes).unwrap();
    bytes
}

/// Chains of in-lined, by-reference, and alternating nodes are all held to the
/// same bound, just below, at, and just above it; and a rejected chain is
/// rejected before any node past the limit is deserialized.
#[test]
fn mixed_chains_share_one_boundary() {
    let limit = serialize::RECURSION_LIMIT as usize;
    let layouts: [(&str, Layout); 4] = [
        ("in-lined", |_| SMALL),
        ("by-reference", |_| LARGE),
        ("alternating, in-lined root", |i| [SMALL, LARGE][i % 2]),
        ("alternating, by-reference root", |i| [LARGE, SMALL][i % 2]),
    ];
    for (name, pad) in layouts {
        let arena: Arena<Db> = Storage::<Db>::new(16, Db::default()).arena;
        let decode = |nodes: usize| {
            let bytes = padded_chain(nodes, pad);
            Frame::reset();
            let ok = arena.deserialize_sp::<Fan, _>(&mut &bytes[..], 0).is_ok();
            (ok, Frame::max())
        };

        assert_eq!(
            decode(limit - 1),
            (true, limit - 1),
            "{name}: below the limit"
        );
        assert_eq!(decode(limit), (true, limit), "{name}: at the limit");
        // Rejected without decoding the node past the limit.
        assert_eq!(decode(limit + 1), (false, limit), "{name}: above the limit");
        assert_eq!(
            decode(4 * limit),
            (false, limit),
            "{name}: far above the limit"
        );
    }
}
