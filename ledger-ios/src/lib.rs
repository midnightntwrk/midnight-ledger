// This file is part of midnight-ledger.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

//! iOS bindings for the Midnight Ledger wallet library.
//!
//! This crate provides UniFFI-based bindings for use in iOS applications.

#![deny(warnings)]

mod addresses;
mod block_context;
mod dust;
mod dust_state;
mod error;
mod events;
mod intent;
mod keys;
mod ledger_state;
mod parameters;
mod transaction;
mod unshielded;
mod util;
mod zswap_state;

pub use addresses::{ContractAddress, PublicAddress};
pub use block_context::BlockContext;
pub use dust::{
    calculate_dust_value, DustGenerationInfo, DustOutput, DustPublicKey, InitialNonce,
    QualifiedDustOutput,
};
pub use dust_state::{DustLocalState, DustParameters, DustSpend, DustSpendResult};
pub use error::LedgerError;
pub use events::{Event, EventSource};
pub use intent::Intent;
pub use keys::*;
pub use ledger_state::LedgerState;
pub use parameters::LedgerParameters;
pub use transaction::Transaction;
pub use unshielded::{UnshieldedOffer, UtxoOutput, UtxoSpend};
pub use util::{from_hex, from_hex_ser, to_hex, to_hex_ser};
pub use zswap_state::{MerkleTreeCollapsedUpdate, PendingOutputEntry, PendingSpendEntry, QualifiedShieldedCoinInfo, ShieldedCoinInfo, ZswapInput, ZswapLocalState, ZswapSpendResult};

use base_crypto::hash::HashOutput;
use coin_structure::coin::{PublicKey as CoinPublicKey, ShieldedTokenType, TokenType, NIGHT};
use std::sync::Arc;
use rand::rngs::OsRng;
use rand::Rng;
use serialize::{tagged_deserialize, tagged_serialize};

// Include the UniFFI scaffolding
uniffi::include_scaffolding!("ledger_ios");

// ============================================================================
// Token Type Functions (Phase 2)
// ============================================================================

/// Returns the shielded token type as a hex-encoded string.
pub fn shielded_token() -> String {
    hex_encode_token(&TokenType::Shielded(ShieldedTokenType(HashOutput([0u8; 32]))))
}

/// Returns the unshielded (native) token type as a hex-encoded string.
pub fn unshielded_token() -> String {
    hex_encode_token(&TokenType::Unshielded(NIGHT))
}

/// Returns the native token type (NIGHT) as a hex-encoded string.
/// Alias for unshielded_token().
pub fn native_token() -> String {
    hex_encode_token(&TokenType::Unshielded(NIGHT))
}

/// Returns the fee token type as a hex-encoded string.
pub fn fee_token() -> String {
    hex_encode_token(&ledger::structure::FEE_TOKEN)
}

// ============================================================================
// Address Functions
// ============================================================================

/// Derives a signature verifying key from a hex-encoded signing key.
pub fn signature_verifying_key(signing_key: String) -> Result<std::sync::Arc<SignatureVerifyingKey>, LedgerError> {
    Ok(std::sync::Arc::new(SignatureVerifyingKey::from_signing_key(signing_key)?))
}

/// Derives a user address from a signature verifying key.
pub fn address_from_key(verifying_key: Arc<SignatureVerifyingKey>) -> String {
    verifying_key.address()
}

// ============================================================================
// Coin Operations (Phase 3)
// ============================================================================

/// Creates a shielded coin info structure.
/// Value is passed as u64.
pub fn create_shielded_coin_info(token_type: String, value: u64) -> Result<Vec<u8>, LedgerError> {
    let token_type_bytes = hex::decode(&token_type).map_err(|_| LedgerError::InvalidData)?;
    let token_type: ShieldedTokenType =
        serialize::Deserializable::deserialize(&mut &token_type_bytes[..], 0)
            .map_err(|_| LedgerError::DeserializationError)?;

    let coin_info = coin_structure::coin::Info {
        type_: token_type,
        value: value as u128,
        nonce: OsRng.r#gen(),
    };

    let mut buf = Vec::new();
    tagged_serialize(&coin_info, &mut buf)?;
    Ok(buf)
}

