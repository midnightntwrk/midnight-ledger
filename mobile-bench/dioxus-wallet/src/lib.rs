#![deny(warnings)]

mod app;
mod bench_stage;
mod bridge;
mod did_picker;
mod identity_centre;
#[cfg(feature = "js-bridge")]
pub(crate) mod eval_bridge;
mod format;
mod logs;
mod platform;
mod proc_stats;
mod session_persist;
mod telemetry_panel;
mod vc_views;
#[cfg(feature = "js-bridge")]
mod protocol;

// QR scanner — each target binds the platform's native scanner:
// Android uses Google ML Kit via JNI, iOS uses AVCaptureSession
// via a Swift extern-C bridge. Desktop falls back to a paste-URL
// prompt (no host camera path through Wry). `ActiveQrScanner` is
// the platform-resolved alias the wallet code instantiates; the
// implementations live in the sibling modules below, each gated
// to its target. See:
//   - docs/superpowers/specs/2026-06-02-native-qr-scanner-android.md
//   - docs/superpowers/specs/2026-xx-xx-native-qr-scanner-ios-avfoundation.md
#[cfg(target_os = "android")]
mod qr_scanner_android;
#[cfg(target_os = "ios")]
mod qr_scanner_ios;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
mod qr_scanner_fallback;

#[cfg(target_os = "android")]
pub(crate) use qr_scanner_android::AndroidQrScanner as ActiveQrScanner;
#[cfg(target_os = "ios")]
pub(crate) use qr_scanner_ios::IosQrScanner as ActiveQrScanner;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub(crate) use qr_scanner_fallback::FallbackQrScanner as ActiveQrScanner;

// Wallet worker thread — central serialiser for heavy chain ops.
// Boots once at App::run, lives until process exit. See
// docs/superpowers/specs/2026-06-02-wallet-worker-thread.md for
// the architecture rationale.
pub(crate) mod worker;

// DID-port adapters — implement wallet-core's OID4VP discovery +
// signing ports on top of the dioxus-wallet runtime types. Used by
// the new IdTokenBuilder + LoginCoordinator pipeline; see
// docs/superpowers/specs/2026-06-02-login-with-did-architecture.md
// for the wider Login-with-DID architecture.
pub(crate) mod did_ports;

pub fn run() {
    // Two tracing layers ride together: the standard `fmt`
    // layer for stderr (developer feedback when running
    // `cargo run`) and `WalletLogLayer` which feeds the UI's
    // Logs tab + the redb archive. Install once at process
    // start; both consume every event the macros emit.
    //
    // We hand the matching `LogCapture` over to `App` via a
    // module-local OnceLock so the bridge can pick it up
    // without threading a parameter through `run()` →
    // `launch()` → component tree.
    let (capture, rx) = logs::LogCapture::new();
    let _ = logs::LOG_CAPTURE.set(capture.clone());
    let _ = logs::LOG_RX.set(std::sync::Mutex::new(Some(rx)));
    use tracing_subscriber::filter::EnvFilter;
    use tracing_subscriber::layer::SubscriberExt as _;
    use tracing_subscriber::util::SubscriberInitExt as _;
    use tracing_subscriber::Layer as _;
    // Honour `RUST_LOG` for stderr, defaulting to a sensible filter
    // that mutes dioxus_core's TRACE deluge (which previously
    // produced multi-GB log files in minutes and burned the App's
    // CPU on rendering trace messages). The UI's Logs tab still
    // captures everything via `WalletLogLayer` — that one is
    // unfiltered so the in-app archive is complete regardless of
    // what stderr shows.
    let stderr_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new(
            "warn,wallet_core=debug,dioxuswalletmain=info,bundle=info,eval-bridge=info,mn-pkg=info,wallet_worker=info",
        )
    });
    let _ = tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_filter(stderr_filter))
        .with(logs::WalletLogLayer::new(capture))
        // BenchStageLayer captures `midnight_bench` target events
        // emitted by `contract-benchmark` and pins the latest stage
        // to a static the Benchmark tab polls. See
        // `bench_stage::current_stage`.
        .with(bench_stage::BenchStageLayer::new())
        .try_init();
    // rustls 0.23 panics on first TLS use if no `CryptoProvider` is
    // marked default — dioxus-desktop pulls in `aws-lc-rs` while
    // reqwest / tokio-tungstenite pull `ring`. Pick the wallet-core
    // default (`ring`) *before* any future hits TLS (probe, indexer
    // fetch, node WS, etc.). Idempotent; safe across reloads.
    wallet_core::ensure_default_crypto_provider();
    desktop_or_mobile_launch();
}

