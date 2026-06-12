//! Persisted SHIELDED (zswap) sync — the zswap analogue of
//! [`crate::dust::syncer::DustSyncer`].
//!
//! Subscribes to `zswapLedgerEvents`, folds them into a
//! `zswap::local::State` via [`super::snapshot::replay_zswap_events`],
//! and persists the result + an event-id checkpoint to redb so
//! subsequent calls resume from `last_id + 1`. The synced state carries
//! the wallet's spendable coins plus the commitment Merkle tree the
//! deposit balancer needs to build spend proofs.
//!
//! v1 keeps the fold inline (no producer/persist pipeline like DUST);
//! it runs as a background task during connect. A 3-stage pipeline is a
//! perf follow-up if PreProd's zswap log proves as heavy as its DUST log.

use std::sync::Arc;

use futures::StreamExt;
use serde_json::json;
use serialize::{tagged_deserialize, tagged_serialize};
use storage::DefaultDB;
use zswap::keys::{Seed, SecretKeys};
use zswap::local::State as ZswapState;

use super::ShieldedError;
use super::snapshot::{
    DecodedEvent, ZSWAP_LEDGER_EVENTS_QUERY, decode_event, replay_zswap_events,
    translate_transport_error,
};
use crate::network::Network;
use crate::store::{ShieldedSyncSnapshot, WalletStore};
use crate::unshielded::transport;

const IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Fold + persist boundary. Caps crash re-work to this many events;
/// each is cheap to re-fetch from the indexer.
const PERSIST_EVERY_N_EVENTS: usize = 2000;

/// Progress for a UI binding (mirrors the DUST `SyncProgress`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShieldedSyncProgress {
    pub current_id: i64,
    pub max_id: i64,
    pub events_processed: usize,
}

/// A wallet-owned spendable shielded coin, in display-friendly form
/// (no zswap types leak to the UI / bridge).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpendableCoin {
    /// Coin value in base units.
    pub value: u128,
    /// On-chain token color (hex of the `ShieldedTokenType`).
    pub color_hex: String,
    /// Merkle-tree commitment index.
    pub mt_index: u64,
}

/// List the wallet's currently-spendable shielded coins from a synced
/// state. Sorted ascending by value (smallest-first selection).
pub fn spendable_coins(state: &ZswapState<DefaultDB>) -> Vec<SpendableCoin> {
    let mut out: Vec<SpendableCoin> = state
        .coins
        .iter()
        .map(|(_nullifier, qci)| SpendableCoin {
            value: qci.value,
            color_hex: hex::encode(qci.type_.0.0),
            mt_index: qci.mt_index,
        })
        .collect();
    out.sort_by(|a, b| a.value.cmp(&b.value));
    out
}

/// Bridges the indexer's `zswapLedgerEvents` subscription and the
/// redb-backed shielded snapshot. Cheap to construct; keep one per
/// network.
pub struct ShieldedSyncer {
    network: Network,
    store: Arc<WalletStore>,
    keys: SecretKeys,
    ws_url: &'static str,
}

impl ShieldedSyncer {
    /// Construct from the wallet seed; the zswap `SecretKeys` (coin +
    /// encryption) are derived internally so the App doesn't need to
    /// name zswap types (mirrors how `DustSyncer::new` takes a derived
    /// key). The keys come from the HD `Zswap` child
    /// (`m/44'/2400'/0'/3/0`), matching `Wallet::from_seed` and the
    /// Midnight wallet SDK — so the decrypt recognises the wallet's
    /// on-chain coins.
    pub fn new(network: Network, store: Arc<WalletStore>, seed: [u8; 32]) -> Self {
        let zswap_seed =
            crate::hd::derive_child_priv(&seed, 0, crate::hd::Role::Zswap, 0).unwrap_or(seed);
        let keys = SecretKeys::from(Seed::from(zswap_seed));
        let ws_url = network.config().indexer_ws_url;
        Self {
            network,
            store,
            keys,
            ws_url,
        }
    }

