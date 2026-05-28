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
use super::snapshot::{DUST_LEDGER_EVENTS_QUERY, DecodedEvent, decode_event};
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
///
/// 512 is sized for the producer/consumer split below: the fold
/// + persist run on `tokio::task::spawn_blocking` worker threads
/// instead of inline on the Dioxus renderer, so we no longer
/// have to fit each batch inside an ~80ms frame budget. A bigger
/// batch lowers redb-txn overhead × (events / batch) and lets
/// `replay_events` amortise its setup cost across more events.
/// The previous value (128) was an inline-fold compromise; the
/// even-earlier 1024 was a UX-freeze grenade because the fold
/// ran on the renderer. Both pre-conditions are gone.
const PERSIST_EVERY_N_EVENTS: usize = 512;

/// `tokio::sync::mpsc` channel capacity between the WS reader
/// producer and the fold consumer. Sized generously so the
/// async I/O thread can race ahead of the CPU-bound fold without
/// backpressuring the WebSocket — the indexer can stream events
/// at ~5k/s but `replay_events` only chews ~1.6k/s on a debug
/// build, so without buffering we'd be network-bound on the slow
/// half of the pipeline.
///
/// 524288 events × ~1–2 KB/event ≈ 0.5–1 GB peak RAM headroom
/// on PreProd's ~534k-event log. Comfortably under the 1 GB
/// budget the operator has approved for this trade-off, and
/// large enough to buffer the entire PreProd history if the
/// fold lags. Cheap idle cost: empty `mpsc::channel` doesn't
/// pre-allocate the queue; only used slots cost memory.
const WS_TO_FOLD_CAPACITY: usize = 524_288;

