// This file is part of midnight-ledger.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

//! Ledger parameters for iOS bindings.

use crate::dust_state::DustParameters;
use crate::error::LedgerError;
use ledger::structure::{
    LedgerParameters as LedgerParametersInner, INITIAL_PARAMETERS,
};
use serialize::{tagged_deserialize, tagged_serialize};
use std::sync::Arc;

/// Network-wide ledger parameters for fee calculation and limits.
pub struct LedgerParameters {
    pub(crate) inner: LedgerParametersInner,
}

impl LedgerParameters {
    /// Returns the initial (genesis) ledger parameters.
    pub fn initial() -> Self {
        LedgerParameters {
            inner: INITIAL_PARAMETERS,
        }
    }

    /// Deserializes ledger parameters from bytes.
    pub fn deserialize(raw: Vec<u8>) -> Result<Self, LedgerError> {
        let inner: LedgerParametersInner = tagged_deserialize(&mut &raw[..])?;
        Ok(LedgerParameters { inner })
    }

    /// Serializes the ledger parameters to bytes.
    pub fn serialize(&self) -> Result<Vec<u8>, LedgerError> {
        let mut buf = Vec::new();
        tagged_serialize(&self.inner, &mut buf)?;
        Ok(buf)
    }

    /// Returns the dust parameters.
    pub fn dust_params(&self) -> Arc<DustParameters> {
        Arc::new(DustParameters {
            inner: self.inner.dust,
        })
    }

    /// Returns the global TTL (time-to-live) in seconds.
    pub fn global_ttl_seconds(&self) -> i64 {
        self.inner.global_ttl.as_seconds() as i64
    }

    /// Returns the transaction byte limit.
    pub fn transaction_byte_limit(&self) -> u64 {
        self.inner.limits.transaction_byte_limit
    }

    /// Returns the Cardano to Midnight bridge fee in basis points (0-10000).
    pub fn cardano_bridge_fee_basis_points(&self) -> u32 {
        self.inner.cardano_to_midnight_bridge_fee_basis_points
    }

    /// Returns the minimum amount for Cardano to Midnight bridge (in atomic units).
    pub fn cardano_bridge_min_amount(&self) -> String {
        self.inner.c_to_m_bridge_min_amount.to_string()
    }

    // Fee price accessors - converted to f64 for easy use

    /// Returns the overall fee price (DUST per full block).
    pub fn fee_overall_price(&self) -> f64 {
        f64::from(self.inner.fee_prices.overall_price)
    }

    /// Returns the read factor for fee calculation.
    pub fn fee_read_factor(&self) -> f64 {
        f64::from(self.inner.fee_prices.read_factor)
    }

    /// Returns the compute factor for fee calculation.
    pub fn fee_compute_factor(&self) -> f64 {
        f64::from(self.inner.fee_prices.compute_factor)
    }

    /// Returns the block usage factor for fee calculation.
    pub fn fee_block_usage_factor(&self) -> f64 {
        f64::from(self.inner.fee_prices.block_usage_factor)
    }

    /// Returns the write factor for fee calculation.
    pub fn fee_write_factor(&self) -> f64 {
        f64::from(self.inner.fee_prices.write_factor)
    }

    /// Returns the minimum claimable rewards amount (in atomic units).
    pub fn min_claimable_rewards(&self) -> String {
        self.inner.min_claimable_rewards().to_string()
    }

    /// Returns a debug string representation.
    pub fn to_debug_string(&self) -> String {
        format!("{:#?}", &self.inner)
    }
}
