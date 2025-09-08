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

#![deny(unreachable_pub)]
//#![deny(warnings)]
#![deny(missing_docs)]
//! Merkle-ized data structures and persistent disk storage.
//!
//! This crate provides storage primitives, primarily maps, for use
//! in larger data structures. It also exposes its deduplicated memory arena,
//! [`Arena`](arena::Arena), and pointers within it, [`Sp`](arena::Sp).

pub mod arena;
pub mod backend;
pub mod db;
pub mod delta_tracking;
pub mod merkle_patricia_trie;
pub mod storable;
pub mod storage;

pub use macros::Storable;
pub use storable::{Storable, WellBehavedHasher};
pub use storage::Storage;

mod cache;

#[cfg(any(
    test,
    all(
        feature = "stress-test",
        any(feature = "parity-db", feature = "sqlite")
    )
))]
mod test;

// Stress testing utilities. Needs to be pub since we call it from a bin
// target. But not meant to be consumed by library users.
#[cfg(feature = "stress-test")]
pub mod stress_test;

/// The default storage mechanism.
pub type DefaultHasher = sha2::Sha256;
/// The default database.
pub type DefaultDB = db::InMemoryDB<DefaultHasher>;
