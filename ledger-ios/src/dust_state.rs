// This file is part of midnight-ledger.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

//! Dust local state management for iOS bindings.

use crate::dust::{DustGenerationInfo, QualifiedDustOutput};
use crate::error::LedgerError;
use crate::events::Event;
use crate::keys::DustSecretKey;
use crate::util::to_hex_ser;
use base_crypto::time::Timestamp;
use ledger::dust::{
    DustLocalState as LedgerDustLocalState, DustParameters as LedgerDustParameters,
    DustSpend as LedgerDustSpend,
};
use ledger::structure::ProofPreimageMarker;
use serialize::{tagged_deserialize, tagged_serialize};
use std::sync::Arc;
use storage::db::InMemoryDB;

/// Parameters for dust operations.
pub struct DustParameters {
    pub(crate) inner: LedgerDustParameters,
}

impl DustParameters {
    /// Creates dust parameters with the given values.
    /// - night_dust_ratio: Ratio of NIGHT to dust
    /// - generation_decay_rate: Rate of decay for dust generation
    /// - dust_grace_period_seconds: Grace period in seconds
    pub fn new(
        night_dust_ratio: u64,
        generation_decay_rate: u32,
        dust_grace_period_seconds: i64,
    ) -> Self {
        DustParameters {
            inner: LedgerDustParameters {
                night_dust_ratio,
                generation_decay_rate,
                dust_grace_period: base_crypto::time::Duration::from_secs(
                    dust_grace_period_seconds as i128,
                ),
            },
        }
    }

    /// Returns the NIGHT to dust ratio.
    pub fn night_dust_ratio(&self) -> u64 {
        self.inner.night_dust_ratio
    }

    /// Returns the generation decay rate.
    pub fn generation_decay_rate(&self) -> u32 {
        self.inner.generation_decay_rate
    }

    /// Returns the dust grace period in seconds.
    pub fn dust_grace_period_seconds(&self) -> i64 {
        self.inner.dust_grace_period.as_seconds() as i64
    }

    /// Serializes the parameters to bytes.
    pub fn serialize(&self) -> Result<Vec<u8>, LedgerError> {
        let mut res = Vec::new();
        tagged_serialize(&self.inner, &mut res)?;
        Ok(res)
    }

    /// Deserializes parameters from bytes.
    pub fn deserialize(raw: Vec<u8>) -> Result<Self, LedgerError> {
        let inner: LedgerDustParameters = tagged_deserialize(&mut &raw[..])?;
        Ok(DustParameters { inner })
    }
}

/// Local state for dust operations (tracking dust UTXOs).
pub struct DustLocalState {
    pub(crate) inner: LedgerDustLocalState<InMemoryDB>,
}

impl DustLocalState {
    /// Creates a new dust local state with the given parameters.
    pub fn new(params: Arc<DustParameters>) -> Self {
        DustLocalState {
            inner: LedgerDustLocalState::new(params.inner),
        }
    }

    /// Returns the wallet balance at the given time (in seconds since epoch).
    pub fn wallet_balance(&self, time_seconds: u64) -> String {
        let time = Timestamp::from_secs(time_seconds);
        self.inner.wallet_balance(time).to_string()
    }

    /// Returns the sync time in seconds since epoch.
    pub fn sync_time_seconds(&self) -> u64 {
        self.inner.sync_time.to_secs()
    }

    /// Processes time-to-live values, removing expired UTXOs.
    /// Returns a new state with expired UTXOs removed.
    pub fn process_ttls(&self, time_seconds: u64) -> Arc<DustLocalState> {
        let time = Timestamp::from_secs(time_seconds);
        Arc::new(DustLocalState {
            inner: self.inner.process_ttls(time),
        })
    }

    /// Replays events to update the local state.
    pub fn replay_events(
        &self,
        secret_key: Arc<DustSecretKey>,
        events: Vec<Arc<Event>>,
    ) -> Result<Arc<DustLocalState>, LedgerError> {
        let sk = secret_key.try_as_inner()?;
        let events_iter = events.iter().map(|e| &e.inner);
        let new_state = self.inner.replay_events(&sk, events_iter)?;
        Ok(Arc::new(DustLocalState { inner: new_state }))
    }

    /// Serializes the local state to bytes.
    pub fn serialize(&self) -> Result<Vec<u8>, LedgerError> {
        let mut res = Vec::new();
        tagged_serialize(&self.inner, &mut res)?;
        Ok(res)
    }

