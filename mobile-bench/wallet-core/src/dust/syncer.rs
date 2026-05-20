//! Persisted DUST sync — Phase 2 of Path B (see `mobile-bench/BACKLOG.md`).
//!
//! On PreProd the operator's wallet has ~534k accumulated
//! `dustLedgerEvents`. The previous `crate::dust::snapshot::snapshot`
//! path replayed all of them on every wallet operation, dominating
//! per-write wall-clock. This module reuses the same event
//! decoder + replay primitive but **persists** the resulting state
//! plus an event-id checkpoint to redb, so subsequent calls resume
//! from `last_id + 1` and apply only the delta.
//!
//! Public API surface (this is what the App holds):
//!
//! - [`DustSyncer::new`] — construct with `Network`, `WalletStore`,
//!   wallet's `DustSecretKey`.
//! - [`DustSyncer::cached_state`] — return the most recent
//!   persisted snapshot without touching the network. Cheap.
//! - [`DustSyncer::sync`] — full catch-up stream. Yields
//!   `SyncProgress` as events are folded in; the cached state is
//!   updated as a side effect. UI binds the stream to a progress
//!   bar.
//!
//! The wallet's `sync_dust()` itself doesn't change in this slice
//! (Phase 3) — that's a follow-up. For now the App can drive the
//! syncer explicitly and pass the resulting state to the wallet's
//! existing surface (Phase 3 wires it into `Wallet::sync_dust`).

// Phase 2 only — the public API surface is built here but no
// caller exists yet. Phase 3 (wallet integration) and Phase 4
// (UI button) wire it up. Strip these once consumers land.
#![allow(dead_code)]

use std::sync::Arc;

use futures::{Stream, StreamExt};
use ledger::dust::{DustLocalState, DustParameters, DustSecretKey};
use serialize::{tagged_deserialize, tagged_serialize};
use serde_json::json;
use storage::DefaultDB;

use super::DustError;
use super::snapshot::{DUST_LEDGER_EVENTS_QUERY, decode_event};
use crate::network::Network;
use crate::store::{DustSyncSnapshot, WalletStore};
use crate::unshielded::transport;

/// Idle timeout matching `snapshot::IDLE_TIMEOUT` — if the stream
/// goes quiet for this long after we've seen at least one event,
/// we treat the subscription as "caught up" and exit cleanly.
const IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Persistence cadence. We don't write on every event because
/// each redb txn is ~ms-cost and PreProd's stream lands events at
/// kHz rates during cold replay. Persisting every N events caps
/// the worst-case "lost work" window on a crash to N events
/// (each is cheap to re-fetch from the indexer anyway).
const PERSIST_EVERY_N_EVENTS: usize = 1024;

/// Progress event the UI binds to. Emitted at each persist
/// boundary so the progress bar updates without flickering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyncProgress {
    /// Last indexer event id we've folded into the state.
    pub current_id: i64,
    /// Indexer's own "max id" marker. When `current_id` reaches
    /// `max_id`, the stream is caught up. Both are -1 until the
    /// first event lands.
    pub max_id: i64,
    /// Total events processed in this run. Lets the UI render
    /// "Indexing 234,567 / 534,302" without recomputing
    /// `current_id - cached.last_id`.
    pub events_processed: usize,
}

/// Owns the bridge between the indexer's `dustLedgerEvents`
/// subscription and the redb-backed DUST snapshot table. Cheap
/// to construct (no work happens until `sync()` is called); the
/// App can keep one per network.
pub struct DustSyncer {
    network: Network,
    store: Arc<WalletStore>,
    dust_key: DustSecretKey,
    params: DustParameters,
    ws_url: &'static str,
}

impl DustSyncer {
    /// `dust_key` is `Wallet::dust_secret_key()` — the syncer needs
    /// it to derive nullifiers when folding `DustSpend` events.
    /// `params` defaults to `INITIAL_PARAMETERS.dust`; Phase 3 will
    /// thread live tip params here if the chain retunes them.
    pub fn new(
        network: Network,
        store: Arc<WalletStore>,
        dust_key: DustSecretKey,
    ) -> Self {
        let params = ledger::structure::INITIAL_PARAMETERS.dust;
        let ws_url = network.config().indexer_ws_url;
        Self {
            network,
            store,
            dust_key,
            params,
            ws_url,
        }
    }