/// Calculates the commitment for a coin.
pub fn coin_commitment(coin_info: Vec<u8>, coin_public_key: String) -> Result<String, LedgerError> {
    let coin_info_parsed: coin_structure::coin::Info =
        tagged_deserialize(&mut &coin_info[..])?;
    let coin_public_key_parsed: CoinPublicKey = from_hex_ser(&coin_public_key)?;
    let commitment = coin_info_parsed
        .commitment(&coin_structure::transfer::Recipient::User(coin_public_key_parsed));
    Ok(to_hex_ser(&commitment))
}

/// Calculates the commitment for a coin from individual fields.
/// This is useful when the coin info is available as an object rather than serialized bytes.
pub fn coin_commitment_from_fields(
    token_type: String,
    nonce: String,
    value: u64,
    coin_public_key: String,
) -> Result<String, LedgerError> {
    let token_type_bytes = hex::decode(&token_type).map_err(|_| LedgerError::InvalidData)?;
    let token_type: ShieldedTokenType =
        serialize::Deserializable::deserialize(&mut &token_type_bytes[..], 0)
            .map_err(|_| LedgerError::DeserializationError)?;

    let nonce_parsed = from_hex_ser(&nonce)?;

    let coin_info = coin_structure::coin::Info {
        type_: token_type,
        value: value as u128,
        nonce: nonce_parsed,
    };

    let coin_public_key_parsed: CoinPublicKey = from_hex_ser(&coin_public_key)?;
    let commitment = coin_info
        .commitment(&coin_structure::transfer::Recipient::User(coin_public_key_parsed));
    Ok(to_hex_ser(&commitment))
}

/// Calculates the nullifier for a coin.
pub fn coin_nullifier(
    coin_info: Vec<u8>,
    coin_secret_key: Arc<CoinSecretKey>,
) -> Result<String, LedgerError> {
    let coin_info_parsed: coin_structure::coin::Info =
        tagged_deserialize(&mut &coin_info[..])?;
    let sk = coin_secret_key.try_unwrap()?;
    let nullifier = coin_info_parsed.nullifier(
        &coin_structure::transfer::SenderEvidence::User(std::borrow::Cow::Borrowed(&sk)),
    );
    Ok(to_hex_ser(&nullifier))
}

/// Calculates the nullifier for a coin from individual fields.
/// This is useful when the coin info is available as an object rather than serialized bytes.
pub fn coin_nullifier_from_fields(
    token_type: String,
    nonce: String,
    value: u64,
    coin_secret_key: Arc<CoinSecretKey>,
) -> Result<String, LedgerError> {
    let token_type_bytes = hex::decode(&token_type).map_err(|_| LedgerError::InvalidData)?;
    let token_type: ShieldedTokenType =
        serialize::Deserializable::deserialize(&mut &token_type_bytes[..], 0)
            .map_err(|_| LedgerError::DeserializationError)?;

    let nonce_parsed = from_hex_ser(&nonce)?;

    let coin_info = coin_structure::coin::Info {
        type_: token_type,
        value: value as u128,
        nonce: nonce_parsed,
    };

    let sk = coin_secret_key.try_unwrap()?;
    let nullifier = coin_info.nullifier(
        &coin_structure::transfer::SenderEvidence::User(std::borrow::Cow::Borrowed(&sk)),
    );
    Ok(to_hex_ser(&nullifier))
}

// ============================================================================
// Signing (Phase 9)
// ============================================================================

/// Signs data with a signing key.
/// Returns the signature as a hex-encoded string.
pub fn sign_data(signing_key: String, payload: Vec<u8>) -> Result<String, LedgerError> {
    let sk: base_crypto::signatures::SigningKey = from_hex_ser(&signing_key)?;
    let signature = sk.sign(&mut OsRng, &payload);
    Ok(to_hex_ser(&signature))
}

