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

use crate::cost_model::CostModel;
use crate::error::TranscriptRejected;
use crate::ops::Op;
use crate::result_mode::ResultMode;
use crate::state::StateValue;
use crate::transcript::Transcript;
use crate::vm::run_program;
use crate::vm_value::{ValueStrength, VmValue};
use base_crypto::cost_model::RunningCost;
use base_crypto::fab::{Aligned, Alignment};
use base_crypto::fab::{InvalidBuiltinDecode, Value, ValueSlice};
use base_crypto::hash::HashOutput;
use base_crypto::time::Timestamp;
use coin_structure::coin::PublicAddress;
use coin_structure::coin::UserAddress;
use coin_structure::coin::{
    Commitment as CoinCommitment, Info as CoinInfo, Nullifier, QualifiedInfo as QualifiedCoinInfo,
    TokenType,
};
use coin_structure::coin::{ShieldedTokenType, UnshieldedTokenType};
use coin_structure::contract::ContractAddress;
use coin_structure::transfer::Recipient;
use derive_where::derive_where;
use hex::FromHexError;
use hex::{FromHex, ToHex};
use onchain_runtime_state::state::ChargedState;
use onchain_vm::error::OnchainProgramError;
use onchain_vm::result_mode::ResultModeVerify;
#[cfg(feature = "proptest")]
use proptest_derive::Arbitrary;
use rand::Rng;
use rand::distributions::Standard;
use rand::prelude::Distribution;
use serde::{
    de::{Deserialize, Deserializer},
    ser::{Serialize, Serializer},
};
use serialize::{self, Deserializable, Serializable, Tagged, tag_enforcement_test};
use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::ops::Deref;
use storage::arena::Sp;
use storage::db::DB;
use storage::storage::Map;
use storage::{Storable, arena::ArenaKey, storable::Loader};
use transient_crypto::curve::Fr;

// Need to: Convert to SerdeBlockContext / SerdeEffects

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SerdeBlockContext {
    seconds_since_epoch: u64,
    seconds_since_epoch_err: u32,
    parent_block_hash: String,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SerdeCallContext {
    own_address: String,
    seconds_since_epoch: u64,
    seconds_since_epoch_err: u32,
    parent_block_hash: String,
    caller: Option<SerdePublicAddress>,
    balance: HashMap<SerdeTokenType, u128>,
    com_indices: HashMap<String, u64>,
}

#[derive(serde::Serialize, serde::Deserialize, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
struct SerdeTokenType {
    tag: String,
    raw: Option<String>,
}

const SERDE_UNSHIELDED_TAG: &str = "unshielded";
const SERDE_SHIELDED_TAG: &str = "shielded";
const SERDE_DUST_TAG: &str = "dust";

#[derive(serde::Serialize, serde::Deserialize, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
struct SerdePublicAddress {
    tag: String,
    address: String,
}

const SERDE_CONTRACT_TAG: &str = "contract";
const SERDE_USER_TAG: &str = "user";

fn hex_from_tt(tt: TokenType) -> SerdeTokenType {
    let (variant, val) = match tt {
        TokenType::Unshielded(unshielded_token_type) => (
            SERDE_UNSHIELDED_TAG.to_string(),
            Some(unshielded_token_type.0),
        ),
        TokenType::Shielded(shielded_token_type) => {
            (SERDE_SHIELDED_TAG.to_string(), Some(shielded_token_type.0))
        }
        TokenType::Dust => (SERDE_DUST_TAG.to_string(), None),
    };

    SerdeTokenType {
        tag: variant,
        raw: val.map(|v| v.0.encode_hex()),
    }
}

fn tt_from_hex(serde_token_type: SerdeTokenType) -> Result<TokenType, std::io::Error> {
    let hash_output = serde_token_type
        .raw
        .map(|raw| Ok::<_, std::io::Error>(HashOutput(FromHex::from_hex(raw).map_err(err_conv)?)))
        .transpose()?;

    match (serde_token_type.tag.as_str(), hash_output) {
        (SERDE_UNSHIELDED_TAG, Some(hash_output)) => {
            Ok(TokenType::Unshielded(UnshieldedTokenType(hash_output)))
        }
        (SERDE_SHIELDED_TAG, Some(hash_output)) => {
            Ok(TokenType::Shielded(ShieldedTokenType(hash_output)))
        }
        (SERDE_SHIELDED_TAG, None) | (SERDE_UNSHIELDED_TAG, None) => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "expected raw data with tag {}, but got none",
                serde_token_type.tag
            ),
        )),
        (SERDE_DUST_TAG, Some(_)) => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "expected no raw data with tag {}, but got some",
                serde_token_type.tag
            ),
        )),
        (SERDE_DUST_TAG, None) => Ok(TokenType::Dust),
        _ => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "Incorrect discriminant, expected one of \"unshielded\", \"shielded\", or \"dust\"; got {}",
                serde_token_type.tag
            ),
        ))?,
    }
}