    /// Read the most recent persisted state for this network (no
    /// network I/O). `None` before any sync has run.
    pub fn cached_state(
        &self,
    ) -> Result<Option<(ZswapState<DefaultDB>, i64)>, ShieldedError> {
        match self.store.get_shielded_sync(self.network) {
            Ok(Some(snap)) => {
                let state: ZswapState<DefaultDB> = tagged_deserialize(&snap.state_bytes[..])
                    .map_err(|e| ShieldedError::Replay(format!("decode cached state: {e}")))?;
                Ok(Some((state, snap.last_id)))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(ShieldedError::Store(e.to_string())),
        }
    }

    /// Full catch-up sync. Hydrates the cached state, subscribes from
    /// `last_id + 1`, folds + persists in batches, and returns the
    /// up-to-date `zswap::local::State`. Termination mirrors the DUST
    /// syncer: stop on the indexer's caught-up marker (`id >= max_id`)
    /// or after an idle gap.
    pub async fn sync(&self) -> Result<ZswapState<DefaultDB>, ShieldedError> {
        let (mut state, mut last_id) = match self.cached_state()? {
            Some(t) => t,
            None => (ZswapState::new(), -1),
        };
        let started_with_cache = last_id >= 0;

        tracing::info!(
            network = %self.network.config().network_id,
            resume_from = last_id + 1,
            "shielded syncer starting"
        );

        let raw = transport::subscribe(
            self.ws_url,
            ZSWAP_LEDGER_EVENTS_QUERY,
            json!({ "id": (last_id + 1).max(0) }),
        )
        .await
        .map_err(translate_transport_error)?;
        let mut stream = std::pin::pin!(raw.map(|item| {
            item.map_err(translate_transport_error)
                .and_then(|v| decode_event(&v))
        }));

        let mut batch: Vec<DecodedEvent> = Vec::with_capacity(PERSIST_EVERY_N_EVENTS);
        let mut events_processed: usize = 0;
        let mut max_id: i64 = -1;
        let mut saw_any = false;

        loop {
            if max_id >= 0 && last_id >= max_id {
                break;
            }
            match tokio::time::timeout(IDLE_TIMEOUT, stream.next()).await {
                Ok(Some(item)) => {
                    let d = item?;
                    saw_any = true;
                    last_id = d.id;
                    max_id = d.max_id;
                    batch.push(d);
                    if batch.len() >= PERSIST_EVERY_N_EVENTS {
                        state = self.fold_and_persist(state, &mut batch, last_id)?;
                        events_processed += PERSIST_EVERY_N_EVENTS;
                    }
                }
                Ok(None) | Err(_) => {
                    // Stream ended or went idle. If we ever saw events
                    // (or resumed from a populated cache) we're caught
                    // up; otherwise the indexer was unreachable.
                    if saw_any || started_with_cache {
                        break;
                    }
                    return Err(ShieldedError::StreamClosedEarly);
                }
            }
        }

        if !batch.is_empty() {
            events_processed += batch.len();
            state = self.fold_and_persist(state, &mut batch, last_id)?;
        } else if started_with_cache {
            // Already at tip — refresh the checkpoint's updated_at.
            self.persist(&state, last_id)?;
        }

        tracing::info!(
            network = %self.network.config().network_id,
            last_id,
            events_processed,
            coins = state.coins.size(),
            "shielded syncer caught up"
        );
        Ok(state)
    }

    fn fold_and_persist(
        &self,
        state: ZswapState<DefaultDB>,
        batch: &mut Vec<DecodedEvent>,
        last_id: i64,
    ) -> Result<ZswapState<DefaultDB>, ShieldedError> {
        let next = replay_zswap_events(&self.keys, state, batch.iter().map(|d| &d.event))?;
        batch.clear();
        self.persist(&next, last_id)?;
        Ok(next)
    }

    fn persist(&self, state: &ZswapState<DefaultDB>, last_id: i64) -> Result<(), ShieldedError> {
        let mut bytes = Vec::new();
        tagged_serialize(state, &mut bytes)
            .map_err(|e| ShieldedError::Replay(format!("serialize state: {e}")))?;
        self.store
            .put_shielded_sync(
                self.network,
                &ShieldedSyncSnapshot {
                    last_id,
                    state_bytes: bytes,
                    updated_at: now_ms(),
                },
            )
            .map_err(|e| ShieldedError::Store(e.to_string()))
    }
}

fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use coin_structure::coin::{Info as CoinInfo, ShieldedTokenType};
    use rand::{Rng, SeedableRng};
    use rand_chacha::ChaCha20Rng;

    /// Deterministic: insert two owned coins into a fresh zswap state
    /// (the same `insert_coin` the event fold drives) and confirm
    /// `spendable_coins` surfaces them with the right values, sorted
    /// ascending, each with a distinct Merkle index. The decrypt path
    /// off live `ZswapOutput` events is exercised in the live
    /// validation step (synthesising a full `ledger::events::Event`
    /// in a unit test isn't practical — same rationale the DUST
    /// snapshot tests give).
    #[test]
    fn spendable_coins_lists_inserted_coins_sorted() {
        let mut rng = ChaCha20Rng::seed_from_u64(42);
        let keys: SecretKeys = Seed::from([7u8; 32]).into();
        let tt = ShieldedTokenType(rng.r#gen());

        let bigger = CoinInfo {
            nonce: rng.r#gen(),
            type_: tt,
            value: 5_000_000,
        };
        let smaller = CoinInfo {
            nonce: rng.r#gen(),
            type_: tt,
            value: 1_000_000,
        };

        let state = ZswapState::<DefaultDB>::new()
            .insert_coin(&keys, bigger)
            .unwrap()
            .insert_coin(&keys, smaller)
            .unwrap();

        let coins = spendable_coins(&state);
        assert_eq!(coins.len(), 2);
        // Sorted ascending by value (smallest-first selection).
        assert_eq!(coins[0].value, 1_000_000);
        assert_eq!(coins[1].value, 5_000_000);
        // Distinct Merkle indices (assigned sequentially on insert).
        assert_ne!(coins[0].mt_index, coins[1].mt_index);
        // Same token color for both.
        assert_eq!(coins[0].color_hex, coins[1].color_hex);
    }
}
