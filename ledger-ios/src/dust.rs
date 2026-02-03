// This file is part of midnight-ledger.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

//! Dust types for iOS bindings.

use crate::dust_state::DustParameters;
use crate::error::LedgerError;
use crate::util::{from_hex_ser, to_hex_ser};
use base_crypto::time::Timestamp;
use ledger::dust::{
    DustGenerationInfo as LedgerDustGenerationInfo, DustOutput as LedgerDustOutput,
    DustPublicKey as LedgerDustPublicKey, InitialNonce as LedgerInitialNonce,
    QualifiedDustOutput as LedgerQualifiedDustOutput,
};
use serialize::{tagged_deserialize, tagged_serialize};
use std::sync::Arc;

/// A dust public key derived from a dust secret key.
pub struct DustPublicKey {
    pub(crate) inner: LedgerDustPublicKey,
}

impl DustPublicKey {
    /// Returns the dust public key as a hex-encoded string (tagged serialization).
    pub fn to_hex(&self) -> String {
        to_hex_ser(&self.inner)
    }

    /// Returns the dust public key as a big-endian hex string for BigInt conversion.
    /// This matches the WASM API's format for DustSecretKey.publicKey.
    pub fn to_bigint_hex(&self) -> String {
        let mut bytes = self.inner.0.as_le_bytes();
        bytes.reverse(); // Convert to big-endian
        hex::encode(bytes)
    }

    /// Creates a DustPublicKey from a hex-encoded string.
    pub fn from_hex(hex: String) -> Result<Self, LedgerError> {
        let inner: LedgerDustPublicKey = from_hex_ser(&hex)?;
        Ok(DustPublicKey { inner })
    }

    /// Serializes the dust public key to bytes.
    pub fn serialize(&self) -> Result<Vec<u8>, LedgerError> {
        let mut buf = Vec::new();
        tagged_serialize(&self.inner, &mut buf)?;
        Ok(buf)
    }

    /// Deserializes a dust public key from bytes.
    pub fn deserialize(raw: Vec<u8>) -> Result<Self, LedgerError> {
        let inner: LedgerDustPublicKey = tagged_deserialize(&mut &raw[..])?;
        Ok(DustPublicKey { inner })
    }
}

/// Initial nonce for dust generation.
pub struct InitialNonce {
    pub(crate) inner: LedgerInitialNonce,
}

impl InitialNonce {
    /// Returns the initial nonce as a hex-encoded string.
    pub fn to_hex(&self) -> String {
        to_hex_ser(&self.inner)
    }

    /// Creates an InitialNonce from a hex-encoded string.
    pub fn from_hex(hex: String) -> Result<Self, LedgerError> {
        let inner: LedgerInitialNonce = from_hex_ser(&hex)?;
        Ok(InitialNonce { inner })
    }

    /// Serializes the initial nonce to bytes.
    pub fn serialize(&self) -> Result<Vec<u8>, LedgerError> {
        let mut buf = Vec::new();
        tagged_serialize(&self.inner, &mut buf)?;
        Ok(buf)
    }

    /// Deserializes an initial nonce from bytes.
    pub fn deserialize(raw: Vec<u8>) -> Result<Self, LedgerError> {
        let inner: LedgerInitialNonce = tagged_deserialize(&mut &raw[..])?;
        Ok(InitialNonce { inner })
    }
}

/// Information about dust generation for a backing NIGHT UTXO.
pub struct DustGenerationInfo {
    pub(crate) inner: LedgerDustGenerationInfo,
}

impl DustGenerationInfo {
    /// Creates a new DustGenerationInfo.
    /// - value: The backing NIGHT value (as decimal string for u128)
    /// - owner: The dust public key owner (hex-encoded)
    /// - nonce: The initial nonce (hex-encoded)
    /// - dtime: Decay start time in seconds since epoch (u64::MAX for never)
    pub fn new(
        value: String,
        owner: Arc<DustPublicKey>,
        nonce: Arc<InitialNonce>,
        dtime_seconds: u64,
    ) -> Result<Self, LedgerError> {
        let value: u128 = value.parse().map_err(|_| LedgerError::InvalidData)?;
        let dtime = Timestamp::from_secs(dtime_seconds);

        Ok(DustGenerationInfo {
            inner: LedgerDustGenerationInfo {
                value,
                owner: owner.inner,
                nonce: nonce.inner,
                dtime,
            },
        })
    }

