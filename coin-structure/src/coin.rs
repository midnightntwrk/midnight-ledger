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

use crate::contract::ContractAddress;
use crate::hash_serde;
use crate::transfer::{Recipient, SenderEvidence};
use base_crypto::fab::{Aligned, Alignment, InvalidBuiltinDecode, Value, ValueAtom, ValueSlice};
use base_crypto::hash::persistent_hash;
use base_crypto::hash::{BLANK_HASH, PERSISTENT_HASH_BYTES};
use base_crypto::repr::{BinaryHashRepr, MemWrite};
use base_crypto::signatures::VerifyingKey;
use fake::Dummy;
#[cfg(feature = "proptest")]
use proptest_derive::Arbitrary;
use rand::{CryptoRng, Rng};
use serde::{Deserialize, Serialize};
#[cfg(feature = "proptest")]
use serialize::randomised_serialization_test;
use serialize::{self, Deserializable, Serializable, Tagged, tag_enforcement_test};
use storage::db::DB;
use storage::{Storable, arena::ArenaKey, storable::Loader};
use transient_crypto::curve::Fr;
use transient_crypto::hash::HashOutput;
use transient_crypto::hash::{degrade_to_transient, transient_hash, upgrade_from_transient};
use transient_crypto::repr::{FieldRepr, FromFieldRepr};
use zeroize::Zeroize;

use std::fmt::{self, Debug, Formatter};
use std::iter::once;

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
#[tag = "zswap-nullifier[v1]"]
#[cfg_attr(feature = "proptest", derive(Arbitrary))]
pub struct Nullifier(pub HashOutput);
tag_enforcement_test!(Nullifier);

impl rand::distributions::Distribution<Nullifier> for rand::distributions::Standard {
    fn sample<R: rand::Rng + ?Sized>(&self, rng: &mut R) -> Nullifier {
        Nullifier(rng.r#gen())
    }
}

#[cfg(feature = "proptest")]
randomised_serialization_test!(Nullifier);

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
#[tag = "zswap-coin-commitment[v1]"]
#[cfg_attr(feature = "proptest", derive(Arbitrary))]
pub struct Commitment(pub HashOutput);
tag_enforcement_test!(Commitment);

impl rand::distributions::Distribution<Commitment> for rand::distributions::Standard {
    fn sample<R: rand::Rng + ?Sized>(&self, rng: &mut R) -> Commitment {
        Commitment(rng.r#gen())
    }
}

#[cfg(feature = "proptest")]
randomised_serialization_test!(Commitment);

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
#[cfg_attr(feature = "proptest", derive(Arbitrary))]
#[tag = "zswap-nonce[v1]"]
pub struct Nonce(pub HashOutput);
tag_enforcement_test!(Nonce);

#[cfg(feature = "proptest")]
randomised_serialization_test!(Nonce);

impl rand::distributions::Distribution<Nonce> for rand::distributions::Standard {
    fn sample<R: rand::Rng + ?Sized>(&self, rng: &mut R) -> Nonce {
        Nonce(rng.r#gen())
    }
}

#[derive(
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
    Dummy,
    Zeroize,
)]
#[tag = "zswap-coin-secret-key[v1]"]
#[cfg_attr(feature = "proptest", derive(Arbitrary))]
pub struct SecretKey(pub HashOutput);
tag_enforcement_test!(SecretKey);

impl Debug for SecretKey {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(formatter, "<coin secret key>")
    }
}

#[cfg(feature = "proptest")]
randomised_serialization_test!(SecretKey);

impl SecretKey {
    pub fn public_key(&self) -> PublicKey {
        let mut data = Vec::with_capacity(38);
        self.binary_repr(&mut data);
        data.extend(b"mdn:pk");
        PublicKey(persistent_hash(&data))
    }
}

impl TryFrom<&ValueAtom> for SecretKey {
    type Error = InvalidBuiltinDecode;

