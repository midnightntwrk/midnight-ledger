// This file is part of midnight-ledger.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

//! Error types for the iOS bindings.

use thiserror::Error;

/// Errors that can occur in the ledger iOS bindings.
#[derive(Debug, Error, Clone)]
pub enum LedgerError {
    #[error("Invalid data provided")]
    InvalidData,

    #[error("Invalid seed: expected 32 bytes")]
    InvalidSeed,

    #[error("Secret keys were cleared")]
    KeysCleared,

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Deserialization error")]
    DeserializationError,

    #[error("Cryptographic operation failed: {0}")]
    CryptoError(String),

    #[error("Transaction error: {0}")]
    TransactionError(String),

    #[error("Invalid state: {0}")]
    InvalidState(String),

    #[error("Not implemented: {0}")]
    NotImplemented(String),
}

impl From<std::io::Error> for LedgerError {
    fn from(e: std::io::Error) -> Self {
        LedgerError::SerializationError(e.to_string())
    }
}

impl From<hex::FromHexError> for LedgerError {
    fn from(_: hex::FromHexError) -> Self {
        LedgerError::InvalidData
    }
}

impl From<ledger::error::EventReplayError> for LedgerError {
    fn from(e: ledger::error::EventReplayError) -> Self {
        LedgerError::InvalidState(format!("Event replay error: {}", e))
    }
}

impl From<transient_crypto::merkle_tree::InvalidUpdate> for LedgerError {
    fn from(e: transient_crypto::merkle_tree::InvalidUpdate) -> Self {
        LedgerError::InvalidState(format!("Invalid merkle tree update: {}", e))
    }
}

impl From<ledger::dust::DustSpendError> for LedgerError {
    fn from(e: ledger::dust::DustSpendError) -> Self {
        LedgerError::TransactionError(format!("Dust spend error: {}", e))
    }
}

impl From<zswap::error::OfferCreationFailed> for LedgerError {
    fn from(e: zswap::error::OfferCreationFailed) -> Self {
        LedgerError::TransactionError(format!("Offer creation failed: {}", e))
    }
}