#[cfg(all(not(target_os = "android"), not(target_os = "ios")))]
fn desktop_or_mobile_launch() {
    use dioxus::desktop::{Config, LogicalSize, WindowBuilder};
    // Default to a phone-sized window (Pixel 7 ≈ 412 × 915 dp; we use
    // 390 × 844 which matches iPhone 14 / Pixel 7a — the same envelope
    // gsd-wallet's popup renders inside). Lets us iterate on the
    // mobile layout without needing an emulator on the desk. The
    // user can still resize freely; we only set the *initial* size.
    // Rasterise the compact Midnight monogram (a 69×69 SVG) to a
    // 128×128 RGBA buffer for the platform window icon. `resvg`
    // handles SVG → pixmap; `tao::Icon::from_rgba` accepts the
    // exact buffer shape. If anything fails (malformed SVG, OOM,
    // etc.) we fall back to no icon — the App still launches.
    let icon = build_window_icon();
    let mut window = WindowBuilder::new()
        .with_title("Midnight Wallet")
        .with_inner_size(LogicalSize::new(390.0, 844.0))
        .with_resizable(true);
    if let Some(i) = icon {
        window = window.with_window_icon(Some(i));
    }
    // Default config: no head injection beyond what Dioxus adds.
    // The `js-bridge` feature opts into vendored TS package loading
    // via `<head>` import map + Wry custom protocol — see
    // [DID_PLAN.md](../../DID_PLAN.md) for the architecture
    // decision. Mainline DID work is Rust-native.
    let cfg = Config::new()
        .with_window(window)
        .with_disable_context_menu(false);

    #[cfg(feature = "js-bridge")]
    let cfg = with_js_bridge(cfg);

    dioxus::LaunchBuilder::desktop()
        .with_cfg(cfg)
        .launch(app::App);
}

/// Rasterise the compact Midnight monogram SVG to a 128×128 RGBA
/// window icon. `resvg` parses + renders; `tiny_skia::Pixmap`
/// owns the RGBA buffer. Returns `None` on any failure (malformed
/// SVG, allocator returned `None`, dioxus version skew) — the
/// App still launches, just without a custom icon.
#[cfg(all(not(target_os = "android"), not(target_os = "ios")))]
fn build_window_icon() -> Option<dioxus::desktop::tao::window::Icon> {
    const ICON_SIZE: u32 = 128;
    let opt = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_str(app::LOGO_ICON_SVG, &opt).ok()?;
    let mut pixmap = tiny_skia::Pixmap::new(ICON_SIZE, ICON_SIZE)?;
    // Fit the SVG into the pixmap. `from_scale` keeps the aspect
    // ratio if we feed equal x/y factors derived from the SVG's
    // intrinsic size.
    let svg_size = tree.size();
    let scale = ICON_SIZE as f32 / svg_size.width().max(svg_size.height());
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    dioxus::desktop::tao::window::Icon::from_rgba(
        pixmap.take(),
        ICON_SIZE,
        ICON_SIZE,
    )
    .ok()
}

/// Inject the mn-pkg:// custom protocol + ESM bundle + import map
/// into the WebView config. Wires up the TS-in-WebView path — see
/// [DID_PLAN.md](../../DID_PLAN.md). Default-off; enable with
/// `cargo build -p dioxus-wallet --features js-bridge`.
///
/// Works on both desktop and Android because `dioxus-mobile`
/// re-exports `dioxus_desktop`'s `Config` (and Wry's custom-protocol
/// API), and the protocol handler now serves bytes out of an
/// `include_dir!`-embedded tree so the same code path runs without a
/// filesystem on Android.
#[cfg(all(feature = "js-bridge", not(target_os = "android"), not(target_os = "ios")))]
fn with_js_bridge(cfg: dioxus::desktop::Config) -> dioxus::desktop::Config {
    with_js_bridge_inner(cfg)
}