    /// Returns the backing NIGHT value as a decimal string.
    pub fn value(&self) -> String {
        self.inner.value.to_string()
    }

    /// Returns the owner dust public key.
    pub fn owner(&self) -> Arc<DustPublicKey> {
        Arc::new(DustPublicKey {
            inner: self.inner.owner,
        })
    }

    /// Returns the initial nonce.
    pub fn nonce(&self) -> Arc<InitialNonce> {
        Arc::new(InitialNonce {
            inner: self.inner.nonce,
        })
    }

    /// Returns the decay start time in seconds since epoch.
    pub fn dtime_seconds(&self) -> u64 {
        self.inner.dtime.to_secs()
    }

    /// Serializes the generation info to bytes.
    pub fn serialize(&self) -> Result<Vec<u8>, LedgerError> {
        let mut buf = Vec::new();
        tagged_serialize(&self.inner, &mut buf)?;
        Ok(buf)
    }

    /// Deserializes generation info from bytes.
    pub fn deserialize(raw: Vec<u8>) -> Result<Self, LedgerError> {
        let inner: LedgerDustGenerationInfo = tagged_deserialize(&mut &raw[..])?;
        Ok(DustGenerationInfo { inner })
    }
}

/// A dust output (unqualified - without merkle tree index).
pub struct DustOutput {
    pub(crate) inner: LedgerDustOutput,
}

impl DustOutput {
    /// Creates a new DustOutput.
    /// - initial_value: Initial dust value as decimal string
    /// - owner: Dust public key owner (hex-encoded)
    /// - nonce: Random nonce (hex-encoded Fr field element)
    /// - seq: Sequence number (for re-spent dust)
    /// - ctime_seconds: Creation time in seconds since epoch
    pub fn new(
        initial_value: String,
        owner: Arc<DustPublicKey>,
        nonce: String,
        seq: u32,
        ctime_seconds: u64,
    ) -> Result<Self, LedgerError> {
        let initial_value: u128 = initial_value
            .parse()
            .map_err(|_| LedgerError::InvalidData)?;
        let nonce = from_hex_ser(&nonce)?;
        let ctime = Timestamp::from_secs(ctime_seconds);

        Ok(DustOutput {
            inner: LedgerDustOutput {
                initial_value,
                owner: owner.inner,
                nonce,
                seq,
                ctime,
            },
        })
    }

    /// Returns the initial dust value as a decimal string.
    pub fn initial_value(&self) -> String {
        self.inner.initial_value.to_string()
    }

    /// Returns the owner dust public key.
    pub fn owner(&self) -> Arc<DustPublicKey> {
        Arc::new(DustPublicKey {
            inner: self.inner.owner,
        })
    }

    /// Returns the nonce as a hex-encoded string.
    pub fn nonce(&self) -> String {
        to_hex_ser(&self.inner.nonce)
    }

    /// Returns the sequence number.
    pub fn seq(&self) -> u32 {
        self.inner.seq
    }

    /// Returns the creation time in seconds since epoch.
    pub fn ctime_seconds(&self) -> u64 {
        self.inner.ctime.to_secs()
    }

    /// Calculates the updated dust value at the given time.
    /// Returns the current value of the dust considering generation and decay.
    pub fn updated_value(
        &self,
        gen_info: Arc<DustGenerationInfo>,
        now_seconds: u64,
        params: Arc<DustParameters>,
    ) -> String {
        let now = Timestamp::from_secs(now_seconds);
        self.inner
            .updated_value(&gen_info.inner, now, &params.inner)
            .to_string()
    }

    /// Serializes the dust output to bytes.
    pub fn serialize(&self) -> Result<Vec<u8>, LedgerError> {
        let mut buf = Vec::new();
        tagged_serialize(&self.inner, &mut buf)?;
        Ok(buf)
    }

    /// Deserializes a dust output from bytes.
    pub fn deserialize(raw: Vec<u8>) -> Result<Self, LedgerError> {
        let inner: LedgerDustOutput = tagged_deserialize(&mut &raw[..])?;
        Ok(DustOutput { inner })
    }
}

