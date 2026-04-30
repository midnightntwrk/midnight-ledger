#![deny(warnings)]

mod app;
mod bridge;
mod platform;
#[cfg(all(feature = "js-bridge", not(target_os = "android")))]
mod protocol;

pub fn run() {
    let _ = tracing_subscriber::fmt::try_init();
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
    let window = WindowBuilder::new()
        .with_title("Midnight Wallet")
        .with_inner_size(LogicalSize::new(390.0, 844.0))
        .with_resizable(true);
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
