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

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::OnceCell;
use wallet_core::{Network, unshielded_bech32m};

#[derive(Clone, Default)]
pub struct BridgeState {
    pub proof_server_url: Arc<OnceCell<String>>,
}

impl BridgeState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Best-effort URL accessor for UI display. Returns `None` until
    /// the local proof-server has finished booting.
    pub fn proof_server_url(&self) -> Option<String> {
        self.proof_server_url.get().cloned()
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