/// A qualified dust output (with merkle tree index and backing NIGHT info).
pub struct QualifiedDustOutput {
    pub(crate) inner: LedgerQualifiedDustOutput,
}

impl QualifiedDustOutput {
    /// Creates a new QualifiedDustOutput.
    pub fn new(
        initial_value: String,
        owner: Arc<DustPublicKey>,
        nonce: String,
        seq: u32,
        ctime_seconds: u64,
        backing_night: Arc<InitialNonce>,
        mt_index: u64,
    ) -> Result<Self, LedgerError> {
        let initial_value: u128 = initial_value
            .parse()
            .map_err(|_| LedgerError::InvalidData)?;
        let nonce = from_hex_ser(&nonce)?;
        let ctime = Timestamp::from_secs(ctime_seconds);

        Ok(QualifiedDustOutput {
            inner: LedgerQualifiedDustOutput {
                initial_value,
                owner: owner.inner,
                nonce,
                seq,
                ctime,
                backing_night: backing_night.inner,
                mt_index,
            },
        })
    }

    /// Returns the initial dust value as a decimal string.
    pub fn initial_value(&self) -> String {
        self.inner.initial_value.to_string()
    }

    /// Returns the owner dust public key.
    pub fn owner(&self) -> Arc<DustPublicKey> {
        Arc::new(DustPublicKey {
            inner: self.inner.owner,
        })
    }

    /// Returns the nonce as a hex-encoded string.
    pub fn nonce(&self) -> String {
        to_hex_ser(&self.inner.nonce)
    }

    /// Returns the sequence number.
    pub fn seq(&self) -> u32 {
        self.inner.seq
    }

    /// Returns the creation time in seconds since epoch.
    pub fn ctime_seconds(&self) -> u64 {
        self.inner.ctime.to_secs()
    }

    /// Returns the backing NIGHT initial nonce.
    pub fn backing_night(&self) -> Arc<InitialNonce> {
        Arc::new(InitialNonce {
            inner: self.inner.backing_night,
        })
    }

    /// Returns the merkle tree index.
    pub fn mt_index(&self) -> u64 {
        self.inner.mt_index
    }

    /// Converts to an unqualified DustOutput.
    pub fn to_dust_output(&self) -> Arc<DustOutput> {
        Arc::new(DustOutput {
            inner: LedgerDustOutput::from(self.inner),
        })
    }

    /// Serializes the qualified dust output to bytes.
    pub fn serialize(&self) -> Result<Vec<u8>, LedgerError> {
        let mut buf = Vec::new();
        tagged_serialize(&self.inner, &mut buf)?;
        Ok(buf)
    }

    /// Deserializes a qualified dust output from bytes.
    pub fn deserialize(raw: Vec<u8>) -> Result<Self, LedgerError> {
        let inner: LedgerQualifiedDustOutput = tagged_deserialize(&mut &raw[..])?;
        Ok(QualifiedDustOutput { inner })
    }
}

/// Calculates the updated dust value at the given time.
///
/// This is a standalone function for calculating dust value without needing
/// to create a full DustOutput object.
///
/// # Arguments
/// * `initial_value` - Initial dust value as decimal string
/// * `ctime_seconds` - Creation time in seconds since epoch
/// * `gen_info` - Generation info for the backing NIGHT
/// * `now_seconds` - Current time in seconds since epoch
/// * `params` - Dust parameters
///
/// # Returns
/// The current dust value as a decimal string.
pub fn calculate_dust_value(
    initial_value: String,
    ctime_seconds: u64,
    gen_info: Arc<DustGenerationInfo>,
    now_seconds: u64,
    params: Arc<DustParameters>,
) -> Result<String, LedgerError> {
    let initial_value: u128 = initial_value
        .parse()
        .map_err(|_| LedgerError::InvalidData)?;
    let ctime = Timestamp::from_secs(ctime_seconds);
    let now = Timestamp::from_secs(now_seconds);

    // Create a temporary DustOutput with a dummy nonce and owner to calculate value
    // The nonce and owner don't affect the value calculation
    let dummy_output = LedgerDustOutput {
        initial_value,
        owner: gen_info.inner.owner,
        nonce: Default::default(),
        seq: 0,
        ctime,
    };

    Ok(dummy_output
        .updated_value(&gen_info.inner, now, &params.inner)
        .to_string())
}
