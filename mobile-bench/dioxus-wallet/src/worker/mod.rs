//! Wallet worker thread — central serialiser for every heavy
//! chain operation the UI dispatches.
//!
//! Lives in a dedicated `std::thread` with an 8 MiB stack so the
//! state machines for `bootstrap_did_with_keys`, `run_authentication`,
//! `run_issuance`, `WalletStore::open`, etc. never have to be
//! materialised on the Chromium WebView dispatch thread's ~256 KiB
//! stack on Android. The UI side only ever sends a [`WorkMsg`] via
//! the worker's channel; results come back as [`WorkOutcome`] and
//! get routed back to the matching click handler through
//! [`router::OutcomeRouter`].
//!
//! Architecture rationale: see
//! `docs/superpowers/specs/2026-06-02-wallet-worker-thread.md`.

mod handlers;
pub mod router;

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::sync::{Mutex, mpsc};

pub use router::OutcomeRouter;

/// Process-local monotonic action token. Used to route a
/// [`WorkOutcome`] back to the [`router::OutcomeHandler`] the
/// click handler registered before sending the [`WorkMsg`]. `u64`
/// → wrap-around is irrelevant in a single session.
pub fn next_action_id() -> u64 {
    static N: AtomicU64 = AtomicU64::new(1);
    N.fetch_add(1, Ordering::Relaxed)
}

/// All heavy chain ops the worker serialises. Each variant
/// carries an `action_id` so the matching [`WorkOutcome`] can be
/// routed back without inspecting the payload.
///
/// The Phase-2 (this) cut intentionally only has `Noop` — once
/// the round-trip is proven, subsequent commits add Bootstrap,
/// OID4VP, OID4VCI, Unlock, etc. variants one at a time and
/// migrate their call sites.
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)] // bootstrap_did_with_keys + create_did will land later
pub enum WorkMsg {
    /// Round-trip ping for plumbing verification. The worker
    /// emits a [`WorkOutcome::NoopAck`] back. Used by the
    /// outcome-pump smoke test in `App::run`; can also be
    /// invoked from a developer affordance for sanity-checking
    /// the channel.
    Noop { action_id: u64 },
}

impl WorkMsg {
    /// Universal `action_id` accessor. Surface — not currently
    /// called by any consumer in Task 1 (the dispatcher matches
    /// arms directly), but exposed so handlers / the migrations
    /// in subsequent tasks can route generically.
    #[allow(dead_code)]
    pub fn action_id(&self) -> u64 {
        match self {
            Self::Noop { action_id } => *action_id,
        }
    }
}

/// Result of processing a [`WorkMsg`]. Mirrors `WorkMsg` arm-for-
/// arm; `Err` is the universal failure variant so handlers don't
/// have to define one variant per op.
///
/// `Err` is intentionally unused in Task 1 (Noop can't fail) —
/// it's part of the public surface every future handler will
/// emit on failure, so we keep it visible from day one to lock
/// in the convention.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Err arm — populated in Task 2 onwards.
pub enum WorkOutcome {
    NoopAck { action_id: u64 },
    Err { action_id: u64, msg: String },
}

impl WorkOutcome {
    pub fn action_id(&self) -> u64 {
        match self {
            Self::NoopAck { action_id } | Self::Err { action_id, .. } => *action_id,
        }
    }
}

/// Worker handle held by the UI. `Clone` so it can be threaded
/// through props / context without an outer `Arc`. The interior
/// `Arc`'d channels are what make this cheap to clone.
#[derive(Clone)]
pub struct AppWorker {
    tx: mpsc::UnboundedSender<WorkMsg>,
    rx_back: Arc<Mutex<mpsc::UnboundedReceiver<WorkOutcome>>>,
    outcomes: Arc<OutcomeRouter>,
}

impl AppWorker {
    /// Spawn the worker thread and return a handle. Call once at
    /// app boot and store the result in `BridgeState`.
    ///
    /// The worker owns a current-thread `tokio` runtime so chain
    /// ops can `.await` without bouncing back to the WebView
    /// event loop. Stack size is 8 MiB — well above what
    /// `Wallet` + indexer + prover + multi-stage wizard streams
    /// need.
    pub fn spawn() -> Self {
        let (tx, mut rx) = mpsc::unbounded_channel::<WorkMsg>();
        let (tx_back, rx_back) = mpsc::unbounded_channel::<WorkOutcome>();
        let outcomes = Arc::new(OutcomeRouter::default());

        std::thread::Builder::new()
            .name("wallet-worker".into())
            .stack_size(8 << 20)
            .spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("wallet-worker tokio runtime");
                rt.block_on(async move {
                    tracing::info!(
                        target: "wallet_worker",
                        "wallet-worker thread alive (stack=8MiB, runtime=current-thread)",
                    );
                    while let Some(msg) = rx.recv().await {
                        let outcome = handlers::dispatch(msg).await;
                        if tx_back.send(outcome).is_err() {
                            tracing::warn!(
                                target: "wallet_worker",
                                "outcome receiver dropped; worker exiting",
                            );
                            return;
                        }
                    }
                    tracing::info!(
                        target: "wallet_worker",
                        "wallet-worker channel closed; thread exiting",
                    );
                });
            })
            .expect("spawn wallet-worker thread");

        Self {
            tx,
            rx_back: Arc::new(Mutex::new(rx_back)),
            outcomes,
        }
    }

    /// Send a [`WorkMsg`] to the worker. The `Result` is the
    /// channel's — `Err` means the worker thread is gone (panic
    /// or shutdown). We log but never crash the UI on send
    /// failure; the operator sees a stuck `busy` signal and can
    /// reload the app.
    pub fn send(&self, msg: WorkMsg) {
        if let Err(e) = self.tx.send(msg) {
            tracing::error!(
                target: "wallet_worker",
                error = %e,
                "WorkMsg dropped — worker thread is gone",
            );
        }
    }

    /// Borrow the outcome router. Click handlers call
    /// `router().register(action_id, handler)` before `send()`.
    pub fn outcomes(&self) -> &Arc<OutcomeRouter> {
        &self.outcomes
    }

    /// Lock the back-channel receiver. Used exclusively by the
    /// outcome-pump `use_future` at app root; never call this
    /// from a click handler.
    pub async fn recv_outcome(&self) -> Option<WorkOutcome> {
        self.rx_back.lock().await.recv().await
    }
}
