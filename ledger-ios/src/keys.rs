// This file is part of midnight-ledger.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

//! Key management types for iOS bindings.

use crate::error::LedgerError;
use crate::util::{from_hex_ser, to_hex_ser};
use serialize::{tagged_serialize, Deserializable, Serializable};
use std::sync::{Arc, Mutex};
use zeroize::{Zeroize, ZeroizeOnDrop};

// ============================================================================
// ZswapSecretKeys
// ============================================================================

/// Holds the internal secret keys with zeroization support.
#[derive(ZeroizeOnDrop)]
struct SecretKeysInner {
    coin_secret_key: Option<coin_structure::coin::SecretKey>,
    encryption_secret_key: Option<transient_crypto::encryption::SecretKey>,
}

impl SecretKeysInner {
    fn new(keys: zswap::keys::SecretKeys) -> Self {
        SecretKeysInner {
            coin_secret_key: Some(keys.coin_secret_key.clone()),
            encryption_secret_key: Some(keys.encryption_secret_key.clone()),
        }
    }

    fn clear(&mut self) {
        self.coin_secret_key = None;
        self.encryption_secret_key = None;
    }
}

impl Zeroize for SecretKeysInner {
    fn zeroize(&mut self) {
        self.clear();
    }
}

/// Secret keys for ZSwap operations (coin and encryption keys).
///
/// This type holds sensitive cryptographic material and supports secure
/// zeroization when cleared or dropped.
pub struct ZswapSecretKeys {
    inner: Arc<Mutex<Option<SecretKeysInner>>>,
}

impl ZswapSecretKeys {
    /// Creates secret keys from a 32-byte seed.
    pub fn from_seed(seed: Vec<u8>) -> Result<Self, LedgerError> {
        let bytes: [u8; 32] = seed
            .try_into()
            .map_err(|_| LedgerError::InvalidSeed)?;
        let seed_parsed = zswap::keys::Seed::from(bytes);
        let keys = zswap::keys::SecretKeys::from(seed_parsed);
        Ok(ZswapSecretKeys {
            inner: Arc::new(Mutex::new(Some(SecretKeysInner::new(keys)))),
        })
    }

    /// Returns the coin public key as a hex-encoded string.
    pub fn coin_public_key(&self) -> Result<String, LedgerError> {
        let guard = self.inner.lock().map_err(|_| LedgerError::InvalidState("lock poisoned".into()))?;
        let inner = guard.as_ref().ok_or(LedgerError::KeysCleared)?;
        let sk = inner.coin_secret_key.as_ref().ok_or(LedgerError::KeysCleared)?;
        Ok(to_hex_ser(&sk.public_key()))
    }

    /// Returns the encryption public key as a hex-encoded string.
    pub fn encryption_public_key(&self) -> Result<String, LedgerError> {
        let guard = self.inner.lock().map_err(|_| LedgerError::InvalidState("lock poisoned".into()))?;
        let inner = guard.as_ref().ok_or(LedgerError::KeysCleared)?;
        let sk = inner.encryption_secret_key.as_ref().ok_or(LedgerError::KeysCleared)?;
        Ok(to_hex_ser(&sk.public_key()))
    }

    /// Returns a reference to the coin secret key.
    pub fn coin_secret_key(&self) -> Result<Arc<CoinSecretKey>, LedgerError> {
        let guard = self.inner.lock().map_err(|_| LedgerError::InvalidState("lock poisoned".into()))?;
        let inner = guard.as_ref().ok_or(LedgerError::KeysCleared)?;
        let sk = inner.coin_secret_key.clone().ok_or(LedgerError::KeysCleared)?;
        Ok(Arc::new(CoinSecretKey::wrap(sk)))
    }

    /// Returns a reference to the encryption secret key.
    pub fn encryption_secret_key(&self) -> Result<Arc<EncryptionSecretKey>, LedgerError> {
        let guard = self.inner.lock().map_err(|_| LedgerError::InvalidState("lock poisoned".into()))?;
        let inner = guard.as_ref().ok_or(LedgerError::KeysCleared)?;
        let sk = inner.encryption_secret_key.clone().ok_or(LedgerError::KeysCleared)?;
        Ok(Arc::new(EncryptionSecretKey::wrap(sk)))
    }