fn public_address_from_hex(
    serde_public_address: SerdePublicAddress,
) -> Result<PublicAddress, std::io::Error> {
    let mut address_bytes =
        &mut &<Vec<u8>>::from_hex(serde_public_address.address.as_bytes()).map_err(err_conv)?[..];

    Ok(match serde_public_address.tag.as_str() {
        SERDE_CONTRACT_TAG => {
            let addr = <ContractAddress as Deserializable>::deserialize(&mut address_bytes, 0)?;
            ensure_fully_deserialized(address_bytes)?;
            PublicAddress::Contract(addr)
        }
        SERDE_USER_TAG => {
            let addr = <UserAddress as Deserializable>::deserialize(&mut address_bytes, 0)?;
            ensure_fully_deserialized(address_bytes)?;
            PublicAddress::User(addr)
        }
        _ => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "Incorrect discriminant, expected \"contract\" or \"user\", got {}",
                serde_public_address.tag
            ),
        ))?,
    })
}

fn public_address_to_hex(public_address: PublicAddress) -> SerdePublicAddress {
    let mut addr_vec = Vec::new();

    let variant = match public_address {
        PublicAddress::Contract(contract_address) => {
            <ContractAddress as Serializable>::serialize(&contract_address, &mut addr_vec)
                .expect("In-memory serialization should succeed");
            SERDE_CONTRACT_TAG
        }
        PublicAddress::User(user_address) => {
            <UserAddress as Serializable>::serialize(&user_address, &mut addr_vec)
                .expect("In-memory serialization should succeed");
            SERDE_USER_TAG
        }
    };

    SerdePublicAddress {
        tag: variant.to_string(),
        address: addr_vec.encode_hex(),
    }
}

fn err_conv(err: FromHexError) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, err.to_string())
}

impl From<BlockContext> for SerdeBlockContext {
    fn from(ctxt: BlockContext) -> SerdeBlockContext {
        SerdeBlockContext {
            seconds_since_epoch: ctxt.tblock.to_secs(),
            seconds_since_epoch_err: ctxt.tblock_err,
            parent_block_hash: ctxt.parent_block_hash.0.encode_hex(),
        }
    }
}

impl TryFrom<SerdeBlockContext> for BlockContext {
    type Error = std::io::Error;

    fn try_from(ctxt: SerdeBlockContext) -> Result<BlockContext, std::io::Error> {
        let hash =
            <[u8; base_crypto::hash::PERSISTENT_HASH_BYTES]>::from_hex(ctxt.parent_block_hash)
                .map_err(err_conv)?;
        Ok(BlockContext {
            tblock: Timestamp::from_secs(ctxt.seconds_since_epoch),
            tblock_err: ctxt.seconds_since_epoch_err,
            parent_block_hash: HashOutput(hash),
        })
    }
}

impl<D: DB> From<CallContext<D>> for SerdeCallContext {
    fn from(ctxt: CallContext<D>) -> SerdeCallContext {
        let mut own_address_vec = Vec::new();
        <ContractAddress as Serializable>::serialize(&ctxt.own_address, &mut own_address_vec)
            .expect("In-memory serialization should succeed");
        SerdeCallContext {
            own_address: own_address_vec.encode_hex(),
            seconds_since_epoch: ctxt.tblock.to_secs(),
            seconds_since_epoch_err: ctxt.tblock_err,
            parent_block_hash: ctxt.parent_block_hash.0.encode_hex(),
            caller: ctxt.caller.map(public_address_to_hex),
            balance: ctxt
                .balance
                .iter()
                .map(|tt_x_val| (hex_from_tt(*tt_x_val.0.deref()), *tt_x_val.1.deref()))
                .collect(),
            com_indices: ctxt
                .com_indices
                .iter()
                .map(|(com, val)| (com.0.0.encode_hex(), *val))
                .collect(),
        }
    }
}

impl<D: DB> TryFrom<SerdeCallContext> for CallContext<D> {
    type Error = std::io::Error;