    fn try_from(value: &ValueAtom) -> Result<SecretKey, InvalidBuiltinDecode> {
        let mut buf = [0u8; PERSISTENT_HASH_BYTES];
        if value.0.len() <= PERSISTENT_HASH_BYTES {
            buf[..value.0.len()].copy_from_slice(&value.0[..]);
            Ok(SecretKey(HashOutput(buf)))
        } else {
            Err(InvalidBuiltinDecode("SecretKey"))
        }
    }
}

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
    Dummy,
)]
#[tag = "zswap-coin-public-key[v1]"]
#[cfg_attr(feature = "proptest", derive(Arbitrary))]
pub struct PublicKey(pub HashOutput);
tag_enforcement_test!(PublicKey);

impl rand::distributions::Distribution<PublicKey> for rand::distributions::Standard {
    fn sample<R: rand::Rng + ?Sized>(&self, rng: &mut R) -> PublicKey {
        PublicKey(rng.r#gen())
    }
}

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
    Dummy,
    Storable,
)]
#[storable(base)]
#[tag = "shielded-token-type[v1]"]
#[cfg_attr(feature = "proptest", derive(Arbitrary))]
pub struct ShieldedTokenType(pub HashOutput);
tag_enforcement_test!(ShieldedTokenType);

impl ShieldedTokenType {
    pub fn into_inner(&self) -> HashOutput {
        self.0
    }
}

impl rand::distributions::Distribution<ShieldedTokenType> for rand::distributions::Standard {
    fn sample<R: rand::Rng + ?Sized>(&self, rng: &mut R) -> ShieldedTokenType {
        ShieldedTokenType(rng.r#gen())
    }
}

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
    Dummy,
    Storable,
)]
#[storable(base)]
#[tag = "unshielded-token-type[v1]"]
#[cfg_attr(feature = "proptest", derive(Arbitrary))]
pub struct UnshieldedTokenType(pub HashOutput);
tag_enforcement_test!(UnshieldedTokenType);

impl UnshieldedTokenType {
    pub fn into_inner(&self) -> HashOutput {
        self.0
    }
}

impl rand::distributions::Distribution<UnshieldedTokenType> for rand::distributions::Standard {
    fn sample<R: rand::Rng + ?Sized>(&self, rng: &mut R) -> UnshieldedTokenType {
        UnshieldedTokenType(rng.r#gen())
    }
}

#[derive(
    Debug, Copy, Clone, Hash, PartialEq, Eq, PartialOrd, Ord, Dummy, Serializable, Storable,
)]
#[tag = "token-type[v1]"]
#[storable(base)]
#[cfg_attr(feature = "proptest", derive(Arbitrary))]
pub enum TokenType {
    Unshielded(UnshieldedTokenType),
    Shielded(ShieldedTokenType),
    Dust,
}
tag_enforcement_test!(TokenType);

impl Aligned for TokenType {
    fn alignment() -> Alignment {
        Alignment::concat([
            &u8::alignment(),
            &<[u8; 32]>::alignment(),
            &<[u8; 32]>::alignment(),
        ])
    }
}

pub const UNSHIELDED_TAG: u8 = 0;
pub const SHIELDED_TAG: u8 = 1;
pub const DUST_TAG: u8 = 2;

impl From<TokenType> for Value {
    fn from(tt: TokenType) -> Value {
        Value(match tt {
            TokenType::Unshielded(tt) => vec![1u8.into(), tt.into(), ().into()],
            TokenType::Shielded(tt) => vec![0u8.into(), ().into(), tt.into()],
            TokenType::Dust => vec![2u8.into(), ().into(), ().into()],
        })
    }
}

impl TryFrom<&ValueSlice> for TokenType {
    type Error = InvalidBuiltinDecode;

