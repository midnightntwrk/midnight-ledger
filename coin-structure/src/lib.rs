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
#![deny(warnings)]
// Proptest derive triggers this.
#![allow(non_local_definitions)]

pub mod coin;
pub mod contract;
mod fab;
pub mod transfer;

macro_rules! hash_serde {
    ($($ty:ident),*) => {
        $(
            impl serde::Serialize for $ty {
                fn serialize<S: serde::ser::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                    serializer.serialize_bytes(&self.0.0)
                }
            }

            impl<'de> serde::Deserialize<'de> for $ty {
                fn deserialize<D: serde::de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                    deserializer.deserialize_bytes(crate::HashVisitor).map($ty)
                }
            }
        )*
    }
}
pub(crate) use hash_serde;

pub(crate) struct HashVisitor;

impl serde::de::Visitor<'_> for HashVisitor {
    type Value = base_crypto::hash::HashOutput;
    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(formatter, "a hash value")
    }

    fn visit_bytes<E: serde::de::Error>(self, v: &[u8]) -> Result<Self::Value, E> {
        let mut res = [0u8; base_crypto::hash::PERSISTENT_HASH_BYTES];
        if v.len() == res.len() {
            res.copy_from_slice(v);
            Ok(base_crypto::hash::HashOutput(res))
        } else {
            Err(E::invalid_length(v.len(), &self))
        }
    }
}
