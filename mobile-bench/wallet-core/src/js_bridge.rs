//! Transport-agnostic JS bridge.
//!
//! The wallet drives JavaScript in two places:
//!
//! - **Production** — Dioxus desktop embeds a WebView; the
//!   dioxus-wallet crate provides a `DioxusEvalBridge` (not in this
//!   module; lives in `mobile-bench/dioxus-wallet/src/`).
//! - **Tests** — a Node child process running the harness at
//!   `mobile-bench/wallet-core/tests/js-harness/harness.mjs`. The
//!   [`NodeChildBridge`] in this module spawns it and pipes
//!   newline-delimited JSON-RPC over stdin/stdout.
//!
//! Both implement the same [`JsBridge`] trait, so the
//! Compact-runtime-driven flows (`call_did_circuit`, etc.) consume
//! the trait and don't care which transport they got.
//!
//! Why two transports — a `cargo test` can't drive the production
//! WebView on macOS (tao requires the main thread; libtest uses
//! workers) or in headless Linux CI (wry's webkitgtk needs a
//! display). The Node harness gives us full coverage of the JS
//! pipeline from `cargo test` with no display dependency.

use std::sync::Arc;

use serde::de::DeserializeOwned;
use serde::Serialize;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::Mutex;

/// Errors a bridge call can produce. The boundary is intentionally
/// thin — transports return strings; callers parse them.
#[derive(Debug, thiserror::Error)]
pub enum JsBridgeError {
    /// IO failure on the underlying transport.
    #[error("transport: {0}")]
    Transport(String),
    /// JS-side returned a structured `error` payload.
    #[error("js error: {0}")]
    JsError(String),
    /// JSON (de)serialisation failure on either direction.
    #[error("encode/decode: {0}")]
    Codec(String),
}

/// Asynchronous Rust → JS call channel. Implementors marshal
/// `{ method, params }` to JS, await `{ result }` or `{ error }`,
/// and return the structured payload.
#[async_trait::async_trait]
pub trait JsBridge: Send + Sync {
    /// Invoke a JS method by name with structured params and
    /// deserialise the result into `T`. Errors propagate either
    /// the transport (process died, channel closed) or the JS
    /// side (`{ error: "..." }` reply).
    async fn call<P: Serialize + Send + Sync, T: DeserializeOwned + Send>(
        &self,
        method: &str,
        params: P,
    ) -> Result<T, JsBridgeError>;
}

/// Subprocess-based bridge backed by `node` running the harness at
/// `tests/js-harness/harness.mjs`. Owns the child's stdin/stdout
/// behind an `Arc<Mutex>` so multiple `call`s from a single test
/// serialise cleanly.
///
/// The child stays alive until [`NodeChildBridge`] is dropped or
/// the process exits on its own (stdin closed).
pub struct NodeChildBridge {
    /// Kept around so the child is killed when the bridge drops.
    /// Public only inside the struct — see [`NodeChildBridge::shutdown`].
    _child: Mutex<Child>,
    inner: Arc<BridgeInner>,
}

struct BridgeInner {
    stdin: Mutex<ChildStdin>,
    /// `BufReader` over the child's stdout. Calls grab the lock,
    /// write a request, then read the next line.
    stdout: Mutex<BufReader<ChildStdout>>,
    /// Monotonically increasing request id.
    next_id: Mutex<u64>,
}

impl NodeChildBridge {
    /// Spawn `node <harness_script>` and capture stdin/stdout.
    /// Stderr is inherited so harness diagnostics surface in
    /// `cargo test -- --nocapture` output.
    pub fn spawn(harness_script: &std::path::Path) -> Result<Self, JsBridgeError> {
        use std::process::Stdio;
        let mut child = tokio::process::Command::new("node")
            .arg(harness_script)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| JsBridgeError::Transport(format!("spawn node: {e}")))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| JsBridgeError::Transport("child stdin missing".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| JsBridgeError::Transport("child stdout missing".into()))?;
        Ok(Self {
            _child: Mutex::new(child),
            inner: Arc::new(BridgeInner {
                stdin: Mutex::new(stdin),
                stdout: Mutex::new(BufReader::new(stdout)),
                next_id: Mutex::new(1),
            }),
        })
    }

    /// Default harness script path inside the wallet-core crate:
    /// `<CARGO_MANIFEST_DIR>/tests/js-harness/harness.mjs`. Suitable
    /// for `cargo test` callers; production paths supply their own.
    pub fn default_harness_path() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("js-harness")
            .join("harness.mjs")
    }
}

#[async_trait::async_trait]
impl JsBridge for NodeChildBridge {
    async fn call<P: Serialize + Send + Sync, T: DeserializeOwned + Send>(
        &self,
        method: &str,
        params: P,
    ) -> Result<T, JsBridgeError> {
        let id = {
            let mut g = self.inner.next_id.lock().await;
            let v = *g;
            *g += 1;
            v
        };
        let req = serde_json::json!({
            "id": id,
            "method": method,
            "params": params,
        });
        let line = serde_json::to_string(&req)
            .map_err(|e| JsBridgeError::Codec(format!("encode req: {e}")))?;
        {
            let mut stdin = self.inner.stdin.lock().await;
            stdin
                .write_all(line.as_bytes())
                .await
                .map_err(|e| JsBridgeError::Transport(format!("write: {e}")))?;
            stdin
                .write_all(b"\n")
                .await
                .map_err(|e| JsBridgeError::Transport(format!("write nl: {e}")))?;
            stdin
                .flush()
                .await
                .map_err(|e| JsBridgeError::Transport(format!("flush: {e}")))?;
        }
        // Read the next response line. The harness is single-
        // threaded and replies in order; we hold the stdout lock
        // for the full request → response window, which also
        // serialises concurrent `call`s.
        let mut buf = String::new();
        {
            let mut stdout = self.inner.stdout.lock().await;
            let n = stdout
                .read_line(&mut buf)
                .await
                .map_err(|e| JsBridgeError::Transport(format!("read: {e}")))?;
            if n == 0 {
                return Err(JsBridgeError::Transport(
                    "child stdout closed before response".into(),
                ));
            }
        }
        let resp: serde_json::Value = serde_json::from_str(buf.trim())
            .map_err(|e| JsBridgeError::Codec(format!("decode resp: {e}")))?;
        if let Some(err) = resp.get("error").and_then(|v| v.as_str()) {
            return Err(JsBridgeError::JsError(err.to_string()));
        }
        let result = resp.get("result").cloned().ok_or_else(|| {
            JsBridgeError::Codec(format!("response missing `result`: {resp}"))
        })?;
        serde_json::from_value::<T>(result)
            .map_err(|e| JsBridgeError::Codec(format!("decode result: {e}")))
    }
}
