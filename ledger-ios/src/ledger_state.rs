// This file is part of midnight-ledger.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

//! Ledger state management for iOS bindings.

use crate::error::LedgerError;
use ledger::structure::LedgerState as LedgerLedgerState;
use serialize::{tagged_deserialize, tagged_serialize};
use storage::db::InMemoryDB;

/// The full ledger state (for validation/testing purposes).
pub struct LedgerState {
    pub(crate) inner: LedgerLedgerState<InMemoryDB>,
}

impl LedgerState {
    /// Creates a blank ledger state for the given network.
    pub fn blank(network_id: String) -> Self {
        LedgerState {
            inner: LedgerLedgerState::new(network_id),
        }
    }

    /// Returns the network ID.
    pub fn network_id(&self) -> String {
        self.inner.network_id.clone()
    }

    /// Serializes the ledger state to bytes.
    pub fn serialize(&self) -> Result<Vec<u8>, LedgerError> {
        let mut res = Vec::new();
        tagged_serialize(&self.inner, &mut res)?;
        Ok(res)
    }

    /// Deserializes a ledger state from bytes.
    pub fn deserialize(raw: Vec<u8>) -> Result<Self, LedgerError> {
        let inner: LedgerLedgerState<InMemoryDB> = tagged_deserialize(&mut &raw[..])?;
        Ok(LedgerState { inner })
    }

    /// Returns a debug string representation.
    pub fn to_debug_string(&self) -> String {
        format!("{:#?}", &self.inner)
    }
}
