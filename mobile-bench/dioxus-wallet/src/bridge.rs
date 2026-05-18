//! JS ↔ Rust bridge for the embedded WebView.
//!
//! Two responsibilities:
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

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tokio::sync::OnceCell;
use wallet_core::store::WalletStore;
use wallet_core::{Network, unshielded_bech32m};

/// Per-DID random controller secret store. Populated by
/// `CreateDidWizard.on_done` and read by the
/// `getControllerSecretKey` bridge RPC during JS-driven circuit
/// execution. In-memory hot cache; the canonical source of truth
/// is the persistent `WalletStore` (when attached via
/// [`BridgeState::set_store`]). Each DID's 32 bytes round-trip
/// across the Dioxus channel as hex but only inside the embedded
/// WebView.
pub type ControllerSecretStore = Arc<Mutex<HashMap<String, [u8; 32]>>>;

#[derive(Clone, Default)]
// `PartialEq` here lets `BridgeState` ride as a Dioxus
// component prop (the `#[component]` macro requires Props
// to be Eq). Two `BridgeState` values are equal iff every
// inner `Arc` is pointer-equal — fine because the App
// constructs exactly one `BridgeState` and clones it; we
// never compare independently-built handles for content.
pub struct BridgeState {
    pub proof_server_url: Arc<OnceCell<String>>,
    /// `did_string → 32-byte sk`. Cloning the BridgeState clones the
    /// Arc, so the map is shared across the bridge loop, the UI, and
    /// any future callers.
    pub controller_secrets: ControllerSecretStore,
    /// Persistent backing store. Set once at app startup via
    /// [`set_store`]. When present, `remember_controller_secret`
    /// writes through (best-effort — a store error is logged but
    /// does not fail the in-memory cache update, so an unhealthy
    /// disk doesn't break a freshly-deployed DID's signing path
    /// for the current session). When absent, behaviour matches
    /// the previous in-memory-only model.
    pub store: Arc<OnceCell<WalletStore>>,
}

impl PartialEq for BridgeState {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.proof_server_url, &other.proof_server_url)
            && Arc::ptr_eq(&self.controller_secrets, &other.controller_secrets)
            && Arc::ptr_eq(&self.store, &other.store)
    }
}
impl Eq for BridgeState {}

impl BridgeState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Best-effort URL accessor for UI display. Returns `None` until
    /// the local proof-server has finished booting.
    pub fn proof_server_url(&self) -> Option<String> {
        self.proof_server_url.get().cloned()
    }

    /// Attach a `WalletStore`. Idempotent — subsequent calls
    /// after the first succeed are no-ops. Returns the store
    /// handle that ended up installed (either the just-set one
    /// or a previously-set one) so the caller doesn't need a
    /// follow-up read.
    pub fn set_store(&self, store: WalletStore) -> WalletStore {
        let _ = self.store.set(store);
        self.store.get().cloned().expect("just-set store reachable")
    }

    /// Borrow the attached store, if any. Returns `None` before
    /// `set_store` has run — useful during the early bridge
    /// boot path that fires before the store is opened.
    #[allow(dead_code)] // Surfaced via [`Self::store`] for future bridge-RPC handlers
    /// that want to persist beyond controller secrets.
    pub fn store(&self) -> Option<&WalletStore> {
        self.store.get()
    }

    /// Record the random sk minted for a freshly-deployed DID.
    /// Overwrites any existing entry (a fresh deploy with the same
    /// id would be impossible on-chain, but defensive). Persists
    /// to the attached `WalletStore` (if any) under
    /// `(network, did)`; a write failure is logged and the
    /// in-memory cache is still populated so the current session
    /// is uninterrupted.
    pub fn remember_controller_secret(&self, network: Network, did: String, sk: [u8; 32]) {
        if let Ok(mut g) = self.controller_secrets.lock() {
            g.insert(did.clone(), sk);
        }
        if let Some(store) = self.store.get() {
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
        let store = self.store.get()?;
        match store.get_controller_secret(network, did) {
            Ok(Some(sk)) => {
                let bytes: [u8; 32] = *sk;
                if let Ok(mut g) = self.controller_secrets.lock() {
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

    /// Legacy network-less accessor — only checks the in-memory
    /// cache. Kept because some hot paths (e.g. the bridge RPC
    /// loop, where we don't have the network in scope cheaply)
    /// can't justify a store hit per call. UI code that already
    /// knows the network should prefer
    /// [`controller_secret_for_on`].
    pub fn controller_secret_for(&self, did: &str) -> Option<[u8; 32]> {
        self.controller_secrets
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
        let Some(store) = self.store.get() else {
            return 0;
        };
        match store.list_controller_secrets(network) {
            Ok(rows) => {
                let n = rows.len();
                if let Ok(mut g) = self.controller_secrets.lock() {
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
/// `js-bridge` feature is on (the JS pipeline talks to it via
/// loopback HTTP). The Rust DID code calls `prover-core` directly
/// via its library API and doesn't need this server at all.
#[cfg(all(feature = "js-bridge", not(target_os = "android")))]
pub async fn spawn_proof_server(state: &BridgeState) -> Result<String, String> {
    use prover_core::spawn_local_server;
    let server = spawn_local_server().await.map_err(|e| e.to_string())?;
    let url = server.base_url();
    // Server keeps running in its own actix-rt thread; we leak the
    // handle on purpose so it lives until process exit.
    std::mem::forget(server);
    state
        .proof_server_url
        .set(url.clone())
        .map_err(|_| "proof_server_url already set".to_string())?;
    tracing::info!(%url, "embedded proof-server ready");
    Ok(url)
}

#[cfg(not(all(feature = "js-bridge", not(target_os = "android"))))]
pub async fn spawn_proof_server(_state: &BridgeState) -> Result<String, String> {
    // No-op stub: the Rust DID path uses `prover_core::ProverCore`
    // directly, no in-process HTTP server needed.
    Err("local proof-server only spawned when --features js-bridge is enabled".into())
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

fn parse_network(s: &str) -> Result<Network, String> {
    match s {
        "mainnet" => Ok(Network::Mainnet),
        "preprod" => Ok(Network::PreProd),
        "preview" => Ok(Network::Preview),
        "qanet" => Ok(Network::QaNet),
        "devnet" => Ok(Network::DevNet),
        "undeployed" => Ok(Network::Undeployed),
        other => Err(format!("unknown network: {other}")),
    }
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
            // Surface the JS-side error at WARN; the structured payload
            // is whatever the JS error reporter built. We don't error
            // out — JS shouldn't crash the bridge on a logging call.
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
            tracing::warn!(target: "bundle", %kind, %msg, %stack, "JS bundle error");
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