    fn try_from(value: &ValueSlice) -> Result<TokenType, InvalidBuiltinDecode> {
        if value.0.len() == 3 {
            let variant: u8 = (&value.0[0]).try_into()?;
            match variant {
                0 => {
                    <()>::try_from(&value.0[1])?;
                    Ok(TokenType::Shielded((&value.0[2]).try_into()?))
                }
                1 => {
                    <()>::try_from(&value.0[2])?;
                    Ok(TokenType::Unshielded((&value.0[1]).try_into()?))
                }
                2 => {
                    <()>::try_from(&value.0[1])?;
                    <()>::try_from(&value.0[2])?;
                    Ok(TokenType::Dust)
                }
                _ => Err(InvalidBuiltinDecode("TokenType")),
            }
        } else {
            Err(InvalidBuiltinDecode("TokenType"))
        }
    }
}

impl Serialize for TokenType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let bytes = match self {
            TokenType::Unshielded(data) => {
                once(UNSHIELDED_TAG).chain(data.0.0.into_iter()).collect()
            }
            TokenType::Shielded(data) => once(SHIELDED_TAG).chain(data.0.0.into_iter()).collect(),
            TokenType::Dust => vec![DUST_TAG],
        };
        serializer.serialize_bytes(&bytes)
    }
}

impl<'de> Deserialize<'de> for TokenType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        pub(crate) struct TokenTypeVisitor;

        impl serde::de::Visitor<'_> for TokenTypeVisitor {
            type Value = TokenType;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a discriminator and maybe a hash value")
            }

            fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                if v.is_empty() {
                    return Err(E::invalid_length(v.len(), &self));
                }
                let tag = v[0];
                if tag == DUST_TAG {
                    if v.len() != 1 {
                        return Err(E::invalid_length(v.len(), &self));
                    }
                    return Ok(TokenType::Dust);
                }
                if v.len() != 33 {
                    return Err(E::invalid_length(v.len(), &self));
                }
                let mut data = [0u8; 32];
                data.copy_from_slice(&v[1..]);

                match tag {
                    UNSHIELDED_TAG => {
                        Ok(TokenType::Unshielded(UnshieldedTokenType(HashOutput(data))))
                    }
                    SHIELDED_TAG => Ok(TokenType::Shielded(ShieldedTokenType(HashOutput(data)))),
                    _ => Err(E::unknown_variant(&tag.to_string(), &["0", "1", "2"])),
                }
            }
        }

        deserializer.deserialize_bytes(TokenTypeVisitor)
    }
}

impl rand::distributions::Distribution<TokenType> for rand::distributions::Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> TokenType {
        let is_shielded: bool = rng.r#gen();
        let value: HashOutput = rng.r#gen();

        if is_shielded {
            TokenType::Shielded(ShieldedTokenType(value))
        } else {
            TokenType::Unshielded(UnshieldedTokenType(value))
        }
    }
}

impl FieldRepr for TokenType {
    fn field_repr<W: MemWrite<Fr>>(&self, writer: &mut W) {
        match self {
            TokenType::Shielded(raw) => {
                // `true` represents `Shielded`
                // First position holds the value
                // Second position is blank
                1u8.field_repr(writer);
                raw.0.field_repr(writer);
                BLANK_HASH.field_repr(writer);
            }
            TokenType::Unshielded(raw) => {
                // `false` represents `Unshielded`
                // First position is blank
                // Second position holds the value
                0u8.field_repr(writer);
                BLANK_HASH.field_repr(writer);
                raw.0.field_repr(writer);
            }
            TokenType::Dust => {
                // 2 represents `Dust`
                // Both positions blank
                2u8.field_repr(writer);
                BLANK_HASH.field_repr(writer);
                BLANK_HASH.field_repr(writer);
            }
        }
    }
    fn field_size(&self) -> usize {
        <HashOutput as FromFieldRepr>::FIELD_SIZE * 2 + <bool as FromFieldRepr>::FIELD_SIZE
    }
}

impl FromFieldRepr for TokenType {
    const FIELD_SIZE: usize =
        <HashOutput as FromFieldRepr>::FIELD_SIZE * 2 + <bool as FromFieldRepr>::FIELD_SIZE;