    fn try_from(ctxt: SerdeCallContext) -> Result<CallContext<D>, std::io::Error> {
        let block_hash =
            <[u8; base_crypto::hash::PERSISTENT_HASH_BYTES]>::from_hex(ctxt.parent_block_hash)
                .map_err(err_conv)?;
        let mut own_address_bytes =
            &mut &<Vec<u8>>::from_hex(ctxt.own_address.as_bytes()).map_err(err_conv)?[..];
        let own_address =
            <ContractAddress as Deserializable>::deserialize(&mut own_address_bytes, 0)?;
        ensure_fully_deserialized(own_address_bytes)?;

        let caller = ctxt.caller.map(public_address_from_hex).transpose()?;
        Ok(CallContext {
            own_address,
            tblock: Timestamp::from_secs(ctxt.seconds_since_epoch),
            tblock_err: ctxt.seconds_since_epoch_err,
            parent_block_hash: HashOutput(block_hash),
            caller,
            balance: ctxt
                .balance
                .into_iter()
                .map(|(tt, val)| Ok::<_, std::io::Error>((tt_from_hex(tt)?, val)))
                .collect::<Result<storage::storage::HashMap<TokenType, u128, D>, _>>()?,
            com_indices: ctxt
                .com_indices
                .into_iter()
                .map(|(com, val)| {
                    Ok::<_, std::io::Error>((
                        CoinCommitment(HashOutput(FromHex::from_hex(com).map_err(err_conv)?)),
                        val,
                    ))
                })
                .collect::<Result<Map<CoinCommitment, u64>, _>>()?,
        })
    }
}

#[derive_where(Clone, Debug, Default)]
pub struct CallContext<D: DB> {
    pub own_address: ContractAddress,
    pub tblock: Timestamp,
    pub tblock_err: u32,
    pub parent_block_hash: HashOutput,
    pub caller: Option<PublicAddress>,
    pub balance: storage::storage::HashMap<TokenType, u128, D>,
    pub com_indices: Map<CoinCommitment, u64>,
}

impl<D: DB> Serialize for CallContext<D> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let ser_effects: SerdeCallContext = self.clone().into();
        <SerdeCallContext as Serialize>::serialize(&ser_effects, serializer)
    }
}

impl<'de, DD: DB> Deserialize<'de> for CallContext<DD> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let ser_effects = <SerdeCallContext as Deserialize>::deserialize(deserializer)?;
        CallContext::try_from(ser_effects).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize, Serializable)]
#[tag = "block-context[v1]"]
#[serde(try_from = "SerdeBlockContext", into = "SerdeBlockContext")]
pub struct BlockContext {
    pub tblock: Timestamp,
    pub tblock_err: u32,
    pub parent_block_hash: HashOutput,
}
tag_enforcement_test!(BlockContext);

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SerdeEffects {
    claimed_nullifiers: HashSet<String>,
    claimed_shielded_receives: HashSet<String>,
    claimed_shielded_spends: HashSet<String>,
    claimed_contract_calls: HashSet<(u64, String, String, Fr)>,
    shielded_mints: HashMap<String, u64>,
    unshielded_mints: HashMap<String, u64>,
    unshielded_inputs: HashMap<SerdeTokenType, u128>,
    unshielded_outputs: HashMap<SerdeTokenType, u128>,
    claimed_unshielded_spends: HashMap<(SerdeTokenType, SerdePublicAddress), u128>,
}

impl<D: DB> From<Effects<D>> for SerdeEffects {
    fn from(eff: Effects<D>) -> SerdeEffects {
        SerdeEffects {
            claimed_nullifiers: eff
                .claimed_nullifiers
                .iter()
                .map(|n| n.0.0.encode_hex())
                .collect(),
            claimed_shielded_receives: eff
                .claimed_shielded_receives
                .iter()
                .map(|cm| cm.0.0.encode_hex())
                .collect(),
            claimed_shielded_spends: eff
                .claimed_shielded_spends
                .iter()
                .map(|cm| cm.0.0.encode_hex())
                .collect(),
            claimed_contract_calls: eff
                .claimed_contract_calls
                .iter()
                .map(|sp| {
                    let (seq, addr, ep_hash, comm_hash) = sp.deref().into_inner();
                    let mut addr_bytes = Vec::new();
                    Serializable::serialize(&addr, &mut addr_bytes)
                        .expect("In-memory serialization must succeed");
                    (
                        seq,
                        addr_bytes.encode_hex(),
                        ep_hash.0.encode_hex(),
                        comm_hash,
                    )
                })
                .collect(),
            shielded_mints: eff
                .shielded_mints
                .into_iter()
                .map(|(tt, val)| (tt.0.encode_hex(), val))
                .collect(),
            unshielded_mints: eff
                .unshielded_mints
                .into_iter()
                .map(|(tt, val)| (tt.0.encode_hex(), val))
                .collect(),
            unshielded_inputs: eff
                .unshielded_inputs
                .into_iter()
                .map(|(tt, val)| (hex_from_tt(tt), val))
                .collect(),
            unshielded_outputs: eff
                .unshielded_outputs
                .into_iter()
                .map(|(tt, val)| (hex_from_tt(tt), val))
                .collect(),
            claimed_unshielded_spends: eff
                .claimed_unshielded_spends
                .into_iter()
                .map(|(spends_key, val)| {
                    let (tt, addr) = spends_key.into_inner();
                    ((hex_from_tt(tt), public_address_to_hex(addr)), val)
                })
                .collect(),
        }
    }
}