#[cfg(all(feature = "js-bridge", any(target_os = "android", target_os = "ios")))]
fn with_js_bridge(cfg: dioxus::mobile::Config) -> dioxus::mobile::Config {
    with_js_bridge_inner(cfg)
}

// Helper that holds the actual injection logic. `dioxus::desktop`
// and `dioxus::mobile` both re-export the same `dioxus_desktop::Config`
// type, so the body works under either path — we just dispatch
// through cfg'd wrappers to keep the import path concrete on each
// platform.
#[cfg(feature = "js-bridge")]
#[cfg(all(not(target_os = "android"), not(target_os = "ios")))]
type DxCfg = dioxus::desktop::Config;
#[cfg(feature = "js-bridge")]
#[cfg(any(target_os = "android", target_os = "ios"))]
type DxCfg = dioxus::mobile::Config;

#[cfg(feature = "js-bridge")]
fn with_js_bridge_inner(cfg: DxCfg) -> DxCfg {
    let error_reporter = r#"
<script>
(function () {
  const buffered = [];
  function send(payload) {
    if (window.midnightWallet?.bundleError) {
      window.midnightWallet.bundleError(payload).catch(() => {});
    } else {
      buffered.push(payload);
    }
  }
  function fmt(e, kind) {
    let msg = "(unknown)", stack = "";
    if (e?.error) { msg = String(e.error?.message || e.error); stack = String(e.error?.stack || ""); }
    else if (e?.reason) { msg = String(e.reason?.message || e.reason); stack = String(e.reason?.stack || ""); }
    else if (e instanceof Error) { msg = e.message; stack = e.stack || ""; }
    else { msg = String(e); }
    return { kind, message: msg, stack: stack.split("\n").slice(0, 12).join(" | ") };
  }
  window.addEventListener("error", (e) => send(fmt(e, "error")));
  window.addEventListener("unhandledrejection", (e) => send(fmt(e, "unhandledrejection")));
  (async () => {
    for (let i = 0; i < 600; i++) {
      if (window.midnightWallet?.bundleError) {
        for (const p of buffered.splice(0)) {
          try { await window.midnightWallet.bundleError(p); } catch (_) {}
        }
        return;
      }
      await new Promise(r => setTimeout(r, 50));
    }
  })();
})();
</script>"#;
    // Keep the import map in lockstep with `web/build.mjs`'s
    // `external` list and `web/vendor.mjs`'s `PACKAGES` list.
    //
    // On desktop Wry registers the custom scheme directly with the
    // platform WebView (`WKURLSchemeHandler` on macOS, etc.) so
    // `mn-pkg://localhost/…` is fetched via our handler. On Android
    // Chromium WebView doesn't honour non-standard schemes from JS
    // — Wry-Android works around this by rewriting custom-protocol
    // URLs to `http://{name}.{authority}/…` and matching that prefix
    // inside the WebView's shouldInterceptRequest callback. The same
    // rewriting only kicks in for the initial-page URL, not for
    // arbitrary `import()` URLs — so on Android the import map has
    // to spell the http form itself.
    // Import map only carries the WASM-bearing packages that
    // `web/build.mjs` marks `external`. Pure-JS packages
    // (`compact-js`, `midnight-js-*`, `effect`, `platform-js`,
    // `wallet-sdk-address-format`, ...) are bundled into
    // `midnight-did.js` so the WebView's loader never sees their
    // bare sub-path specifiers — esbuild rewrites every
    // `effect/Function`, `@midnight-ntwrk/compact-js/effect/Contract`,
    // ... reference into a local relative import inside the bundle.
    // Desktop (macOS / Linux / Windows) AND iOS both use `WKWebView`-
    // family WebViews that honour Wry-registered custom URL scheme
    // handlers natively, so the `mn-pkg://localhost/…` form works
    // on both. Only Android (Chromium WebView) needs the `http://`
    // rewrite trick.
    #[cfg(not(target_os = "android"))]
    let import_map = r#"
<script type="importmap">
{
  "imports": {
    "@midnight-ntwrk/midnight-did-contract":         "mn-pkg://localhost/midnight-did-contract/dist/index.js",
    "@midnight-ntwrk/midnight-did-jubjub-schnorr":   "mn-pkg://localhost/midnight-did-jubjub-schnorr/dist/index.js",
    "@midnight-ntwrk/compact-runtime":               "mn-pkg://localhost/compact-runtime/dist/index.js",
    "@midnight-ntwrk/onchain-runtime-v3":            "mn-pkg://localhost/onchain-runtime-v3/midnight_onchain_runtime_wasm.js",
    "@midnight-ntwrk/ledger-v8":                     "mn-pkg://localhost/ledger-v8/midnight_ledger_wasm.js",
    "@input-output-hk/passport-vault-contract":       "mn-pkg://localhost/passport-vault-contract/dist/index.js",
    "@input-output-hk/passport-vault-contract/":      "mn-pkg://localhost/passport-vault-contract/dist/",
    "@midnight-ntwrk/midnight-did-credentials-digital-passport":  "mn-pkg://localhost/midnight-did-credentials-digital-passport/dist/index.js",
    "@midnight-ntwrk/midnight-did-credentials-digital-passport/": "mn-pkg://localhost/midnight-did-credentials-digital-passport/dist/",
    "object-inspect":                                "mn-pkg://localhost/object-inspect/index.js",
    "@noble/hashes/":                                "mn-pkg://localhost/@noble/hashes/esm/",
    "@noble/hashes/crypto":                           "mn-pkg://localhost/@noble/hashes/esm/crypto.js"
  }
}
</script>"#;
    #[cfg(target_os = "android")]
    let import_map = r#"
<script type="importmap">
{
  "imports": {
    "@midnight-ntwrk/midnight-did-contract":         "http://mn-pkg.localhost/midnight-did-contract/dist/index.js",
    "@midnight-ntwrk/midnight-did-jubjub-schnorr":   "http://mn-pkg.localhost/midnight-did-jubjub-schnorr/dist/index.js",
    "@midnight-ntwrk/compact-runtime":               "http://mn-pkg.localhost/compact-runtime/dist/index.js",
    "@midnight-ntwrk/onchain-runtime-v3":            "http://mn-pkg.localhost/onchain-runtime-v3/midnight_onchain_runtime_wasm.js",
    "@midnight-ntwrk/ledger-v8":                     "http://mn-pkg.localhost/ledger-v8/midnight_ledger_wasm.js",
    "@input-output-hk/passport-vault-contract":       "http://mn-pkg.localhost/passport-vault-contract/dist/index.js",
    "@input-output-hk/passport-vault-contract/":      "http://mn-pkg.localhost/passport-vault-contract/dist/",
    "@midnight-ntwrk/midnight-did-credentials-digital-passport":  "http://mn-pkg.localhost/midnight-did-credentials-digital-passport/dist/index.js",
    "@midnight-ntwrk/midnight-did-credentials-digital-passport/": "http://mn-pkg.localhost/midnight-did-credentials-digital-passport/dist/",
    "object-inspect":                                "http://mn-pkg.localhost/object-inspect/index.js",
    "@noble/hashes/":                                "http://mn-pkg.localhost/@noble/hashes/esm/",
    "@noble/hashes/crypto":                           "http://mn-pkg.localhost/@noble/hashes/esm/crypto.js"
  }
}
</script>"#;
    let bundle_module = format!(
        "<script type=\"module\">\n{}\n</script>",
        include_str!("../assets/web/midnight-did.js"),
    );
    // `viewport-fit=cover` on iOS Safari / WKWebView is required
    // for `env(safe-area-inset-*)` to resolve to non-zero values.
    // Without it, the iPhone 17 Pro's rounded corners + Dynamic
    // Island side bezels clip right-aligned action columns (e.g.
    // the bench-table "Run" button). On desktop + Android the
    // env() values resolve to 0, so the meta is harmless to
    // include unconditionally.
    let viewport_meta = r#"<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover">"#;
    // Relay between an embedded dApp (the passport-vault dApp rendered
    // in an iframe — see the Dapp screen) and the wallet's JS bridge.
    // The dApp installs a `window.midnight` host shim (see
    // `passport-vault-dapp/lib/midnight/mobile-bench-host.ts`) whose
    // connector methods post `{ __type:"mn-host-req", id, method, args }`
    // to this top document; we forward to
    // `window.midnightWallet.call(method, args)` and post the result
    // back as `{ __type:"mn-host-res", id, result|error }`.
    let dapp_relay = r#"
<script>
(function () {
  window.addEventListener("message", async (ev) => {
    const d = ev.data;
    if (!d || d.__type !== "mn-host-req" || typeof d.id !== "number") return;
    // Hardening: only accept requests from an embedded child frame (the dApp
    // iframe) - never the top document itself or unrelated windows - and target
    // replies back at that frame, using its origin when the scheme exposes one.
    // The dApp iframe is the only frame the wallet hosts, so a child-frame
    // source is a robust check that does not depend on the iframe's origin
    // (which varies with MIDNIGHT_DAPP_URL).
    let fromChildFrame = false;
    try {
      for (let i = 0; i < window.frames.length; i++) {
        if (window.frames[i] === ev.source) { fromChildFrame = true; break; }
      }
    } catch (_) {}
    // Safe fallback: if frame enumeration is unavailable, still reject only
    // self-posts (the relay's own window) - never the cross-window dApp iframe -
    // so this hardening can't break legitimate dApp -> wallet messaging.
    if (!fromChildFrame && ev.source && ev.source !== window) fromChildFrame = true;
    if (!fromChildFrame) return;
    const replyOrigin = (ev.origin && ev.origin !== "null") ? ev.origin : "*";
    const reply = (msg) => {
      try { ev.source.postMessage(msg, replyOrigin); } catch (_) {}
    };
    try {
      for (let i = 0; i < 600 && !(window.midnightWallet && window.midnightWallet.call); i++) {
        await new Promise((r) => setTimeout(r, 50));
      }
      if (!(window.midnightWallet && window.midnightWallet.call)) {
        throw new Error("wallet bridge unavailable");
      }
      const result = await window.midnightWallet.call(d.method, d.args || {});
      try { console.log("[dapp-relay] reply", d.method, JSON.stringify(result)); } catch (_) {}
      reply({ __type: "mn-host-res", id: d.id, result });
    } catch (e) {
      try { console.log("[dapp-relay] error", d.method, (e && e.message) || String(e)); } catch (_) {}
      reply({ __type: "mn-host-res", id: d.id, error: (e && e.message) || String(e) });
    }
  });
})();
</script>"#;
    let bundle_script = format!(
        "{viewport_meta}\n{error_reporter}\n{dapp_relay}\n{import_map}\n{bundle_module}"
    );
    cfg.with_custom_head(bundle_script)
        .with_custom_protocol("mn-pkg".to_string(), protocol::build_handler())
}