    /// Read the most recent persisted state for this network.
    /// `None` if no row has been written yet (first launch or
    /// after `WalletStore::clear_dust_sync`). Cheap read txn.
    pub fn cached_state(
        &self,
    ) -> Result<Option<(DustLocalState<DefaultDB>, i64)>, DustError> {
        match self.store.get_dust_sync(self.network) {
            Ok(Some(snap)) => {
                let state: DustLocalState<DefaultDB> =
                    tagged_deserialize(&snap.state_bytes[..]).map_err(|e| {
                        DustError::Replay(format!("decode cached state: {e}"))
                    })?;
                Ok(Some((state, snap.last_id)))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(DustError::Replay(format!("store get: {e}"))),
        }
    }

    /// Run a full catch-up sync. Hydrates the cached state if
    /// present, subscribes from `last_id + 1`, folds events, and
    /// persists the updated snapshot. Yields `SyncProgress` at
    /// each persist boundary so the UI can render a progress bar.
    ///
    /// Termination: stops when the stream's `max_id` is reached
    /// (the indexer's own "caught up" signal) or the stream goes
    /// idle for `IDLE_TIMEOUT` — same termination logic as
    /// `snapshot::fold_events`.
    pub fn sync(
        self: Arc<Self>,
    ) -> impl Stream<Item = Result<SyncProgress, DustError>> + Send + 'static
    {
        async_stream::try_stream! {
            // 1. Load cached state (or start fresh).
            let (mut state, mut last_id) = match self.cached_state()? {
                Some(t) => t,
                None => (DustLocalState::new(self.params), -1),
            };

            tracing::info!(
                network = %self.network.config().network_id,
                resume_from = last_id + 1,
                "dust syncer starting"
            );

            // 2. Subscribe from `last_id + 1` (inclusive — the
            //    indexer's `id` arg is the FIRST event to deliver).
            let stream = transport::subscribe(
                self.ws_url,
                DUST_LEDGER_EVENTS_QUERY,
                json!({ "id": (last_id + 1).max(0) }),
            )
            .await
            .map_err(translate_unshielded_error)?;
            let mut stream = std::pin::pin!(stream.map(|item| {
                item.map_err(translate_unshielded_error)
                    .and_then(|v| decode_event(&v))
            }));

            // 3. Fold loop. Apply events to the state in batches
            //    so we get one persist + one progress emit per
            //    batch instead of one per event.
            let mut batch: Vec<ledger::events::Event<DefaultDB>> = Vec::new();
            let mut target_max: Option<i64> = None;
            let mut events_processed: usize = 0;

            'outer: loop {
                if let Some(max) = target_max {
                    if last_id >= max {
                        break;
                    }
                }
                let next = tokio::time::timeout(IDLE_TIMEOUT, stream.next()).await;
                match next {
                    Ok(Some(item)) => {
                        let decoded = item?;
                        last_id = decoded.id;
                        target_max = Some(decoded.max_id);
                        batch.push(decoded.event);

                        // Flush at batch boundary OR when we hit
                        // the target_max (whichever first).
                        let caught_up = last_id >= decoded.max_id;
                        if batch.len() >= PERSIST_EVERY_N_EVENTS || caught_up {
                            state = state
                                .replay_events(&self.dust_key, batch.iter())
                                .map_err(|e| {
                                    DustError::Replay(format!("replay: {e}"))
                                })?;
                            events_processed += batch.len();
                            batch.clear();
                            self.persist(&state, last_id).await?;
                            yield SyncProgress {
                                current_id: last_id,
                                max_id: decoded.max_id,
                                events_processed,
                            };
                            if caught_up {
                                break 'outer;
                            }
                        }
                    }
                    Ok(None) => {
                        // Stream ended. Flush whatever's pending.
                        if !batch.is_empty() {
                            state = state
                                .replay_events(&self.dust_key, batch.iter())
                                .map_err(|e| {
                                    DustError::Replay(format!("replay: {e}"))
                                })?;
                            events_processed += batch.len();
                            self.persist(&state, last_id).await?;
                        }
                        if target_max.is_some() {
                            yield SyncProgress {
                                current_id: last_id,
                                max_id: target_max.unwrap_or(-1),
                                events_processed,
                            };
                            break 'outer;
                        }
                        Err(DustError::StreamClosedEarly)?;
                    }
                    Err(_) => {
                        // Idle timeout. If we've seen any events
                        // we're done; otherwise the indexer never
                        // sent us anything → surface that.
                        if !batch.is_empty() {
                            state = state
                                .replay_events(&self.dust_key, batch.iter())
                                .map_err(|e| {
                                    DustError::Replay(format!("replay: {e}"))
                                })?;
                            events_processed += batch.len();
                            self.persist(&state, last_id).await?;
                        }
                        if target_max.is_some() {
                            yield SyncProgress {
                                current_id: last_id,
                                max_id: target_max.unwrap_or(-1),
                                events_processed,
                            };
                            break 'outer;
                        }
                        Err(DustError::StreamClosedEarly)?;
                    }
                }
            }

            tracing::info!(
                network = %self.network.config().network_id,
                last_id,
                events_processed,
                "dust syncer caught up"
            );
        }
    }

    async fn persist(
        &self,
        state: &DustLocalState<DefaultDB>,
        last_id: i64,
    ) -> Result<(), DustError> {
        let mut bytes = Vec::new();
        tagged_serialize(state, &mut bytes)
            .map_err(|e| DustError::Replay(format!("serialize state: {e}")))?;
        let snap = DustSyncSnapshot {
            last_id,
            state_bytes: bytes,
            updated_at: now_ms(),
        };
        self.store
            .put_dust_sync(self.network, &snap)
            .map_err(|e| DustError::Replay(format!("store put: {e}")))?;
        Ok(())
    }
}

fn translate_unshielded_error(e: crate::unshielded::UnshieldedError) -> DustError {
    use crate::unshielded::UnshieldedError as U;
    match e {
        U::WsConnect(s) => DustError::WsConnect(s),
        U::WsHandshake(s) => DustError::WsHandshake(s),
        U::GqlError(s) => DustError::GqlError(s),
        U::UnexpectedFrame(s) => DustError::UnexpectedFrame(s),
        U::Decode(s) => DustError::Decode(s),
        U::StreamClosedEarly => DustError::StreamClosedEarly,
        // Forward-compat: any new variants degrade to a generic
        // decode error rather than panicking.
        other => DustError::Decode(format!("{other:?}")),
    }
}

fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
