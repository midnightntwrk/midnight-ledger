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

use crate::{from_value, from_value_hex_ser, to_value, to_value_hex_ser};
use base_crypto::hash::HashOutput;
use coin_structure::coin::{
    Info as ShieldedCoinInfo, Nonce, PublicKey as CoinPublicKey,
    QualifiedInfo as QualifiedShieldedCoinInfo, ShieldedTokenType, TokenType, UnshieldedTokenType,
    UserAddress,
};
use coin_structure::contract::ContractAddress;
use js_sys::Uint8Array;
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

#[derive(Serialize, Deserialize)]
struct ShieldedCoinInfoEncoded {
    #[serde(with = "serde_bytes")]
    color: Vec<u8>,
    #[serde(with = "serde_bytes")]
    nonce: Vec<u8>,
    value: u128,
}

impl TryFrom<ShieldedCoinInfoEncoded> for ShieldedCoinInfo {
    type Error = &'static str;
    fn try_from(value: ShieldedCoinInfoEncoded) -> Result<Self, Self::Error> {
        Ok(ShieldedCoinInfo {
            type_: ShieldedTokenType(HashOutput(
                value
                    .color
                    .try_into()
                    .map_err(|_| "failed to decode type")?,
            )),
            nonce: Nonce(HashOutput(
                value
                    .nonce
                    .try_into()
                    .map_err(|_| "failed to decode nonce")?,
            )),
            value: value.value,
        })
    }
}