#[cfg(any(target_os = "android", target_os = "ios"))]
fn desktop_or_mobile_launch() {
    // Dioxus-mobile re-exports `dioxus_desktop::Config`, so we go
    // through the same `LaunchBuilder` shape as desktop. This lets
    // `with_js_bridge` apply on Android + iOS too — the WebView
    // wired up by Wry honours `with_custom_head` and
    // `with_custom_protocol` on every platform.
    let cfg = dioxus::mobile::Config::new();
    #[cfg(feature = "js-bridge")]
    let cfg = with_js_bridge(cfg);
    dioxus::LaunchBuilder::mobile()
        .with_cfg(cfg)
        .launch(app::App);
}

/// Android entry point — see `dioxus-bench/src/lib.rs` for the
/// `JNI_OnLoad` rationale.
///
/// Before launching the App we point `base_crypto::MidnightDataProvider`
/// at a known cache directory by setting `$MIDNIGHT_PP`. Android's
/// process environment has no `$HOME`, so without this the provider
/// fails with "Could not determine $HOME, $XDG_CACHE_HOME, or
/// $MIDNIGHT_PP". We prefer the app-private cache dir
/// (`/data/data/<applicationId>/cache/midnight-pp/`) because it is
/// writable by the app process — `MidnightDataProvider` can then
/// stream missing `bls_midnight_2pN` files down from
/// `https://srs.midnight.network/` on first prove, instead of
/// silently failing with EACCES against `/data/local/tmp/`. Fall back
/// to the legacy `adb push` path if creation fails (e.g. system image
/// quirks); existing pre-pushed files still get picked up.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub extern "C" fn main() -> i32 {
    // Keep this in lockstep with `applicationId` in
    // `android/app/build.gradle.kts`. If the rename happens, both
    // must change together.
    const APP_ID: &str = "io.iohk.midnight.wallet";
    let private_pp = format!("/data/data/{APP_ID}/cache/midnight-pp");
    let pp: &str = if std::fs::create_dir_all(&private_pp).is_ok() {
        // Leak to get a `&'static str` — runs once at startup, the
        // leaked bytes live for the lifetime of the process anyway.
        Box::leak(private_pp.into_boxed_str())
    } else {
        let legacy = "/data/local/tmp/midnight-pp";
        let _ = std::fs::create_dir_all(legacy);
        legacy
    };
    // SAFETY: This runs once at process start, before any Tokio /
    // thread spawn — no other thread is racing on the environment.
    unsafe {
        std::env::set_var("MIDNIGHT_PP", pp);
    }
    // Enable disk-backed coset spill by default on Android. At k>=19
    // the extended-domain `fixed_cosets` + `permutation.cosets`
    // collections push peak heap above the per-app budget; spilling
    // to a tempfile inside the app's private cache (same partition
    // as MIDNIGHT_PP, ~93 GiB on an S24) lets the OS evict cold
    // mmap pages under pressure. The spill files are auto-deleted
    // on drop. See midnight-proofs::plonk::prover::spill_cosets_to_disk
    // for the architectural rationale.
    let spill_dir = format!("/data/data/{APP_ID}/cache/midnight-cosets");
    if std::fs::create_dir_all(&spill_dir).is_ok() {
        // SAFETY: same single-threaded process-start window as above.
        unsafe {
            std::env::set_var("MIDNIGHT_SPILL_COSETS", "1");
            std::env::set_var("MIDNIGHT_SPILL_DIR", &spill_dir);
        }
    }
    // Raise the per-process address-space ceiling so the proving
    // stack's mmap calls for k >= 19 can land. Android's
    // ActivityManager applies a tighter `RLIMIT_AS` than the
    // device's available memory would suggest; bumping it to
    // RLIM_INFINITY removes the artificial cap so the kernel falls
    // back to honouring the actual memory budget. Best-effort —
    // if the BSP refuses to raise it we silently continue and the
    // runtime hits the smaller cap at the same k it would have
    // anyway.
    unsafe {
        let rlim = libc::rlimit {
            rlim_cur: libc::RLIM_INFINITY,
            rlim_max: libc::RLIM_INFINITY,
        };
        let _ = libc::setrlimit(libc::RLIMIT_AS, &rlim);
        let _ = libc::setrlimit(libc::RLIMIT_DATA, &rlim);
    }
    // We can't initialise rustls-platform-verifier here yet —
    // `ndk_context::android_context()` panics with "android context
    // was not initialized" because dioxus-mobile only fills that in
    // *after* it calls our `main`. `extern "C" fn` cannot unwind, so
    // an early panic aborts the process. Instead we kick a task on
    // the Dioxus executor (started inside `run()`) which retries
    // until the context shows up. See `try_init_android_tls`.
    run();
    0
}

