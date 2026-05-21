#![deny(warnings)]

mod app;
mod bridge;
#[cfg(feature = "js-bridge")]
pub(crate) mod eval_bridge;
mod format;
mod logs;
mod platform;
#[cfg(feature = "js-bridge")]
mod protocol;

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
            "warn,wallet_core=debug,dioxuswalletmain=info,bundle=info,eval-bridge=info,mn-pkg=info",
        )
    });
    let _ = tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_filter(stderr_filter))
        .with(logs::WalletLogLayer::new(capture))
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
    "@midnight-ntwrk/midnight-did-contract":   "mn-pkg://localhost/midnight-did-contract/dist/index.js",
    "@midnight-ntwrk/compact-runtime":         "mn-pkg://localhost/compact-runtime/dist/index.js",
    "@midnight-ntwrk/onchain-runtime-v3":      "mn-pkg://localhost/onchain-runtime-v3/midnight_onchain_runtime_wasm.js",
    "@midnight-ntwrk/ledger-v8":               "mn-pkg://localhost/ledger-v8/midnight_ledger_wasm.js",
    "object-inspect":                          "mn-pkg://localhost/object-inspect/index.js"
  }
}
</script>"#;
    #[cfg(target_os = "android")]
    let import_map = r#"
<script type="importmap">
{
  "imports": {
    "@midnight-ntwrk/midnight-did-contract":   "http://mn-pkg.localhost/midnight-did-contract/dist/index.js",
    "@midnight-ntwrk/compact-runtime":         "http://mn-pkg.localhost/compact-runtime/dist/index.js",
    "@midnight-ntwrk/onchain-runtime-v3":      "http://mn-pkg.localhost/onchain-runtime-v3/midnight_onchain_runtime_wasm.js",
    "@midnight-ntwrk/ledger-v8":               "http://mn-pkg.localhost/ledger-v8/midnight_ledger_wasm.js",
    "object-inspect":                          "http://mn-pkg.localhost/object-inspect/index.js"
  }
}
</script>"#;
    let bundle_module = format!(
        "<script type=\"module\">\n{}\n</script>",
        include_str!("../assets/web/midnight-did.js"),
    );
    let bundle_script = format!("{error_reporter}\n{import_map}\n{bundle_module}");
    cfg.with_custom_head(bundle_script).with_custom_protocol(
        "mn-pkg".to_string(),
        protocol::build_handler(),
    )
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
        let pp = std::path::PathBuf::from(home)
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