impl<D: DB> TryFrom<SerdeEffects> for Effects<D> {
    type Error = std::io::Error;

    fn try_from(eff: SerdeEffects) -> Result<Effects<D>, std::io::Error> {
        Ok(Effects {
            claimed_nullifiers: eff
                .claimed_nullifiers
                .into_iter()
                .map(|n| Ok::<_, FromHexError>(Nullifier(HashOutput(FromHex::from_hex(n)?))))
                .collect::<Result<_, _>>()
                .map_err(err_conv)?,
            claimed_shielded_receives: eff
                .claimed_shielded_receives
                .into_iter()
                .map(|cm| Ok::<_, FromHexError>(CoinCommitment(HashOutput(FromHex::from_hex(cm)?))))
                .collect::<Result<_, _>>()
                .map_err(err_conv)?,
            claimed_shielded_spends: eff
                .claimed_shielded_spends
                .into_iter()
                .map(|cm| Ok::<_, FromHexError>(CoinCommitment(HashOutput(FromHex::from_hex(cm)?))))
                .collect::<Result<_, _>>()
                .map_err(err_conv)?,
            claimed_contract_calls: eff
                .claimed_contract_calls
                .into_iter()
                .map(|(seq, addr, ep_hash, comm_hash)| {
                    let addr_bytes: Vec<u8> = FromHex::from_hex(addr).map_err(err_conv)?;
                    Ok::<_, std::io::Error>(ClaimedContractCallsValue(
                        seq,
                        Deserializable::deserialize(&mut &addr_bytes[..], 0)?,
                        HashOutput(FromHex::from_hex(ep_hash).map_err(err_conv)?),
                        comm_hash,
                    ))
                })
                .collect::<Result<_, _>>()?,
            shielded_mints: eff
                .shielded_mints
                .into_iter()
                .map(|(tt, val)| Ok::<_, FromHexError>((HashOutput(FromHex::from_hex(tt)?), val)))
                .collect::<Result<_, _>>()
                .map_err(err_conv)?,
            unshielded_mints: eff
                .unshielded_mints
                .into_iter()
                .map(|(tt, val)| Ok::<_, FromHexError>((HashOutput(FromHex::from_hex(tt)?), val)))
                .collect::<Result<_, _>>()
                .map_err(err_conv)?,
            unshielded_inputs: eff
                .unshielded_inputs
                .into_iter()
                .map(|(tt, val)| Ok::<_, std::io::Error>((tt_from_hex(tt)?, val)))
                .collect::<Result<storage::storage::HashMap<TokenType, u128, D>, _>>()?,
            unshielded_outputs: eff
                .unshielded_outputs
                .into_iter()
                .map(|(tt, val)| Ok::<_, std::io::Error>((tt_from_hex(tt)?, val)))
                .collect::<Result<storage::storage::HashMap<TokenType, u128, D>, _>>()?,
            claimed_unshielded_spends:
                eff.claimed_unshielded_spends
                    .into_iter()
                    .map(|((tt, addr), val)| {
                        Ok::<_, std::io::Error>((
                            ClaimedUnshieldedSpendsKey(
                                tt_from_hex(tt)?,
                                public_address_from_hex(addr)?,
                            ),
                            val,
                        ))
                    })
                    .collect::<Result<
                        storage::storage::HashMap<ClaimedUnshieldedSpendsKey, u128, D>,
                        _,
                    >>()?,
        })
    }
}

