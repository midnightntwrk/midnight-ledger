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

#![deny(unreachable_pub)]
//#![deny(warnings)]
#![deny(missing_docs)]
// Proptest derive triggers this.
#![allow(non_local_definitions)]

//! This crate collects cryptographic primitives used in Midnight's ledger.
//! All primitives, including zero-knowledge, curve choice, and crypto-aware
//! data structures are defined here, and should be added here to decouple from
//! any specific implementation.

pub mod cost_model;
#[cfg(feature = "data-provider")]
pub mod data_provider;

/// The part of the data provider that is not a data provider.
///
/// `hexhash` is a `const fn` over a literal and has nothing to do with fetching anything, but
/// it lives beside the code that does. Without `data-provider` the module is this and only
/// this, so callers of `hexhash` need no feature and no HTTP client.
#[cfg(not(feature = "data-provider"))]
pub mod data_provider {
    /// Parse a 256-bit hex hash at const time.
    pub const fn hexhash(hex: &[u8]) -> [u8; 32] {
        match const_hex::const_decode_to_array(hex) {
            Ok(hash) => hash,
            Err(_) => panic!("hash should be correct format"),
        }
    }
}
pub mod fab;
pub mod hash;
pub mod repr;
pub mod rng;
pub mod signatures;
pub mod time;

pub use repr::*;
