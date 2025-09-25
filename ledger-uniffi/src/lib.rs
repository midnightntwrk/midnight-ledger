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

#![deny(warnings)]
uniffi::setup_scaffolding!();
pub mod contract;
pub mod conversions;
pub mod crypto;
pub mod dust;
pub mod intent;
pub mod state;
pub mod transcript;
pub mod tx;
pub mod types;
pub mod unshielded;
pub mod zswap_keys;
pub mod zswap_state;
pub mod zswap_uniffi;
mod errors;
pub use errors::FfiError;

// Object modules
pub mod objects {
    pub mod cost_model;
    pub mod dust;
    pub mod parameters;
    pub mod token_types;
    pub mod transaction;
    pub mod proof;
}

// Re-export types for UniFFI
pub use objects::token_types::*;
pub use objects::transaction::*;
pub use objects::proof::*;

use base_crypto::hash::HashOutput;
use base_crypto::signatures;
use coin_structure::{
    coin::{
        PublicKey as CoinPublicKey, UserAddress as InternalUserAddress,
    },
    transfer::Recipient,
};
use conversions::{
    bigint_to_fr,
};
use ledger::{
    self,
    structure::ProofPreimageVersioned as InternalProofPreimageVersioned,
};
use rand::Rng;
use rand::rngs::OsRng;
use serialize::{tagged_deserialize, tagged_serialize};
use transient_crypto::{curve::Fr, proofs::ProvingKeyMaterial as InternalProvingKeyMaterial};
use transient_crypto::{encryption::PublicKey as EncryptionPublicKey, proofs::WrappedIr as InternalWrappedIr};

pub(crate) use conversions::to_value_hex_ser;

#[uniffi::export]
pub fn hello() -> Result<String, FfiError> {
    Ok("test message".to_string())
}

#[uniffi::export]
pub fn native_token() -> TokenType {
    objects::token_types::TokenType::Unshielded
}

#[uniffi::export]
pub fn fee_token() -> TokenType {
    TokenType::Dust
}

#[uniffi::export]
pub fn shielded_token() -> TokenType {
    TokenType::Shielded
}

#[uniffi::export]
pub fn unshielded_token() -> TokenType {
    TokenType::Unshielded
}


#[uniffi::export]
pub fn create_shielded_coin_info(
    token_type: ShieldedTokenType,
    value: i64,
) -> ShieldedCoinInfo {
    let coin_info = coin_structure::coin::Info {
        type_: token_type.into(),
        value: value as u128,
        nonce: OsRng.r#gen(),
    };
    ShieldedCoinInfo::from(coin_info)
}

#[uniffi::export]
pub fn sample_coin_public_key() -> PublicKey {
    PublicKey::from(CoinPublicKey(OsRng.r#gen::<HashOutput>()))
}

#[uniffi::export]
pub fn sample_encryption_public_key() -> Result<String, FfiError> {
    Ok(to_value_hex_ser(&OsRng.r#gen::<EncryptionPublicKey>())?)
}

#[uniffi::export]
pub fn sample_intent_hash() -> objects::transaction::IntentHash {
    objects::transaction::IntentHash::from(OsRng.r#gen::<ledger::structure::IntentHash>())
}


#[uniffi::export]
pub fn coin_nullifier(
    coin_info: ShieldedCoinInfo,
    coin_secret_key: String,
) -> Result<String, FfiError> {
    use coin_structure::transfer::SenderEvidence;
    
    // Parse the secret key from hex string
    let secret_key_bytes = hex::decode(&coin_secret_key)
        .map_err(|e| FfiError::InvalidInput { details: format!("Failed to decode secret key hex: {}", e) })?;
    
    // Convert to the correct secret key type
    let mut hash_bytes = [0u8; 32];
    if secret_key_bytes.len() != 32 {
        return Err(FfiError::InvalidInput { 
            details: format!("Secret key must be 32 bytes, got {}", secret_key_bytes.len()) 
        });
    }
    hash_bytes.copy_from_slice(&secret_key_bytes);
    let coin_secret_key = coin_structure::coin::SecretKey(base_crypto::hash::HashOutput(hash_bytes));
    
    let coin_info_inner: coin_structure::coin::Info = coin_info.into();
    let nullifier = coin_info_inner.nullifier(&SenderEvidence::User(coin_secret_key));
    
    Ok(to_value_hex_ser(&nullifier)?)
}

#[uniffi::export]
pub fn coin_commitment(
    coin_info: ShieldedCoinInfo,
    coin_public_key: PublicKey,
) -> Commitment {
    let coin_info_inner: coin_structure::coin::Info = coin_info.into();
    let coin_public_key_inner: CoinPublicKey = coin_public_key.into();
    let commitment = coin_info_inner.commitment(&Recipient::User(coin_public_key_inner));
    Commitment::from(commitment)
}

#[uniffi::export]
pub fn address_from_key(key: &str) -> Result<UserAddress, FfiError> {
    let key: signatures::VerifyingKey = conversions::from_hex_ser(key)?;
    let internal_address = InternalUserAddress::from(key);
    Ok(UserAddress::from(internal_address))
}

#[uniffi::export]
pub fn create_proving_transaction_payload(
    _tx_serialized: Vec<u8>,
    _proving_data: std::collections::HashMap<String, Vec<u8>>,
) -> Result<Vec<u8>, FfiError> {
    // For now, return an error since transaction parsing is complex
    // TODO: Implement proper transaction deserialization
    Err(FfiError::UnsupportedVariant { 
        details: "Transaction parsing not yet implemented - requires proper transaction type handling".to_string() 
    })
}


#[uniffi::export]
pub fn create_proving_payload(
    preimage: std::sync::Arc<objects::proof::ProofPreimageVersioned>,
    overwrite_binding_input: Option<String>,
    key_material: Option<objects::proof::ProvingKeyMaterial>,
) -> Result<Vec<u8>, FfiError> {
    let preimage_inner = preimage.inner().clone();
    let overwrite_binding_input = overwrite_binding_input.map(bigint_to_fr).transpose()?;
    let proof_data = key_material.map(|km| km.into());
    
    let payload: (
        InternalProofPreimageVersioned,
        Option<InternalProvingKeyMaterial>,
        Option<Fr>,
    ) = (preimage_inner, proof_data, overwrite_binding_input);
    let mut res = Vec::new();
    tagged_serialize(&payload, &mut res)
        .map_err(|e| format!("Serialization error: {}", e))?;
    Ok(res)
}

#[uniffi::export]
pub fn create_check_payload(
    preimage: std::sync::Arc<objects::proof::ProofPreimageVersioned>,
    ir: Option<objects::proof::WrappedIr>,
) -> Result<Vec<u8>, FfiError> {
    let preimage_inner = preimage.inner().clone();
    let ir = ir.map(|wrapped_ir| wrapped_ir.into());
    let payload: (InternalProofPreimageVersioned, Option<InternalWrappedIr>) = (preimage_inner, ir);
    let mut res = Vec::new();
    tagged_serialize(&payload, &mut res)
        .map_err(|e| format!("Serialization error: {}", e))?;
    Ok(res)
}

#[uniffi::export]
pub fn parse_check_result(result: Vec<u8>) -> Result<Vec<Option<u64>>, FfiError> {
    let res: Vec<Option<u64>> = tagged_deserialize(&mut &result[..])
        .map_err(|e| format!("Deserialization error: {}", e))?;
    Ok(res)
}