impl From<ShieldedCoinInfo> for ShieldedCoinInfoEncoded {
    fn from(value: ShieldedCoinInfo) -> Self {
        ShieldedCoinInfoEncoded {
            color: value.type_.0.0.to_vec(),
            nonce: value.nonce.0.0.to_vec(),
            value: value.value,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct QualifiedShieldedCoinInfoEncoded {
    #[serde(with = "serde_bytes")]
    color: Vec<u8>,
    #[serde(with = "serde_bytes")]
    nonce: Vec<u8>,
    value: u128,
    mt_index: u64,
}

impl TryFrom<QualifiedShieldedCoinInfoEncoded> for QualifiedShieldedCoinInfo {
    type Error = &'static str;
    fn try_from(value: QualifiedShieldedCoinInfoEncoded) -> Result<Self, Self::Error> {
        Ok(QualifiedShieldedCoinInfo {
            nonce: Nonce(HashOutput(
                value
                    .nonce
                    .try_into()
                    .map_err(|_| "failed to decode nonce")?,
            )),
            type_: ShieldedTokenType(HashOutput(
                value
                    .color
                    .try_into()
                    .map_err(|_| "failed to decode type")?,
            )),
            value: value.value,
            mt_index: value.mt_index,
        })
    }
}

impl From<QualifiedShieldedCoinInfo> for QualifiedShieldedCoinInfoEncoded {
    fn from(value: QualifiedShieldedCoinInfo) -> Self {
        QualifiedShieldedCoinInfoEncoded {
            color: value.type_.0.0.to_vec(),
            nonce: value.nonce.0.0.to_vec(),
            value: value.value,
            mt_index: value.mt_index,
        }
    }
}

#[wasm_bindgen(js_name = "encodeShieldedCoinInfo")]
pub fn encode_shielded_coin_info(coin: JsValue) -> Result<JsValue, JsError> {
    let coin: ShieldedCoinInfo = value_to_shielded_coininfo(coin)?;
    Ok(to_value(&ShieldedCoinInfoEncoded::from(coin))?)
}

#[wasm_bindgen(js_name = "encodeQualifiedShieldedCoinInfo")]
pub fn encode_qualified_shielded_coin_info(coin: JsValue) -> Result<JsValue, JsError> {
    let coin: QualifiedShieldedCoinInfo = value_to_qualified_shielded_coininfo(coin)?;
    Ok(to_value(&QualifiedShieldedCoinInfoEncoded::from(coin))?)
}

#[wasm_bindgen(js_name = "decodeShieldedCoinInfo")]
pub fn decode_shielded_coin_info(coin: JsValue) -> Result<JsValue, JsError> {
    let coin: ShieldedCoinInfoEncoded = from_value(coin)?;
    shielded_coininfo_to_value(&coin.try_into().map_err(JsError::new)?)
}

#[wasm_bindgen(js_name = "decodeQualifiedShieldedCoinInfo")]
pub fn decode_qualified_shielded_coin_info(coin: JsValue) -> Result<JsValue, JsError> {
    let coin: QualifiedShieldedCoinInfoEncoded = from_value(coin)?;
    qualified_shielded_coininfo_to_value(&coin.try_into().map_err(JsError::new)?)
}

#[wasm_bindgen(js_name = "encodeRawTokenType")]
pub fn encode_raw_token_type(tt: &str) -> Result<Uint8Array, JsError> {
    let tt = ShieldedTokenType(from_value_hex_ser(tt)?);
    Ok(Uint8Array::from(&tt.0.0[..]))
}

#[wasm_bindgen(js_name = "decodeRawTokenType")]
pub fn decode_raw_token_type(tt: Uint8Array) -> Result<String, JsError> {
    let tt = ShieldedTokenType(HashOutput(tt.to_vec().try_into().map_err(
        |vec: Vec<u8>| {
            JsError::new(&format!(
                "invalid length for TokenType: {} (expected 32)",
                vec.len()
            ))
        },
    )?));
    to_value_hex_ser(&tt.0)
}

#[wasm_bindgen(js_name = "encodeContractAddress")]
pub fn encode_contract_address(addr: &str) -> Result<Uint8Array, JsError> {
    let addr: ContractAddress = from_value_hex_ser(addr)?;
    Ok(Uint8Array::from(&addr.0.0[..]))
}

#[wasm_bindgen(js_name = "decodeContractAddress")]
pub fn decode_contract_address(addr: Uint8Array) -> Result<String, JsError> {
    let addr = ContractAddress(HashOutput(addr.to_vec().try_into().map_err(
        |vec: Vec<u8>| {
            JsError::new(&format!(
                "invalid length for ContractAddress: {} (expected 32)",
                vec.len()
            ))
        },
    )?));
    to_value_hex_ser(&addr)
}

#[wasm_bindgen(js_name = "encodeUserAddress")]
pub fn encode_user_address(addr: &str) -> Result<Uint8Array, JsError> {
    let addr: UserAddress = from_value_hex_ser(addr)?;
    Ok(Uint8Array::from(&addr.0.0[..]))
}

#[wasm_bindgen(js_name = "decodeUserAddress")]
pub fn decode_user_address(addr: Uint8Array) -> Result<String, JsError> {
    let addr = UserAddress(HashOutput(addr.to_vec().try_into().map_err(
        |vec: Vec<u8>| {
            JsError::new(&format!(
                "invalid length for UserAddress: {} (expected 32)",
                vec.len()
            ))
        },
    )?));
    to_value_hex_ser(&addr)
}

#[wasm_bindgen(js_name = "encodeCoinPublicKey")]
pub fn encode_coin_public_key(pk: &str) -> Result<Uint8Array, JsError> {
    let pk: CoinPublicKey = from_value_hex_ser(pk)?;
    Ok(Uint8Array::from(&pk.0.0[..]))
}

#[wasm_bindgen(js_name = "decodeCoinPublicKey")]
pub fn decode_coin_public_key(pk: Uint8Array) -> Result<String, JsError> {
    let tt = CoinPublicKey(HashOutput(pk.to_vec().try_into().map_err(
        |vec: Vec<u8>| {
            JsError::new(&format!(
                "invalid length for CoinPublicKey: {} (expected 32)",
                vec.len()
            ))
        },
    )?));
    to_value_hex_ser(&tt)
}

#[derive(Serialize, Deserialize)]
struct PreShieldedCoinInfo {
    #[serde(rename = "type")]
    type_: String,
    nonce: String,
    value: u128,
}

#[derive(Serialize, Deserialize)]
struct PreQualifiedShieldedCoinInfo {
    #[serde(rename = "type")]
    type_: String,
    nonce: String,
    value: u128,
    mt_index: u64,
}

#[derive(Serialize, Deserialize)]
struct PreTokenType {
    tag: String,
    raw: Option<String>,
}

pub fn value_to_shielded_coininfo(value: JsValue) -> Result<ShieldedCoinInfo, JsError> {
    let pre: PreShieldedCoinInfo = from_value(value)?;
    Ok(ShieldedCoinInfo {
        type_: ShieldedTokenType(from_value_hex_ser(&pre.type_)?),
        nonce: from_value_hex_ser(&pre.nonce)?,
        value: pre.value,
    })
}

pub fn value_to_qualified_shielded_coininfo(
    value: JsValue,
) -> Result<QualifiedShieldedCoinInfo, JsError> {
    let pre: PreQualifiedShieldedCoinInfo = from_value(value)?;
    Ok(QualifiedShieldedCoinInfo {
        type_: ShieldedTokenType(from_value_hex_ser(&pre.type_)?),
        nonce: from_value_hex_ser(&pre.nonce)?,
        value: pre.value,
        mt_index: pre.mt_index,
    })
}

pub fn shielded_coininfo_to_value(coin: &ShieldedCoinInfo) -> Result<JsValue, JsError> {
    Ok(to_value(&PreShieldedCoinInfo {
        type_: to_value_hex_ser(&coin.type_.0)?,
        nonce: to_value_hex_ser(&coin.nonce)?,
        value: coin.value,
    })?)
}

pub fn qualified_shielded_coininfo_to_value(
    coin: &QualifiedShieldedCoinInfo,
) -> Result<JsValue, JsError> {
    Ok(to_value(&PreQualifiedShieldedCoinInfo {
        type_: to_value_hex_ser(&coin.type_.0)?,
        nonce: to_value_hex_ser(&coin.nonce)?,
        value: coin.value,
        mt_index: coin.mt_index,
    })?)
}

pub fn token_type_to_value(token_type: &TokenType) -> Result<JsValue, JsError> {
    Ok(to_value(&match token_type {
        TokenType::Shielded(value) => PreTokenType {
            tag: String::from("shielded"),
            raw: Some(to_value_hex_ser(&value.0)?),
        },
        TokenType::Unshielded(value) => PreTokenType {
            tag: String::from("unshielded"),
            raw: Some(to_value_hex_ser(&value.0)?),
        },
        TokenType::Dust => PreTokenType {
            tag: String::from("dust"),
            raw: None,
        },
    })?)
}

pub fn value_to_token_type(value: JsValue) -> Result<TokenType, JsError> {
    let pre: PreTokenType = from_value(value)?;
    Ok(match (pre.tag.as_str(), pre.raw.as_ref()) {
        ("shielded", Some(raw)) => TokenType::Shielded(ShieldedTokenType(from_value_hex_ser(raw)?)),
        ("unshielded", Some(raw)) => {
            TokenType::Unshielded(UnshieldedTokenType(from_value_hex_ser(raw)?))
        }
        ("dust", None) => TokenType::Dust,
        ("dust", Some(_)) => Err(JsError::new("Expected no data for dust token type"))?,
        ("shielded", None) | ("unshielded", None) => Err(JsError::new(&format!(
            "Expected data for {} token type",
            pre.tag.as_str()
        )))?,
        (tag, _) => Err(JsError::new(&format!("Unknown token type tag: {tag}")))?,
    })
}
