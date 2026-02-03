// This file is part of midnight-ledger.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

//! ZSwap local state management for iOS bindings.

use crate::error::LedgerError;
use crate::events::Event;
use crate::keys::ZswapSecretKeys;
use crate::util::{from_hex_ser, to_hex_ser};
use coin_structure::coin::{Info as CoinInfo, PublicKey as CoinPublicKey, QualifiedInfo as LedgerQualifiedShieldedCoinInfo};
use ledger::semantics::ZswapLocalStateExt;
use rand::rngs::OsRng;
use serialize::{tagged_deserialize, tagged_serialize};
use std::sync::Arc;
use storage::db::InMemoryDB;
use transient_crypto::merkle_tree;
use transient_crypto::proofs::ProofPreimage;

/// A shielded coin info - contains info about a pending coin (not yet in merkle tree).
pub struct ShieldedCoinInfo {
    pub(crate) inner: CoinInfo,
}

impl ShieldedCoinInfo {
    /// Returns the token type as a hex-encoded string.
    pub fn token_type(&self) -> String {
        to_hex_ser(&self.inner.type_)
    }

    /// Returns the nonce as a hex-encoded string.
    pub fn nonce(&self) -> String {
        to_hex_ser(&self.inner.nonce)
    }

    /// Returns the value as a decimal string (u128).
    pub fn value(&self) -> String {
        self.inner.value.to_string()
    }

    /// Serializes the coin info to bytes.
    pub fn serialize(&self) -> Result<Vec<u8>, LedgerError> {
        let mut res = Vec::new();
        tagged_serialize(&self.inner, &mut res)?;
        Ok(res)
    }
}

/// A qualified shielded coin info - contains all info needed to spend a coin.
pub struct QualifiedShieldedCoinInfo {
    pub(crate) inner: LedgerQualifiedShieldedCoinInfo,
}

impl QualifiedShieldedCoinInfo {
    /// Returns the token type as a hex-encoded string.
    pub fn token_type(&self) -> String {
        to_hex_ser(&self.inner.type_)
    }

    /// Returns the nonce as a hex-encoded string.
    pub fn nonce(&self) -> String {
        to_hex_ser(&self.inner.nonce)
    }

    /// Returns the value as a decimal string (u128).
    pub fn value(&self) -> String {
        self.inner.value.to_string()
    }

    /// Returns the merkle tree index.
    pub fn mt_index(&self) -> u64 {
        self.inner.mt_index
    }

    /// Serializes the coin info to bytes.
    pub fn serialize(&self) -> Result<Vec<u8>, LedgerError> {
        let mut res = Vec::new();
        tagged_serialize(&self.inner, &mut res)?;
        Ok(res)
    }

    /// Deserializes a coin info from bytes.
    pub fn deserialize(raw: Vec<u8>) -> Result<Self, LedgerError> {
        let inner: LedgerQualifiedShieldedCoinInfo = tagged_deserialize(&mut &raw[..])?;
        Ok(QualifiedShieldedCoinInfo { inner })
    }
}

/// A collapsed merkle tree update for applying to local state.
pub struct MerkleTreeCollapsedUpdate {
    pub(crate) inner: merkle_tree::MerkleTreeCollapsedUpdate,
}

impl MerkleTreeCollapsedUpdate {
    /// Deserializes a collapsed update from bytes.
    pub fn deserialize(raw: Vec<u8>) -> Result<Self, LedgerError> {
        let inner: merkle_tree::MerkleTreeCollapsedUpdate = tagged_deserialize(&mut &raw[..])?;
        Ok(MerkleTreeCollapsedUpdate { inner })
    }

    /// Serializes the collapsed update to bytes.
    pub fn serialize(&self) -> Result<Vec<u8>, LedgerError> {
        let mut res = Vec::new();
        tagged_serialize(&self.inner, &mut res)?;
        Ok(res)
    }
}

/// Local state for ZSwap operations (tracking owned coins).
pub struct ZswapLocalState {
    pub(crate) inner: zswap::local::State<InMemoryDB>,
}

impl Default for ZswapLocalState {
    fn default() -> Self {
        ZswapLocalState {
            inner: zswap::local::State::new(),
        }
    }
}

