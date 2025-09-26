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

#![deny(warnings)]
pub mod contract;
pub mod conversions;
pub mod crypto;
pub mod dust;
pub mod intent;
pub mod state;
pub mod transcript;
pub mod tx;
pub mod unshielded;
pub mod zswap_keys;
pub mod zswap_state;
pub mod zswap_wasm;

use base_crypto::hash::HashOutput;
use base_crypto::signatures;
use coin_structure::{
    coin::{
        PublicKey as CoinPublicKey, ShieldedTokenType, UnshieldedTokenType, UserAddress,
        {NIGHT, TokenType},
    },
    transfer::{Recipient, SenderEvidence},
};
use conversions::{
    bigint_to_fr, from_hex_ser, shielded_coininfo_to_value, value_to_shielded_coininfo,
};
use js_sys::{Array, BigInt, Map, Reflect, Uint8Array};
use ledger::{
    self,
    structure::{FEE_TOKEN, IntentHash, ProofPreimageVersioned},
};
use rand::Rng;
use rand::rngs::OsRng;
use serde_wasm_bindgen::from_value;
use serialize::{tagged_deserialize, tagged_serialize};
use transient_crypto::{curve::Fr, proofs::ProvingKeyMaterial};
use transient_crypto::{encryption::PublicKey as EncryptionPublicKey, proofs::WrappedIr};
use tx::{Transaction, TransactionTypes};
use wasm_bindgen::{JsCast, JsError, JsValue, prelude::wasm_bindgen};
use zswap_keys::CoinSecretKey;

pub mod onchain_runtime {
    pub use onchain_runtime_wasm::context::*;
    pub use onchain_runtime_wasm::conversions::*;
    pub use onchain_runtime_wasm::primitives::*;
    pub use onchain_runtime_wasm::state::*;
    pub use onchain_runtime_wasm::vm::*;
    pub use onchain_runtime_wasm::*;
}

pub(crate) use onchain_runtime::{from_value_hex_ser, to_value_hex_ser, token_type_to_value};

#[wasm_bindgen(getter, js_name = "nativeToken")]
pub fn native_token() -> Result<JsValue, JsError> {
    Ok(token_type_to_value(&TokenType::Unshielded(NIGHT))?)
}

#[wasm_bindgen(getter, js_name = "feeToken")]
pub fn fee_token() -> Result<JsValue, JsError> {
    Ok(token_type_to_value(&FEE_TOKEN)?)
}

#[wasm_bindgen(getter, js_name = "shieldedToken")]
pub fn shielded_token() -> Result<JsValue, JsError> {
    Ok(token_type_to_value(&TokenType::Shielded(
        ShieldedTokenType(HashOutput([0u8; 32])),
    ))?)
}

#[wasm_bindgen(getter, js_name = "unshieldedToken")]
pub fn unshielded_token() -> Result<JsValue, JsError> {
    Ok(token_type_to_value(&TokenType::Unshielded(
        UnshieldedTokenType(HashOutput([0u8; 32])),
    ))?)
}

#[wasm_bindgen(js_name = "createShieldedCoinInfo")]
pub fn create_shielded_coin_info(type_: String, value: JsValue) -> Result<JsValue, JsError> {
    let token_type = ShieldedTokenType(from_hex_ser(&type_)?);
    let amount = from_value(value)?;
    shielded_coininfo_to_value(&coin_structure::coin::Info {
        type_: token_type,
        value: amount,
        nonce: OsRng.r#gen(),
    })
}

