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

// Regression test: deserializing a deeply nested storage graph must fail
// gracefully rather than overflowing the stack.

use midnight_storage::Storage;
use midnight_storage::arena::{Arena, TopoSortedNode, TopoSortedNodes};
use midnight_storage::db::InMemoryDB;
use midnight_storage::merkle_patricia_trie::Node;
use midnight_storage::storable::SizeAnn;
use serialize::Serializable;

type MptNode = Node<(), InMemoryDB, SizeAnn>;

fn extension_data(path_len: u8, compressed: &[u8]) -> Vec<u8> {
    let mut data = Vec::new();
    u8::serialize(&3u8, &mut data).unwrap();
    SizeAnn(1).serialize(&mut data).unwrap();
    u8::serialize(&path_len, &mut data).unwrap();
    compressed.to_vec().serialize(&mut data).unwrap();
    data
}

fn chain(depth: usize) -> TopoSortedNodes {
    let mut nodes = Vec::with_capacity(depth + 1);
    // Leaf-most node: `Node::Empty`
    nodes.push(TopoSortedNode {
        child_indices: vec![],
        data: vec![0u8],
    });
    for i in 0..depth {
        nodes.push(TopoSortedNode {
            child_indices: vec![i as u64],
            data: extension_data(1, &[0x00]),
        });
    }
    TopoSortedNodes { nodes }
}

#[test]
fn deeply_nested_inline_chain_is_rejected() {
    std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(|| {
            let arena: Arena<InMemoryDB> =
                Storage::<InMemoryDB>::new(16, InMemoryDB::default()).arena;
            let mut bytes = Vec::new();
            chain(50_000).serialize(&mut bytes).unwrap();
            let res = arena.deserialize_sp::<MptNode, _>(&mut &bytes[..], 0);
            assert!(res.is_err(), "expected an error, got a value");
            println!("error: {}", res.err().unwrap());
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn extension_path_length_beyond_compressed_data_is_rejected() {
    // `len` is encoded independently of the compressed path bytes, so every
    // combination where it overruns them must be rejected rather than indexing
    // out of bounds in `expand_nibbles`. Regression coverage for the
    // declared-length check in `Node::from_binary_repr`'s `Extension` arm.
    let cases: &[(u8, &[u8])] = &[
        (1, &[]),           // index 0, len 0
        (3, &[0x00]),       // index 1, len 1
        (255, &[]),         // index 0, len 0
        (255, &[0x00; 42]), // index 42, len 42
        (5, &[0x00, 0x00]), // exactly one nibble too many
    ];
    for (len, compressed) in cases {
        let arena: Arena<InMemoryDB> = Storage::<InMemoryDB>::new(16, InMemoryDB::default()).arena;
        let nodes = TopoSortedNodes {
            nodes: vec![
                TopoSortedNode {
                    child_indices: vec![],
                    data: vec![0u8],
                },
                TopoSortedNode {
                    child_indices: vec![0],
                    data: extension_data(*len, compressed),
                },
            ],
        };
        let mut bytes = Vec::new();
        nodes.serialize(&mut bytes).unwrap();
        let res = arena.deserialize_sp::<MptNode, _>(&mut &bytes[..], 0);
        match res {
            Ok(_) => panic!("len {len} over {} byte(s) was accepted", compressed.len()),
            Err(err) => println!("len {len} over {} byte(s): {err}", compressed.len()),
        }
    }
}

#[test]
fn extension_path_length_within_compressed_data_still_loads() {
    // The counterpart: a path whose declared length matches its encoded bytes
    // must not be rejected by that check. A single `Extension` over an `Empty`
    // child is invariant-invalid (the size annotation won't match), so assert
    // on the *kind* of error rather than on success.
    let arena: Arena<InMemoryDB> = Storage::<InMemoryDB>::new(16, InMemoryDB::default()).arena;
    let nodes = TopoSortedNodes {
        nodes: vec![
            TopoSortedNode {
                child_indices: vec![],
                data: vec![0u8],
            },
            TopoSortedNode {
                child_indices: vec![0],
                data: extension_data(2, &[0x12]),
            },
        ],
    };
    let mut bytes = Vec::new();
    nodes.serialize(&mut bytes).unwrap();
    let err = match arena.deserialize_sp::<MptNode, _>(&mut &bytes[..], 0) {
        Ok(_) => return,
        Err(err) => err.to_string(),
    };
    println!("error: {err}");
    assert!(
        !err.contains("nibble byte length"),
        "a path that fits was rejected by the declared-length check: {err}"
    );
}
