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

//! Hashing functions for use across Midnight.

use crate::repr::{BinaryHashRepr, MemWrite};
use const_hex::ToHexExt;
use fake::Dummy;
#[cfg(feature = "proptest")]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};
use serialize::{Deserializable, Serializable, Tagged, tag_enforcement_test};
use sha2::{Digest, Sha256};
use std::fmt::{self, Debug, Display, Formatter};
use std::io;
use zeroize::Zeroize;

/// The number of bytes output by [`persistent_hash`].
pub const PERSISTENT_HASH_BYTES: usize = 32;

/// A wrapper around hash outputs.
#[derive(
    Copy,
    Clone,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    BinaryHashRepr,
    Serializable,
    Serialize,
    Deserialize,
    Dummy,
    Zeroize,
)]
#[cfg_attr(feature = "proptest", derive(Arbitrary))]
pub struct HashOutput(pub [u8; PERSISTENT_HASH_BYTES]);
tag_enforcement_test!(HashOutput);

impl Tagged for HashOutput {
    fn tag() -> std::borrow::Cow<'static, str> {
        <[u8; PERSISTENT_HASH_BYTES]>::tag()
    }
    fn tag_unique_factor() -> String {
        <[u8; PERSISTENT_HASH_BYTES]>::tag_unique_factor()
    }
}

#[cfg(feature = "proptest")]
serialize::randomised_serialization_test!(HashOutput);

/// A zeroed [`HashOutput`].
pub const BLANK_HASH: HashOutput = HashOutput([0u8; PERSISTENT_HASH_BYTES]);

impl rand::distributions::Distribution<HashOutput> for rand::distributions::Standard {
    fn sample<R: rand::Rng + ?Sized>(&self, rng: &mut R) -> HashOutput {
        HashOutput(rng.r#gen())
    }
}

impl Debug for HashOutput {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(formatter, "{}", self.0.encode_hex())
    }
}

impl Display for HashOutput {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(formatter, "{}", &self.0.encode_hex()[..10])
    }
}

/// A hash function that is guaranteed for long-term support.
pub fn persistent_hash(a: &[u8]) -> HashOutput {
    HashOutput(Sha256::digest(a).into())
}

/// Commits to a value using `persistent_hash`.
pub fn persistent_commit<T: BinaryHashRepr + ?Sized>(value: &T, opening: HashOutput) -> HashOutput {
    let mut writer = PersistentHashWriter::new();
    opening.binary_repr(&mut writer);
    value.binary_repr(&mut writer);
    writer.finalize()
}

/// A writer object for building large persistent commitments of data.
pub struct PersistentHashWriter(Sha256);

impl MemWrite<u8> for PersistentHashWriter {
    fn write(&mut self, buf: &[u8]) {
        self.0.update(buf);
    }
}

impl io::Write for PersistentHashWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.update(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Default for PersistentHashWriter {
    fn default() -> Self {
        PersistentHashWriter(Sha256::new())
    }
}

impl PersistentHashWriter {
    /// Initializes a black hasher.
    pub fn new() -> Self {
        Default::default()
    }

    /// Finalizes the hasher, and returns the result.
    pub fn finalize(self) -> HashOutput {
        HashOutput(self.0.finalize().into())
    }
}
