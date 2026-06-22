//! JS ↔ Rust bridge for the embedded WebView **plus** the
//! `BridgeState` aggregate that holds the shell's shared
//! application state (the wallet store handle, the metrics
//! pipeline, the wallet-worker handle, etc.). The two concerns
//! grew together because the bridge RPC methods need to look at
//! state, so they share a file — but `BridgeState` is the
//! interesting one architecturally.
//!
//! ## Bridge: JS ↔ Rust JSON-RPC channel
//!
//! Two responsibilities for the bridge half:
//!
//! 1. **Local proof-server.** On desktop we spawn
//!    `midnight-proof-server` on `127.0.0.1:0` at app startup. The JS
//!    bundle (midnight-did + deps) talks to it via the same HTTP
//!    protocol upstream packages already use, so we avoid bridging the
//!    proof preimage / proving key payload through the JSON-RPC channel.
//!    Android skips this — phase D wires up a remote URL fallback.
//!
//! 2. **JSON-RPC channel for everything else.** A long-lived
//!    Dioxus document JS runner accepts requests from JS via
//!    `dioxus.send(...)` and replies via `dioxus.recv()`. Methods are
//!    deliberately small — sign/derive operations the wallet keeps in
//!    Rust because the seed never leaves Rust. The JS side wraps them
//!    as `window.midnightWallet.<method>(...)` with promise semantics.
//!
//! ## BridgeState: shell-level application state
//!
//! `BridgeState` is a cheap-to-clone (Arc-wrapped fields)
//! aggregate that carries the shell's mid-flight state — the
//! handles every click handler, worker callback, and the
//! outcome-pump `use_future` reach into. Today it bundles three
//! distinct concerns:
//!
//! 1. **Persistence handles** — `store`, `active_wallet_id`,
//!    `controller_secrets`.
//! 2. **Observability** — `metrics`, `resource_probe`,
//!    `log_capture`.
//! 3. **Runtime infrastructure** — `worker`, `proof_server_url`.
//!
//! A clean future split would turn `BridgeState` into a façade
//! over `Persistence`/`Observability`/`Runtime` sub-aggregates;
//! the architectural payoff appears once we ship a second shell
//! (iOS, react-native) where the `Runtime` mix changes and the
//! Persistence/Observability layers stay common. Deferring the
//! mechanical split until that lands — see the audit doc
//! `docs/superpowers/specs/2026-06-03-hex-architecture-audit.md`
//! §5.D for the rationale + entry-point inventory.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};
use tokio::sync::OnceCell;
use wallet_core::store::{WalletId, WalletStore};
use wallet_core::{
    CompositeMetrics, InMemoryMetrics, Metrics, Network, RedbVcStore, RusageProbe,
    TracingMetrics, VaultLockPolicy, VcOpening, Wallet, unshielded_bech32m,
};

use crate::identity_centre::vc_store_path;
use crate::logs::LogCapture;
use crate::vc_views::digital_passport::{
    CLAIM_DATE_OF_BIRTH, CLAIM_DOCUMENT_NUMBER, CLAIM_FIRST_NAME, CLAIM_ISSUING_STATE,
    CLAIM_LAST_NAME, decode_text_padded, is_digital_passport,
};

/// Per-DID random controller secret store. Populated by
/// `CreateDidWizard.on_done` and read by the
/// `getControllerSecretKey` bridge RPC during JS-driven circuit
/// execution. In-memory hot cache; the canonical source of truth
/// is the persistent `WalletStore` (when attached via
/// [`BridgeState::set_store`]). Each DID's 32 bytes round-trip
/// across the Dioxus channel as hex but only inside the embedded
/// WebView.
pub type ControllerSecretStore = Arc<Mutex<HashMap<String, [u8; 32]>>>;

// ─── Sub-state groups ────────────────────────────────────────────
// `BridgeState` decomposes into three focused sub-states, one per
// concern. Each is cheap-to-clone (every field is `Arc<…>`) and
// rides as a value inside `BridgeState`. The previous flat layout
// mixed persistence, observability, and runtime infrastructure
// fields together; the audit doc
// (`docs/superpowers/specs/2026-06-03-hex-architecture-audit.md`
// §5.D) flagged this as a deferred decomposition. The split is
// load-bearing once we ship a second shell where the `Runtime`
// mix differs and the `Persistence` / `Observability` halves stay
// common — until then it's a clarity refactor: each field is now
// reachable under its concern, and a reader can tell at a glance
// what surface a given click handler actually needs.
//
// The accessor methods on `BridgeState` stay as a façade so call
// sites (~31 of them across `app.rs`, `identity_centre.rs`,
// `worker/handlers.rs`) keep working with no edit. The one place
// the field path appears externally (`app.rs:9094`'s
// `bridge_state.controller_secrets.lock()`) is migrated to
// `bridge_state.persistence.controller_secrets.lock()` in this
// commit.

/// Persistence-layer handles — the on-disk wallet store, the
/// pinned wallet id, the per-DID controller-secret cache.
#[derive(Clone, Default)]
pub struct Persistence {
    /// Persistent backing store. Set once at app startup via
    /// [`BridgeState::set_store`]. When present,
    /// `remember_controller_secret` writes through (best-effort —
    /// a store error is logged but does not fail the in-memory
    /// cache update). When absent, behaviour matches the previous
    /// in-memory-only model.
    pub store: Arc<OnceCell<WalletStore>>,
    /// `WalletId` the rest of the UI binds against — Keys tab,
    /// Operation Builder VM picker, SignTab key picker. Set by
    /// the App during hydration; one slot per `(network, wallet)`
    /// swap. `None` means "no wallet active for the current
    /// network yet" — pickers hide their lists.
    pub active_wallet_id: Arc<Mutex<Option<WalletId>>>,
    /// `did_string → 32-byte sk`. Cloning the `Persistence` (and
    /// therefore `BridgeState`) clones the `Arc`, so the map is
    /// shared across the bridge loop, the UI, and any future
    /// callers.
    pub controller_secrets: ControllerSecretStore,
}

impl PartialEq for Persistence {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.store, &other.store)
            && Arc::ptr_eq(&self.active_wallet_id, &other.active_wallet_id)
            && Arc::ptr_eq(&self.controller_secrets, &other.controller_secrets)
    }
}
impl Eq for Persistence {}

