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

//! Custom spend rules for shielded tokens.
//!
//! A [`SpendRule`] is folded into a token type at derivation time (see
//! [`ContractAddress::custom_shielded_token_type`](crate::contract::ContractAddress)),
//! committing all coins of that type to additional spend conditions. The rule
//! itself never appears on-chain; only the token type derived from its
//! [`descriptor_hash`](SpendRule::descriptor_hash) does.

use crate::contract::ContractAddress;
use base_crypto::hash::persistent_hash;
use fake::Dummy;
#[cfg(feature = "proptest")]
use proptest_derive::Arbitrary;
use serialize::{
    Deserializable, Serializable, Tagged, tag_enforcement_test, tagged_serialize,
    tagged_serialized_size,
};
use storage::db::DB;
use storage::{Storable, arena::ArenaKey, storable::Loader};
use transient_crypto::hash::HashOutput;

/// An additional condition attached to spending a shielded token, committed
/// to in the token type's derivation.
///
/// [`SpendRule::Empty`] denotes the absence of a custom rule and MUST map to
/// the legacy (pre-descriptor) token type derivation; it must never be hashed
/// into a derivation preimage itself.
#[derive(
    Debug, Clone, Default, Hash, PartialEq, Eq, PartialOrd, Ord, Dummy, Serializable, Storable,
)]
#[storable(base)]
#[tag = "spend-rule[v1]"]
#[cfg_attr(feature = "proptest", derive(Arbitrary))]
pub enum SpendRule {
    /// No custom spend condition; the token behaves as a plain zswap token.
    #[default]
    Empty,
    /// Spending additionally requires satisfying the circuit whose verifier
    /// key hashes (via `persistent_hash` of its canonical serialization) to
    /// the contained value.
    GateCircuit(HashOutput),
    /// Spending additionally requires a call, within the same transaction, to
    /// the given contract entry point (identified by its `ep_hash`).
    GateContract(ContractAddress, HashOutput),
}
tag_enforcement_test!(SpendRule);

impl SpendRule {
    /// A fixed-length (32-byte) commitment to this rule, appended to the
    /// token type derivation preimage for non-[`Empty`](SpendRule::Empty)
    /// rules. The hash preimage is the rule's tagged serialization
    /// (`midnight:spend-rule[v1]:` followed by the canonical encoding), so
    /// domain separation follows the crate-wide tagging scheme.
    ///
    /// Returns [`None`] for [`Empty`](SpendRule::Empty): the legacy 96-byte
    /// derivation preimage must be preserved exactly, so the absence of a
    /// rule contributes no bytes at all.
    pub fn descriptor_hash(&self) -> Option<HashOutput> {
        match self {
            SpendRule::Empty => None,
            _ => {
                let mut data = Vec::with_capacity(tagged_serialized_size(self));
                tagged_serialize(self, &mut data)
                    .expect("SpendRule serialization into a Vec cannot fail");
                Some(persistent_hash(&data))
            }
        }
    }
}

impl rand::distributions::Distribution<SpendRule> for rand::distributions::Standard {
    fn sample<R: rand::Rng + ?Sized>(&self, rng: &mut R) -> SpendRule {
        match rng.gen_range(0..3u8) {
            0 => SpendRule::Empty,
            1 => SpendRule::GateCircuit(rng.r#gen()),
            _ => SpendRule::GateContract(ContractAddress(rng.r#gen()), rng.r#gen()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base_crypto::hash::persistent_commit;

    #[test]
    fn empty_rule_preserves_legacy_derivation() {
        let addr = ContractAddress(HashOutput([1u8; 32]));
        let domain_sep = HashOutput([2u8; 32]);
        // The pre-`SpendRule` derivation, spelled out independently of the
        // implementation in `contract.rs`.
        let legacy = persistent_commit(
            &(domain_sep, addr.0),
            HashOutput(*b"midnight:derive_token\0\0\0\0\0\0\0\0\0\0\0"),
        );
        assert_eq!(
            addr.custom_shielded_token_type(domain_sep, &SpendRule::Empty)
                .0,
            legacy
        );
        assert_eq!(addr.custom_unshielded_token_type(domain_sep).0, legacy);
    }

    #[test]
    fn rules_produce_distinct_token_types() {
        let addr = ContractAddress(HashOutput([1u8; 32]));
        let domain_sep = HashOutput([2u8; 32]);
        let types = [
            SpendRule::Empty,
            SpendRule::GateCircuit(HashOutput([3u8; 32])),
            SpendRule::GateCircuit(HashOutput([4u8; 32])),
            SpendRule::GateContract(addr, HashOutput([3u8; 32])),
        ]
        .iter()
        .map(|rule| addr.custom_shielded_token_type(domain_sep, rule))
        .collect::<std::collections::HashSet<_>>();
        assert_eq!(types.len(), 4);
    }
}