    /// Deserializes a local state from bytes.
    pub fn deserialize(raw: Vec<u8>) -> Result<Self, LedgerError> {
        let inner: LedgerDustLocalState<InMemoryDB> = tagged_deserialize(&mut &raw[..])?;
        Ok(DustLocalState { inner })
    }

    /// Returns a debug string representation.
    pub fn to_debug_string(&self) -> String {
        format!("{:#?}", &self.inner)
    }

    /// Returns the number of UTXOs.
    pub fn utxos_count(&self) -> u64 {
        self.inner.utxos().count() as u64
    }

    /// Returns the dust parameters.
    pub fn params(&self) -> Arc<DustParameters> {
        Arc::new(DustParameters {
            inner: self.inner.params,
        })
    }

    /// Returns all tracked dust UTXOs as QualifiedDustOutput objects.
    pub fn utxos(&self) -> Vec<Arc<QualifiedDustOutput>> {
        self.inner
            .utxos()
            .map(|utxo| {
                Arc::new(QualifiedDustOutput {
                    inner: utxo,
                })
            })
            .collect()
    }

    /// Returns the generation info for a qualified dust output, if available.
    pub fn generation_info(
        &self,
        utxo: Arc<QualifiedDustOutput>,
    ) -> Option<Arc<DustGenerationInfo>> {
        self.inner.generation_info(&utxo.inner).map(|info| {
            Arc::new(DustGenerationInfo { inner: info })
        })
    }

    /// Spends a dust UTXO.
    ///
    /// Returns the updated local state and a dust spend that can be used to build a transaction.
    ///
    /// # Arguments
    /// * `secret_key` - The dust secret key for signing
    /// * `utxo` - The qualified dust output to spend
    /// * `v_fee` - The fee value (as a decimal string, u128)
    /// * `ctime_seconds` - The creation time in seconds since epoch
    ///
    /// # Returns
    /// A DustSpendResult containing the new state and the dust spend.
    pub fn spend(
        &self,
        secret_key: Arc<DustSecretKey>,
        utxo: Arc<QualifiedDustOutput>,
        v_fee: String,
        ctime_seconds: u64,
    ) -> Result<Arc<DustSpendResult>, LedgerError> {
        let sk = secret_key.try_as_inner()?;
        let v_fee_value: u128 = v_fee.parse().map_err(|_| LedgerError::InvalidData)?;
        let ctime = Timestamp::from_secs(ctime_seconds);

        let (new_state, dust_spend) = self.inner.spend(&sk, &utxo.inner, v_fee_value, ctime)?;

        Ok(Arc::new(DustSpendResult {
            state: Arc::new(DustLocalState { inner: new_state }),
            spend: Arc::new(DustSpend { inner: dust_spend }),
        }))
    }
}

/// A dust spend (spend proof preimage).
/// This is returned by spend() and contains the data needed to create a dust transaction.
pub struct DustSpend {
    pub(crate) inner: LedgerDustSpend<ProofPreimageMarker, InMemoryDB>,
}

impl DustSpend {
    /// Returns the fee value as a decimal string.
    pub fn v_fee(&self) -> String {
        self.inner.v_fee.to_string()
    }

    /// Returns the old nullifier as a hex-encoded string.
    pub fn old_nullifier(&self) -> String {
        to_hex_ser(&self.inner.old_nullifier)
    }

    /// Returns the new commitment as a hex-encoded string.
    pub fn new_commitment(&self) -> String {
        to_hex_ser(&self.inner.new_commitment)
    }

    /// Serializes the dust spend to bytes.
    pub fn serialize(&self) -> Result<Vec<u8>, LedgerError> {
        let mut res = Vec::new();
        tagged_serialize(&self.inner, &mut res)?;
        Ok(res)
    }

    /// Deserializes a dust spend from bytes.
    pub fn deserialize(raw: Vec<u8>) -> Result<Self, LedgerError> {
        let inner: LedgerDustSpend<ProofPreimageMarker, InMemoryDB> = tagged_deserialize(&mut &raw[..])?;
        Ok(DustSpend { inner })
    }

    /// Returns a debug string representation.
    pub fn to_debug_string(&self) -> String {
        format!("{:#?}", &self.inner)
    }
}

/// Result of a dust spend operation: the new state and the spend.
pub struct DustSpendResult {
    pub state: Arc<DustLocalState>,
    pub spend: Arc<DustSpend>,
}

impl DustSpendResult {
    /// Returns the new local state after the spend.
    pub fn state(&self) -> Arc<DustLocalState> {
        Arc::clone(&self.state)
    }

    /// Returns the dust spend.
    pub fn spend(&self) -> Arc<DustSpend> {
        Arc::clone(&self.spend)
    }
}