/// iOS entry point — called from the Swift `@main` `App.init()`.
/// Unlike Android, iOS doesn't need a JNI bridge: the static lib
/// is linked directly into the app binary and the Swift side just
/// calls a C-ABI symbol to hand control to Rust. `rustls-platform-
/// verifier` auto-detects iOS at first TLS use (no `init_with_env`
/// dance), so we just set `$MIDNIGHT_PP` to a writable sandbox
/// path and dive into `run()`.
#[cfg(target_os = "ios")]
#[unsafe(no_mangle)]
pub extern "C" fn start_app() {
    // iOS apps have a private writable sandbox at `$HOME` (which
    // points to the app's container root, with `Documents/`,
    // `Library/`, `tmp/` underneath). Put the SRS cache under
    // `Library/Caches/midnight-pp/` so the OS can evict it under
    // pressure without losing user data.
    if let Some(home) = std::env::var_os("HOME") {
        let home_path = std::path::PathBuf::from(home);
        let pp = home_path
            .join("Library")
            .join("Caches")
            .join("midnight-pp");
        let _ = std::fs::create_dir_all(&pp);
        // SAFETY: This runs once at process start, before any
        // Tokio / thread spawn — no other thread is racing on the
        // environment.
        unsafe {
            std::env::set_var("MIDNIGHT_PP", &pp);
        }
        // Enable disk-backed coset spill by default on iOS too. At
        // k>=19 the extended-domain `fixed_cosets` + `permutation
        // .cosets` collections push peak heap above the per-app
        // jetsam budget (~3 GiB on pre-15-Pro hardware, ~5 GiB on
        // 15 Pro+); spilling to a tempfile inside the app's
        // sandbox `Library/Caches/` lets the OS evict cold mmap
        // pages under pressure. The spill files are auto-deleted
        // on drop. See midnight-proofs::plonk::prover::spill_cosets_to_disk
        // for the architectural rationale. Mirrors the Android
        // setup in `main()` above.
        let spill_dir = home_path
            .join("Library")
            .join("Caches")
            .join("midnight-cosets");
        if std::fs::create_dir_all(&spill_dir).is_ok() {
            // SAFETY: same single-threaded process-start window.
            unsafe {
                std::env::set_var("MIDNIGHT_SPILL_COSETS", "1");
                std::env::set_var("MIDNIGHT_SPILL_DIR", &spill_dir);
            }
        }
    }
    run();
}

