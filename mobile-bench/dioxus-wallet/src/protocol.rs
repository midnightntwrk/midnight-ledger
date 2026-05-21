//! Wry custom-protocol handler for `mn-pkg://`.
//!
//! Maps `mn-pkg://<package>/<rest...>` to a file inside an embedded
//! `assets/web/pkg/` tree (compiled in via [`include_dir!`]) and
//! serves it with the right `Content-Type`. Combined with the
//! import map injected into `<head>` (see `lib.rs`), this lets the
//! WebView's native ES-module + WebAssembly machinery resolve and
//! instantiate upstream packages that bring `.wasm` along
//! (compact-runtime, onchain-runtime-wasm, midnight-did-contract,
//! ledger-v8).
//!
//! Dynamic `import("@midnight-ntwrk/midnight-did-contract")` in the
//! WebView resolves through the import map → `mn-pkg://...` →
//! protocol handler → file bytes → browser parses → recursively loads
//! the module's own relative `./xxx.wasm` imports through the same
//! protocol → native `WebAssembly.instantiate` happens in the engine.
//! No esbuild WASM plugin, no synthetic wrappers.
//!
//! Using `include_dir!` (rather than reading from `CARGO_MANIFEST_DIR`
//! at runtime) is what lets the same handler work on Android — the
//! app sandbox has no filesystem access to the host's source tree.

use std::borrow::Cow;
use std::path::Path;

// `dioxus::desktop` is only re-exported when the `desktop` feature
// is on; on Android + iOS the same types are reachable via
// `dioxus::mobile` (which itself re-exports `dioxus_desktop::*`).
// Pick the right path under cfg so this module compiles on every
// platform.
#[cfg(all(not(target_os = "android"), not(target_os = "ios")))]
use dioxus::desktop::wry::http::{HeaderValue, Request, Response, StatusCode};
#[cfg(any(target_os = "android", target_os = "ios"))]
use dioxus::mobile::wry::http::{HeaderValue, Request, Response, StatusCode};
use include_dir::{Dir, include_dir};

/// Compile-time embed of `mobile-bench/dioxus-wallet/assets/web/pkg/`.
/// ~30 MB on disk (mostly `midnight-did-contract/dist`). Costs
/// binary size but means the JS bundle's `import()` calls resolve
/// without filesystem reads — same code path on desktop and Android.
static PKG_TREE: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/assets/web/pkg");

/// Build the protocol handler. The returned closure is
/// `'static + Fn` and matches the signature Dioxus 0.6's
/// `Config::with_custom_protocol` expects.
pub fn build_handler()
-> impl Fn(Request<Vec<u8>>) -> Response<Cow<'static, [u8]>> + 'static {
    |req: Request<Vec<u8>>| handle(req)
}

fn handle(req: Request<Vec<u8>>) -> Response<Cow<'static, [u8]>> {
    let uri = req.uri();
    tracing::info!(target: "mn-pkg", url = %uri, "request");
    // The authority (`localhost` in the import map) is just a
    // placeholder so URL parsers don't choke; ignore it. We map the
    // *path* directly under `PKG_TREE`, so a request like
    // `mn-pkg://localhost/midnight-did-contract/dist/index.js`
    // resolves to `midnight-did-contract/dist/index.js` inside the
    // embedded tree.
    let rel = uri.path().trim_start_matches('/').to_string();

    if !is_safe(&rel) {
        tracing::warn!(target: "mn-pkg", %rel, "rejected unsafe path");
        return error(StatusCode::FORBIDDEN, "unsafe path");
    }

    let Some(file) = PKG_TREE.get_file(&rel) else {
        tracing::warn!(target: "mn-pkg", %rel, "asset not found in embedded tree");
        return error(StatusCode::NOT_FOUND, "asset not found");
    };
    let bytes = file.contents();
    let content_type = mime_for(Path::new(&rel));
    tracing::debug!(
        target: "mn-pkg",
        rel = %rel, len = bytes.len(), %content_type, "served"
    );

    let mut resp = Response::new(Cow::Borrowed(bytes));
    *resp.status_mut() = StatusCode::OK;
    resp.headers_mut().insert(
        "content-type",
        HeaderValue::from_static(content_type),
    );
    // Some module loads (e.g. via `import()`) treat the response as
    // cross-origin if CORS isn't permissive. Loopback and a custom
    // scheme make this academic but we set it to keep the engine
    // happy.
    resp.headers_mut().insert(
        "access-control-allow-origin",
        HeaderValue::from_static("*"),
    );
    resp
}

fn error(status: StatusCode, msg: &str) -> Response<Cow<'static, [u8]>> {
    let mut resp = Response::new(Cow::Owned(msg.as_bytes().to_vec()));
    *resp.status_mut() = status;
    resp.headers_mut().insert(
        "content-type",
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    resp
}

fn is_safe(rel: &str) -> bool {
    !rel.split(['/', '\\']).any(|seg| seg == ".." || seg.is_empty() && rel.contains(".."))
}

fn mime_for(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("js" | "mjs" | "cjs") => "application/javascript; charset=utf-8",
        Some("json" | "map") => "application/json; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("ts" | "mts" | "cts") => "application/typescript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("html") => "text/html; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        _ => "application/octet-stream",
    }
}