impl ZswapLocalState {
    /// Creates a new empty ZSwap local state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the first free index in the merkle tree.
    pub fn first_free(&self) -> u64 {
        self.inner.first_free
    }

    /// Replays events to update the local state.
    pub fn replay_events(
        &self,
        secret_keys: Arc<ZswapSecretKeys>,
        events: Vec<Arc<Event>>,
    ) -> Result<Arc<ZswapLocalState>, LedgerError> {
        let sk = secret_keys.try_as_inner()?;
        let events_iter = events.iter().map(|e| &e.inner);
        let new_state = self.inner.replay_events(&sk, events_iter)?;
        Ok(Arc::new(ZswapLocalState { inner: new_state }))
    }

    /// Applies a collapsed merkle tree update.
    pub fn apply_collapsed_update(
        &self,
        update: Arc<MerkleTreeCollapsedUpdate>,
    ) -> Result<Arc<ZswapLocalState>, LedgerError> {
        let new_state = self.inner.apply_collapsed_update(&update.inner)?;
        Ok(Arc::new(ZswapLocalState { inner: new_state }))
    }

    /// Watches for a specific coin (adds it to pending outputs).
    pub fn watch_for(
        &self,
        coin_public_key: String,
        coin_info: Vec<u8>,
    ) -> Result<Arc<ZswapLocalState>, LedgerError> {
        let pk: CoinPublicKey = from_hex_ser(&coin_public_key)?;
        let coin: CoinInfo = tagged_deserialize(&mut &coin_info[..])?;
        let new_state = self.inner.watch_for(&pk, &coin);
        Ok(Arc::new(ZswapLocalState { inner: new_state }))
    }

    /// Serializes the local state to bytes.
    pub fn serialize(&self) -> Result<Vec<u8>, LedgerError> {
        let mut res = Vec::new();
        tagged_serialize(&self.inner, &mut res)?;
        Ok(res)
    }

    /// Deserializes a local state from bytes.
    pub fn deserialize(raw: Vec<u8>) -> Result<Self, LedgerError> {
        let inner: zswap::local::State<InMemoryDB> = tagged_deserialize(&mut &raw[..])?;
        Ok(ZswapLocalState { inner })
    }

    /// Returns a debug string representation.
    pub fn to_debug_string(&self) -> String {
        format!("{:#?}", &self.inner)
    }

    /// Returns the number of owned coins.
    pub fn coins_count(&self) -> u64 {
        self.inner.coins.iter().count() as u64
    }

    /// Returns all owned coins as serialized qualified coin info (hex strings).
    pub fn coins(&self) -> Result<Vec<String>, LedgerError> {
        let mut result = Vec::new();
        for (_, coin) in self.inner.coins.iter() {
            result.push(to_hex_ser(&coin));
        }
        Ok(result)
    }

    /// Returns all owned coins as QualifiedShieldedCoinInfo objects.
    pub fn coins_data(&self) -> Vec<Arc<QualifiedShieldedCoinInfo>> {
        self.inner
            .coins
            .iter()
            .map(|(_, coin)| {
                Arc::new(QualifiedShieldedCoinInfo { inner: (*coin).clone() })
            })
            .collect()
    }

    /// Returns pending spends as (nullifier_hex, QualifiedShieldedCoinInfo) pairs.
    /// These are coins that have been submitted for spending but not yet confirmed.
    pub fn pending_spends_data(&self) -> Vec<Arc<PendingSpendEntry>> {
        self.inner
            .pending_spends
            .iter()
            .map(|(nul, coin)| {
                Arc::new(PendingSpendEntry {
                    nullifier: to_hex_ser(&nul),
                    coin: Arc::new(QualifiedShieldedCoinInfo { inner: (*coin).clone() }),
                })
            })
            .collect()
    }

    /// Returns pending outputs as (commitment_hex, ShieldedCoinInfo) pairs.
    /// These are coins expected to be received but not yet confirmed on chain.
    pub fn pending_outputs_data(&self) -> Vec<Arc<PendingOutputEntry>> {
        self.inner
            .pending_outputs
            .iter()
            .map(|(cm, coin)| {
                Arc::new(PendingOutputEntry {
                    commitment: to_hex_ser(&cm),
                    coin: Arc::new(ShieldedCoinInfo { inner: (*coin).clone() }),
                })
            })
            .collect()
    }