/// Observability — metrics, resource sampling, captured logs.
/// Everything a future Diagnostics tab needs to render a session
/// dashboard.
#[derive(Clone, Default)]
pub struct Observability {
    /// Process-wide telemetry aggregator. Populated once at App
    /// boot with an `InMemoryMetrics` (or composite). Read by the
    /// Diagnostics tab via `metrics_snapshot()`; written by
    /// `MeteredHttpClient` (HTTP latencies) and `time_op` brackets
    /// around intensive operations.
    pub metrics: Arc<InMemoryMetrics>,
    /// POSIX `getrusage`-backed sampler — RSS + CPU-time deltas
    /// around bracketed operations. Stateless; share the `Arc`.
    pub resource_probe: Arc<RusageProbe>,
    /// In-memory log ring + persist-channel handle. The Logs tab
    /// reads via `snapshot()`; the App spawns the drainer once
    /// the store is attached.
    pub log_capture: Arc<OnceCell<LogCapture>>,
}

impl PartialEq for Observability {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.metrics, &other.metrics)
            && Arc::ptr_eq(&self.resource_probe, &other.resource_probe)
            && Arc::ptr_eq(&self.log_capture, &other.log_capture)
    }
}
impl Eq for Observability {}

/// Runtime infrastructure — the wallet-worker thread and the
/// embedded proof-server URL. Shell-specific (the worker thread
/// + the desktop-only proof-server spawn live here today);
/// changes when we ship a second shell (iOS host, react-native,
/// headless CLI).
#[derive(Clone, Default)]
pub struct Runtime {
    /// Dedicated worker-thread handle (8 MiB stack, single-thread
    /// tokio rt). Set once at App::run before any UI component
    /// mounts; from then on every heavy chain op routes through
    /// `bridge_state.worker().send(WorkMsg::…)` instead of
    /// `spawn(Box::pin(async move {…}))`. See
    /// `docs/superpowers/specs/2026-06-02-wallet-worker-thread.md`
    /// for the migration plan.
    pub worker: Arc<OnceCell<crate::worker::AppWorker>>,
    /// URL of the embedded local proof-server (set once at boot;
    /// `None` on Android where the wallet uses the in-process
    /// `LocalProvingProvider` instead).
    pub proof_server_url: Arc<OnceCell<String>>,
}

impl PartialEq for Runtime {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.worker, &other.worker)
            && Arc::ptr_eq(&self.proof_server_url, &other.proof_server_url)
    }
}
impl Eq for Runtime {}

#[derive(Clone, Default, PartialEq, Eq)]
// `PartialEq` here lets `BridgeState` ride as a Dioxus component
// prop (the `#[component]` macro requires Props to be Eq). Two
// `BridgeState` values are equal iff every inner `Arc` is
// pointer-equal — fine because the App constructs exactly one
// `BridgeState` and clones it; we never compare independently-
// built handles for content. The derived `PartialEq` delegates to
// each sub-state's `PartialEq`, which in turn uses `Arc::ptr_eq`.
pub struct BridgeState {
    /// On-disk store + active wallet + controller-secret cache.
    pub persistence: Persistence,
    /// Metrics + resource probe + captured logs.
    pub observability: Observability,
    /// Wallet-worker handle + embedded-proof-server URL.
    pub runtime: Runtime,
}

impl BridgeState {
    pub fn new() -> Self {
        Self::default()
    }

    // ─── Runtime accessors ──────────────────────────────────────

    /// Install the worker handle. Called once at App::run after
    /// [`crate::worker::AppWorker::spawn`]. Subsequent calls are
    /// no-ops (matches the [`set_store`] / [`OnceCell`] pattern).
    pub fn set_worker(&self, worker: crate::worker::AppWorker) {
        let _ = self.runtime.worker.set(worker);
    }

    /// Borrow the worker handle. Returns `None` before
    /// `set_worker` has fired (i.e. during the very early bridge
    /// boot path, or in unit tests that drive BridgeState
    /// without an App). Production click handlers can `.expect`
    /// this — the App always sets it before any UI mounts.
    pub fn worker(&self) -> Option<&crate::worker::AppWorker> {
        self.runtime.worker.get()
    }

    /// Best-effort URL accessor for UI display. Returns `None` until
    /// the local proof-server has finished booting.
    pub fn proof_server_url(&self) -> Option<String> {
        self.runtime.proof_server_url.get().cloned()
    }

    // ─── Persistence accessors ──────────────────────────────────

    /// Attach a `WalletStore`. Idempotent — subsequent calls
    /// after the first succeed are no-ops. Returns the store
    /// handle that ended up installed (either the just-set one
    /// or a previously-set one) so the caller doesn't need a
    /// follow-up read.
    pub fn set_store(&self, store: WalletStore) -> WalletStore {
        let _ = self.persistence.store.set(store);
        self.persistence
            .store
            .get()
            .cloned()
            .expect("just-set store reachable")
    }

    /// Borrow the attached store, if any. Returns `None` before
    /// `set_store` has run — useful during the early bridge
    /// boot path that fires before the store is opened.
    #[allow(dead_code)] // Surfaced via [`Self::store`] for future bridge-RPC handlers
    /// that want to persist beyond controller secrets.
    pub fn store(&self) -> Option<&WalletStore> {
        self.persistence.store.get()
    }

    /// Pin the wallet the rest of the UI binds against. Called
    /// once per network swap (and on first unlock) so the
    /// pickers know which `WalletId` to read from. Silent
    /// best-effort — a poisoned mutex just leaves the prior
    /// value in place; the UI degrades to "no wallet active".
    pub fn set_active_wallet_id(&self, id: Option<WalletId>) {
        if let Ok(mut g) = self.persistence.active_wallet_id.lock() {
            *g = id;
        }
    }

    /// Snapshot the currently-active `WalletId`. `None` before
    /// the App has finished hydrating for the active network.
    pub fn active_wallet_id(&self) -> Option<WalletId> {
        self.persistence
            .active_wallet_id
            .lock()
            .ok()
            .and_then(|g| g.clone())
    }

    // ─── Observability accessors ────────────────────────────────

    /// Attach the process-global `LogCapture` handle. Called
    /// once during App construction; later sets are no-ops.
    pub fn set_log_capture(&self, capture: LogCapture) {
        let _ = self.observability.log_capture.set(capture);
    }

    /// Borrow the captured-logs handle, if attached. The
    /// Logs tab reads this every render via `snapshot()`;
    /// pre-attach calls return `None` (the tab shows an
    /// empty state).
    pub fn log_capture(&self) -> Option<&LogCapture> {
        self.observability.log_capture.get()
    }

