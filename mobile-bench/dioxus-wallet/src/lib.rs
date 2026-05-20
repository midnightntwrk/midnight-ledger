#![deny(warnings)]

mod app;
mod bridge;
mod logs;
mod platform;
#[cfg(all(feature = "js-bridge", not(target_os = "android")))]
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
        EnvFilter::new("warn,wallet_core=debug,dioxuswalletmain=info,bundle=info")
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

#[cfg(not(target_os = "android"))]
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
#[cfg(not(target_os = "android"))]
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
/// into the WebView config. Wires up the legacy TS-in-WebView path
/// — see [DID_PLAN.md](../../DID_PLAN.md). Default-off; enable with
/// `cargo build -p dioxus-wallet --features js-bridge`.
#[cfg(feature = "js-bridge")]
fn with_js_bridge(cfg: dioxus::desktop::Config) -> dioxus::desktop::Config {
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
    let import_map = r#"
<script type="importmap">
{
  "imports": {
    "@midnight-ntwrk/midnight-did-contract":   "mn-pkg://localhost/midnight-did-contract/dist/index.js",
    "@midnight-ntwrk/compact-runtime":         "mn-pkg://localhost/compact-runtime/dist/index.js",
    "@midnight-ntwrk/compact-js":              "mn-pkg://localhost/compact-js/dist/index.js",
    "@midnight-ntwrk/onchain-runtime-v3":      "mn-pkg://localhost/onchain-runtime-v3/midnight_onchain_runtime_wasm.js",
    "@midnight-ntwrk/ledger-v8":               "mn-pkg://localhost/ledger-v8/midnight_ledger_wasm.js",
    "object-inspect":                          "mn-pkg://localhost/object-inspect/index.js"
  }
}
</script>"#;
    let bundle_module = format!(
        "<script type=\"module\">\n{}\n</script>",
        include_str!("../assets/web/midnight-did.js"),
    );
    let bundle_script = format!("{error_reporter}\n{import_map}\n{bundle_module}");
    let assets_root =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets");
    cfg.with_custom_head(bundle_script).with_custom_protocol(
        "mn-pkg".to_string(),
        protocol::build_handler(assets_root),
    )
}

#[cfg(target_os = "android")]
fn desktop_or_mobile_launch() {
    dioxus::launch(app::App);
}

/// Android entry point — see `dioxus-bench/src/lib.rs` for the
/// `JNI_OnLoad` rationale.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub extern "C" fn main() -> i32 {
    run();
    0
}
