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

//! Custom spend rules for shielded tokens (the custom spend logic MIP).
//!
//! A shielded token type binds a *spend rule descriptor* as the last operand
//! of its derivation preimage (see
//! [`ContractAddress::custom_shielded_token_type`]):
//!
//! ```text
//! RawTokenType = SHA256("midnight:derive_token" padded to 32 bytes ‖ domain_sep ‖ contract_address ‖ descriptor)
//! ```
//!
//! An *unguarded* type, mode 1, has an empty descriptor, so it keeps today's
//! derivation byte for byte. A *guarded* type appends a fixed-width
//! [`Descriptor`]. The rule is stored neither in the coin nor in any registry:
//! every shielded spend re-derives the coin's type from its
//! [`ShieldedTokenPreimage`].

use crate::coin::ShieldedTokenType;
use crate::contract::ContractAddress;
use fake::Dummy;
#[cfg(feature = "proptest")]
use proptest_derive::Arbitrary;
use serialize::{Deserializable, Serializable, Tagged, tag_enforcement_test};
use storage::db::DB;
use storage::{Storable, arena::ArenaKey, storable::Loader};
use transient_crypto::hash::HashOutput;

/// A spend rule descriptor: the fixed-width record a guarded token type
/// appends to its derivation preimage.
///
/// ```text
/// mode      1 byte     mode number: 2 or 3
/// version   1 byte     1
/// payload   32 bytes   mode 2, the guard's verifying-key hash; mode 3, reserved
/// ```
///
/// Any byte values parse; the ledger refuses to mint, and the spend circuits
/// to spend, any descriptor but [`Descriptor::is_defined`] ones.
#[derive(
    Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord, Dummy, Serializable, Storable,
)]
#[storable(base)]
#[tag = "spend-rule-descriptor[v1]"]
#[cfg_attr(feature = "proptest", derive(Arbitrary))]
pub struct Descriptor {
    pub mode: u8,
    pub version: u8,
    pub payload: HashOutput,
}
tag_enforcement_test!(Descriptor);

impl Descriptor {
    /// The width of a descriptor in a derivation preimage.
    pub const LEN: usize = 34;
    /// Mode 2: a guard, verified recursively.
    pub const MODE_GUARD: u8 = 2;
    /// Mode 3: a contract call bound to the spend (reserved).
    pub const MODE_CONTRACT_CALL: u8 = 3;
    /// The only version of mode 2 defined.
    pub const VERSION_1: u8 = 1;

    /// The mode 2, version 1 descriptor of the guard whose verifying key
    /// hashes to `vk_hash`.
    pub fn guard(vk_hash: HashOutput) -> Self {
        Descriptor {
            mode: Self::MODE_GUARD,
            version: Self::VERSION_1,
            payload: vk_hash,
        }
    }

    /// Whether the ledger and the spend circuits accept this descriptor:
    /// mode 2 with version 1.
    pub fn is_defined(&self) -> bool {
        self.mode == Self::MODE_GUARD && self.version == Self::VERSION_1
    }

    /// The descriptor as it is appended to a derivation preimage.
    pub fn to_bytes(&self) -> [u8; Self::LEN] {
        let mut out = [0u8; Self::LEN];
        out[0] = self.mode;
        out[1] = self.version;
        out[2..].copy_from_slice(&self.payload.0);
        out
    }
}

/// The spend rule of a shielded token type: the empty descriptor (mode 1, no
/// rule), or a [`Descriptor`].
#[derive(
    Debug, Clone, Copy, Default, Hash, PartialEq, Eq, PartialOrd, Ord, Dummy, Serializable, Storable,
)]
#[storable(base)]
#[tag = "spend-rule[v2]"]
#[cfg_attr(feature = "proptest", derive(Arbitrary))]
pub enum SpendRule {
    /// Mode 1: no rule; the type keeps today's derivation.
    #[default]
    Empty,
    /// A guarded type's descriptor.
    Descriptor(Descriptor),
}
tag_enforcement_test!(SpendRule);

impl SpendRule {
    /// The mode 2 rule of the guard whose verifying key hashes to `vk_hash`.
    pub fn guard(vk_hash: HashOutput) -> Self {
        SpendRule::Descriptor(Descriptor::guard(vk_hash))
    }

    /// The descriptor, if the rule is not empty.
    pub fn descriptor(&self) -> Option<&Descriptor> {
        match self {
            SpendRule::Empty => None,
            SpendRule::Descriptor(descriptor) => Some(descriptor),
        }
    }

