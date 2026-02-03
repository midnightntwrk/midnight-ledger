// This file is part of midnight-ledger.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

//! Utility functions for serialization and hex encoding.

use crate::error::LedgerError;
use serialize::{Deserializable, Serializable};

/// Serializes a value and hex-encodes it.
pub fn to_hex_ser<T: Serializable>(value: &T) -> String {
    let mut buf = Vec::new();
    value
        .serialize(&mut buf)
        .expect("serialization should not fail");
    hex::encode(buf)
}

/// Deserializes a value from a hex-encoded string.
pub fn from_hex_ser<T: Deserializable>(s: &str) -> Result<T, LedgerError> {
    let bytes = hex::decode(s).map_err(|_| LedgerError::InvalidData)?;
    T::deserialize(&mut &bytes[..], 0).map_err(|_| LedgerError::DeserializationError)
}

/// Hex-encodes raw bytes.
pub fn to_hex(bytes: &[u8]) -> String {
    hex::encode(bytes)
}

/// Decodes a hex string to bytes.
pub fn from_hex(s: &str) -> Result<Vec<u8>, LedgerError> {
    hex::decode(s).map_err(|_| LedgerError::InvalidData)
}