#[derive(
    Clone, Debug, PartialEq, Eq, Serializable, Storable, Hash, serde::Serialize, serde::Deserialize,
)]
#[storable(base)]
#[tag = "contract-effects-claimed-unshielded-spends-key[v1]"]
pub struct ClaimedUnshieldedSpendsKey(pub TokenType, pub PublicAddress);
tag_enforcement_test!(ClaimedUnshieldedSpendsKey);

impl ClaimedUnshieldedSpendsKey {
    pub fn into_inner(&self) -> (TokenType, PublicAddress) {
        (self.0, self.1)
    }

    pub fn from_inner(tt: TokenType, addr: PublicAddress) -> ClaimedUnshieldedSpendsKey {
        ClaimedUnshieldedSpendsKey(tt, addr)
    }
}

impl From<ClaimedUnshieldedSpendsKey> for Value {
    fn from(val: ClaimedUnshieldedSpendsKey) -> Value {
        let v1: Value = val.0.into();
        let v2: Value = val.1.into();
        Value::concat([&v1, &v2])
    }
}

impl TryFrom<&ValueSlice> for ClaimedUnshieldedSpendsKey {
    type Error = InvalidBuiltinDecode;

    fn try_from(value: &ValueSlice) -> Result<ClaimedUnshieldedSpendsKey, InvalidBuiltinDecode> {
        if value.0.len() == 6 {
            Ok(ClaimedUnshieldedSpendsKey(
                (&value[0..3]).try_into()?,
                (&value[3..6]).try_into()?,
            ))
        } else {
            Err(InvalidBuiltinDecode("ClaimedUnshieldedSpendsKey"))
        }
    }
}

impl Aligned for ClaimedUnshieldedSpendsKey {
    fn alignment() -> Alignment {
        Alignment::concat([&TokenType::alignment(), &PublicAddress::alignment()])
    }
}

impl Distribution<ClaimedUnshieldedSpendsKey> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> ClaimedUnshieldedSpendsKey {
        ClaimedUnshieldedSpendsKey(rng.r#gen(), rng.r#gen())
    }
}

#[derive(
    Clone,
    Debug,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serializable,
    Storable,
    Hash,
    serde::Serialize,
    serde::Deserialize,
)]
#[storable(base)]
#[tag = "contract-effects-claimed-contract-calls-value[v1]"]
pub struct ClaimedContractCallsValue(pub u64, pub ContractAddress, pub HashOutput, pub Fr);
tag_enforcement_test!(ClaimedContractCallsValue);

impl ClaimedContractCallsValue {
    pub fn into_inner(&self) -> (u64, ContractAddress, HashOutput, Fr) {
        (self.0, self.1, self.2, self.3)
    }

    pub fn from_inner(
        pos: u64,
        addr: ContractAddress,
        hash: HashOutput,
        rnd: Fr,
    ) -> ClaimedContractCallsValue {
        ClaimedContractCallsValue(pos, addr, hash, rnd)
    }
}

impl From<ClaimedContractCallsValue> for Value {
    fn from(val: ClaimedContractCallsValue) -> Value {
        let v1: Value = val.0.into();
        let v2: Value = val.1.into();
        let v3: Value = val.2.into();
        let v4: Value = val.3.into();
        Value::concat([&v1, &v2, &v3, &v4])
    }
}

impl TryFrom<&ValueSlice> for ClaimedContractCallsValue {
    type Error = InvalidBuiltinDecode;

    fn try_from(value: &ValueSlice) -> Result<ClaimedContractCallsValue, InvalidBuiltinDecode> {
        if value.0.len() == 4 {
            Ok(ClaimedContractCallsValue(
                (&value.0[0]).try_into()?,
                (&value.0[1]).try_into()?,
                (&value.0[2]).try_into()?,
                (&value.0[3]).try_into()?,
            ))
        } else {
            Err(InvalidBuiltinDecode("ClaimedContractCallsValue"))
        }
    }
}

impl Aligned for ClaimedContractCallsValue {
    fn alignment() -> Alignment {
        Alignment::concat([
            &u64::alignment(),
            &ContractAddress::alignment(),
            &HashOutput::alignment(),
            &Fr::alignment(),
        ])
    }
}

impl Distribution<ClaimedContractCallsValue> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> ClaimedContractCallsValue {
        ClaimedContractCallsValue(rng.r#gen(), rng.r#gen(), rng.r#gen(), rng.r#gen())
    }
}

