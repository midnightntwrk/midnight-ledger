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

use arena::{ArenaKey, Sp};
use db::DB;
use derive_where::derive_where;
use midnight_storage as storage;
use midnight_storage::*;
use serialize::Serializable;
use storable::Loader;

#[derive(Debug, Storable)]
#[storable(db = D)]
#[derive_where(Clone, Hash, Eq, PartialEq, PartialOrd, Ord)]
struct Foo<D: DB>(#[storable(child)] Sp<u8, D>);

#[test]
fn sp_children_test() {
    let storage = Storage::new(16, DefaultDB::default());
    let arena = storage.arena;
    let ptr_u8 = arena.alloc(42);
    let ptr_bar = arena.alloc(Foo(ptr_u8.clone()));
    let mut bytes: std::vec::Vec<u8> = std::vec::Vec::new();
    Sp::serialize(&ptr_bar, &mut bytes).unwrap();
    let ptr_bar_prime = arena.deserialize_sp(&mut bytes.as_slice(), 0).unwrap();
    assert_eq!(ptr_bar, ptr_bar_prime);
}