    /// Borrow the telemetry aggregator. The future Diagnostics
    /// tab calls `.snapshot()` on this to render the
    /// counter / HTTP / op histograms.
    pub fn metrics(&self) -> Arc<InMemoryMetrics> {
        self.observability.metrics.clone()
    }

    /// Composite sink: forwards every record to both the
    /// in-memory aggregator (read by the Diagnostics tab) and
    /// `TracingMetrics` (so events flow into the Logs tab via
    /// `WalletLogLayer`). Construct once per call site —
    /// the `Arc`s are cheap.
    pub fn metrics_dyn(&self) -> Arc<dyn Metrics> {
        Arc::new(CompositeMetrics::new(vec![
            self.observability.metrics.clone(),
            Arc::new(TracingMetrics),
        ]))
    }

    /// Borrow the shared resource probe.
    pub fn resource_probe(&self) -> Arc<RusageProbe> {
        self.observability.resource_probe.clone()
    }

    /// Record the random sk minted for a freshly-deployed DID.
    /// Overwrites any existing entry (a fresh deploy with the same
    /// id would be impossible on-chain, but defensive). Persists
    /// to the attached `WalletStore` (if any) under
    /// `(network, did)`; a write failure is logged and the
    /// in-memory cache is still populated so the current session
    /// is uninterrupted.
    ///
    /// Today the only live caller sits under
    /// `#[cfg(feature = "preprod-live")]` (`seed_preprod_live_state`
    /// in `app.rs`); the in-app Create-DID wizard is currently
    /// unmounted (see `SessionEvent::Deploy`'s doc-comment). The
    /// method stays on the public API so re-enabling the wizard
    /// is a one-liner — silence dead-code for the
    /// non-`preprod-live` profile rather than hiding the method.
    #[cfg_attr(not(feature = "preprod-live"), allow(dead_code))]
    pub fn remember_controller_secret(&self, network: Network, did: String, sk: [u8; 32]) {
        if let Ok(mut g) = self.persistence.controller_secrets.lock() {
            g.insert(did.clone(), sk);
        }
        if let Some(store) = self.persistence.store.get() {
            if let Err(e) = store.put_controller_secret(network, &did, &sk) {
                tracing::warn!(error=%e, did=%did, "persist controller secret failed");
            }
        }
    }

    /// Look up the sk for a given DID. Hits the in-memory cache
    /// first; falls back to the persistent store on miss (and
    /// repopulates the cache on success). The store-fallback
    /// path needs the network the DID belongs to — callers
    /// usually have it; the wrapper [`controller_secret_for`]
    /// keeps the legacy network-less surface for hot reads.
    pub fn controller_secret_for_on(
        &self,
        network: Network,
        did: &str,
    ) -> Option<[u8; 32]> {
        if let Some(found) = self.controller_secret_for(did) {
            return Some(found);
        }
        let store = self.persistence.store.get()?;
        match store.get_controller_secret(network, did) {
            Ok(Some(sk)) => {
                let bytes: [u8; 32] = *sk;
                if let Ok(mut g) = self.persistence.controller_secrets.lock() {
                    g.insert(did.to_string(), bytes);
                }
                Some(bytes)
            }
            Ok(None) => None,
            Err(e) => {
                tracing::warn!(error=%e, did=%did, "load controller secret failed");
                None
            }
        }
    }

    /// Drop the in-session controller-secret cache entry for a
    /// DID. The persistent store row is owned by
    /// `WalletStore::forget_did`; this just keeps the in-memory
    /// shadow consistent so a subsequent `controller_secret_for_on`
    /// re-reads from the (now empty) store rather than returning a
    /// stale cached value. Used by the "Forget locally" path on
    /// the DID detail view.
    pub fn forget_controller_secret_on(&self, _network: Network, did: &str) {
        if let Ok(mut g) = self.persistence.controller_secrets.lock() {
            g.remove(did);
        }
    }

    /// Legacy network-less accessor — only checks the in-memory
    /// cache. Kept because some hot paths (e.g. the bridge RPC
    /// loop, where we don't have the network in scope cheaply)
    /// can't justify a store hit per call. UI code that already
    /// knows the network should prefer
    /// [`controller_secret_for_on`].
    pub fn controller_secret_for(&self, did: &str) -> Option<[u8; 32]> {
        self.persistence
            .controller_secrets
            .lock()
            .ok()
            .and_then(|g| g.get(did).copied())
    }

    /// Pull every controller secret on `network` out of the
    /// persistent store and into the in-memory cache. Called
    /// once at app startup right after [`set_store`]. Returns
    /// the number of secrets hydrated (or 0 if no store is
    /// attached).
    pub fn hydrate_controller_secrets(&self, network: Network) -> usize {
        let Some(store) = self.persistence.store.get() else {
            return 0;
        };
        match store.list_controller_secrets(network) {
            Ok(rows) => {
                let n = rows.len();
                if let Ok(mut g) = self.persistence.controller_secrets.lock() {
                    for (did, sk) in rows {
                        let bytes: [u8; 32] = *sk;
                        g.insert(did, bytes);
                    }
                }
                n
            }
            Err(e) => {
                tracing::warn!(error=%e, "hydrate controller secrets failed");
                0
            }
        }
    }
}

/// Spawn the embedded proof-server. Only built when the
/// `proof-server-http` feature is on (desktop-only — the actix
/// stack doesn't cross-compile to Android). On Android the wallet
/// always uses the in-process `LocalProvingProvider`; the JS
/// pipeline still works there, it just routes proving directly
/// through Rust without the HTTP wrapper.
#[cfg(feature = "proof-server-http")]
pub async fn spawn_proof_server(state: &BridgeState) -> Result<String, String> {
    use prover_core::spawn_local_server;
    let server = spawn_local_server().await.map_err(|e| e.to_string())?;
    let url = server.base_url();
    // Server keeps running in its own actix-rt thread; we leak the
    // handle on purpose so it lives until process exit.
    std::mem::forget(server);
    state
        .runtime
        .proof_server_url
        .set(url.clone())
        .map_err(|_| "proof_server_url already set".to_string())?;
    // Also expose the URL to `app::app_wallet_for` via the
    // process-wide static so every wallet constructed during this
    // App session routes proving through the embedded server.
    crate::app::set_proof_server_url(url.clone());
    tracing::info!(%url, "embedded proof-server ready");
    Ok(url)
}