#[derive(Storable)]
#[derive_where(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "proptest", derive(Arbitrary))]
#[storable(db = D)]
#[tag = "contract-effects[v2]"]
pub struct Effects<D: DB> {
    pub claimed_nullifiers: storage::storage::HashSet<Nullifier, D>,
    pub claimed_shielded_receives: storage::storage::HashSet<CoinCommitment, D>,
    pub claimed_shielded_spends: storage::storage::HashSet<CoinCommitment, D>,
    pub claimed_contract_calls: storage::storage::HashSet<ClaimedContractCallsValue, D>,
    pub shielded_mints: storage::storage::HashMap<HashOutput, u64, D>,
    pub unshielded_mints: storage::storage::HashMap<HashOutput, u64, D>,
    pub unshielded_inputs: storage::storage::HashMap<TokenType, u128, D>,
    pub unshielded_outputs: storage::storage::HashMap<TokenType, u128, D>,
    pub claimed_unshielded_spends: storage::storage::HashMap<ClaimedUnshieldedSpendsKey, u128, D>,
}
tag_enforcement_test!(Effects<InMemoryDB>);

impl<D: DB> Serialize for Effects<D> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let ser_effects: SerdeEffects = self.clone().into();
        <SerdeEffects as Serialize>::serialize(&ser_effects, serializer)
    }
}

impl<'de, DD: DB> Deserialize<'de> for Effects<DD> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let ser_effects = <SerdeEffects as Deserialize>::deserialize(deserializer)?;
        Effects::try_from(ser_effects).map_err(serde::de::Error::custom)
    }
}