    /// Spends a coin from this local state.
    ///
    /// Returns the updated local state and a ZSwap input that can be used to build a transaction.
    ///
    /// # Arguments
    /// * `secret_keys` - The ZSwap secret keys for signing
    /// * `coin` - The qualified coin info to spend (serialized bytes)
    /// * `segment` - Optional segment ID for the spend
    ///
    /// # Returns
    /// A tuple of (new_state, input) where:
    /// - new_state: The updated local state with the coin marked as pending spend
    /// - input: The ZSwap input containing the spend proof preimage
    pub fn spend(
        &self,
        secret_keys: Arc<ZswapSecretKeys>,
        coin: Vec<u8>,
        segment: Option<u16>,
    ) -> Result<Arc<ZswapSpendResult>, LedgerError> {
        let sk = secret_keys.try_as_inner()?;
        let coin_info: LedgerQualifiedShieldedCoinInfo = tagged_deserialize(&mut &coin[..])?;

        let (new_state, input) = self.inner.spend(&mut OsRng, &sk, &coin_info, segment)?;

        Ok(Arc::new(ZswapSpendResult {
            state: Arc::new(ZswapLocalState { inner: new_state }),
            input: Arc::new(ZswapInput { inner: input }),
        }))
    }
}

/// An entry in the pending spends map.
pub struct PendingSpendEntry {
    pub nullifier: String,
    pub coin: Arc<QualifiedShieldedCoinInfo>,
}

impl PendingSpendEntry {
    /// Returns the nullifier as a hex-encoded string.
    pub fn nullifier(&self) -> String {
        self.nullifier.clone()
    }

    /// Returns the coin info.
    pub fn coin(&self) -> Arc<QualifiedShieldedCoinInfo> {
        Arc::clone(&self.coin)
    }
}

/// An entry in the pending outputs map.
pub struct PendingOutputEntry {
    pub commitment: String,
    pub coin: Arc<ShieldedCoinInfo>,
}

impl PendingOutputEntry {
    /// Returns the commitment as a hex-encoded string.
    pub fn commitment(&self) -> String {
        self.commitment.clone()
    }

    /// Returns the coin info.
    pub fn coin(&self) -> Arc<ShieldedCoinInfo> {
        Arc::clone(&self.coin)
    }
}

/// A ZSwap input (spend proof preimage).
/// This is returned by spend() and contains the data needed to create a ZSwap transaction.
pub struct ZswapInput {
    pub(crate) inner: zswap::Input<ProofPreimage, InMemoryDB>,
}

impl ZswapInput {
    /// Returns the nullifier as a hex-encoded string.
    pub fn nullifier(&self) -> String {
        to_hex_ser(&self.inner.nullifier)
    }

    /// Returns the contract address as a hex-encoded string, if any.
    pub fn contract_address(&self) -> Option<String> {
        self.inner.contract_address.as_ref().map(|addr| {
            to_hex_ser(&**addr)
        })
    }

    /// Serializes the input to bytes.
    pub fn serialize(&self) -> Result<Vec<u8>, LedgerError> {
        let mut res = Vec::new();
        tagged_serialize(&self.inner, &mut res)?;
        Ok(res)
    }

    /// Deserializes an input from bytes.
    pub fn deserialize(raw: Vec<u8>) -> Result<Self, LedgerError> {
        let inner: zswap::Input<ProofPreimage, InMemoryDB> = tagged_deserialize(&mut &raw[..])?;
        Ok(ZswapInput { inner })
    }

    /// Returns a debug string representation.
    pub fn to_debug_string(&self) -> String {
        format!("{:#?}", &self.inner)
    }
}

/// Result of a spend operation: the new state and the input.
pub struct ZswapSpendResult {
    pub state: Arc<ZswapLocalState>,
    pub input: Arc<ZswapInput>,
}

impl ZswapSpendResult {
    /// Returns the new local state after the spend.
    pub fn state(&self) -> Arc<ZswapLocalState> {
        Arc::clone(&self.state)
    }

    /// Returns the ZSwap input (spend proof preimage).
    pub fn input(&self) -> Arc<ZswapInput> {
        Arc::clone(&self.input)
    }
}