    /// Whether the ledger and the spend circuits accept this rule: the empty
    /// rule, or a defined descriptor.
    pub fn is_defined(&self) -> bool {
        self.descriptor().is_none_or(Descriptor::is_defined)
    }
}

impl rand::distributions::Distribution<Descriptor> for rand::distributions::Standard {
    fn sample<R: rand::Rng + ?Sized>(&self, rng: &mut R) -> Descriptor {
        Descriptor::guard(rng.r#gen())
    }
}

impl rand::distributions::Distribution<SpendRule> for rand::distributions::Standard {
    fn sample<R: rand::Rng + ?Sized>(&self, rng: &mut R) -> SpendRule {
        match rng.gen_range(0..2u8) {
            0 => SpendRule::Empty,
            _ => SpendRule::Descriptor(rng.r#gen()),
        }
    }
}

/// The preimage of a shielded token type: what every spend circuit re-derives
/// the coin's type from.
///
/// The spender supplies it on every shielded spend. Genesis mints under the
/// all-zero contract address (see [`ShieldedTokenPreimage::genesis`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ShieldedTokenPreimage {
    /// The domain separator the minting contract chose.
    pub domain_sep: HashOutput,
    /// The minting contract.
    pub contract_address: ContractAddress,
    /// The type's spend rule.
    pub rule: SpendRule,
}

impl ShieldedTokenPreimage {
    /// The preimage of a type genesis mints under `domain_sep`.
    pub fn genesis(domain_sep: HashOutput, rule: SpendRule) -> Self {
        ShieldedTokenPreimage {
            domain_sep,
            contract_address: ContractAddress::GENESIS,
            rule,
        }
    }

    /// The token type this preimage derives.
    pub fn token_type(&self) -> ShieldedTokenType {
        self.contract_address
            .custom_shielded_token_type(self.domain_sep, &self.rule)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::ContractAddress;
    use base_crypto::hash::{persistent_commit, persistent_hash};

    const INNER: [u8; 32] = *b"midnight:derive_token\0\0\0\0\0\0\0\0\0\0\0";

    #[test]
    fn empty_rule_preserves_legacy_derivation() {
        let addr = ContractAddress(HashOutput([1u8; 32]));
        let domain_sep = HashOutput([2u8; 32]);
        // The pre-`SpendRule` derivation, spelled out independently of the
        // implementation in `contract.rs`.
        let legacy = persistent_commit(&(domain_sep, addr.0), HashOutput(INNER));
        assert_eq!(
            legacy,
            persistent_hash(&[&INNER[..], &domain_sep.0, &addr.0.0].concat())
        );
        assert_eq!(
            addr.custom_shielded_token_type(domain_sep, &SpendRule::Empty)
                .0,
            legacy
        );
        assert_eq!(addr.custom_unshielded_token_type(domain_sep).0, legacy);
    }

    #[test]
    fn guarded_derivation_appends_the_descriptor() {
        let addr = ContractAddress(HashOutput([1u8; 32]));
        let domain_sep = HashOutput([2u8; 32]);
        let rule = SpendRule::guard(HashOutput([3u8; 32]));
        let mut descriptor = vec![2u8, 1u8];
        descriptor.extend([3u8; 32]);
        assert_eq!(
            addr.custom_shielded_token_type(domain_sep, &rule).0,
            persistent_hash(&[&INNER[..], &domain_sep.0, &addr.0.0, &descriptor].concat())
        );
    }

    #[test]
    fn rules_produce_distinct_token_types() {
        let addr = ContractAddress(HashOutput([1u8; 32]));
        let domain_sep = HashOutput([2u8; 32]);
        let types = [
            SpendRule::Empty,
            SpendRule::guard(HashOutput([3u8; 32])),
            SpendRule::guard(HashOutput([4u8; 32])),
            SpendRule::Descriptor(Descriptor {
                mode: Descriptor::MODE_GUARD,
                version: 2,
                payload: HashOutput([3u8; 32]),
            }),
        ]
        .iter()
        .map(|rule| addr.custom_shielded_token_type(domain_sep, rule))
        .collect::<std::collections::HashSet<_>>();
        assert_eq!(types.len(), 4);
    }

    #[test]
    fn only_mode_2_version_1_is_defined() {
        assert!(SpendRule::Empty.is_defined());
        assert!(SpendRule::guard(HashOutput([0u8; 32])).is_defined());
        for (mode, version) in [(2u8, 2u8), (3, 1), (1, 1), (0, 0)] {
            let rule = SpendRule::Descriptor(Descriptor {
                mode,
                version,
                payload: HashOutput([0u8; 32]),
            });
            assert!(!rule.is_defined(), "mode {mode} version {version}");
        }
    }
}
