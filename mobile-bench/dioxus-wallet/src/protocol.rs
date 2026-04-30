//! Wry custom-protocol handler for `mn-pkg://`.
//!
//! Maps `mn-pkg://<package>/<rest...>` to
//! `<assets_root>/web/pkg/<package>/<rest...>` on disk and serves the
//! file with the right `Content-Type`. Combined with the import map
//! injected into `<head>` (see `lib.rs`), this lets the WebView's
//! native ES-module + WebAssembly machinery resolve and instantiate
//! upstream packages that bring `.wasm` along (compact-runtime,
//! onchain-runtime-wasm, midnight-did-contract, ledger-v8).
//!
//! Dynamic `import("@midnight-ntwrk/midnight-did-contract")` in the
//! WebView resolves through the import map → `mn-pkg://...` →
//! protocol handler → file bytes → browser parses → recursively loads
//! the module's own relative `./xxx.wasm` imports through the same
//! protocol → native `WebAssembly.instantiate` happens in the engine.
//! No esbuild WASM plugin, no synthetic wrappers.
//!
//! **Path-traversal guard.** We reject any URL whose normalized path
//! escapes `<assets_root>/web/pkg/`. Without this, a JS bundle could
//! exfiltrate arbitrary files via `mn-pkg://x/../../etc/passwd`.

use std::borrow::Cow;
use std::path::{Path, PathBuf};

use dioxus::desktop::wry::http::{HeaderValue, Request, Response, StatusCode};

/// Build the protocol handler for the given assets root. The
/// returned closure is `'static + Fn` and matches the signature
/// Dioxus 0.6's `Config::with_custom_protocol` expects.
pub fn build_handler(
    assets_root: PathBuf,
) -> impl Fn(Request<Vec<u8>>) -> Response<Cow<'static, [u8]>> + 'static {
    let pkg_root = assets_root.join("web").join("pkg");
    move |req: Request<Vec<u8>>| handle(&pkg_root, req)
}

fn handle(pkg_root: &Path, req: Request<Vec<u8>>) -> Response<Cow<'static, [u8]>> {
    let uri = req.uri();
    tracing::info!(target: "mn-pkg", url = %uri, "request");
    // The authority (`localhost` in the import map) is just a
    // placeholder so URL parsers don't choke; ignore it. We map the
    // *path* directly under `pkg_root`, so a request like
    // `mn-pkg://localhost/midnight-did-contract/dist/index.js`
    // resolves to `<pkg_root>/midnight-did-contract/dist/index.js`.
    let rel = uri.path().trim_start_matches('/').to_string();

    if !is_safe(&rel) {
        tracing::warn!(target: "mn-pkg", %rel, "rejected unsafe path");
        return error(StatusCode::FORBIDDEN, "unsafe path");
    }

    let file_path = pkg_root.join(&rel);
    let bytes = match std::fs::read(&file_path) {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!(
                target: "mn-pkg",
                rel = %rel,
                file = %file_path.display(),
                error = %e,
                "asset not found"
            );
            return error(StatusCode::NOT_FOUND, &e.to_string());
        }
    };

    let content_type = mime_for(&file_path);
    tracing::debug!(target: "mn-pkg", rel = %rel, len = bytes.len(), %content_type, "served");

    let mut resp = Response::new(Cow::Owned(bytes));
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
