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
