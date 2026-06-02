//! WebView-side JS bridge driven by Dioxus `document::eval`.
//!
//! Mirrors the shape of [`wallet_core::js_bridge::NodeChildBridge`]
//! but runs every call through `window.midnightDidBundle.<method>(...)`
//! inside the embedded WebView instead of a Node child process. This
//! is what unblocks DID write flows (e.g. *Update DID* /
//! *Deactivate*) on Android, where the APK can't spawn `node`.
//!
//! Threading model. `dioxus::document::eval` is only safe to call
//! from inside the Dioxus runtime (the `Eval` handle is `!Send`
//! because it wraps a `GenerationalBox` keyed against the current
//! `RuntimeContext`). The wallet-core wizard streams run on
//! arbitrary tokio tasks, so we can't call `eval` directly from
//! their await points. Instead, a single "driver" task — spawned
//! by the App once at startup via `run_driver` — owns a
//! `tokio::sync::mpsc::UnboundedReceiver<EvalRequest>` and is the
//! only thing that touches `document::eval`. The
//! [`DioxusEvalBridge`] handle is `Send + Sync`: it just clones an
//! `UnboundedSender` and ferries `{ method, params, reply }`
//! requests over it.
//!
//! Each request carries a `tokio::sync::oneshot::Sender` for the
//! reply; the driver fills it once the JS promise resolves (or
//! rejects). That keeps every concurrent `call` independent of the
//! others — the driver processes them in order but the bridge
//! itself imposes no global serialisation.

use std::sync::Arc;
use std::sync::OnceLock;

use tokio::sync::mpsc;
use tokio::sync::oneshot;
use wallet_core::js_bridge::{JsBridge, JsBridgeError, JsBridgeExt};

/// In-flight call from the bridge handle to the driver task. The
/// driver runs the JS, then sends the JSON value back over `reply`.
pub struct EvalRequest {
    method: String,
    params: serde_json::Value,
    reply: oneshot::Sender<Result<serde_json::Value, JsBridgeError>>,
}

/// `Send + Sync` handle to the WebView-side JS bundle. Clone freely —
/// every clone shares the underlying mpsc sender, so all calls land
/// on the same driver task.
#[derive(Clone)]
pub struct DioxusEvalBridge {
    tx: mpsc::UnboundedSender<EvalRequest>,
}

impl DioxusEvalBridge {
    /// Build the bridge handle + driver receiver. The caller installs
    /// the bridge on `Wallet::with_js_bridge` and hands the receiver
    /// to [`run_driver`] inside a Dioxus `use_future`.
    pub fn new() -> (Self, mpsc::UnboundedReceiver<EvalRequest>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (Self { tx }, rx)
    }
}

#[async_trait::async_trait]
impl JsBridge for DioxusEvalBridge {
    async fn call_json(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, JsBridgeError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        let req = EvalRequest {
            method: method.to_string(),
            params,
            reply: reply_tx,
        };
        self.tx.send(req).map_err(|_| {
            JsBridgeError::Transport(
                "DioxusEvalBridge driver task is no longer running"
                    .to_string(),
            )
        })?;
        reply_rx.await.map_err(|_| {
            JsBridgeError::Transport(
                "DioxusEvalBridge driver dropped the reply channel"
                    .to_string(),
            )
        })?
    }
}

/// Drive a single `EvalRequest` through `document::eval`. The
/// generated JS expression awaits `window.midnightDidBundle[method]`
/// with the JSON-encoded params and returns either the raw result
/// or `{ error: "<msg>" }` — same convention the existing
/// `bridgeProbe` / `bridgeWitnessTest` snippets use.
/// Per-call eval timeout. JS-side methods that hang (unhandled
/// rejections, WASM compilation stalls, network roundtrips that
/// never finish) would otherwise wedge the driver indefinitely —
/// `document::eval` returns no future-cancellation handle, so we
/// race it against this timeout and report the call as
/// `Transport("eval timeout after Ns")` if the deadline wins. The
/// orphaned eval *future* leaks (we can't cancel it from the
/// outside), but the driver moves on to the next request so
/// subsequent calls aren't blocked. Empirically the heaviest
/// in-bundle call is `prepareUnprovenCallTx` for the largest DID
/// circuit; budget allows ~3× the slowest real-device path
/// observed so far.
const EVAL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(180);

async fn run_one(req: EvalRequest) {
    use dioxus::prelude::document;
    let params_json = serde_json::to_string(&req.params).unwrap_or_else(|_| "null".into());
    let method_json = serde_json::to_string(&req.method)
        .unwrap_or_else(|_| "\"\"".into());
    let snippet = format!(
        r#"const _m = {method_json}, _p = {params_json};
        if (!window.midnightDidBundle) {{
            return {{ error: "midnightDidBundle not loaded — js-bridge feature off?" }};
        }}
        const _fn = window.midnightDidBundle[_m];
        if (typeof _fn !== "function") {{
            return {{ error: "midnightDidBundle." + _m + " is not a function" }};
        }}
        try {{
            const r = await _fn(_p);
            return r;
        }} catch (e) {{
            return {{ error: String(e?.stack || e?.message || e) }};
        }}"#,
    );
    let outcome = match tokio::time::timeout(EVAL_TIMEOUT, document::eval(&snippet)).await {
        Ok(Ok(v)) => {
            if let Some(err) = v.get("error").and_then(|e| e.as_str()) {
                Err(JsBridgeError::JsError(err.to_string()))
            } else {
                Ok(v)
            }
        }
        Ok(Err(e)) => Err(JsBridgeError::Transport(format!("eval failed: {e}"))),
        Err(_) => {
            tracing::warn!(
                target: "eval-bridge",
                method = %req.method,
                "eval timed out after {:?}", EVAL_TIMEOUT,
            );
            Err(JsBridgeError::Transport(format!(
                "eval timeout after {}s — JS bundle did not reply",
                EVAL_TIMEOUT.as_secs(),
            )))
        }
    };
    // Receiver may have been dropped if the caller timed out / was
    // cancelled. Best-effort: a dropped reply isn't an error here.
    let _ = req.reply.send(outcome);
}

