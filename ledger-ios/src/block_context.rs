// This file is part of midnight-ledger.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

//! Block context for iOS bindings.

use crate::error::LedgerError;
use crate::util::to_hex_ser;
use base_crypto::time::Timestamp;
use onchain_runtime::context::BlockContext as LedgerBlockContext;
use serialize::{tagged_deserialize, tagged_serialize};

/// Block context containing blockchain timing and hash information.
pub struct BlockContext {
    pub(crate) inner: LedgerBlockContext,
}

impl BlockContext {
    /// Creates a new BlockContext with the given parameters.
    /// - tblock_seconds: Block timestamp in seconds since epoch
    /// - tblock_err: Block timestamp error margin
    /// - parent_block_hash: Hex-encoded hash of the parent block
    pub fn new(
        tblock_seconds: u64,
        tblock_err: u32,
        parent_block_hash: String,
    ) -> Result<Self, LedgerError> {
        let hash_bytes = hex::decode(&parent_block_hash).map_err(|_| LedgerError::InvalidData)?;
        if hash_bytes.len() != 32 {
            return Err(LedgerError::InvalidData);
        }
        let mut hash_arr = [0u8; 32];
        hash_arr.copy_from_slice(&hash_bytes);

        Ok(BlockContext {
            inner: LedgerBlockContext {
                tblock: Timestamp::from_secs(tblock_seconds),
                tblock_err,
                parent_block_hash: base_crypto::hash::HashOutput(hash_arr),
            },
        })
    }

    /// Creates a default BlockContext with zero values.
    pub fn default_context() -> Self {
        BlockContext {
            inner: LedgerBlockContext::default(),
        }
    }

    /// Creates a BlockContext with just the block time.
    pub fn with_time(tblock_seconds: u64) -> Self {
        BlockContext {
            inner: LedgerBlockContext {
                tblock: Timestamp::from_secs(tblock_seconds),
                ..LedgerBlockContext::default()
            },
        }
    }

    /// Returns the block timestamp in seconds since epoch.
    pub fn tblock_seconds(&self) -> u64 {
        self.inner.tblock.to_secs()
    }

    /// Returns the block timestamp error margin.
    pub fn tblock_err(&self) -> u32 {
        self.inner.tblock_err
    }

    /// Returns the parent block hash as a hex-encoded string.
    pub fn parent_block_hash(&self) -> String {
        to_hex_ser(&self.inner.parent_block_hash)
    }

    /// Serializes the block context to bytes.
    pub fn serialize(&self) -> Result<Vec<u8>, LedgerError> {
        let mut buf = Vec::new();
        tagged_serialize(&self.inner, &mut buf)?;
        Ok(buf)
    }

    /// Deserializes a block context from bytes.
    pub fn deserialize(raw: Vec<u8>) -> Result<Self, LedgerError> {
        let inner: LedgerBlockContext = tagged_deserialize(&mut &raw[..])?;
        Ok(BlockContext { inner })
    }

    /// Returns a debug string representation.
    pub fn to_debug_string(&self) -> String {
        format!("{:#?}", &self.inner)
    }
}