    /// Tries to get the internal zswap::keys::SecretKeys.
    #[allow(dead_code)]
    pub(crate) fn try_as_inner(&self) -> Result<zswap::keys::SecretKeys, LedgerError> {
        let guard = self.inner.lock().map_err(|_| LedgerError::InvalidState("lock poisoned".into()))?;
        let inner = guard.as_ref().ok_or(LedgerError::KeysCleared)?;
        let coin_sk = inner.coin_secret_key.clone().ok_or(LedgerError::KeysCleared)?;
        let enc_sk = inner.encryption_secret_key.clone().ok_or(LedgerError::KeysCleared)?;
        Ok(zswap::keys::SecretKeys {
            coin_secret_key: coin_sk,
            encryption_secret_key: enc_sk,
        })
    }

    /// Securely clears the secret keys from memory.
    pub fn clear(&self) {
        if let Ok(mut guard) = self.inner.lock() {
            if let Some(inner) = guard.as_mut() {
                inner.clear();
            }
            *guard = None;
        }
    }
}

// ============================================================================
// DustSecretKey
// ============================================================================

/// Secret key for dust operations.
pub struct DustSecretKey {
    inner: Arc<Mutex<Option<ledger::dust::DustSecretKey>>>,
}

impl DustSecretKey {
    /// Creates a dust secret key from a 32-byte seed.
    pub fn from_seed(seed: Vec<u8>) -> Result<Self, LedgerError> {
        let bytes: [u8; 32] = seed
            .try_into()
            .map_err(|_| LedgerError::InvalidSeed)?;
        let key = ledger::dust::DustSecretKey::derive_secret_key(&bytes);
        Ok(DustSecretKey {
            inner: Arc::new(Mutex::new(Some(key))),
        })
    }

    /// Returns the public key as a big-endian hex string for BigInt conversion.
    /// WASM-compatible: matches the format expected by wallet-sdk-address-format.
    pub fn public_key(&self) -> Result<String, LedgerError> {
        let guard = self.inner.lock().map_err(|_| LedgerError::InvalidState("lock poisoned".into()))?;
        let sk = guard.as_ref().ok_or(LedgerError::KeysCleared)?;
        let pk = ledger::dust::DustPublicKey::from(sk.clone());
        // Convert field element to big-endian hex for BigInt compatibility
        let mut bytes = pk.0.as_le_bytes();
        bytes.reverse();
        Ok(hex::encode(bytes))
    }

    /// Tries to get the internal key.
    #[allow(dead_code)]
    pub(crate) fn try_as_inner(&self) -> Result<ledger::dust::DustSecretKey, LedgerError> {
        let guard = self.inner.lock().map_err(|_| LedgerError::InvalidState("lock poisoned".into()))?;
        guard.clone().ok_or(LedgerError::KeysCleared)
    }

    /// Securely clears the secret key from memory.
    pub fn clear(&self) {
        if let Ok(mut guard) = self.inner.lock() {
            *guard = None;
        }
    }
}

// ============================================================================
// SignatureVerifyingKey
// ============================================================================

/// A signature verifying key (public key for unshielded wallet).
pub struct SignatureVerifyingKey {
    pub(crate) inner: base_crypto::signatures::VerifyingKey,
}

impl SignatureVerifyingKey {
    /// Creates a verifying key from a hex-encoded signing key.
    pub fn from_signing_key(signing_key: String) -> Result<Self, LedgerError> {
        let sk: base_crypto::signatures::SigningKey = from_hex_ser(&signing_key)?;
        Ok(SignatureVerifyingKey {
            inner: sk.verifying_key(),
        })
    }

    /// Creates a verifying key from a hex-encoded verifying key.
    pub fn from_hex(hex: String) -> Result<Self, LedgerError> {
        let key: base_crypto::signatures::VerifyingKey = from_hex_ser(&hex)?;
        Ok(SignatureVerifyingKey { inner: key })
    }

    /// Returns the address derived from this verifying key.
    pub fn address(&self) -> String {
        to_hex_ser(&coin_structure::coin::UserAddress::from(self.inner.clone()))
    }