impl<D: DB> rand::distributions::Distribution<Effects<D>> for rand::distributions::Standard {
    fn sample<R: rand::Rng + ?Sized>(&self, rng: &mut R) -> Effects<D> {
        Effects {
            claimed_nullifiers: vec![rng.r#gen(); 5].into_iter().collect(),
            claimed_shielded_receives: vec![rng.r#gen(); 5].into_iter().collect(),
            claimed_shielded_spends: vec![rng.r#gen(); 5].into_iter().collect(),
            claimed_contract_calls: vec![rng.r#gen(); 5].into_iter().collect(),
            shielded_mints: vec![rng.r#gen(); 5].into_iter().collect(),
            unshielded_mints: vec![rng.r#gen(); 5].into_iter().collect(),
            unshielded_inputs: vec![rng.r#gen(); 5].into_iter().collect(),
            unshielded_outputs: vec![rng.r#gen(); 5].into_iter().collect(),
            claimed_unshielded_spends: vec![rng.r#gen(); 5].into_iter().collect(),
        }
    }
}

#[cfg(all(test, feature = "proptest"))]
use storage::db::InMemoryDB;
#[cfg(feature = "proptest")]
serialize::randomised_serialization_test!(Effects<InMemoryDB>);

impl<'a, D: DB> From<&'a Effects<D>> for VmValue<D> {
    fn from(eff: &'a Effects<D>) -> VmValue<D> {
        VmValue::new(
            ValueStrength::Weak,
            StateValue::Array(
                vec![
                    StateValue::Map(
                        eff.claimed_nullifiers
                            .iter()
                            .map(|k| ((**k).into(), StateValue::Null))
                            .collect(),
                    ),
                    StateValue::Map(
                        eff.claimed_shielded_receives
                            .iter()
                            .map(|k| ((**k).into(), StateValue::Null))
                            .collect(),
                    ),
                    StateValue::Map(
                        eff.claimed_shielded_spends
                            .iter()
                            .map(|k| ((**k).into(), StateValue::Null))
                            .collect(),
                    ),
                    StateValue::Map(
                        eff.claimed_contract_calls
                            .iter()
                            .map(|sp_item| {
                                let ref value_sp = *sp_item;
                                let value: ClaimedContractCallsValue =
                                    (*(*value_sp).clone()).clone();
                                (value.into(), StateValue::Null)
                            })
                            .collect(),
                    ),
                    StateValue::Map(
                        eff.shielded_mints
                            .iter()
                            .map(|x| ((*x.0).into(), StateValue::Cell(Sp::new((*(x.1)).into()))))
                            .collect(),
                    ),
                    StateValue::Map(
                        eff.unshielded_mints
                            .iter()
                            .map(|x| ((*x.0).into(), StateValue::Cell(Sp::new((*(x.1)).into()))))
                            .collect(),
                    ),
                    StateValue::Map(
                        eff.unshielded_inputs
                            .iter()
                            .map(|x| ((*x.0).into(), StateValue::Cell(Sp::new((*(x.1)).into()))))
                            .collect(),
                    ),
                    StateValue::Map(
                        eff.unshielded_outputs
                            .iter()
                            .map(|x| ((*x.0).into(), StateValue::Cell(Sp::new((*(x.1)).into()))))
                            .collect(),
                    ),
                    StateValue::Map(
                        eff.claimed_unshielded_spends
                            .iter()
                            .map(|sp_item| {
                                let (ref key_sp, ref value_sp) = *sp_item;
                                let key: ClaimedUnshieldedSpendsKey = (*(*key_sp).clone()).clone();
                                let value: u128 = *(*value_sp).clone();
                                (key.into(), StateValue::Cell(Sp::new(value.into())))
                            })
                            .collect(),
                    ),
                ]
                .into(),
            ),
        )
    }
}

impl<D: DB> TryFrom<VmValue<D>> for Effects<D> {
    type Error = TranscriptRejected<D>;

    fn try_from(val: VmValue<D>) -> Result<Effects<D>, TranscriptRejected<D>> {
        fn map_from<
            K: Eq
                + Hash
                + for<'a> TryFrom<&'a ValueSlice, Error = InvalidBuiltinDecode>
                + Serializable
                + Storable<D>,
            V: Default + for<'a> TryFrom<&'a ValueSlice, Error = InvalidBuiltinDecode> + Storable<D>,
            D: DB,
        >(
            st: &StateValue<D>,
        ) -> Result<storage::storage::HashMap<K, V, D>, TranscriptRejected<D>> {
            if let StateValue::Map(m) = st {
                Ok(m.iter()
                    .map(|kv| {
                        let v = match *kv.1 {
                            StateValue::Cell(ref v) => (&*v.value).try_into()?,
                            StateValue::Null => V::default(),
                            _ => return Err(TranscriptRejected::EffectDecodeError),
                        };
                        Ok::<_, TranscriptRejected<D>>((
                            (&**AsRef::<Value>::as_ref(&(*kv.0))).try_into()?,
                            v,
                        ))
                    })
                    .collect::<Result<_, _>>()?)
            } else {
                Err(TranscriptRejected::EffectDecodeError)
            }
        }
        if let StateValue::Array(arr) = &val.value {
            if arr.len() == 9 {
                return Ok(Effects {
                    claimed_nullifiers: map_from::<Nullifier, (), D>(arr.get(0).unwrap())?
                        .iter()
                        .map(|x| *x.0)
                        .collect(),
                    claimed_shielded_receives: map_from::<CoinCommitment, (), D>(
                        arr.get(1).unwrap(),
                    )?
                    .iter()
                    .map(|x| *x.0)
                    .collect(),
                    claimed_shielded_spends: map_from::<CoinCommitment, (), D>(
                        arr.get(2).unwrap(),
                    )?
                    .iter()
                    .map(|x| *x.0)
                    .collect(),
                    claimed_contract_calls: map_from::<ClaimedContractCallsValue, (), D>(
                        arr.get(3).unwrap(),
                    )?
                    .iter()
                    .map(|x| (*x.0).clone())
                    .collect(),
                    shielded_mints: map_from(arr.get(4).unwrap())?,
                    unshielded_mints: map_from(arr.get(5).unwrap())?,
                    unshielded_inputs: map_from(arr.get(6).unwrap())?,
                    unshielded_outputs: map_from(arr.get(7).unwrap())?,
                    claimed_unshielded_spends: map_from(arr.get(8).unwrap())?,
                });
            }
        }
        Err(TranscriptRejected::EffectDecodeError)
    }
}

#[derive_where(Clone, Debug)]
pub struct QueryContext<D: DB> {
    pub state: ChargedState<D>,
    pub effects: Effects<D>,
    // TODO WG
    // Either this (`address`) should be removed, or `own_address` should be removed
    // from `CallContext` and `call_context` should be optional.
    pub address: ContractAddress,
    pub call_context: CallContext<D>,
}

impl<D: DB> From<&QueryContext<D>> for VmValue<D> {
    fn from(context: &QueryContext<D>) -> VmValue<D> {
        VmValue::new(
            ValueStrength::Weak,
            StateValue::Array(
                vec![
                    StateValue::Cell(Sp::new(context.address.into())),
                    StateValue::Map(
                        context
                            .call_context
                            .com_indices
                            .iter()
                            .map(|(k, v)| {
                                (k.into(), StateValue::Cell(Sp::new((*v.clone()).into())))
                            })
                            .collect(),
                    ),
                    StateValue::Cell(Sp::new(context.call_context.tblock.into())),
                    StateValue::Cell(Sp::new(context.call_context.tblock_err.into())),
                    StateValue::Cell(Sp::new(context.call_context.parent_block_hash.into())),
                    StateValue::Map(
                        context
                            .call_context
                            .balance
                            .iter()
                            .map(|tt_x_amount| {
                                (
                                    (*tt_x_amount.0.deref()).into(),
                                    StateValue::Cell(Sp::new((*tt_x_amount.1.deref()).into())),
                                )
                            })
                            .collect(),
                    ),
                    match context.call_context.caller {
                        Some(x) => StateValue::Cell(Sp::new(x.into())),
                        None => StateValue::Null,
                    },
                ]
                .into(),
            ),
        )
    }
}

#[derive(Debug)]
pub struct QueryResults<M: ResultMode<D>, D: DB> {
    pub context: QueryContext<D>,
    pub events: Vec<M::Event>,
    pub gas_cost: RunningCost,
}

impl<D: DB> QueryContext<D> {
    pub fn new(state: ChargedState<D>, address: ContractAddress) -> Self {
        QueryContext {
            state,
            address,
            effects: Effects::default(),
            call_context: CallContext::default(),
        }
    }

    pub fn qualify(&self, coin: &CoinInfo) -> Option<QualifiedCoinInfo> {
        self.call_context
            .com_indices
            .get(&coin.commitment(&Recipient::Contract(self.address)))
            .map(|idx| coin.qualify(*idx))
    }

    #[instrument(skip(self, cost_model))]
    pub fn query<M: ResultMode<D>>(
        &self,
        query: &[Op<M, D>],
        gas_limit: Option<RunningCost>,
        cost_model: &CostModel,
    ) -> Result<QueryResults<M, D>, TranscriptRejected<D>> {
        let mut state: Self = (*self).clone();
        let mut res = run_program(&self.to_vm_stack(), query, gas_limit, cost_model)?;
        if res.stack.len() != 3 {
            return Err(TranscriptRejected::FinalStackWrongLength);
        }
        let new_state = match res.stack.pop().unwrap() {
            VmValue {
                strength: ValueStrength::Strong,
                value,
            } => value,
            VmValue {
                strength: ValueStrength::Weak,
                ..
            } => return Err(TranscriptRejected::WeakStateReturned),
        };
        state.effects = res.stack.pop().unwrap().try_into()?;

        let (new_charged_state, state_cost) = state.state.update(
            new_state,
            |writes, deletes| {
                RunningCost::compute(
                    cost_model.gc_rcmap_constant
                        + cost_model.gc_rcmap_coeff_keys_removed_size * deletes
                        + cost_model.update_rcmap_constant
                        + cost_model.update_rcmap_coeff_keys_added_size * writes
                        + cost_model.get_writes_constant
                        + cost_model.get_writes_coeff_keys_added_size * writes,
                )
            },
            |budget| {
                (budget.compute_time / cost_model.gc_rcmap_coeff_keys_removed_size)
                    .into_atomic_units(1) as usize
            },
        );
        state.state = new_charged_state;
        let gas_cost = res.gas_cost + state_cost;
        if let Some(gas_limit) = gas_limit {
            if gas_cost > gas_limit {
                // TODO?: return a more specific error, explaining that gas
                // limit was exceeded by write+delete vs by cpu during vm eval?
                return Err(TranscriptRejected::Execution(OnchainProgramError::OutOfGas));
            }
        }

        trace!("transcript application successful");
        Ok(QueryResults {
            context: state,
            events: res.events,
            gas_cost,
        })
    }

    pub fn to_vm_stack(&self) -> Vec<VmValue<D>> {
        vec![
            self.into(),
            (&self.effects).into(),
            VmValue::new(ValueStrength::Strong, (*self.state.get()).clone()),
        ]
    }

    #[instrument(skip(self, cost_model))]
    pub fn run_transcript(
        &self,
        transcript: &Transcript<D>,
        cost_model: &CostModel,
    ) -> Result<QueryResults<ResultModeVerify, D>, TranscriptRejected<D>> {
        Ok(self.query(
            &Vec::from(&transcript.program),
            Some(transcript.gas),
            cost_model,
        )?)
    }
}

fn ensure_fully_deserialized(data: &[u8]) -> Result<(), std::io::Error> {
    if data.len() != 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Not all bytes read, {} bytes remaining", data.len()),
        ));
    }
    Ok(())
}
