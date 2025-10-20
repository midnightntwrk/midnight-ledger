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

use arena::Sp;
use midnight_storage::{self as storage, *};
use rayon::prelude::*;
use serialize::{Deserializable, Serializable};
use std::hash::Hash;
use storable::Loader;
use storage::{arena::ArenaKey, db::DB};

#[derive(Serializable, Storable, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[storable(base)]
struct Foo {
    a: u8,
    b: u64,
}

#[derive(Serializable, Storable, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[storable(base)]
struct Bar {
    a: u8,
    b: u64,
}

#[test]
fn test_storable_clash() {
    let storage = Storage::new(16, DefaultDB::default());
    let arena = storage.arena;
    let _ptr_a = arena.alloc(Foo { a: 5, b: 42 });
    let _ptr_b = arena.alloc(Bar { a: 5, b: 42 });
}

#[test]
fn test_parallel_clash() {
    enum Either {
        Left(Sp<Foo>),
        Right(Sp<Bar>),
    }
    fn get_a(x: Either) -> u8 {
        match x {
            Either::Left(x) => x.a,
            Either::Right(x) => x.a,
        }
    }
    for _ in 0..100 {
        (0..100)
            .into_par_iter()
            .map(|i| {
                if i % 2 == 0 {
                    Either::Left(Sp::new(Foo { a: 5, b: 42 }))
                } else {
                    Either::Right(Sp::new(Bar { a: 5, b: 42 }))
                }
            })
            .map(get_a)
            .collect::<std::vec::Vec<_>>();
    }
}