/// Try to initialise the Android-side certificate verifier. Run
/// once Dioxus has booted and `ndk-context` has been seeded.
/// Calling it before that point would panic at
/// `ndk_context::android_context()`, hence the [`catch_unwind`]
/// guard. Returns `Ok(true)` once init succeeds; the caller is
/// expected to keep polling until it does.
///
/// Required before any reqwest / tonic / tungstenite call —
/// otherwise the panic in `rustls-platform-verifier/src/android.rs`
/// fires on first TLS handshake.
#[cfg(target_os = "android")]
pub(crate) fn try_init_android_tls() -> Result<bool, String> {
    use jni::JavaVM;
    use jni::objects::JObject;
    use std::panic::{AssertUnwindSafe, catch_unwind};

    // `android_context()` panics if not initialised yet — wrap so
    // the caller can retry instead of aborting the process.
    let ctx = match catch_unwind(AssertUnwindSafe(ndk_context::android_context)) {
        Ok(c) => c,
        Err(_) => return Ok(false),
    };
    // SAFETY: `ndk_context::android_context()` returns raw `JavaVM*`
    // and `jobject` pointers owned by the host Android runtime —
    // valid for the lifetime of the process. `JavaVM::from_raw`
    // borrows the pointer without taking ownership.
    let vm = unsafe { JavaVM::from_raw(ctx.vm().cast()) }
        .map_err(|e| format!("attach JavaVM: {e}"))?;
    let mut env = vm
        .attach_current_thread()
        .map_err(|e| format!("attach thread: {e}"))?;
    let activity_0_6 = unsafe { JObject::from_raw(ctx.context().cast()) };
    rustls_platform_verifier::android::init_with_env(&mut env, activity_0_6)
        .map_err(|e| format!("init_with_env (0.6): {e}"))?;
    // Two copies of `rustls-platform-verifier` end up in the dep
    // tree: `reqwest` brings in `0.6`, `subxt` brings in `0.5`. Each
    // has its own `OnceLock` so initialising `0.6` doesn't satisfy
    // `0.5`'s call site. Without this second init, the dust syncer's
    // WS subscribe (subxt path) panics with "Expect
    // rustls-platform-verifier to be initialized" on first TLS use.
    let activity_0_5 = unsafe { JObject::from_raw(ctx.context().cast()) };
    rustls_platform_verifier_v05::android::init_with_env(&mut env, activity_0_5)
        .map_err(|e| format!("init_with_env (0.5): {e}"))?;
    Ok(true)
}
