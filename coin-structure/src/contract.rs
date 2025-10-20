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

use crate::coin::{ShieldedTokenType, UnshieldedTokenType};
use crate::hash_serde;
use base_crypto::hash::{HashOutput, persistent_commit};
use base_crypto::repr::{BinaryHashRepr, MemWrite};
use fake::Dummy;
#[cfg(feature = "proptest")]
use proptest_derive::Arbitrary;
use serialize::{self, Deserializable, Serializable, Tagged, tag_enforcement_test};
use storage::Storable;
use storage::arena::ArenaKey;
use storage::db::DB;
use storage::storable::Loader;
use transient_crypto::curve::Fr;
use transient_crypto::repr::{FieldRepr, FromFieldRepr};

#[derive(
    Debug,
    Default,
    Copy,
    Clone,
    Hash,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    FieldRepr,
    FromFieldRepr,
    BinaryHashRepr,
    Serializable,
    Storable,
    Dummy,
)]
#[storable(base)]
#[tag = "contract-address[v2]"]
#[cfg_attr(feature = "proptest", derive(Arbitrary))]
pub struct ContractAddress(pub HashOutput);
tag_enforcement_test!(ContractAddress);

impl rand::distributions::Distribution<ContractAddress> for rand::distributions::Standard {
    fn sample<R: rand::Rng + ?Sized>(&self, rng: &mut R) -> ContractAddress {
        ContractAddress(rng.r#gen())
    }
}

impl ContractAddress {
    fn custom_token_type(&self, domain_sep: HashOutput) -> HashOutput {
        let inner_domain_sep = HashOutput(*b"midnight:derive_token\0\0\0\0\0\0\0\0\0\0\0");
        persistent_commit(&(domain_sep, self.0), inner_domain_sep)
    }

    pub fn custom_shielded_token_type(&self, domain_sep: HashOutput) -> ShieldedTokenType {
        ShieldedTokenType(self.custom_token_type(domain_sep))
    }

    pub fn custom_unshielded_token_type(&self, domain_sep: HashOutput) -> UnshieldedTokenType {
        UnshieldedTokenType(self.custom_token_type(domain_sep))
    }
}

#[cfg(feature = "proptest")]
serialize::randomised_serialization_test!(ContractAddress);
hash_serde!(ContractAddress);
