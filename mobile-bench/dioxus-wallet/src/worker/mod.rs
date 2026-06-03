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

use wallet_core::store::WalletStore;
use wallet_core::{DidId, Network};

use crate::bridge::BridgeState;

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

    /// Bootstrap a fresh Identity Centre DID — runs the upstream
    /// `bootstrap_did_with_keys` pipeline (HKDF + key import +
    /// on-chain DID write with the configured `controller_secret`).
    /// `seed` is the 32-byte master used for the HKDF derive; today
    /// every caller passes `DEMO_IC_SEED` ([42u8; 32]), but the
    /// message-shape is ready for a future "user-supplied seed"
    /// affordance without breaking the worker contract.
    Bootstrap {
        action_id: u64,
        network: Network,
        seed: [u8; 32],
    },

    /// Drive the OID4VP / SIOPv2 "Login with DID" Mode-A flow:
    /// parse the QR URL → fetch the request object → mint a
    /// DID-bound id_token via [`LoginCoordinator::mode_a`] +
    /// [`IdTokenBuilder`] → POST it back. Returns the issuer's
    /// `session_id` + `status`.
    ///
    /// Routed through the worker so the heavy state machine
    /// (Wallet + indexer + http + JWS construction + POST) is
    /// constructed on the worker thread's 8 MiB stack, not the
    /// WebView dispatch thread's 256 KiB.
    Oid4vpAuthenticate {
        action_id: u64,
        network: Network,
        /// Holder DID the wallet authenticates as. Picked by the
        /// UI's DID picker before dispatch.
        did: DidId,
        /// QR payload — the `openid4vp://…?request_uri=…` URL.
        qr_url: String,
    },

    /// Drive the OID4VCI Pre-Authorized Code Flow:
    /// parse the credential offer → POST /token (exchange
    /// pre-auth code for access_token + c_nonce) → build a
    /// DID-bound proof-of-possession JWS → POST /credential →
    /// persist the issued VC into the wallet's redb vc store.
    /// Returns the freshly-issued `vc_uri`.
    ///
    /// Same threading rationale as `Oid4vpAuthenticate`: the
    /// state machine (Wallet + indexer + http + JWS PoP +
    /// credential POST + redb insert) is well over the WebView
    /// dispatch thread's stack.
    Oid4vciIssuance {
        action_id: u64,
        network: Network,
        /// Holder DID the wallet binds the issued credential to.
        did: DidId,
        /// QR payload — the
        /// `openid-credential-offer://…` URL.
        qr_url: String,
    },

    /// Open the on-disk wallet store at
    /// `crate::app::wallet_store_path()` with the supplied
    /// passphrase. Returns a [`WorkOutcome::OpenStoreOk`] carrying
    /// the [`WalletStore`] handle (which is `Clone + Send`, with
    /// the heavy [`redb::Database`] behind an `Arc`), or a generic
    /// [`WorkOutcome::Err`] on failure (wrong passphrase, bad file,
    /// migration failure).
    ///
    /// Why a worker hop: `WalletStore::open` walks the entire
    /// table tree to run pending migrations and decode the
    /// per-network ledger snapshot. On a populated demo store
    /// (preprod-live with ~534k DUST events cached) this is the
    /// single deepest async state machine in the app — well over
    /// the WebView dispatch thread's ~256 KiB stack budget.
    /// Mirrors the Bootstrap path which moved
    /// `bootstrap_did_with_keys` for the same reason.
    OpenStore {
        action_id: u64,
        /// User-typed passphrase. Lives in memory only as long as
        /// this message + the resulting `WalletStore` need it.
        passphrase: String,
    },
}