    fn from_field_repr(fields: &[Fr]) -> Option<Self> {
        // Ensure we have the correct number of fields
        if fields.len() != Self::FIELD_SIZE {
            return None;
        }

        // Read the discriminator boolean that tells us if this is Shielded, Unshielded, or Dust
        let variant = u8::from_field_repr(&fields[0..1])?;

        match variant {
            1 => {
                // For Shielded:
                // First comes the actual value
                let value = HashOutput::from_field_repr(
                    &fields[1..1 + <HashOutput as FromFieldRepr>::FIELD_SIZE],
                )?;

                // Then we verify the blank hash
                let blank = HashOutput::from_field_repr(
                    &fields[1 + <HashOutput as FromFieldRepr>::FIELD_SIZE..],
                )?;
                if blank != BLANK_HASH {
                    return None;
                }

                Some(TokenType::Shielded(ShieldedTokenType(value)))
            }
            0 => {
                // For Unshielded:
                // First we verify the blank hash
                let blank = HashOutput::from_field_repr(
                    &fields[1..1 + <HashOutput as FromFieldRepr>::FIELD_SIZE],
                )?;
                if blank != BLANK_HASH {
                    return None;
                }

                // Then we read the actual value
                let value = HashOutput::from_field_repr(
                    &fields[1 + <HashOutput as FromFieldRepr>::FIELD_SIZE..],
                )?;

                Some(TokenType::Unshielded(UnshieldedTokenType(value)))
            }
            2 => {
                // For Unshielded:
                // First we verify the blank hash
                let blank = HashOutput::from_field_repr(
                    &fields[1..1 + <HashOutput as FromFieldRepr>::FIELD_SIZE],
                )?;
                if blank != BLANK_HASH {
                    return None;
                }

                // Then we verify the blank hash
                let blank = HashOutput::from_field_repr(
                    &fields[1 + <HashOutput as FromFieldRepr>::FIELD_SIZE..],
                )?;
                if blank != BLANK_HASH {
                    return None;
                }

                Some(TokenType::Dust)
            }
            _ => None,
        }
    }
}

impl BinaryHashRepr for TokenType {
    fn binary_repr<W: MemWrite<u8>>(&self, writer: &mut W) {
        match self {
            TokenType::Unshielded(data) => {
                writer.write(&[UNSHIELDED_TAG]);
                writer.write(&data.0.0);
            }
            TokenType::Shielded(data) => {
                writer.write(&[SHIELDED_TAG]);
                writer.write(&data.0.0);
            }
            TokenType::Dust => {
                writer.write(&[DUST_TAG]);
            }
        }
    }

    fn binary_len(&self) -> usize {
        match self {
            TokenType::Dust => 1,
            _ => 32,
        }
    }
}

#[cfg(feature = "proptest")]
randomised_serialization_test!(TokenType);

pub const NIGHT: UnshieldedTokenType = UnshieldedTokenType(HashOutput([0u8; 32]));

#[derive(
    Default,
    Debug,
    Clone,
    Copy,
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
    Serialize,
    Deserialize,
    Dummy,
)]
#[storable(base)]
#[tag = "shielded-coin-info[v2]"]
#[cfg_attr(feature = "proptest", derive(Arbitrary))]
pub struct Info {
    pub nonce: Nonce,
    #[serde(rename = "type")]
    pub type_: ShieldedTokenType,
    pub value: u128,
}
tag_enforcement_test!(Info);

impl rand::distributions::Distribution<Info> for rand::distributions::Standard {
    fn sample<R: rand::Rng + ?Sized>(&self, rng: &mut R) -> Info {
        Info {
            nonce: rng.r#gen(),
            type_: rng.r#gen(),
            value: rng.r#gen(),
        }
    }
}

#[cfg(feature = "proptest")]
randomised_serialization_test!(Info);

impl Info {
    pub fn new<R: Rng + CryptoRng + ?Sized>(
        rng: &mut R,
        value: u128,
        type_: ShieldedTokenType,
    ) -> Self {
        Info {
            nonce: rng.r#gen(),
            value,
            type_,
        }
    }