// ============================================================================
// Sample/Test Functions
// ============================================================================

/// Generates a random sample coin public key for testing.
pub fn sample_coin_public_key() -> String {
    to_hex_ser(&CoinPublicKey(OsRng.r#gen::<HashOutput>()))
}

/// Generates a random sample encryption public key for testing.
pub fn sample_encryption_public_key() -> String {
    to_hex_ser(&OsRng.r#gen::<transient_crypto::encryption::PublicKey>())
}

// ============================================================================
// Deserialize Functions (Phase 4)
// ============================================================================

/// Deserializes an event from bytes.
pub fn deserialize_event(raw: Vec<u8>) -> Result<Arc<Event>, LedgerError> {
    Ok(Arc::new(Event::deserialize(raw)?))
}

/// Deserializes a merkle tree collapsed update from bytes.
pub fn deserialize_merkle_tree_collapsed_update(raw: Vec<u8>) -> Result<Arc<MerkleTreeCollapsedUpdate>, LedgerError> {
    Ok(Arc::new(MerkleTreeCollapsedUpdate::deserialize(raw)?))
}

/// Deserializes dust parameters from bytes.
pub fn deserialize_dust_parameters(raw: Vec<u8>) -> Result<Arc<DustParameters>, LedgerError> {
    Ok(Arc::new(DustParameters::deserialize(raw)?))
}

/// Deserializes a ZSwap local state from bytes.
pub fn deserialize_zswap_local_state(raw: Vec<u8>) -> Result<Arc<ZswapLocalState>, LedgerError> {
    Ok(Arc::new(ZswapLocalState::deserialize(raw)?))
}

/// Deserializes a dust local state from bytes.
pub fn deserialize_dust_local_state(raw: Vec<u8>) -> Result<Arc<DustLocalState>, LedgerError> {
    Ok(Arc::new(DustLocalState::deserialize(raw)?))
}

/// Deserializes a ledger state from bytes.
pub fn deserialize_ledger_state(raw: Vec<u8>) -> Result<Arc<LedgerState>, LedgerError> {
    Ok(Arc::new(LedgerState::deserialize(raw)?))
}

// ============================================================================
// Phase 5-6: Transaction and Offers
// ============================================================================

/// Deserializes a UTXO spend from bytes.
pub fn deserialize_utxo_spend(raw: Vec<u8>) -> Result<Arc<UtxoSpend>, LedgerError> {
    Ok(Arc::new(UtxoSpend::deserialize(raw)?))
}

/// Deserializes a UTXO output from bytes.
pub fn deserialize_utxo_output(raw: Vec<u8>) -> Result<Arc<UtxoOutput>, LedgerError> {
    Ok(Arc::new(UtxoOutput::deserialize(raw)?))
}

/// Deserializes an unshielded offer from bytes.
pub fn deserialize_unshielded_offer(raw: Vec<u8>) -> Result<Arc<UnshieldedOffer>, LedgerError> {
    Ok(Arc::new(UnshieldedOffer::deserialize(raw)?))
}

/// Deserializes an intent from bytes.
pub fn deserialize_intent(raw: Vec<u8>) -> Result<Arc<Intent>, LedgerError> {
    Ok(Arc::new(Intent::deserialize(raw)?))
}

/// Deserializes a transaction from bytes.
pub fn deserialize_transaction(raw: Vec<u8>) -> Result<Arc<Transaction>, LedgerError> {
    Ok(Arc::new(Transaction::deserialize(raw)?))
}

/// Deserializes a transaction with specific type markers.
pub fn deserialize_transaction_typed(
    signature_marker: String,
    proof_marker: String,
    binding_marker: String,
    raw: Vec<u8>,
) -> Result<Arc<Transaction>, LedgerError> {
    Ok(Arc::new(Transaction::deserialize_typed(
        signature_marker,
        proof_marker,
        binding_marker,
        raw,
    )?))
}

/// Creates a new transaction from parts.
pub fn create_transaction(
    network_id: String,
    intent: Option<Arc<Intent>>,
) -> Result<Arc<Transaction>, LedgerError> {
    Ok(Arc::new(Transaction::from_parts(network_id, intent)?))
}

/// Creates a new transaction with a randomized segment ID.
pub fn create_transaction_randomized(
    network_id: String,
    intent: Option<Arc<Intent>>,
) -> Result<Arc<Transaction>, LedgerError> {
    Ok(Arc::new(Transaction::from_parts_randomized(network_id, intent)?))
}

/// Creates a new intent with the given TTL.
pub fn create_intent(ttl_seconds: u64) -> Arc<Intent> {
    Arc::new(Intent::new(ttl_seconds))
}

/// Creates a new UTXO spend.
pub fn create_utxo_spend(
    value: String,
    owner: String,
    token_type: String,
    intent_hash: String,
    output_no: u32,
) -> Result<Arc<UtxoSpend>, LedgerError> {
    Ok(Arc::new(UtxoSpend::new(value, owner, token_type, intent_hash, output_no)?))
}

/// Creates a new UTXO output.
pub fn create_utxo_output(
    value: String,
    owner: String,
    token_type: String,
) -> Result<Arc<UtxoOutput>, LedgerError> {
    Ok(Arc::new(UtxoOutput::new(value, owner, token_type)?))
}

/// Creates a new unshielded offer.
pub fn create_unshielded_offer(
    inputs: Vec<Arc<UtxoSpend>>,
    outputs: Vec<Arc<UtxoOutput>>,
    signatures: Vec<String>,
) -> Result<Arc<UnshieldedOffer>, LedgerError> {
    Ok(Arc::new(UnshieldedOffer::new(inputs, outputs, signatures)?))
}

/// Creates a new unsigned unshielded offer.
pub fn create_unshielded_offer_unsigned(
    inputs: Vec<Arc<UtxoSpend>>,
    outputs: Vec<Arc<UtxoOutput>>,
) -> Result<Arc<UnshieldedOffer>, LedgerError> {
    Ok(Arc::new(UnshieldedOffer::new_unsigned(inputs, outputs)?))
}

// ============================================================================
// Phase 7: Dust Operations
// ============================================================================

/// Deserializes a dust public key from bytes.
pub fn deserialize_dust_public_key(raw: Vec<u8>) -> Result<Arc<DustPublicKey>, LedgerError> {
    Ok(Arc::new(DustPublicKey::deserialize(raw)?))
}

/// Deserializes an initial nonce from bytes.
pub fn deserialize_initial_nonce(raw: Vec<u8>) -> Result<Arc<InitialNonce>, LedgerError> {
    Ok(Arc::new(InitialNonce::deserialize(raw)?))
}

/// Deserializes dust generation info from bytes.
pub fn deserialize_dust_generation_info(raw: Vec<u8>) -> Result<Arc<DustGenerationInfo>, LedgerError> {
    Ok(Arc::new(DustGenerationInfo::deserialize(raw)?))
}

/// Deserializes a dust output from bytes.
pub fn deserialize_dust_output(raw: Vec<u8>) -> Result<Arc<DustOutput>, LedgerError> {
    Ok(Arc::new(DustOutput::deserialize(raw)?))
}

/// Deserializes a qualified dust output from bytes.
pub fn deserialize_qualified_dust_output(raw: Vec<u8>) -> Result<Arc<QualifiedDustOutput>, LedgerError> {
    Ok(Arc::new(QualifiedDustOutput::deserialize(raw)?))
}

/// Creates a dust public key from a hex-encoded string.
pub fn create_dust_public_key(hex: String) -> Result<Arc<DustPublicKey>, LedgerError> {
    Ok(Arc::new(DustPublicKey::from_hex(hex)?))
}

/// Creates an initial nonce from a hex-encoded string.
pub fn create_initial_nonce(hex: String) -> Result<Arc<InitialNonce>, LedgerError> {
    Ok(Arc::new(InitialNonce::from_hex(hex)?))
}

/// Calculates the updated dust value at the given time.
/// This is a convenience function wrapping the calculate_dust_value function.
pub fn dust_updated_value(
    initial_value: String,
    ctime_seconds: u64,
    gen_info: Arc<DustGenerationInfo>,
    now_seconds: u64,
    params: Arc<DustParameters>,
) -> Result<String, LedgerError> {
    calculate_dust_value(initial_value, ctime_seconds, gen_info, now_seconds, params)
}

// ============================================================================
// Phase 8: Parameters and Extended Events
// ============================================================================

/// Returns the initial (genesis) ledger parameters.
pub fn initial_ledger_parameters() -> Arc<LedgerParameters> {
    Arc::new(LedgerParameters::initial())
}

/// Deserializes ledger parameters from bytes.
pub fn deserialize_ledger_parameters(raw: Vec<u8>) -> Result<Arc<LedgerParameters>, LedgerError> {
    Ok(Arc::new(LedgerParameters::deserialize(raw)?))
}

/// Creates a BlockContext with the given time.
pub fn create_block_context(tblock_seconds: u64) -> Arc<BlockContext> {
    Arc::new(BlockContext::with_time(tblock_seconds))
}

/// Creates a BlockContext with full parameters.
pub fn create_block_context_full(
    tblock_seconds: u64,
    tblock_err: u32,
    parent_block_hash: String,
) -> Result<Arc<BlockContext>, LedgerError> {
    Ok(Arc::new(BlockContext::new(
        tblock_seconds,
        tblock_err,
        parent_block_hash,
    )?))
}

/// Deserializes a block context from bytes.
pub fn deserialize_block_context(raw: Vec<u8>) -> Result<Arc<BlockContext>, LedgerError> {
    Ok(Arc::new(BlockContext::deserialize(raw)?))
}

// ============================================================================
// Phase 9: Addresses and Signature Verification
// ============================================================================

/// Creates a contract address from a hex-encoded string (32 bytes).
pub fn create_contract_address(hex: String) -> Result<Arc<ContractAddress>, LedgerError> {
    Ok(Arc::new(ContractAddress::from_hex(hex)?))
}

/// Deserializes a contract address from bytes.
pub fn deserialize_contract_address(raw: Vec<u8>) -> Result<Arc<ContractAddress>, LedgerError> {
    Ok(Arc::new(ContractAddress::deserialize(raw)?))
}

/// Creates a public address from a contract address.
pub fn create_public_address_contract(contract: Arc<ContractAddress>) -> Arc<PublicAddress> {
    Arc::new(PublicAddress::from_contract(contract))
}

/// Creates a public address from a user address (hex-encoded).
pub fn create_public_address_user(user_address: String) -> Result<Arc<PublicAddress>, LedgerError> {
    Ok(Arc::new(PublicAddress::from_user(user_address)?))
}

/// Deserializes a public address from bytes.
pub fn deserialize_public_address(raw: Vec<u8>) -> Result<Arc<PublicAddress>, LedgerError> {
    Ok(Arc::new(PublicAddress::deserialize(raw)?))
}

/// Creates a signature verifying key from a hex-encoded verifying key (not signing key).
pub fn create_verifying_key(hex: String) -> Result<Arc<SignatureVerifyingKey>, LedgerError> {
    Ok(Arc::new(SignatureVerifyingKey::from_hex(hex)?))
}

/// Verifies a signature against a message using a verifying key.
pub fn verify_signature(
    verifying_key: Arc<SignatureVerifyingKey>,
    message: Vec<u8>,
    signature: String,
) -> Result<bool, LedgerError> {
    verifying_key.verify(message, signature)
}

// ============================================================================
// Internal Helpers
// ============================================================================

/// Hex-encodes a token type.
fn hex_encode_token(token: &TokenType) -> String {
    let mut buf = Vec::new();
    tagged_serialize(token, &mut buf).expect("token serialization should not fail");
    hex::encode(buf)
}
