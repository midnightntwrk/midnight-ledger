// This file is part of midnight-ledger.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

//! Event types for iOS bindings.

use crate::error::LedgerError;
use crate::util::to_hex_ser;
use ledger::events::{Event as LedgerEvent, EventDetails};
use serialize::{tagged_deserialize, tagged_serialize};
use std::sync::Arc;
use storage::db::InMemoryDB;

/// Event source information.
pub struct EventSource {
    pub(crate) inner: ledger::events::EventSource,
}

impl EventSource {
    /// Returns the transaction hash as a hex string.
    pub fn transaction_hash(&self) -> String {
        to_hex_ser(&self.inner.transaction_hash)
    }

    /// Returns the logical segment number.
    pub fn logical_segment(&self) -> u16 {
        self.inner.logical_segment
    }

    /// Returns the physical segment number.
    pub fn physical_segment(&self) -> u16 {
        self.inner.physical_segment
    }
}

/// A ledger event that can be used to update local state.
pub struct Event {
    pub(crate) inner: LedgerEvent<InMemoryDB>,
}

impl Event {
    /// Deserializes an event from bytes.
    pub fn deserialize(raw: Vec<u8>) -> Result<Self, LedgerError> {
        let inner: LedgerEvent<InMemoryDB> = tagged_deserialize(&mut &raw[..])?;
        Ok(Event { inner })
    }

    /// Serializes the event to bytes.
    pub fn serialize(&self) -> Result<Vec<u8>, LedgerError> {
        let mut res = Vec::new();
        tagged_serialize(&self.inner, &mut res)?;
        Ok(res)
    }

    /// Returns the event type as a string.
    /// Possible values: "zswap_input", "zswap_output", "contract_deploy",
    /// "contract_log", "param_change", "dust_initial_utxo",
    /// "dust_generation_dtime_update", "dust_spend_processed", "unknown"
    pub fn event_type(&self) -> String {
        match &self.inner.content {
            EventDetails::ZswapInput { .. } => "zswap_input".to_string(),
            EventDetails::ZswapOutput { .. } => "zswap_output".to_string(),
            EventDetails::ContractDeploy { .. } => "contract_deploy".to_string(),
            EventDetails::ContractLog { .. } => "contract_log".to_string(),
            EventDetails::ParamChange(_) => "param_change".to_string(),
            EventDetails::DustInitialUtxo { .. } => "dust_initial_utxo".to_string(),
            EventDetails::DustGenerationDtimeUpdate { .. } => {
                "dust_generation_dtime_update".to_string()
            }
            EventDetails::DustSpendProcessed { .. } => "dust_spend_processed".to_string(),
            _ => "unknown".to_string(),
        }
    }

    /// Returns the event source information.
    pub fn source(&self) -> Arc<EventSource> {
        Arc::new(EventSource {
            inner: self.inner.source.clone(),
        })
    }

    /// Returns true if this is a zswap (shielded coin) event.
    pub fn is_zswap_event(&self) -> bool {
        matches!(
            &self.inner.content,
            EventDetails::ZswapInput { .. } | EventDetails::ZswapOutput { .. }
        )
    }

    /// Returns true if this is a dust event.
    pub fn is_dust_event(&self) -> bool {
        matches!(
            &self.inner.content,
            EventDetails::DustInitialUtxo { .. }
                | EventDetails::DustGenerationDtimeUpdate { .. }
                | EventDetails::DustSpendProcessed { .. }
        )
    }

    /// Returns true if this is a contract event.
    pub fn is_contract_event(&self) -> bool {
        matches!(
            &self.inner.content,
            EventDetails::ContractDeploy { .. } | EventDetails::ContractLog { .. }
        )
    }

    /// Returns true if this is a parameter change event.
    pub fn is_param_change_event(&self) -> bool {
        matches!(&self.inner.content, EventDetails::ParamChange(_))
    }

    /// For ZswapInput events, returns the nullifier as hex string.
    /// Returns None for other event types.
    pub fn zswap_input_nullifier(&self) -> Option<String> {
        if let EventDetails::ZswapInput { nullifier, .. } = &self.inner.content {
            Some(to_hex_ser(nullifier))
        } else {
            None
        }
    }

    /// For ZswapOutput events, returns the commitment as hex string.
    /// Returns None for other event types.
    pub fn zswap_output_commitment(&self) -> Option<String> {
        if let EventDetails::ZswapOutput { commitment, .. } = &self.inner.content {
            Some(to_hex_ser(commitment))
        } else {
            None
        }
    }

    /// For ZswapOutput events, returns the merkle tree index.
    /// Returns None for other event types.
    pub fn zswap_output_mt_index(&self) -> Option<u64> {
        if let EventDetails::ZswapOutput { mt_index, .. } = &self.inner.content {
            Some(*mt_index)
        } else {
            None
        }
    }

    /// Returns a debug string representation.
    pub fn to_debug_string(&self) -> String {
        format!("{:#?}", &self.inner)
    }
}