/// `tokio::sync::mpsc` channel capacity between the fold worker
/// and the persist worker. Each in-flight job carries one
/// already-serialized `DustLocalState` snapshot (a few MB on
/// PreProd) plus the corresponding `last_id`.
///
/// Tradeoff: if the app crashes with N jobs queued, the next
/// `sync()` call will re-fetch from the last-persisted id (i.e.
/// up to N × PERSIST_EVERY_N_EVENTS events of re-work). Sizing
/// at 8 caps the worst case at ~4096 events of re-fetch — a few
/// seconds of recovery — while letting the fold race ahead for
/// 8 batches before back-pressuring on disk.
///
/// Each queued job supersedes the previous (the persist worker
/// writes them sequentially, latest-wins on durable state), so
/// there's no correctness reason to queue more.
const PERSIST_QUEUE_CAPACITY: usize = 8;

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

    /// Convenience: return the wallet's current DUST balance in
    /// atomic units (`10^-15 DUST`), evaluated against
    /// wall-clock "now". Returns `Ok(None)` if no snapshot has
    /// been persisted yet. Avoids forcing the UI to import
    /// `base_crypto::time::Timestamp` just to call
    /// `DustLocalState::wallet_balance`.
    pub fn current_balance_atomic(&self) -> Result<Option<u128>, DustError> {
        let Some((state, _)) = self.cached_state()? else {
            return Ok(None);
        };
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let now = base_crypto::time::Timestamp::from_secs(now_secs);
        Ok(Some(state.wallet_balance(now)))
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
            let (state_init, last_id_init) = match self.cached_state()? {
                Some(t) => t,
                None => (DustLocalState::new(self.params), -1),
            };
            // Held in an `Option` so we can `take()` it into
            // `spawn_blocking` and put the new state back without
            // tripping the borrow checker mid-loop. Always `Some`
            // outside the per-batch consume → swap → restore step.
            let mut state_holder: Option<DustLocalState<DefaultDB>> = Some(state_init);
            let mut last_id = last_id_init;
            // Remember whether we resumed from a populated cache.
            // If yes and the indexer sends zero events (already at
            // tip), the silent stream is "caught up", not an
            // error. Without this, every `sync()` call on an
            // up-to-date wallet would surface as
            // `StreamClosedEarly` — exactly the bug the user hit
            // on first Submit batch after the App pre-warmed via
            // the WalletSyncPane.
            let started_with_cache = last_id >= 0;
            let starting_last_id = last_id;

            tracing::info!(
                network = %self.network.config().network_id,
                resume_from = last_id + 1,
                ws_to_fold_capacity = WS_TO_FOLD_CAPACITY,
                persist_queue_capacity = PERSIST_QUEUE_CAPACITY,
                batch_size = PERSIST_EVERY_N_EVENTS,
                "dust syncer starting (3-stage I/O→CPU→I/O pipeline)"
            );

            // 2. Subscribe from `last_id + 1` (inclusive — the
            //    indexer's `id` arg is the FIRST event to deliver).
            let raw_stream = transport::subscribe(
                self.ws_url,
                DUST_LEDGER_EVENTS_QUERY,
                json!({ "id": (last_id + 1).max(0) }),
            )
            .await
            .map_err(translate_unshielded_error)?;

            // 3. Producer task: drains the WS into a bounded
            //    channel and exits. ZERO CPU work here — decoded
            //    events flow straight through. The async runtime
            //    keeps the I/O socket busy independent of how
            //    fast the consumer chews through the fold.
            let (tx, mut rx) = tokio::sync::mpsc::channel::<
                Result<DecodedEvent, DustError>,
            >(WS_TO_FOLD_CAPACITY);
            let producer = tokio::spawn(async move {
                let mut stream = std::pin::pin!(raw_stream.map(|item| {
                    item.map_err(translate_unshielded_error)
                        .and_then(|v| decode_event(&v))
                }));
                loop {
                    match tokio::time::timeout(IDLE_TIMEOUT, stream.next()).await {
                        Ok(Some(item)) => {
                            // Peek for "caught up" BEFORE the move
                            // (we own the event by-value otherwise).
                            let caught_up = matches!(&item, Ok(d) if d.id >= d.max_id);
                            if tx.send(item).await.is_err() {
                                // Consumer dropped (probably
                                // errored out). Exit silently.
                                break;
                            }
                            if caught_up {
                                break;
                            }
                        }
                        Ok(None) | Err(_) => {
                            // Stream ended or went idle.
                            // Dropping `tx` here closes the channel
                            // and signals end-of-feed to the consumer.
                            break;
                        }
                    }
                }
            });

            // 4. Persist worker. Lives on a dedicated blocking
            //    thread for the duration of the sync. Pulls
            //    pre-serialized snapshots off `persist_rx` via
            //    `blocking_recv` (legal inside `spawn_blocking`)
            //    and commits each to redb. Decoupling persist
            //    from fold lets the fold worker immediately start
            //    `replay_events` on the next batch as soon as it
            //    finishes the current one, instead of waiting for
            //    the redb txn to commit.
            //
            //    Errors flow back via `persist_err_tx`/`_rx` —
            //    the outer try_stream selects on it after every
            //    batch so a fatal disk error aborts the whole
            //    sync instead of silently dropping work.
            #[derive(Debug)]
            struct PersistJob {
                last_id: i64,
                state_bytes: Vec<u8>,
            }
            let (persist_tx, persist_rx) =
                tokio::sync::mpsc::channel::<PersistJob>(PERSIST_QUEUE_CAPACITY);
            let (persist_err_tx, mut persist_err_rx) =
                tokio::sync::mpsc::channel::<DustError>(1);
            let persist_handle = tokio::task::spawn_blocking({
                let store = self.store.clone();
                let network = self.network;
                let persist_err_tx = persist_err_tx.clone();
                let mut persist_rx = persist_rx;
                move || {
                    // blocking_recv is the sync equivalent of
                    // recv().await — designed for exactly this
                    // case (async channel, sync consumer thread).
                    while let Some(job) = persist_rx.blocking_recv() {
                        let snap = DustSyncSnapshot {
                            last_id: job.last_id,
                            state_bytes: job.state_bytes,
                            updated_at: now_ms(),
                        };
                        if let Err(e) = store.put_dust_sync(network, &snap) {
                            // First persist failure aborts the
                            // pipeline. Capacity-1 channel so
                            // subsequent errors silently drop;
                            // we don't need the full history.
                            let _ = persist_err_tx.blocking_send(
                                DustError::Replay(format!("store put: {e}")),
                            );
                            return;
                        }
                    }
                }
            });

            // 5. Fold consumer. Pull batches from `rx` (WS feed),
            //    move them into `spawn_blocking` for the CPU-bound
            //    `replay_events` + `tagged_serialize`, then hand
            //    the serialized bytes off to `persist_tx`. The
            //    fold worker thread is freed to start the next
            //    batch as soon as serialization finishes; the
            //    persist worker thread writes redb asynchronously.
            let mut events_processed: usize = 0;
            let mut pulled: Vec<Result<DecodedEvent, DustError>> =
                Vec::with_capacity(PERSIST_EVERY_N_EVENTS);
            let mut latest_max: i64 = -1;
            let mut saw_any_event = false;

            loop {
                // Bail early if the persist worker already
                // surfaced an error from a prior batch.
                if let Ok(e) = persist_err_rx.try_recv() {
                    Err(e)?;
                }
                pulled.clear();
                let n = rx.recv_many(&mut pulled, PERSIST_EVERY_N_EVENTS).await;
                if n == 0 {
                    // Channel closed → producer is done.
                    break;
                }
                saw_any_event = true;

                // Unwrap Results in order; first Err aborts the
                // whole sync (propagates out of `try_stream!`).
                let mut events: Vec<DecodedEvent> = Vec::with_capacity(n);
                for r in pulled.drain(..) {
                    events.push(r?);
                }
                let batch_last_id =
                    events.last().map(|d| d.id).unwrap_or(last_id);
                latest_max = events.last().map(|d| d.max_id).unwrap_or(latest_max);

                let prev_state = state_holder
                    .take()
                    .expect("state_holder always Some between consume cycles");
                let dust_key = self.dust_key.clone();

                // CPU-bound stage on the blocking pool: replay +
                // serialise. Returns the new state PLUS the
                // already-serialized bytes ready for persist.
                // Doing the serialization HERE (not in the persist
                // worker) keeps the persist worker focused on
                // pure disk I/O — no CPU on the I/O thread.
                let join_outcome = tokio::task::spawn_blocking(
                    move || -> Result<(DustLocalState<DefaultDB>, Vec<u8>), DustError> {
                        let next = prev_state
                            .replay_events(
                                &dust_key,
                                events.iter().map(|d| &d.event),
                            )
                            .map_err(|e| DustError::Replay(format!("replay: {e}")))?;
                        let mut bytes = Vec::new();
                        tagged_serialize(&next, &mut bytes).map_err(|e| {
                            DustError::Replay(format!("serialize state: {e}"))
                        })?;
                        Ok((next, bytes))
                    },
                )
                .await
                .map_err(|e| DustError::Replay(format!("blocking join: {e}")))?;

                let (new_state, state_bytes) = join_outcome?;
                state_holder = Some(new_state);
                last_id = batch_last_id;
                events_processed += n;

                // Hand off to the persist worker. If the worker
                // is keeping up (the usual case), this returns
                // immediately. If it's behind by
                // `PERSIST_QUEUE_CAPACITY` jobs, this awaits — a
                // proper back-pressure signal that gives the
                // disk time to drain. The `send` only fails if
                // the persist task panicked (channel closed by
                // drop); we surface that as a Replay error.
                persist_tx
                    .send(PersistJob {
                        last_id: batch_last_id,
                        state_bytes,
                    })
                    .await
                    .map_err(|_| {
                        DustError::Replay(
                            "persist worker dropped its receiver".into(),
                        )
                    })?;

                yield SyncProgress {
                    current_id: last_id,
                    max_id: latest_max,
                    events_processed,
                };
            }

            // 6. Drain phase. Close the persist channel, wait for
            //    the worker to finish writing any queued jobs,
            //    then surface a late error if one happened during
            //    the final drain.
            drop(persist_tx);
            let _ = persist_handle.await;
            if let Ok(e) = persist_err_rx.try_recv() {
                Err(e)?;
            }

            // Producer should be done by now (its channel close
            // is what woke us up to exit the fold loop). Join it
            // to surface any panic; ignore Ok/cancel.
            let _ = producer.await;

            // Termination signalling — three cases:
            //  (a) at least one event in this run → normal
            //      completion; final SyncProgress already emitted
            //      inside the loop.
            //  (b) zero events + resumed from cache → "already
            //      at tip", emit a final flat progress.
            //  (c) fresh wallet, zero events ever → indexer
            //      probably unreachable; surface as
            //      StreamClosedEarly.
            if !saw_any_event {
                if started_with_cache {
                    yield SyncProgress {
                        current_id: starting_last_id,
                        max_id: starting_last_id,
                        events_processed: 0,
                    };
                } else {
                    Err(DustError::StreamClosedEarly)?;
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