    pub fn evolve_from(&self, domain_sep: &[u8], value: u128, type_: ShieldedTokenType) -> Self {
        Info {
            nonce: Nonce(upgrade_from_transient(transient_hash(&[
                Fr::from_le_bytes(domain_sep).expect("Domain sep should be in range for field"),
                degrade_to_transient(self.nonce.0),
            ]))),
            value,
            type_,
        }
    }

    pub fn commitment(&self, recipient: &Recipient) -> Commitment {
        let mut data = Vec::with_capacity(119);
        self.binary_repr(&mut data);
        match &recipient {
            Recipient::User(d) => (true, d.0).binary_repr(&mut data),
            Recipient::Contract(d) => (false, d.0).binary_repr(&mut data),
        }
        data.extend(b"mdn:cc");
        Commitment(persistent_hash(&data))
    }

    pub fn nullifier(&self, se: &SenderEvidence) -> Nullifier {
        let mut data = Vec::with_capacity(119);
        self.binary_repr(&mut data);
        match &se {
            SenderEvidence::User(d) => (true, d.0).binary_repr(&mut data),
            SenderEvidence::Contract(d) => (false, d.0).binary_repr(&mut data),
        }
        data.extend(b"mdn:cn");
        Nullifier(persistent_hash(&data))
    }

    pub fn qualify(&self, mt_index: u64) -> QualifiedInfo {
        QualifiedInfo {
            nonce: self.nonce,
            value: self.value,
            type_: self.type_,
            mt_index,
        }
    }
}

#[derive(
    Debug,
    Default,
    Clone,
    Copy,
    Hash,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    FieldRepr,
    FromFieldRepr,
    BinaryHashRepr,
    Serialize,
    Deserialize,
    Serializable,
    Storable,
    Dummy,
)]
#[storable(base)]
#[tag = "shielded-qualified-coin-info[v2]"]
#[cfg_attr(feature = "proptest", derive(Arbitrary))]
pub struct QualifiedInfo {
    pub nonce: Nonce,
    #[serde(rename = "type")]
    pub type_: ShieldedTokenType,
    pub value: u128,
    pub mt_index: u64,
}
tag_enforcement_test!(QualifiedInfo);

impl rand::distributions::Distribution<QualifiedInfo> for rand::distributions::Standard {
    fn sample<R: rand::Rng + ?Sized>(&self, rng: &mut R) -> QualifiedInfo {
        QualifiedInfo {
            nonce: rng.r#gen(),
            type_: rng.r#gen(),
            value: rng.r#gen(),
            mt_index: rng.r#gen(),
        }
    }
}

#[cfg(feature = "proptest")]
randomised_serialization_test!(QualifiedInfo);

impl From<&QualifiedInfo> for Info {
    fn from(qi: &QualifiedInfo) -> Info {
        Info {
            nonce: qi.nonce,
            value: qi.value,
            type_: qi.type_,
        }
    }
}

#[derive(
    Debug, Copy, Clone, Hash, PartialEq, Eq, PartialOrd, Ord, Serializable, Storable, Dummy,
)]
#[storable(base)]
#[tag = "public-address[v1]"]
pub enum PublicAddress {
    Contract(ContractAddress),
    User(UserAddress),
}
tag_enforcement_test!(PublicAddress);

impl Serialize for PublicAddress {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let (tag, raw) = self.into_tagged_tuple();
        let mut bytes_with_tag = [0u8; 33];
        bytes_with_tag[0] = tag;
        bytes_with_tag[1..].copy_from_slice(&raw.0);
        serializer.serialize_bytes(&bytes_with_tag)
    }
}

impl<'de> Deserialize<'de> for PublicAddress {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        pub(crate) struct PublicAddressVisitor;

        impl serde::de::Visitor<'_> for PublicAddressVisitor {
            type Value = PublicAddress;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a discriminator and a hash value")
            }

            fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                if v.len() != 33 {
                    return Err(E::invalid_length(v.len(), &self));
                }

                let tag = v[0];
                let mut data = [0u8; 32];
                data.copy_from_slice(&v[1..]);

                PublicAddress::from_tagged_tuple(tag, HashOutput(data))
                    .map_err(|_| E::unknown_variant(&tag.to_string(), &["0", "1"]))
            }
        }

        deserializer.deserialize_bytes(PublicAddressVisitor)
    }
}