    /// Returns the key as a hex-encoded string.
    pub fn to_hex(&self) -> String {
        to_hex_ser(&self.inner)
    }

    /// Verifies a signature against a message.
    /// Returns true if the signature is valid.
    pub fn verify(&self, message: Vec<u8>, signature: String) -> Result<bool, LedgerError> {
        let sig: base_crypto::signatures::Signature = from_hex_ser(&signature)?;
        Ok(self.inner.verify(&message, &sig))
    }
}

// ============================================================================
// CoinSecretKey
// ============================================================================

/// A coin secret key for ZSwap operations.
pub struct CoinSecretKey {
    inner: Arc<Mutex<Option<coin_structure::coin::SecretKey>>>,
}

impl CoinSecretKey {
    /// Wraps an existing coin secret key.
    pub(crate) fn wrap(key: coin_structure::coin::SecretKey) -> Self {
        CoinSecretKey {
            inner: Arc::new(Mutex::new(Some(key))),
        }
    }

    /// Tries to unwrap the inner secret key.
    pub(crate) fn try_unwrap(&self) -> Result<coin_structure::coin::SecretKey, LedgerError> {
        let guard = self.inner.lock().map_err(|_| LedgerError::InvalidState("lock poisoned".into()))?;
        guard.clone().ok_or(LedgerError::KeysCleared)
    }

    /// Returns the public key as a hex-encoded string.
    pub fn public_key(&self) -> Result<String, LedgerError> {
        let guard = self.inner.lock().map_err(|_| LedgerError::InvalidState("lock poisoned".into()))?;
        let sk = guard.as_ref().ok_or(LedgerError::KeysCleared)?;
        Ok(to_hex_ser(&sk.public_key()))
    }

    /// Serializes the secret key (use with caution!).
    pub fn serialize(&self) -> Result<Vec<u8>, LedgerError> {
        let guard = self.inner.lock().map_err(|_| LedgerError::InvalidState("lock poisoned".into()))?;
        let sk = guard.as_ref().ok_or(LedgerError::KeysCleared)?;
        let mut res = Vec::new();
        tagged_serialize(sk, &mut res)?;
        Ok(res)
    }

    /// Securely clears the secret key from memory.
    pub fn clear(&self) {
        if let Ok(mut guard) = self.inner.lock() {
            *guard = None;
        }
    }
}

// ============================================================================
// EncryptionSecretKey
// ============================================================================

/// An encryption secret key for ZSwap operations.
pub struct EncryptionSecretKey {
    inner: Arc<Mutex<Option<transient_crypto::encryption::SecretKey>>>,
}

impl EncryptionSecretKey {
    /// Wraps an existing encryption secret key.
    pub(crate) fn wrap(key: transient_crypto::encryption::SecretKey) -> Self {
        EncryptionSecretKey {
            inner: Arc::new(Mutex::new(Some(key))),
        }
    }

    /// Deserializes an encryption secret key.
    pub fn deserialize(raw: Vec<u8>) -> Result<Self, LedgerError> {
        let key: transient_crypto::encryption::SecretKey =
            Deserializable::deserialize(&mut &raw[..], 0)
                .map_err(|_| LedgerError::DeserializationError)?;
        Ok(EncryptionSecretKey::wrap(key))
    }

    /// Returns the public key as a hex-encoded string.
    pub fn public_key(&self) -> Result<String, LedgerError> {
        let guard = self.inner.lock().map_err(|_| LedgerError::InvalidState("lock poisoned".into()))?;
        let sk = guard.as_ref().ok_or(LedgerError::KeysCleared)?;
        Ok(to_hex_ser(&sk.public_key()))
    }

    /// Serializes the secret key (use with caution!).
    pub fn serialize(&self) -> Result<Vec<u8>, LedgerError> {
        let guard = self.inner.lock().map_err(|_| LedgerError::InvalidState("lock poisoned".into()))?;
        let sk = guard.as_ref().ok_or(LedgerError::KeysCleared)?;
        let mut res = Vec::new();
        sk.serialize(&mut res)?;
        Ok(res)
    }

    /// Securely clears the secret key from memory.
    pub fn clear(&self) {
        if let Ok(mut guard) = self.inner.lock() {
            *guard = None;
        }
    }
}
