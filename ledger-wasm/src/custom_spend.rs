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

//! The JS forms of a shielded spend's type preimage and guarded spend (the
//! custom spend logic MIP).

use crate::conversions::{from_hex_ser, value_to_shielded_coininfo};
use base_crypto::hash::HashOutput;
use coin_structure::contract::ContractAddress;
use coin_structure::custom_spend::{ShieldedTokenPreimage, SpendRule};
use hex::{FromHex, ToHex};
use serde::Deserialize;
use serde_wasm_bindgen::from_value;
use wasm_bindgen::prelude::*;
use zswap::{GuardProof, GuardedSpend, SpendRecipient};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PreShieldedTokenPreimage {
    #[serde(with = "serde_bytes")]
    domain_sep: Vec<u8>,
    contract: String,
    spend_rule: Option<String>,
}

#[derive(Deserialize)]
#[serde(tag = "tag", rename_all = "camelCase")]
enum PreSpendRecipient {
    #[serde(rename_all = "camelCase")]
    User {
        coin_public_key: String,
        encryption_public_key: String,
    },
    Contract { address: String },
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PreGuardProof {
    #[serde(with = "serde_bytes")]
    verifier_key: Vec<u8>,
    #[serde(with = "serde_bytes")]
    proof: Vec<u8>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PreGuardedSpend {
    recipient: PreSpendRecipient,
    guard: Option<PreGuardProof>,
}

fn spend_rule_from_hex(rule_hex: &str) -> Result<SpendRule, JsError> {
    let bytes: Vec<u8> = FromHex::from_hex(rule_hex)
        .map_err(|_| JsError::new("Expected hex-encoded spend rule"))?;
    serialize::tagged_deserialize(&mut &bytes[..])
        .map_err(|e| JsError::new(&format!("Invalid spend rule: {e}")))
}

/// Reads a `ShieldedTokenPreimage`: `{ domainSep, contract, spendRule? }`.
pub fn value_to_shielded_token_preimage(value: JsValue) -> Result<ShieldedTokenPreimage, JsError> {
    let pre: PreShieldedTokenPreimage = from_value(value)?;
    let domain_sep = <[u8; 32]>::try_from(pre.domain_sep)
        .map_err(|_| JsError::new("Expected a 32-byte domain separator"))?;
    Ok(ShieldedTokenPreimage {
        domain_sep: HashOutput(domain_sep),
        contract_address: from_hex_ser(&pre.contract)?,
        rule: pre
            .spend_rule
            .as_deref()
            .map(spend_rule_from_hex)
            .transpose()?
            .unwrap_or_default(),
    })
}

/// Reads an optional `GuardedSpend`: `{ recipient, output?, guard? }`.
pub fn value_to_guarded_spend(value: Option<JsValue>) -> Result<Option<GuardedSpend>, JsError> {
    let Some(value) = value.filter(|v| !v.is_undefined() && !v.is_null()) else {
        return Ok(None);
    };
    let output = js_sys::Reflect::get(&value, &"output".into())
        .map_err(|_| JsError::new("Expected a guarded spend object"))?;
    let output = if output.is_undefined() || output.is_null() {
        None
    } else {
        Some(value_to_shielded_coininfo(output)?)
    };
    let pre: PreGuardedSpend = from_value(value)?;
    let recipient = match pre.recipient {
        PreSpendRecipient::User {
            coin_public_key,
            encryption_public_key,
        } => SpendRecipient::User {
            coin_key: from_hex_ser(&coin_public_key)?,
            encryption_key: from_hex_ser(&encryption_public_key)?,
        },
        PreSpendRecipient::Contract { address } => SpendRecipient::Contract(from_hex_ser(&address)?),
    };
    Ok(Some(GuardedSpend {
        recipient,
        output,
        guard: pre.guard.map(|g| GuardProof {
            verifier_key: g.verifier_key,
            proof: g.proof,
        }),
    }))
}

/// The hash of a guard's verifying key (as midnight-zk writes it), which a
/// mode 2 descriptor's payload holds.
#[wasm_bindgen(js_name = "guardVerifierKeyHash")]
pub fn guard_verifier_key_hash(verifier_key: &[u8]) -> Result<String, JsError> {
    let vk = zswap::spend_circuit::read_guard_vk(verifier_key)
        .ok_or_else(|| JsError::new("Not a verifying key of the guard shape"))?;
    Ok(zswap::spend_circuit::guard_vk_hash(&vk).0.encode_hex())
}

/// Verifies a guard proof against its verifying key, on the spend's guard
/// arguments (field elements, as the guarded circuit hands them to the
/// guard).
#[wasm_bindgen(js_name = "verifyGuardProof")]
pub fn verify_guard_proof(
    verifier_key: &[u8],
    proof: &[u8],
    args: Vec<js_sys::BigInt>,
) -> Result<bool, JsError> {
    let args = args
        .into_iter()
        .map(crate::conversions::bigint_to_fr)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(zswap::spend_circuit::verify_guard_proof(verifier_key, proof, &args).is_ok())
}

/// The hex-encoded tagged mode 2, version 1 spend rule of the guard whose
/// verifying key hashes to `vkHash`, as `rawTokenType` takes it.
#[wasm_bindgen(js_name = "guardSpendRule")]
pub fn guard_spend_rule(vk_hash: &str) -> Result<String, JsError> {
    let hash = <[u8; 32]>::from_hex(vk_hash)
        .map_err(|_| JsError::new("Expected a hex-encoded 32-byte key hash"))?;
    let mut bytes = Vec::new();
    serialize::tagged_serialize(&SpendRule::guard(HashOutput(hash)), &mut bytes)?;
    Ok(bytes.encode_hex())
}

/// The all-zero contract address genesis mints shielded types under.
#[wasm_bindgen(js_name = "genesisContractAddress")]
pub fn genesis_contract_address() -> Result<String, JsError> {
    crate::conversions::to_hex_ser(&ContractAddress::GENESIS)
}
