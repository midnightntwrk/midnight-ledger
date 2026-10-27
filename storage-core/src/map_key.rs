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

//! How decoding a `Map` checks that its keys are valid.
//!
//! Checking each key separately would take time exponential in the encoding
//! of a trie with shared subtrees. Keys are instead checked either by
//! [`EncodingShape`], an automaton run once per trie node and automaton state,
//! or, for keys without a small automaton, against the value stored with
//! them, which pins every key to one path and so rules out sharing.
//!
//! It lives beside `ArenaKey` because the two `MapKey` impls are only disjoint
//! in the crate defining `ArenaKey`, where it is known to have no `EncodingShape`.

use crate::WellBehavedHasher;
use crate::arena::ArenaKey;
use serialize::EncodingShape;

/// A key type of a `Map` whose values are `V`, with the way decoding checks it.
pub trait MapKey<V> {
    /// Either [`ByShape`] or [`ByValue`].
    type Check;
}

/// Keys are checked against their [`EncodingShape`].
pub struct ByShape;

/// Keys are checked against the key each value holds, via [`HoldsKey`].
pub struct ByValue;

/// A value that records the key it is stored under.
pub trait HoldsKey<K> {
    /// The key this value is stored under.
    fn key(&self) -> &K;
}

impl<K: EncodingShape, V> MapKey<V> for K {
    type Check = ByShape;
}

impl<H: WellBehavedHasher, V: HoldsKey<ArenaKey<H>>> MapKey<V> for ArenaKey<H> {
    type Check = ByValue;
}