impl WorkMsg {
    /// Universal `action_id` accessor. Surface — not currently
    /// called by any consumer in Task 1 (the dispatcher matches
    /// arms directly), but exposed so handlers / the migrations
    /// in subsequent tasks can route generically.
    #[allow(dead_code)]
    pub fn action_id(&self) -> u64 {
        match self {
            Self::Noop { action_id }
            | Self::Bootstrap { action_id, .. }
            | Self::Oid4vpAuthenticate { action_id, .. }
            | Self::Oid4vciIssuance { action_id, .. }
            | Self::OpenStore { action_id, .. } => *action_id,
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
pub enum WorkOutcome {
    NoopAck {
        action_id: u64,
    },
    BootstrapOk {
        action_id: u64,
        /// `to_did_string()` of the freshly-minted DID.
        did_str: String,
        /// 32-byte controller secret. Persisted into
        /// [`BridgeState::controller_secrets`] by the worker
        /// itself before this outcome flies; the UI side rarely
        /// needs to read it (the secret is reachable via
        /// `bridge_state.controller_secrets`), but we still
        /// surface it on the outcome so the handler can update
        /// caller-side caches if needed.
        #[allow(dead_code)] // Read once Sign/Update/Deactivate migrations land.
        controller_sk: [u8; 32],
    },
    /// OID4VP / SIOPv2 authentication round-trip succeeded.
    /// Carries the issuer's session correlation id + the literal
    /// `status` string the issuer returned (Phase-1 mock-issuer
    /// emits `"authenticated"`).
    Oid4vpOk {
        action_id: u64,
        session_id: String,
        status: String,
    },
    /// OID4VCI credential issuance finished — the VC has been
    /// persisted to the wallet's vc_store. Carries the credential's
    /// `vc_uri` so the click site can surface it in the success
    /// banner / trigger the inventory refresh.
    Oid4vciOk {
        action_id: u64,
        vc_uri: String,
    },
    /// [`WalletStore`] opened successfully. The handle is cheap
    /// to `Clone` (interior `Arc<Database>`) and lives on the UI
    /// thread once handed over. The UI's outcome handler is
    /// responsible for calling [`BridgeState::set_store`] and
    /// running the rest of the inline unlock pipeline
    /// (`find_or_create_wallet_for_network`, DustSyncer
    /// registration, inventory / resolved-cache hydration,
    /// session-snap restore, per-DID auto-resolve fan-out) — the
    /// signal writes in there are `!Send` and have to stay on
    /// the UI thread anyway, so splitting at the
    /// `WalletStore::open` boundary lets the worker take the
    /// single biggest stack consumer while leaving the
    /// signal-touching tail in place.
    OpenStoreOk {
        action_id: u64,
        store: WalletStore,
    },
    Err {
        action_id: u64,
        msg: String,
    },
}

impl WorkOutcome {
    pub fn action_id(&self) -> u64 {
        match self {
            Self::NoopAck { action_id }
            | Self::BootstrapOk { action_id, .. }
            | Self::Oid4vpOk { action_id, .. }
            | Self::Oid4vciOk { action_id, .. }
            | Self::OpenStoreOk { action_id, .. }
            | Self::Err { action_id, .. } => *action_id,
        }
    }
}

/// Worker handle held by the UI. `Clone` so it can be threaded
/// through props / context without an outer `Arc`. The interior
/// `Arc`'d channels are what make this cheap to clone.
///
/// `Send + Sync` — the only state here is mpsc channel handles
/// (both halves are Send) plus an `Arc<Mutex<_>>` around the
/// outcome receiver. The [`router`] module's `thread_local!`
/// holds the actual outcome handlers; those are `!Send` and
/// never touched off the UI thread.
#[derive(Clone)]
pub struct AppWorker {
    tx: mpsc::UnboundedSender<WorkMsg>,
    rx_back: Arc<Mutex<mpsc::UnboundedReceiver<WorkOutcome>>>,
}

impl AppWorker {
    /// Spawn the worker thread and return a handle. Call once at
    /// app boot and store the result in `BridgeState`.
    ///
    /// `bridge_state` is the same handle the UI side carries — the
    /// worker captures its own clone so handlers can read `store`,
    /// `active_wallet_id`, `metrics`, etc. without re-threading
    /// them through every `WorkMsg`. All BridgeState fields are
    /// `Arc`-wrapped so the clone is cheap and points at the
    /// same backing state the UI sees (and that the UI may
    /// mutate, e.g. `set_store`, after the worker has started).
    ///
    /// The worker owns a current-thread `tokio` runtime so chain
    /// ops can `.await` without bouncing back to the WebView
    /// event loop. Stack size is 8 MiB — well above what
    /// `Wallet` + indexer + prover + multi-stage wizard streams
    /// need.
    pub fn spawn(bridge_state: BridgeState) -> Self {
        let (tx, mut rx) = mpsc::unbounded_channel::<WorkMsg>();
        let (tx_back, rx_back) = mpsc::unbounded_channel::<WorkOutcome>();

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
                        let outcome = handlers::dispatch(&bridge_state, msg).await;
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

    /// Lock the back-channel receiver. Used exclusively by the
    /// outcome-pump `use_future` at app root; never call this
    /// from a click handler.
    pub async fn recv_outcome(&self) -> Option<WorkOutcome> {
        self.rx_back.lock().await.recv().await
    }
}