/// Long-lived driver loop. Owns the receiver end of the channel;
/// processes requests **in order** because each `run_one` awaits
/// `document::eval`. JS-side methods that need parallelism should
/// fan out within JS — Rust always sees a serialised call sequence.
///
/// Spawn this from a Dioxus `use_future` so the `document::eval`
/// inside `run_one` runs in the right runtime context.
pub async fn run_driver(mut rx: mpsc::UnboundedReceiver<EvalRequest>) {
    tracing::info!(target: "eval-bridge", "driver started");
    while let Some(req) = rx.recv().await {
        run_one(req).await;
    }
    tracing::info!(target: "eval-bridge", "driver shutting down — channel closed");
}

/// Process-wide bridge handle. Set once by the App's startup path;
/// read by `app_wallet_for` on every wallet construction so all DID
/// write flows route through the WebView JS bundle.
static BRIDGE: OnceLock<DioxusEvalBridge> = OnceLock::new();

/// Install the bridge into the process-wide slot and return the
/// receiver the driver task should consume. The first caller wins;
/// subsequent calls log a warning and drop the new bridge so the
/// already-installed one stays canonical. Returns `None` when the
/// slot was already populated — in that case there's nothing the
/// driver task could do for this caller; bail out cleanly.
pub fn install_global() -> Option<mpsc::UnboundedReceiver<EvalRequest>> {
    let (bridge, rx) = DioxusEvalBridge::new();
    match BRIDGE.set(bridge) {
        Ok(()) => Some(rx),
        Err(_) => {
            tracing::warn!(
                target: "eval-bridge",
                "global bridge already installed; ignoring duplicate install",
            );
            None
        }
    }
}

/// Get a clone of the process-wide bridge, if it has been installed.
/// `None` before `install_global` has run; the caller decides whether
/// to fall back to a different transport (desktop builds can still
/// spawn `NodeChildBridge` for tests; production builds always
/// install the eval bridge at startup).
pub fn global_bridge() -> Option<Arc<dyn JsBridge>> {
    BRIDGE.get().cloned().map(|b| {
        let b: Arc<dyn JsBridge> = Arc::new(b);
        b
    })
}

/// Drive the WebView-side QR scanner. Opens a full-viewport overlay
/// with a camera preview, runs jsQR on every animation frame, and
/// resolves with the decoded string on the first hit. Cancel button
/// returns `JsBridgeError::Transport("cancelled")` so the caller can
/// differentiate a user-initiated abort from a real failure.
///
/// Wraps the `scanQr` JS function declared in
/// `mobile-bench/dioxus-wallet/web/src/entry.ts`. Returns the raw
/// decoded payload (e.g. an `openid4vp://…` or
/// `openid-credential-offer://…` URL) — the caller's responsible for
/// parsing. The desktop [`crate::qr_scanner_fallback::FallbackQrScanner`]
/// is the canonical caller; Android uses
/// [`crate::qr_scanner_android::AndroidQrScanner`] (ML Kit) and
/// iOS uses [`crate::qr_scanner_ios::IosQrScanner`] (AVCaptureSession)
/// — both bypass this function, hence the `cfg(not(android | ios))`
/// gate.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub async fn scan_qr(bridge: &dyn JsBridge) -> Result<String, JsBridgeError> {
    #[derive(serde::Deserialize)]
    struct ScanResult {
        url: Option<String>,
        error: Option<String>,
    }
    let res: ScanResult = bridge.call("scanQr", serde_json::json!({})).await?;
    if let Some(url) = res.url {
        return Ok(url);
    }
    Err(JsBridgeError::Transport(
        res.error.unwrap_or_else(|| "no result".to_string()),
    ))
}

/// Read the system clipboard's text content.
///
/// Routes through the JS bundle's `pasteText` method, which
/// calls `navigator.clipboard.readText()`. Works on every
/// supported target:
///
/// - **Desktop Wry (macOS / Linux / Windows):** native paste via
///   Cmd-V already works into `<textarea>`; this helper is an
///   alternative entry point so the UI can offer a "📋 Paste"
///   button without depending on the user knowing the shortcut.
/// - **iOS sim + real device:** the long-press / Cmd-V paste
///   path into `<textarea>` is unreliable inside WKWebView —
///   this button is the only reliable way to land clipboard
///   contents in the input. iOS may show a one-time system
///   prompt before the first read.
/// - **Android WebView:** same `navigator.clipboard.readText()`
///   API; works on API 30+ inside a user gesture.
///
/// The `JsBridgeError::Transport` variant carries the JS-side
/// reason on failure: `"navigator.clipboard.readText unavailable"`,
/// permission denials, etc.
pub async fn paste_text(bridge: &dyn JsBridge) -> Result<String, JsBridgeError> {
    #[derive(serde::Deserialize)]
    struct PasteResult {
        text: Option<String>,
        error: Option<String>,
    }
    let res: PasteResult = bridge.call("pasteText", serde_json::json!({})).await?;
    if let Some(text) = res.text {
        return Ok(text);
    }
    Err(JsBridgeError::Transport(
        res.error.unwrap_or_else(|| "no result".to_string()),
    ))
}