#[wasm_bindgen(js_name = "sampleCoinPublicKey")]
pub fn sample_coin_public_key() -> Result<String, JsError> {
    to_value_hex_ser(&CoinPublicKey(OsRng.r#gen::<HashOutput>()))
}

#[wasm_bindgen(js_name = "sampleEncryptionPublicKey")]
pub fn sample_encryption_public_key() -> Result<String, JsError> {
    to_value_hex_ser(&OsRng.r#gen::<EncryptionPublicKey>())
}

#[wasm_bindgen(js_name = "sampleIntentHash")]
pub fn sample_intent_hash() -> Result<String, JsError> {
    to_value_hex_ser(&OsRng.r#gen::<IntentHash>())
}

#[wasm_bindgen(js_name = "coinNullifier")]
pub fn coin_nullifier(
    coin_info: JsValue,
    coin_secret_key: &CoinSecretKey,
) -> Result<String, JsError> {
    let coin_info_parsed = value_to_shielded_coininfo(coin_info)?;
    let nullifier =
        coin_info_parsed.nullifier(&SenderEvidence::User(coin_secret_key.try_unwrap()?.clone()));
    to_value_hex_ser(&nullifier)
}

#[wasm_bindgen(js_name = "coinCommitment")]
pub fn coin_commitment(coin: JsValue, coin_public_key: String) -> Result<String, JsError> {
    let coin_info_parsed = value_to_shielded_coininfo(coin)?;
    let coin_public_key_parsed: CoinPublicKey = from_hex_ser(&coin_public_key)?;
    let commitment = coin_info_parsed.commitment(&Recipient::User(coin_public_key_parsed));
    to_value_hex_ser(&commitment)
}

#[wasm_bindgen(js_name = "addressFromKey")]
pub fn address_from_key(key: &str) -> Result<String, JsError> {
    let key: signatures::VerifyingKey = from_value_hex_ser(key)?;
    to_value_hex_ser(&UserAddress::from(key))
}

#[wasm_bindgen(js_name = "createProvingTransactionPayload")]
pub fn create_proving_transaction_payload(
    tx: &Transaction,
    proving_data: Map,
) -> Result<Uint8Array, JsError> {
    let Transaction(TransactionTypes::UnprovenWithSignaturePreBinding(tx)) = tx else {
        return Err(JsError::new("invalid transaction kind"));
    };
    let mut proof_data = std::collections::HashMap::<String, ProvingKeyMaterial>::new();
    let mut err = false;
    proving_data.for_each(&mut |v, k| {
        let k = k.as_string();
        let pk = Reflect::get(&v, &"proverKey".into()).and_then(|pk| pk.dyn_into::<Uint8Array>());
        let vk = Reflect::get(&v, &"verifierKey".into()).and_then(|vk| vk.dyn_into::<Uint8Array>());
        let ir = Reflect::get(&v, &"ir".into()).and_then(|ir| ir.dyn_into::<Uint8Array>());
        match (k, pk, vk, ir) {
            (Some(k), Ok(pk), Ok(vk), Ok(ir)) => {
                proof_data.insert(
                    k.into(),
                    ProvingKeyMaterial {
                        prover_key: pk.to_vec(),
                        verifier_key: vk.to_vec(),
                        ir_source: ir.to_vec(),
                    },
                );
            }
            _ => err = true,
        }
    });
    if err {
        return Err(JsError::new("failed to decode proving data map"));
    }
    let mut buf = Vec::new();
    serialize::tagged_serialize(&(tx, proof_data), &mut buf)?;
    Ok(buf[..].into())
}

fn try_proof_preimage(serialized_preimage: &[u8]) -> Result<ProofPreimageVersioned, JsError> {
    Ok(
        tagged_deserialize(&mut &serialized_preimage[..]).or_else(|_| {
            tagged_deserialize(&mut &serialized_preimage[..]).map(ProofPreimageVersioned::V1)
        })?,
    )
}

#[wasm_bindgen(js_name = "createProvingPayload")]
pub fn create_proving_payload(
    serialized_preimage: Uint8Array,
    overwrite_binding_input: Option<BigInt>,
    key_material: JsValue,
) -> Result<Uint8Array, JsError> {
    let preimage: ProofPreimageVersioned = try_proof_preimage(&serialized_preimage.to_vec()[..])?;
    let overwrite_binding_input = overwrite_binding_input.map(bigint_to_fr).transpose()?;
    let proof_data = if key_material.is_undefined() || key_material.is_null() {
        None
    } else {
        let pk = Reflect::get(&key_material, &"proverKey".into())
            .and_then(|pk| pk.dyn_into::<Uint8Array>())
            .map_err(|_| JsError::new("expected Uint8Array"))?
            .to_vec();
        let vk = Reflect::get(&key_material, &"verifierKey".into())
            .and_then(|vk| vk.dyn_into::<Uint8Array>())
            .map_err(|_| JsError::new("expected Uint8Array"))?
            .to_vec();
        let ir = Reflect::get(&key_material, &"ir".into())
            .and_then(|ir| ir.dyn_into::<Uint8Array>())
            .map_err(|_| JsError::new("expected Uint8Array"))?
            .to_vec();
        Some(ProvingKeyMaterial {
            prover_key: pk,
            verifier_key: vk,
            ir_source: ir,
        })
    };
    let payload: (
        ProofPreimageVersioned,
        Option<ProvingKeyMaterial>,
        Option<Fr>,
    ) = (preimage, proof_data, overwrite_binding_input);
    let mut res = Vec::new();
    tagged_serialize(&payload, &mut res)?;
    Ok(res[..].into())
}

#[wasm_bindgen(js_name = "createCheckPayload")]
pub fn create_check_payload(
    serialized_preimage: Uint8Array,
    ir: Option<Uint8Array>,
) -> Result<Uint8Array, JsError> {
    let preimage: ProofPreimageVersioned = try_proof_preimage(&serialized_preimage.to_vec()[..])?;
    let ir = ir.map(|data| WrappedIr(data.to_vec()));
    let payload: (ProofPreimageVersioned, Option<WrappedIr>) = (preimage, ir);
    let mut res = Vec::new();
    tagged_serialize(&payload, &mut res)?;
    Ok(res[..].into())
}

#[wasm_bindgen(js_name = "parseCheckResult")]
pub fn parse_check_result(result: Uint8Array) -> Result<Array, JsError> {
    let res: Vec<Option<u64>> = tagged_deserialize(&mut &result.to_vec()[..])?;
    Ok(res
        .into_iter()
        .map(|v| match v {
            Some(v) => JsValue::from(BigInt::from(v)),
            None => JsValue::UNDEFINED,
        })
        .collect())
}
