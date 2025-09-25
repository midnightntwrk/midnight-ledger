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
// limitations under the License

use coin_structure::coin::Info as ShieldedCoinInfo;
use serde::{Deserialize, Serialize};
use serialize::{Deserializable, Serializable};
use std::io::Read;
use transient_crypto::curve::Fr;

pub fn to_value<T: Serialize + ?Sized>(value: &T) -> Result<String, serde_json::Error> {
    serde_json::to_string(value)
}

fn from_hex_ser_checked<T: Deserializable, R: Read>(mut bytes: R) -> Result<T, std::io::Error> {
    let value = T::deserialize(&mut bytes, 0)?;
    Ok(value)
}

pub fn from_hex_ser<T: Deserializable>(hex: &str) -> Result<T, String> {
    let bytes = hex::decode(hex).map_err(|e| format!("Failed to decode hex: {}", e))?;
    from_hex_ser_checked::<T, _>(&bytes[..])
        .map_err(|e| format!("Failed to deserialize: {}", e))
}

pub fn to_value_hex_ser<T: Serializable>(value: &T) -> Result<String, String> {
    let mut bytes = Vec::new();
    value.serialize(&mut bytes)
        .map_err(|e| format!("Failed to serialize: {}", e))?;
    Ok(hex::encode(bytes))
}

pub fn from_value<T: for<'de> Deserialize<'de>>(value: String) -> Result<T, String> {
    serde_json::from_str(&value)
        .map_err(|e| format!("Failed to deserialize JSON: {}", e))
}

pub fn shielded_coininfo_to_value(coin_info: &ShieldedCoinInfo) -> Result<String, serde_json::Error> {
    Ok(to_value(coin_info)?)
}

pub fn value_to_shielded_coininfo(value: String) -> Result<ShieldedCoinInfo, String> {
    from_value(value)
}

pub fn bigint_to_fr(bigint: String) -> Result<Fr, String> {
    // Parse the bigint string and convert to Fr
    // This is a simplified conversion - you may need to implement proper bigint parsing
    let value = u64::from_str_radix(&bigint, 10)
        .map_err(|e| format!("Failed to parse bigint: {}", e))?;
    Ok(Fr::from(value))
}

pub fn token_type_to_value<T: Serialize>(token_type: &T) -> Result<String, serde_json::Error> {
    Ok(to_value(token_type)?)
}