#[cfg(not(feature = "proof-server-http"))]
pub async fn spawn_proof_server(_state: &BridgeState) -> Result<String, String> {
    // No-op stub: the Rust DID path uses `prover_core::ProverCore`
    // directly, no in-process HTTP server needed. Reached on
    // Android (no actix) and on desktop builds without
    // `--features proof-server-http`.
    Err("local proof-server only spawned when --features proof-server-http is enabled".into())
}

// ─── JSON-RPC payloads ─────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct RpcRequest {
    id: u64,
    method: String,
    #[serde(default)]
    params: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct RpcResponse {
    id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AddressParams {
    network: String,
}

// Placeholders for the JS-side wallet provider — fields are read once
// the corresponding methods are wired in Phase B+.
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct PublicKeyParams {
    /// Role index 0..=4 — see `wallet_core::Role`.
    role: u32,
    network: String,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct SignParams {
    /// Role index 0..=4.
    role: u32,
    /// Hex-encoded payload to sign.
    data: String,
}

// ─── Method implementations ────────────────────────────────────────

pub(crate) fn parse_network(s: &str) -> Result<Network, String> {
    match s {
        "mainnet" => Ok(Network::Mainnet),
        "preprod" => Ok(Network::PreProd),
        "preview" => Ok(Network::Preview),
        "qanet" => Ok(Network::QaNet),
        "devnet" => Ok(Network::DevNet),
        "undeployed" => Ok(Network::Undeployed),
        "undeployedyurii" => Ok(Network::UndeployedYurii),
        other => Err(format!("unknown network: {other}")),
    }
}

/// Default passport-vault contract address (the preprod demo vault).
/// Mirrors the dApp's `VAULT_CONTRACT_ADDRESS` default so the embedded
/// dApp's argument-less `vaultTotalLocked()` / `vaultDeposit()` verbs
/// resolve to the same contract without the dApp having to pass it.
const DEFAULT_VAULT_CONTRACT_ADDRESS: &str =
    "bdec50fe2f43959767a9bbc3b0626d5d9e9e08e06a723d3d2d0faca2e6c1dc25";

/// Resolve the vault contract address for a vault verb: explicit
/// `contractAddress` param first, then the
/// `MIDNIGHT_VAULT_CONTRACT_ADDRESS` env var, then the preprod default.
fn vault_contract_address(params: &serde_json::Value) -> String {
    if let Some(addr) = params.get("contractAddress").and_then(|v| v.as_str()) {
        let addr = addr.trim();
        if !addr.is_empty() {
            return addr.to_string();
        }
    }
    if let Ok(addr) = std::env::var("MIDNIGHT_VAULT_CONTRACT_ADDRESS") {
        let addr = addr.trim().to_string();
        if !addr.is_empty() {
            return addr;
        }
    }
    DEFAULT_VAULT_CONTRACT_ADDRESS.to_string()
}

/// Resolve the network for a vault verb: explicit `network` param first,
/// then the `MIDNIGHT_VAULT_NETWORK` env var, then PreProd (the demo
/// vault's network; matches `getConfiguration`'s default).
fn vault_network(params: &serde_json::Value) -> Network {
    if let Some(net) = params.get("network").and_then(|v| v.as_str()) {
        if let Ok(n) = parse_network(net.trim()) {
            return n;
        }
    }
    if let Ok(net) = std::env::var("MIDNIGHT_VAULT_NETWORK") {
        if let Ok(n) = parse_network(net.trim()) {
            return n;
        }
    }
    // Default to the wallet's launch network (PreProd unless
    // MIDNIGHT_WALLET_NETWORK overrides), so vault ops follow the wallet
    // unless explicitly pinned via MIDNIGHT_VAULT_NETWORK.
    crate::app::startup_network()
}

/// Resolve the network for a STANDARD connector method (addresses /
/// connection status): explicit `network` param first, else the
/// wallet's launch network. Address encodings are network-scoped
/// (HRP), so this must match the network the dApp connected to.
fn connected_network(params: &serde_json::Value) -> Network {
    if let Some(net) = params.get("network").and_then(|v| v.as_str()) {
        if let Ok(n) = parse_network(net.trim()) {
            return n;
        }
    }
    crate::app::startup_network()
}

/// Resolve the holder credential bundle (v3 JSON) for a claim: a
/// selected stored credential (`vcUri`, assembled from the wallet's
/// `vcs.redb`) or an explicit `bundle` object the dApp passes directly.
/// The env-fixture path is gone — claims now use the phone's real
/// stored credentials.
fn resolve_credential_bundle(params: &serde_json::Value) -> Result<serde_json::Value, String> {
    if let Some(b) = params.get("bundle") {
        if b.is_object() {
            return Ok(b.clone());
        }
    }
    if let Some(vc_uri) = params.get("vcUri").and_then(|v| v.as_str()) {
        let vc_uri = vc_uri.trim();
        if !vc_uri.is_empty() {
            return assemble_credential_bundle(vc_uri);
        }
    }
    Err("vaultClaim: select a credential — pass `vcUri` (a stored \
         digital-passport credential) or an explicit `bundle`"
        .to_string())
}

/// Build the v3 credential bundle `prepareVaultClaim` consumes from a
/// stored digital-passport VC. Re-encodes the credential body + proof
/// as `compact-value-v1.base64url`, and maps each per-claim opening
/// (`/credentialSubject/<field>`) into the `privateParts` the WebView
/// presentation builder + age predicate need. The holder presentation
/// key is NOT recovered here: digital-passport uses explicit-DID holder
/// binding (no committed holder key), so the WebView signs the
/// presentation with a deterministic keypair and pulls the holder
/// verification-method ref from the credential itself.
fn assemble_credential_bundle(vc_uri: &str) -> Result<serde_json::Value, String> {
    let store =
        RedbVcStore::open(vc_store_path()).map_err(|e| format!("open vc store: {e}"))?;
    let vc = store
        .get_vc(vc_uri)
        .map_err(|e| format!("read credential {vc_uri}: {e}"))?
        .ok_or_else(|| format!("no stored credential with uri {vc_uri}"))?;
    if vc.body.is_empty() || vc.proof.is_empty() {
        return Err(format!(
            "stored credential {vc_uri} is missing the compact body/proof needed to claim"
        ));
    }

    let opening = |path: &str| store.get_opening(vc_uri, path).ok().flatten();

    // dateOfBirth is mandatory for the age predicate (4-byte LE days,
    // matching the digital-passport opening encoding).
    let dob = opening(CLAIM_DATE_OF_BIRTH).ok_or_else(|| {
        format!("stored credential {vc_uri} has no dateOfBirth opening; cannot prove age")
    })?;
    if dob.plaintext.len() != 4 {
        return Err(format!(
            "dateOfBirth opening for {vc_uri} is {} bytes; expected 4 (LE days)",
            dob.plaintext.len()
        ));
    }
    let dob_days = u32::from_le_bytes([
        dob.plaintext[0],
        dob.plaintext[1],
        dob.plaintext[2],
        dob.plaintext[3],
    ]);

    // Optional disclosed claims; absent ones become zero-padding (only
    // consulted by the WebView when the lock policy requires them).
    let first = opening(CLAIM_FIRST_NAME);
    let last = opening(CLAIM_LAST_NAME);
    let doc = opening(CLAIM_DOCUMENT_NUMBER);
    let state = opening(CLAIM_ISSUING_STATE);

    let value_hex = |o: &Option<VcOpening>, len: usize| -> String {
        match o {
            Some(v) => hex::encode(&v.plaintext),
            None => hex::encode(vec![0u8; len]),
        }
    };
    let opening_hex = |o: &Option<VcOpening>| -> String {
        match o {
            Some(v) => hex::encode(&v.opening),
            None => hex::encode([0u8; 32]),
        }
    };

    Ok(serde_json::json!({
        "version": 3,
        "credential": {
            "encoding": "compact-value-v1.base64url",
            "payload": URL_SAFE_NO_PAD.encode(&vc.body),
        },
        "credentialProof": {
            "encoding": "compact-value-v1.base64url",
            "payload": URL_SAFE_NO_PAD.encode(&vc.proof),
        },
        "privateParts": {
            "claimValues": {
                "firstNameValuePaddedHex": value_hex(&first, 64),
                "lastNameValuePaddedHex": value_hex(&last, 64),
                "dateOfBirthDays": dob_days.to_string(),
                "documentNumberValueHex": value_hex(&doc, 32),
                "issuingStateValueHex": value_hex(&state, 32),
            },
            "openings": {
                "firstNameOpeningHex": opening_hex(&first),
                "lastNameOpeningHex": opening_hex(&last),
                "dateOfBirthOpeningHex": hex::encode(&dob.opening),
                "documentNumberOpeningHex": opening_hex(&doc),
                "issuingStateOpeningHex": opening_hex(&state),
            }
        }
    }))
}

/// Enumerate the wallet's stored digital-passport credentials as
/// display metadata (NO secrets) for the dApp's claim selector. Each
/// entry carries the `vcUri` the claim verb feeds back, plus a
/// best-effort display name and a `claimable` flag (has body/proof +
/// dateOfBirth opening).
fn list_credentials_json() -> Result<serde_json::Value, String> {
    let store =
        RedbVcStore::open(vc_store_path()).map_err(|e| format!("open vc store: {e}"))?;
    let vcs = store
        .list_ordered()
        .map_err(|e| format!("list credentials: {e}"))?;
    let mut out = Vec::new();
    for vc in vcs {
        if !is_digital_passport(&vc) {
            continue;
        }
        let fname = store
            .get_opening(&vc.vc_uri, CLAIM_FIRST_NAME)
            .ok()
            .flatten()
            .map(|o| decode_text_padded(&o.plaintext))
            .unwrap_or_default();
        let lname = store
            .get_opening(&vc.vc_uri, CLAIM_LAST_NAME)
            .ok()
            .flatten()
            .map(|o| decode_text_padded(&o.plaintext))
            .unwrap_or_default();
        let display_name = format!("{fname} {lname}").trim().to_string();
        let has_dob = store
            .get_opening(&vc.vc_uri, CLAIM_DATE_OF_BIRTH)
            .ok()
            .flatten()
            .is_some();
        let claimable = !vc.body.is_empty() && !vc.proof.is_empty() && has_dob;
        out.push(serde_json::json!({
            "vcUri": vc.vc_uri,
            "issuerDid": vc.issuer_did,
            "holderDid": vc.holder_did,
            "format": vc.format,
            "issuedAtMs": vc.issued_at_ms,
            "displayName": if display_name.is_empty() { serde_json::Value::Null } else { serde_json::Value::String(display_name) },
            "claimable": claimable,
        }));
    }
    Ok(serde_json::json!({ "credentials": out }))
}

/// Parse a `lockId` verb param (decimal string or number).
fn parse_lock_id(params: &serde_json::Value) -> Result<u64, String> {
    params
        .get("lockId")
        .and_then(|v| v.as_str().and_then(|s| s.trim().parse::<u64>().ok()).or_else(|| v.as_u64()))
        .ok_or_else(|| "missing/invalid lockId (decimal string)".to_string())
}

/// Parse an `amountBaseUnits` verb param (decimal string).
fn parse_amount(params: &serde_json::Value, verb: &str) -> Result<u128, String> {
    params
        .get("amountBaseUnits")
        .and_then(|v| v.as_str())
        .and_then(|s| s.trim().parse::<u128>().ok())
        .ok_or_else(|| format!("{verb}: missing/invalid amountBaseUnits (decimal string)"))
}

/// Default verifier-challenge preimage when the dApp doesn't pin one.
/// Any deterministic 32-byte value works: the lock stores it and the
/// claim presentation reads it back, so create/claim always agree.
const DEFAULT_CHALLENGE_LABEL: &[u8] = b"passport-vault:lock-challenge";

/// Build a 32-byte padded value from an optional verb string param,
/// returning `(present, padded)`. Empty / missing ⇒ `(false, zeros)`.
fn parse_optional_padded(params: &serde_json::Value, key: &str) -> (bool, [u8; 32]) {
    if let Some(s) = params.get(key).and_then(|v| v.as_str()) {
        let s = s.trim();
        if !s.is_empty() {
            let mut out = [0u8; 32];
            let b = s.as_bytes();
            let n = b.len().min(32);
            out[..n].copy_from_slice(&b[..n]);
            return (true, out);
        }
    }
    (false, [0u8; 32])
}

/// Resolve the lock's verifier challenge hash from an optional
/// `verifierChallengeHex` (32-byte hex) param, else the default label.
fn parse_challenge(params: &serde_json::Value) -> [u8; 32] {
    if let Some(h) = params.get("verifierChallengeHex").and_then(|v| v.as_str()) {
        if let Ok(bytes) = hex::decode(h.trim()) {
            if bytes.len() == 32 {
                let mut out = [0u8; 32];
                out.copy_from_slice(&bytes);
                return out;
            }
        }
    }
    let mut out = [0u8; 32];
    let n = DEFAULT_CHALLENGE_LABEL.len().min(32);
    out[..n].copy_from_slice(&DEFAULT_CHALLENGE_LABEL[..n]);
    out
}

/// Build a `VaultLockPolicy` from the `vaultCreateLock` verb params.
/// `minAge` is required; `maxClaimBaseUnits` defaults to
/// `fallback_max_claim` (the initial deposit) so a single redeemer can
/// draw the seeded pool. Issuing-state / document-number gates are
/// optional (disabled unless a value is supplied).
fn parse_lock_policy(
    params: &serde_json::Value,
    fallback_max_claim: u128,
) -> Result<VaultLockPolicy, String> {
    let min_age_raw = params
        .get("minAge")
        .and_then(|v| v.as_str().and_then(|s| s.trim().parse::<u64>().ok()).or_else(|| v.as_u64()))
        .ok_or_else(|| "vaultCreateLock: missing/invalid minAge".to_string())?;
    let min_age: u8 = min_age_raw
        .try_into()
        .map_err(|_| "vaultCreateLock: minAge out of range (0-255)".to_string())?;
    let max_claim = params
        .get("maxClaimBaseUnits")
        .and_then(|v| v.as_str())
        .and_then(|s| s.trim().parse::<u128>().ok())
        .filter(|m| *m > 0)
        .unwrap_or(fallback_max_claim);
    if max_claim == 0 {
        return Err(
            "vaultCreateLock: maxClaimBaseUnits (or a positive initial amount) is required"
                .to_string(),
        );
    }
    let (require_issuing_state, required_issuing_state) =
        parse_optional_padded(params, "issuingState");
    let (require_document_number, required_document_number) =
        parse_optional_padded(params, "documentNumber");
    Ok(VaultLockPolicy {
        min_age,
        require_issuing_state,
        required_issuing_state,
        require_document_number,
        required_document_number,
        max_claim,
        verifier_challenge_hash: parse_challenge(params),
    })
}

/// JS calls these methods through `window.midnightWallet.*`. The
/// **active wallet's seed** lives in Rust and is what we sign with —
/// we do not return it to JS. For iter-1 the active wallet is the
/// demo seed; later we'll thread the user-selected wallet through.
async fn dispatch(
    req: RpcRequest,
    state: &BridgeState,
    active_seed_hex: &str,
) -> RpcResponse {
    let id = req.id;
    let result = run_method(&req.method, req.params, state, active_seed_hex).await;
    match result {
        Ok(v) => RpcResponse { id, result: Some(v), error: None },
        Err(e) => RpcResponse { id, result: None, error: Some(e) },
    }
}

async fn run_method(
    method: &str,
    params: serde_json::Value,
    state: &BridgeState,
    active_seed_hex: &str,
) -> Result<serde_json::Value, String> {
    tracing::info!(rpc.method = %method, "bridge dispatch");
    match method {
        "ping" => Ok(serde_json::json!({"ok": true})),
        "bundleError" => {
            // Route to the right tracing level based on the
            // JS-side `kind` field. The channel is mis-named
            // ("bundleError") for historical reasons — JS uses
            // it for INFO status messages too (e.g.
            // "contract layer loaded"), not just errors. Don't
            // shout WARN at every info ping.
            let kind = params
                .get("kind")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let msg = params
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("(no message)")
                .to_string();
            let stack = params
                .get("stack")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            match kind.as_str() {
                "info" => {
                    tracing::info!(target: "bundle", %msg, "JS bundle event")
                }
                "warn" | "warning" => {
                    tracing::warn!(target: "bundle", %msg, %stack, "JS bundle warning")
                }
                "error" => {
                    tracing::error!(target: "bundle", %msg, %stack, "JS bundle error")
                }
                other => {
                    // Unknown kind → keep WARN so unfamiliar
                    // payload shapes still surface, but tag
                    // the kind so it's clear we didn't route.
                    tracing::warn!(
                        target: "bundle",
                        kind = %other,
                        %msg,
                        %stack,
                        "JS bundle event (unknown kind)",
                    )
                }
            }
            Ok(serde_json::json!({"ok": true}))
        }
        "getProofServerUrl" => state
            .proof_server_url()
            .map(|url| serde_json::json!(url))
            .ok_or_else(|| "proof-server not yet ready".to_string()),
        "getBech32Address" => {
            let p: AddressParams = serde_json::from_value(params)
                .map_err(|e| format!("invalid params: {e}"))?;
            let net = parse_network(&p.network)?;
            let seed = decode_seed(active_seed_hex)?;
            let addr =
                unshielded_bech32m(&seed, net).map_err(|e| format!("address: {e}"))?;
            Ok(serde_json::json!(addr))
        }
        "getPublicKey" => {
            let _p: PublicKeyParams = serde_json::from_value(params)
                .map_err(|e| format!("invalid params: {e}"))?;
            // TODO Phase B+: surface the role's public key bytes here.
            Err("getPublicKey: not implemented yet".to_string())
        }
        "signData" => {
            let _p: SignParams = serde_json::from_value(params)
                .map_err(|e| format!("invalid params: {e}"))?;
            // TODO Phase B+: derive role-specific signing key and
            // produce a schnorr signature over the payload.
            Err("signData: not implemented yet".to_string())
        }
        "getControllerSecretKey" => {
            // The on-chain `localSecretKey()` witness for a circuit
            // call on a specific DID. Each DID has its own random
            // controller sk minted at `create_did` time and stored
            // in `BridgeState.controller_secrets`. The 32 bytes
            // never leave the embedded WebView — they round-trip
            // across the Dioxus channel as hex strings.
            #[derive(serde::Deserialize)]
            struct Params {
                did: String,
            }
            let p: Params = serde_json::from_value(params)
                .map_err(|e| format!("invalid params: {e}"))?;
            let sk = state
                .controller_secret_for(&p.did)
                .ok_or_else(|| format!(
                    "no controller secret known for {} — was the DID created in this session?",
                    p.did
                ))?;
            Ok(serde_json::json!({ "secretKeyHex": hex::encode(sk) }))
        }
        // ──────────────────────────────────────────────────────
        // ContractCall pipeline hookpoints. See
        // `dioxus-wallet/src/app.rs::DidOperationsPanel` — drafts
        // collected there will be submitted through these methods
        // once wired. The JS side will use the bundled
        // `@midnight-ntwrk/midnight-did-contract` package to run
        // the Compact circuit against current state and return a
        // serialised `ContractCallPrototype`; Rust then wraps it
        // in an `Intent`, balances dust, proves the spend, and
        // submits.
        "didOp.prepareCall" => {
            // Expected params (TODO finalize):
            //   { did: string, circuit: string, inputs: object,
            //     controllerPublicKey: hex }
            // Expected result: { prototype: hex-serialised
            //   ContractCallPrototype<DefaultDB> }
            // The JS side runs the circuit against on-chain state
            // and returns the prototype; Rust builds the rest of
            // the transaction.
            Err("didOp.prepareCall: not implemented yet (Compact runtime bridge)".to_string())
        }
        "didOp.submit" => {
            // Expected params: { prototype: hex, did: string }
            // Returns: { tx_hash, block_hash, did }
            Err("didOp.submit: not implemented yet (Compact runtime bridge)".to_string())
        }
        // ──────────────────────────────────────────────────────
        // DApp Connector surface for an embedded dApp (the
        // passport-vault dApp hosted in this WebView). The dApp's
        // `window.midnight` host shim proxies these over postMessage
        // to the relay (see `lib.rs`), which forwards to
        // `window.midnightWallet.call(method, args)` → here.
        // ──────────────────────────────────────────────────────
        "getConfiguration" => {
            // Indexer / node / proof-server URIs + network id for the
            // requested network. Mirrors the DApp Connector
            // `getConfiguration()` shape so the embedded dApp can point
            // its own indexer/proof clients at the same services.
            #[derive(serde::Deserialize)]
            struct Params {
                network: Option<String>,
            }
            let p: Params =
                serde_json::from_value(params).unwrap_or(Params { network: None });
            let net = parse_network(p.network.as_deref().unwrap_or("preprod"))?;
            let cfg = net.config();
            Ok(serde_json::json!({
                "indexerUri": cfg.indexer_http_url,
                "indexerWsUri": cfg.indexer_ws_url,
                "proverServerUri": cfg.proving_server_url,
                "substrateNodeUri": cfg.node_ws_url,
                "networkId": cfg.network_id,
            }))
        }
        // ──────────────────────────────────────────────────────
        // STANDARD DApp Connector API — identity / status reads.
        // Backed by the active wallet's deps-free key derivation
        // (`Wallet::from_seed_hex`), so the embedded dApp's standard
        // `connect()` flow (getConnectionStatus + getShieldedAddresses)
        // resolves with real data instead of silently failing. These
        // mirror the `@midnight-ntwrk/dapp-connector-api` shapes.
        // (Balances + `getDustAddress` are deferred — they need sync
        // orchestration / a dust-address derivation not yet exposed by
        // wallet-core.)
        // ──────────────────────────────────────────────────────
        "getConnectionStatus" => {
            let net = connected_network(&params);
            Ok(serde_json::json!({
                "status": "connected",
                "networkId": net.config().network_id,
            }))
        }
        "getUnshieldedAddress" => {
            let net = connected_network(&params);
            let wallet = Wallet::from_seed_hex(active_seed_hex, net)
                .map_err(|e| format!("wallet: {e}"))?;
            let addr = wallet
                .unshielded_address()
                .map_err(|e| format!("unshielded address: {e}"))?;
            Ok(serde_json::json!({ "unshieldedAddress": addr }))
        }
        "getShieldedAddresses" => {
            let net = connected_network(&params);
            let wallet = Wallet::from_seed_hex(active_seed_hex, net)
                .map_err(|e| format!("wallet: {e}"))?;
            let shielded_address = wallet
                .shielded_address()
                .map_err(|e| format!("shielded address: {e}"))?;
            let coin = wallet
                .coin_public_key_hex()
                .map_err(|e| format!("coin public key: {e}"))?;
            let enc = wallet
                .encryption_public_key_hex()
                .map_err(|e| format!("encryption public key: {e}"))?;
            Ok(serde_json::json!({
                "shieldedAddress": shielded_address,
                "shieldedCoinPublicKey": coin,
                "shieldedEncryptionPublicKey": enc,
            }))
        }
        // Passport-vault lock / claim / total — the embedded dApp's
        // `vaultDeposit` / `vaultClaim` / `vaultTotalLocked` land here.
        // These build an app-owned `Wallet` (via `app_vault_wallet_for`,
        // which carries the network deps + the eval bridge + the
        // configurable vault-admin seed) and route to the native
        // pipeline. `vaultTotalLocked` is fully wired (indexer read +
        // in-WebView ledger decode); `vaultDeposit` / `vaultClaim` are
        // wired to the wallet but report the one remaining piece
        // (shielded-coin selection / holder presentation — scoped
        // follow-ups).
        "vaultTotalLocked" => {
            let net = vault_network(&params);
            let addr = vault_contract_address(&params);
            let total = crate::app::app_vault_wallet_for(net)
                .vault_total_locked(addr)
                .await?;
            Ok(serde_json::json!({ "totalLockedBaseUnits": total.to_string() }))
        }
        "vaultListLocks" => {
            // Enumerate the vault's locks (id, policy, per-lock pool) +
            // lockCount, for the dApp's lock list + claim selector.
            let net = vault_network(&params);
            let addr = vault_contract_address(&params);
            crate::app::app_vault_wallet_for(net).list_locks(addr).await
        }
        "vaultListCredentials" => {
            // Enumerate the phone's stored digital-passport credentials
            // (display metadata only) so the claimer can pick one.
            list_credentials_json()
        }
        "vaultCreateLock" => {
            // Create a lock with the locker-defined policy (min age, etc.)
            // and an optional initial deposit (`amountBaseUnits`). The
            // running wallet becomes the lock's creator. Funds the
            // `receiveUnshielded` with the wallet's unshielded NIGHT.
            let net = vault_network(&params);
            let addr = vault_contract_address(&params);
            let initial_amount: u128 = params
                .get("amountBaseUnits")
                .and_then(|v| v.as_str())
                .and_then(|s| s.trim().parse().ok())
                .unwrap_or(0);
            let policy = parse_lock_policy(&params, initial_amount)?;
            let outcome = crate::app::app_vault_wallet_for(net)
                .create_lock(addr, policy, initial_amount)
                .await?;
            Ok(serde_json::json!({
                "txHash": outcome.tx_hash,
                "lockId": outcome.lock_id.to_string(),
            }))
        }
        "vaultDeposit" => {
            // Top up an existing lock's pool: compose depositToLock(lockId,
            // amount), add the wallet's unshielded NIGHT spend + change +
            // DUST fees, prove, submit. Lock-creator-only on-chain.
            let net = vault_network(&params);
            let addr = vault_contract_address(&params);
            let lock_id = parse_lock_id(&params)?;
            let amount = parse_amount(&params, "vaultDeposit")?;
            let tx_hash = crate::app::app_vault_wallet_for(net)
                .deposit_to_lock(addr, lock_id, amount)
                .await?;
            Ok(serde_json::json!({ "txHash": tx_hash }))
        }
        "vaultClaim" => {
            // Unlock funds from a chosen lock with a chosen stored
            // credential: assemble the v3 bundle from `vcUri`, compose
            // claimFromLock(lockId, ...) (selective-disclosure presentation
            // + age proof), DUST-balance + prove + submit. The released
            // NIGHT goes to this wallet's unshielded address.
            let net = vault_network(&params);
            let addr = vault_contract_address(&params);
            let lock_id = parse_lock_id(&params)?;
            let amount = parse_amount(&params, "vaultClaim")?;
            let bundle = resolve_credential_bundle(&params)?;
            let tx_hash = crate::app::app_vault_wallet_for(net)
                .claim_from_lock(addr, lock_id, amount, bundle, None)
                .await?;
            Ok(serde_json::json!({ "txHash": tx_hash }))
        }
        other => Err(format!("unknown method: {other}")),
    }
}

fn decode_seed(hex_str: &str) -> Result<[u8; 32], String> {
    let bytes = hex::decode(hex_str).map_err(|e| format!("hex decode: {e}"))?;
    bytes
        .try_into()
        .map_err(|v: Vec<u8>| format!("expected 32 bytes, got {}", v.len()))
}

// ─── JS shim ───────────────────────────────────────────────────────

/// JS that exposes `window.midnightWallet.*` and pumps requests
/// through `dioxus.send` / `dioxus.recv`. The shim is single-loop:
/// every outgoing request gets a fresh id; every response is matched
/// against the pending map and resolves the original promise.
pub(crate) const BRIDGE_JS: &str = r#"
window.midnightWallet = window.midnightWallet || {};
(function () {
    const pending = new Map();
    let nextId = 1;
    function call(method, params) {
        return new Promise((resolve, reject) => {
            const id = nextId++;
            pending.set(id, { resolve, reject });
            dioxus.send({ id, method, params: params || {} });
        });
    }
    window.midnightWallet.ping              = ()        => call("ping");
    window.midnightWallet.getProofServerUrl = ()        => call("getProofServerUrl");
    window.midnightWallet.getBech32Address  = (network) => call("getBech32Address", { network });
    window.midnightWallet.getPublicKey      = (role, network) => call("getPublicKey", { role, network });
    window.midnightWallet.signData          = (role, data)    => call("signData", { role, data });
    window.midnightWallet.bundleError       = (payload)       => call("bundleError", payload);
    // Witness callback used by the DID-circuit JS executor. Returns
    // `{ secretKeyHex }` for the specified DID. The 32 bytes never
    // leave the WebView. Errors out if no sk is known for that DID
    // (e.g. created in a previous session — in-memory store).
    window.midnightWallet.getControllerSecretKey = (did) => call("getControllerSecretKey", { did });
    // Generic passthrough so the embedded-dApp relay (see lib.rs) can
    // invoke any bridge verb by name (getConfiguration / vaultDeposit /
    // vaultClaim / vaultTotalLocked / ...). Keeps the relay decoupled
    // from the per-method wrapper list above.
    window.midnightWallet.call = call;
    window.midnightWallet.getConfiguration  = (network) => call("getConfiguration", { network });
    // Multi-lock passport-vault surface. `createLock` defines a policy
    // (min age, optional country/doc) + seeds a pool; `deposit`/`claim`
    // target a specific `lockId`; `claim` also selects a stored
    // credential by `vcUri`. `listLocks` / `listCredentials` drive the
    // dApp's lock list + credential selector.
    window.midnightWallet.vaultCreateLock     = (params) => call("vaultCreateLock", params || {});
    window.midnightWallet.vaultDeposit        = (params) => call("vaultDeposit", params || {});
    window.midnightWallet.vaultClaim          = (params) => call("vaultClaim", params || {});
    window.midnightWallet.vaultListLocks      = ()       => call("vaultListLocks");
    window.midnightWallet.vaultListCredentials = ()      => call("vaultListCredentials");
    window.midnightWallet.vaultTotalLocked    = ()       => call("vaultTotalLocked");

    // Drain responses forever.
    (async () => {
        while (true) {
            const resp = await dioxus.recv();
            const handler = pending.get(resp.id);
            if (!handler) continue;
            pending.delete(resp.id);
            if (resp.error) handler.reject(new Error(resp.error));
            else handler.resolve(resp.result);
        }
    })();
})();
"#;

pub(crate) async fn handle_request(
    raw: serde_json::Value,
    state: &BridgeState,
    active_seed_hex: &str,
) -> Option<serde_json::Value> {
    let req: RpcRequest = match serde_json::from_value(raw) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error=%e, "invalid RPC request from JS");
            return None;
        }
    };
    let resp = dispatch(req, state, active_seed_hex).await;
    serde_json::to_value(&resp).ok()
}

/// Long-lived loop that drives the JS shim. Spawned as a `use_future`
/// from `App` once at mount time; lives until the window closes and
/// the future is dropped. Uses Dioxus' document JS-runner channel:
/// outgoing JSON messages drive `dioxus.recv()` on the JS side; each
/// `dioxus.send(...)` from JS is delivered back via `.recv()`.
pub async fn run_bridge_loop(state: BridgeState, active_seed_hex: String) {
    use dioxus::prelude::document;
    let mut handle = document::eval(BRIDGE_JS);
    loop {
        match handle.recv::<serde_json::Value>().await {
            Ok(raw) => {
                if let Some(json) = handle_request(raw, &state, &active_seed_hex).await {
                    let _ = handle.send(json);
                }
            }
            Err(e) => {
                tracing::warn!(error=?e, "bridge JS-runner channel closed");
                break;
            }
        }
    }
}