impl rand::distributions::Distribution<PublicAddress> for rand::distributions::Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> PublicAddress {
        let is_contract: bool = rng.r#gen();
        let value: HashOutput = rng.r#gen();

        if is_contract {
            PublicAddress::Contract(ContractAddress(value))
        } else {
            PublicAddress::User(UserAddress(value))
        }
    }
}

pub const CONTRACT_TAG: u8 = 0;
pub const USER_TAG: u8 = 1;

impl Aligned for PublicAddress {
    fn alignment() -> Alignment {
        Alignment::concat([
            &bool::alignment(),
            &<[u8; 32]>::alignment(),
            &<[u8; 32]>::alignment(),
        ])
    }
}

impl From<PublicAddress> for Value {
    fn from(addr: PublicAddress) -> Value {
        Value(match addr {
            PublicAddress::Contract(addr) => vec![true.into(), addr.into(), ().into()],
            PublicAddress::User(addr) => vec![false.into(), ().into(), addr.into()],
        })
    }
}

impl TryFrom<&ValueSlice> for PublicAddress {
    type Error = InvalidBuiltinDecode;

    fn try_from(value: &ValueSlice) -> Result<PublicAddress, InvalidBuiltinDecode> {
        if value.0.len() == 3 {
            let is_left: bool = (&value.0[0]).try_into()?;
            if is_left {
                <()>::try_from(&value.0[2])?;
                Ok(PublicAddress::Contract((&value.0[1]).try_into()?))
            } else {
                <()>::try_from(&value.0[1])?;
                Ok(PublicAddress::User((&value.0[2]).try_into()?))
            }
        } else {
            Err(InvalidBuiltinDecode("PublicAddress"))
        }
    }
}

impl PublicAddress {
    pub fn into_tagged_tuple(self) -> (u8, HashOutput) {
        match self {
            PublicAddress::Contract(addr) => (CONTRACT_TAG, addr.0),
            PublicAddress::User(addr) => (USER_TAG, addr.0),
        }
    }

    pub fn from_tagged_tuple(disc: u8, hash_output: HashOutput) -> Result<Self, std::io::Error> {
        Ok(match disc {
            CONTRACT_TAG => PublicAddress::Contract(ContractAddress(hash_output)),
            USER_TAG => PublicAddress::User(UserAddress(hash_output)),
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Incorrect discriminant, expected 0 or 1, got {}", disc),
            ))?,
        })
    }

    pub fn into_inner(&self) -> &HashOutput {
        match self {
            PublicAddress::Contract(raw) => &raw.0,
            PublicAddress::User(raw) => &raw.0,
        }
    }

    pub fn new(shielded: bool, hash_output: HashOutput) -> PublicAddress {
        if shielded {
            PublicAddress::Contract(ContractAddress(hash_output))
        } else {
            PublicAddress::User(UserAddress(hash_output))
        }
    }
}

// This isn't really the right file, but it seems the best choice for now
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
    Dummy,
    Storable,
)]
#[storable(base)]
#[tag = "user-address[v1]"]
pub struct UserAddress(pub HashOutput);
tag_enforcement_test!(UserAddress);

impl rand::distributions::Distribution<UserAddress> for rand::distributions::Standard {
    fn sample<R: rand::Rng + ?Sized>(&self, rng: &mut R) -> UserAddress {
        UserAddress(rng.r#gen())
    }
}

impl From<VerifyingKey> for UserAddress {
    fn from(value: VerifyingKey) -> Self {
        UserAddress(persistent_hash(value.binary_vec().as_slice()))
    }
}

#[cfg(feature = "proptest")]
serialize::randomised_serialization_test!(ContractAddress);
hash_serde!(
    Nullifier,
    Commitment,
    Nonce,
    UnshieldedTokenType,
    ShieldedTokenType,
    PublicKey,
    SecretKey,
    UserAddress
);
