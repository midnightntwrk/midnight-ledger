//! Identity Centre Phase 1 — pragmatic linear page that wires the
//! four shipped wallet-core flows into the UI:
//!
//! 1. Bootstrap DID with VC keys ([`bootstrap_did_with_keys`]).
//! 2. OID4VP / SIOPv2 authenticate against a pasted `openid4vp://`
//!    URL ([`oid4vp_run_authentication`]).
//! 3. OID4VCI issue against a pasted `openid-credential-offer://`
//!    URL ([`oid4vci_run_issuance`]).
//! 4. List + self-verify cached VCs ([`self_verify_and_cache`]).
//!
//! The full Identity Centre design (carousel + FAB + nested sub-tabs)
//! from plan Tasks 30-36 is deferred to Phase 2; this module is the
//! minimum viable surface that lets an operator drive the QR-1 + KYC
//! + QR-2 contract against the running `IssuerDIDIT-mock` service
//! end-to-end. Paste-URL only — no camera scanner yet.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use dioxus::prelude::*;
use tracing::Instrument;

use wallet_core::{
    HttpClient, MeteredHttpClient, Metrics, Network, RedbVcStore, ReqwestHttpClient,
    SelfVerifyResult, StoredVc, SystemClock, VcOpening, bootstrap_did_with_keys,
    oid4vci_run_issuance, oid4vp_run_authentication, self_verify_and_cache, time_op,
    time_op_simple,
};

/// Monotonic per-session counter for Identity Centre action IDs.
/// Rendered as hex so a 4-char string keeps the Logs-tab prefix
/// short while still being unique across the ~thousand-click
/// session ceiling we'd ever realistically hit. Wraps at u64::MAX
/// after roughly 18 quintillion clicks.
static NEXT_ACTION_ID: AtomicU64 = AtomicU64::new(0);

/// Mint a fresh hex-string action ID. One per top-level Identity
/// Centre click. Used as a `tracing::Span` field so every nested
/// `time_op` op record, `MeteredHttpClient` http record, and
/// `tracing::info!` event captured by `WalletLogLayer` gets a
/// `[action=…]` prefix in the Logs tab — letting an operator
/// filter all events that came from one click.
fn next_action_id() -> String {
    format!("{:x}", NEXT_ACTION_ID.fetch_add(1, Ordering::Relaxed))
}

/// Wrap `app_wallet_for(network)` with `with_metering` so the
/// chain-op pipeline (indexer queries + prover calls) records
/// per-call timings + RSS/CPU deltas through the supplied
/// telemetry sink. Falls back silently to the un-metered wallet
/// if indexer-default construction fails (offline env / bad URL)
/// — telemetry is a "nice to have", never a hard error path.
fn metered_app_wallet_for(
    network: Network,
    metrics: std::sync::Arc<dyn Metrics>,
    probe: std::sync::Arc<dyn wallet_core::ResourceProbe>,
) -> wallet_core::Wallet {
    match app_wallet_for(network).with_metering(metrics, probe) {
        Ok(w) => w,
        Err(e) => {
            tracing::warn!(
                target: "dioxus_wallet::identity_centre",
                error = %e,
                "with_metering failed — running un-metered chain-ops",
            );
            // `with_metering` consumed the wallet on failure; build a fresh one.
            app_wallet_for(network)
        }
    }
}
use wallet_core::secret_storage::{SecretStorage, redb_secret_store::RedbSecretStore};

use crate::app::{app_wallet_for, truncate_did, wallet_store_path};
use crate::bridge::BridgeState;
use crate::eval_bridge;

/// Fixed 32-byte demo seed. Kept constant so an operator can re-run
/// `bootstrap_did_with_keys` across resets and get the same
/// HKDF-derived Ed25519 + Jubjub material — useful for end-to-end
/// debugging against the issuer-mock. NOT for production use; a real
/// wallet would derive this from the HD root.
const DEMO_IC_SEED: [u8; 32] = [42u8; 32];

/// VC store filename — sits next to `wallet.redb` so the dir
/// computed by `wallet_store_path` carries both stores.
const VC_STORE_FILENAME: &str = "vcs.redb";

/// Resolve the path the demo `VcStore` lives at. The same dir as
/// the main wallet redb — under `Documents/midnight-dx-wallet/` on
/// iOS, `~/.midnight/wallet-prototype/` on desktop, and
/// `/data/data/<pkg>/files/midnight-dx-wallet/` on Android.
fn vc_store_path() -> std::path::PathBuf {
    let mut p = wallet_store_path();
    p.set_file_name(VC_STORE_FILENAME);
    p
}

/// Top-level Identity Centre panel. Renders the four sections
/// stacked vertically.
///
/// `on_did_minted` fires after a successful `bootstrap_did_with_keys`
/// so the parent (App) can insert the new DID into its
/// `did_inventory` signal — the Dids tab renders from that
/// signal, so without this callback the freshly-minted DID
/// stays invisible until the next rehydration. Same channel the
/// Dids-tab Create-DID wizard uses for "DID was minted, please
/// update the inventory" notification.
#[component]
pub fn IdentityCentrePanel(
    network: Network,
    bridge_state: BridgeState,
    on_did_minted: EventHandler<(String, Network)>,
) -> Element {
    // The "current Identity Centre DID" — populated either from a
    // fresh bootstrap or by scanning the secret store for a key
    // whose `kid` carries the `#key-auth` fragment. Held at panel
    // scope so all four sections share it.
    let ic_did = use_signal::<Option<String>>(|| None);

    rsx! {
        div { class: "card",
            div { class: "card-header", "Identity Centre" }
            div { class: "detail-empty",
                "Phase 1 — drive the four shipped wallet-core flows. "
                "Paste URLs from the issuer-mock to authenticate, "
                "fetch a credential, and self-verify."
            }
        }

        BootstrapSection {
            network,
            bridge_state: bridge_state.clone(),
            ic_did,
            on_did_minted,
        }

        Oid4vpSection {
            network,
            bridge_state: bridge_state.clone(),
            ic_did,
        }

        Oid4vciSection {
            network,
            bridge_state: bridge_state.clone(),
            ic_did,
        }

        VcInventorySection {
            network,
            bridge_state,
        }
    }
}

// ─── Section 1 ─────────────────────────────────────────────────────

/// Bootstrap the Identity Centre DID. Idempotent across re-runs:
/// each click re-runs `bootstrap_did_with_keys` (which mints a fresh
/// on-chain DID), so multiple clicks produce multiple DIDs. The UI
/// just shows the most recent one + still leaves the older keys in
/// the secret store.
#[component]
fn BootstrapSection(
    network: Network,
    bridge_state: BridgeState,
    ic_did: Signal<Option<String>>,
    on_did_minted: EventHandler<(String, Network)>,
) -> Element {
    let mut busy = use_signal(|| false);
    let mut err_msg = use_signal::<Option<String>>(|| None);
    let mut ok_msg = use_signal::<Option<String>>(|| None);
    // Live activity feed shown under the Bootstrap button while
    // `busy == true`. Populated by polling
    // `BridgeState::log_capture().snapshot()` every 500 ms and
    // keeping the last 3 events targeted at
    // `wallet_core::metrics` (HTTP / op / counter records from
    // the telemetry layer). Gives the operator visible feedback
    // during the ~2-3 minute bootstrap instead of a frozen
    // "Bootstrapping…" button.
    let activity = use_signal::<Vec<String>>(Vec::new);

    // On first mount, probe the secret store for an existing
    // `#key-auth` kid → infer the IC DID from its prefix. Saves the
    // operator from re-bootstrapping when the panel mounts after a
    // restart.
    {
        let bridge_state = bridge_state.clone();
        let mut ic_did = ic_did;
        use_effect(move || {
            if ic_did.read().is_some() {
                return;
            }
            let Some(store) = bridge_state.store().cloned() else {
                return;
            };
            let Some(wallet_id) = bridge_state.active_wallet_id() else {
                return;
            };
            let s = RedbSecretStore::new(store, wallet_id);
            let keys: Result<Vec<_>, _> =
                futures::executor::block_on(s.list_keys(None));
            if let Ok(ks) = keys {
                for k in ks {
                    if k.id.contains("#key-auth") {
                        if let Some(hash_idx) = k.id.find('#') {
                            ic_did.set(Some(k.id[..hash_idx].to_string()));
                            break;
                        }
                    }
                }
            }
        });
    }

    let bootstrap = {
        let bridge_state = bridge_state.clone();
        let mut ic_did = ic_did;
        move |_| {
            if *busy.read() {
                return;
            }
            busy.set(true);
            err_msg.set(None);
            ok_msg.set(None);
            let bridge_state = bridge_state.clone();
            let action_id = next_action_id();
            let span = tracing::info_span!("ic.bootstrap", action_id = %action_id);
            spawn(async move {
                let Some(store) = bridge_state.store().cloned() else {
                    err_msg.set(Some("wallet store not opened yet".into()));
                    busy.set(false);
                    return;
                };
                let Some(wallet_id) = bridge_state.active_wallet_id() else {
                    err_msg.set(Some("no active wallet".into()));
                    busy.set(false);
                    return;
                };
                let metrics = bridge_state.metrics_dyn();
                let in_mem_metrics = bridge_state.metrics();
                let probe = bridge_state.resource_probe();
                // Build a chain-op-metered Wallet so the
                // indexer queries (chain_tip, contract_state)
                // and prover calls (halo2 prove) under the
                // hood also land in the aggregator as
                // `indexer.*` / `prover.prove` ops. Falls
                // back to un-metered if the default indexer
                // fails to build (offline env).
                let wallet =
                    metered_app_wallet_for(network, metrics.clone(), probe.clone());
                let mut secret_store = RedbSecretStore::new(store, wallet_id);
                // Bracket the heaviest Identity-Centre op
                // (HKDF + key import + on-chain DID write)
                // so the Diagnostics tab can quantify how
                // much wall + RSS / CPU it costs. The
                // chain-op decorators provide the per-call
                // breakdown for what runs underneath this.
                let result = time_op(
                    &*metrics,
                    &*probe,
                    "bootstrap_did",
                    bootstrap_did_with_keys(&wallet, &mut secret_store, &DEMO_IC_SEED),
                )
                .await;
                match result {
                    Ok(b) => {
                        in_mem_metrics.incr("dids.bootstrapped", 1);
                        let did_str = b.did.to_did_string();
                        // Persist the per-DID controller secret
                        // so future Update / Deactivate circuits
                        // can pull it from `BridgeState`. Same
                        // path the Create-DID wizard uses.
                        // Without this the new DID's Update /
                        // Deactivate buttons stay disabled.
                        bridge_state.remember_controller_secret(
                            network,
                            did_str.clone(),
                            b.controller_sk,
                        );
                        // Notify the parent App so it can insert
                        // a Pending entry into the live
                        // `did_inventory` signal AND persist to
                        // redb (the latter via
                        // `persist_inventory_entry`). The Dids
                        // tab renders from the signal, not from
                        // a snapshot of redb, so a pure redb
                        // write here wouldn't surface until the
                        // next rehydration. The callback bridges
                        // that gap.
                        on_did_minted.call((did_str.clone(), network));
                        ic_did.set(Some(did_str.clone()));
                        ok_msg.set(Some(format!(
                            "Bootstrapped {did_str}. Switch to the Dids \
                             tab to see it; click Resolve there to fill \
                             in counter / VM counts.",
                        )));
                    }
                    Err(e) => {
                        in_mem_metrics.incr("dids.bootstrap_failed", 1);
                        err_msg.set(Some(format!("bootstrap failed: {e}")));
                    }
                }
                busy.set(false);
            }.instrument(span));
        }
    };

    // Activity-feed polling. While `busy == true`, snapshot the
    // process-global `LogCapture` every ~500 ms and keep the
    // last 3 events whose target starts with
    // `wallet_core::metrics` — these are the per-op events
    // emitted by `time_op` (issuance, indexer.chain_tip,
    // prover.prove, indexer.contract_state, …). The captured
    // ring buffer is bounded (1k events), so this poll is
    // cheap. When `busy` flips to false the effect's read of
    // `busy` triggers a re-run; the snapshot stays as the
    // "final" activity slice until the next bootstrap starts.
    {
        let bridge_state = bridge_state.clone();
        let mut activity = activity;
        use_effect(move || {
            let running = *busy.read();
            if !running {
                return;
            }
            let Some(cap) = bridge_state.log_capture().cloned() else {
                return;
            };
            spawn(async move {
                while *busy.read() {
                    let snap = cap.snapshot();
                    let recent: Vec<String> = snap
                        .into_iter()
                        .filter(|e| e.target.starts_with("wallet_core::metrics"))
                        .take(3)
                        .map(|e| e.message)
                        .collect();
                    if !recent.is_empty() {
                        activity.set(recent);
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                }
            });
        });
    }

    let current = ic_did.read().clone();
    let button_label = if current.is_some() {
        "Re-bootstrap (creates a new DID)"
    } else {
        "Bootstrap DID with VC keys (Ed25519 + Jubjub)"
    };
    let activity_lines = activity.read().clone();
    let is_busy = *busy.read();

    rsx! {
        div { class: "card",
            div { class: "card-header", "Identity Centre DID" }
            if let Some(did) = current.as_ref() {
                div { class: "row label", "Current" }
                div { class: "seed-blob", "{did}" }
                div { class: "detail-empty", "{truncate_did(did)}" }
            } else {
                div { class: "detail-empty",
                    "No Identity Centre DID yet. Click below to mint one "
                    "(uses fixed demo seed [42u8; 32])."
                }
            }
            div { class: "row",
                button {
                    class: "cta",
                    disabled: is_busy,
                    onclick: bootstrap,
                    {if is_busy { "Working…" } else { button_label }}
                }
            }

            // Live activity strip — shown only while a bootstrap
            // is in flight. Three rows max, newest at top, each
            // a recent op-metric message. Gives the operator a
            // sense of what's happening during the ~2-3 minute
            // wait instead of staring at a frozen Working…
            // button.
            if is_busy && !activity_lines.is_empty() {
                div { class: "card-section-header", "Current activity" }
                ul {
                    style: "list-style: none; margin: 0; padding: 0; \
                            font-family: monospace; font-size: 11px; \
                            line-height: 1.4; color: var(--text-muted);",
                    for line in activity_lines.iter() {
                        li {
                            style: "padding: 2px 0; \
                                    white-space: nowrap; overflow: hidden; \
                                    text-overflow: ellipsis;",
                            "{line}"
                        }
                    }
                }
            }

            if let Some(msg) = ok_msg.read().as_ref() {
                div { class: "wizard-outcome ok",
                    div { class: "row label", "Bootstrap" }
                    div { class: "seed-blob", "{msg}" }
                }
            }
            if let Some(msg) = err_msg.read().as_ref() {
                div { class: "wizard-outcome err",
                    div { class: "row label", "Failed" }
                    div { class: "seed-blob", "{msg}" }
                }
            }
        }
    }
}

// ─── Section 2 ─────────────────────────────────────────────────────

#[component]
fn Oid4vpSection(
    network: Network,
    bridge_state: BridgeState,
    ic_did: Signal<Option<String>>,
) -> Element {
    let mut url_input = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let mut err_msg = use_signal::<Option<String>>(|| None);
    let mut ok_msg = use_signal::<Option<String>>(|| None);

    let authenticate = {
        let bridge_state = bridge_state.clone();
        let ic_did = ic_did;
        move |_| {
            if *busy.read() {
                return;
            }
            let url = url_input.read().trim().to_string();
            if url.is_empty() {
                err_msg.set(Some("paste an openid4vp:// URL first".into()));
                return;
            }
            let Some(did_str) = ic_did.read().clone() else {
                err_msg.set(Some("bootstrap an Identity Centre DID first".into()));
                return;
            };
            let did = match wallet_core::DidId::parse(&did_str) {
                Ok(d) => d,
                Err(e) => {
                    err_msg.set(Some(format!("did parse: {e}")));
                    return;
                }
            };
            let Some(store) = bridge_state.store().cloned() else {
                err_msg.set(Some("wallet store not opened yet".into()));
                return;
            };
            let Some(wallet_id) = bridge_state.active_wallet_id() else {
                err_msg.set(Some("no active wallet".into()));
                return;
            };
            busy.set(true);
            err_msg.set(None);
            ok_msg.set(None);
            let metrics = bridge_state.metrics_dyn();
            let in_mem_metrics = bridge_state.metrics();
            let probe = bridge_state.resource_probe();
            let action_id = next_action_id();
            let span =
                tracing::info_span!("ic.oid4vp_authenticate", action_id = %action_id);
            spawn(async move {
                let wallet =
                    metered_app_wallet_for(network, metrics.clone(), probe.clone());
                let secret_store = RedbSecretStore::new(store, wallet_id);
                let raw_http: Arc<dyn HttpClient> = Arc::new(ReqwestHttpClient::default());
                let http: Arc<dyn HttpClient> =
                    Arc::new(MeteredHttpClient::new(raw_http, metrics.clone()));
                let clock = SystemClock;
                let result = time_op(
                    &*metrics,
                    &*probe,
                    "oid4vp_authenticate",
                    oid4vp_run_authentication(
                        &*http,
                        &clock,
                        &url,
                        &wallet,
                        &secret_store,
                        &did,
                    ),
                )
                .await;
                match result {
                    Ok(r) => {
                        in_mem_metrics.incr("oid4vp.ok", 1);
                        ok_msg.set(Some(format!(
                            "session_id={} status={}",
                            r.session_id, r.status
                        )));
                    }
                    Err(e) => {
                        in_mem_metrics.incr("oid4vp.failed", 1);
                        err_msg.set(Some(format!("authenticate failed: {e}")));
                    }
                }
                busy.set(false);
            }.instrument(span));
        }
    };

    let scan = {
        let mut url_input = url_input;
        let mut err_msg = err_msg;
        move |_| {
            err_msg.set(None);
            spawn(async move {
                let Some(bridge) = eval_bridge::global_bridge() else {
                    err_msg.set(Some(
                        "JS bridge not installed yet (js-bridge feature off?)"
                            .into(),
                    ));
                    return;
                };
                match eval_bridge::scan_qr(&*bridge).await {
                    Ok(url) => url_input.set(url),
                    Err(wallet_core::js_bridge::JsBridgeError::Transport(msg))
                        if msg == "cancelled" =>
                    {
                        // User pressed Cancel — silent no-op.
                    }
                    Err(e) => err_msg.set(Some(format!("scan failed: {e}"))),
                }
            });
        }
    };

    // Explicit paste button — iOS WKWebView's long-press / Cmd-V
    // paste into `<textarea>` is unreliable; the bundle's
    // `pasteText()` calls `navigator.clipboard.readText()` from a
    // button click (a valid user gesture) and works on every
    // supported target.
    let paste = {
        let mut url_input = url_input;
        let mut err_msg = err_msg;
        move |_| {
            err_msg.set(None);
            spawn(async move {
                let Some(bridge) = eval_bridge::global_bridge() else {
                    err_msg.set(Some(
                        "JS bridge not installed yet (js-bridge feature off?)"
                            .into(),
                    ));
                    return;
                };
                match eval_bridge::paste_text(&*bridge).await {
                    Ok(text) => url_input.set(text),
                    Err(e) => err_msg.set(Some(format!("paste failed: {e}"))),
                }
            });
        }
    };

    rsx! {
        div { class: "card",
            div { class: "card-header", "Authenticate with QR (OID4VP)" }
            textarea {
                value: "{url_input.read()}",
                oninput: move |e| url_input.set(e.value()),
                placeholder: "paste openid4vp:// URL here",
                rows: "3",
                style: "width: 100%; padding: 6px 8px; background: var(--surface-2); color: var(--text); border: 1px solid var(--border); border-radius: 6px; font-family: ui-monospace, monospace; font-size: 11px;"
            }
            div { class: "row",
                button {
                    class: "cta",
                    disabled: *busy.read(),
                    onclick: authenticate,
                    {if *busy.read() { "Authenticating…" } else { "Authenticate" }}
                }
                button {
                    disabled: *busy.read(),
                    onclick: paste,
                    "📋 Paste"
                }
                button {
                    disabled: *busy.read(),
                    onclick: scan,
                    "📷 Scan QR"
                }
            }
            if let Some(msg) = ok_msg.read().as_ref() {
                div { class: "wizard-outcome ok",
                    div { class: "row label", "Response" }
                    div { class: "seed-blob", "{msg}" }
                }
            }
            if let Some(msg) = err_msg.read().as_ref() {
                div { class: "wizard-outcome err",
                    div { class: "row label", "Failed" }
                    div { class: "seed-blob", "{msg}" }
                }
            }
        }
    }
}

// ─── Section 3 ─────────────────────────────────────────────────────

#[component]
fn Oid4vciSection(
    network: Network,
    bridge_state: BridgeState,
    ic_did: Signal<Option<String>>,
) -> Element {
    let mut url_input = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let mut err_msg = use_signal::<Option<String>>(|| None);
    let mut ok_msg = use_signal::<Option<String>>(|| None);

    let request_vc = {
        let bridge_state = bridge_state.clone();
        let ic_did = ic_did;
        move |_| {
            if *busy.read() {
                return;
            }
            let url = url_input.read().trim().to_string();
            if url.is_empty() {
                err_msg.set(Some("paste an openid-credential-offer:// URL first".into()));
                return;
            }
            let Some(did_str) = ic_did.read().clone() else {
                err_msg.set(Some("bootstrap an Identity Centre DID first".into()));
                return;
            };
            let did = match wallet_core::DidId::parse(&did_str) {
                Ok(d) => d,
                Err(e) => {
                    err_msg.set(Some(format!("did parse: {e}")));
                    return;
                }
            };
            let Some(store) = bridge_state.store().cloned() else {
                err_msg.set(Some("wallet store not opened yet".into()));
                return;
            };
            let Some(wallet_id) = bridge_state.active_wallet_id() else {
                err_msg.set(Some("no active wallet".into()));
                return;
            };
            busy.set(true);
            err_msg.set(None);
            ok_msg.set(None);
            let metrics = bridge_state.metrics_dyn();
            let in_mem_metrics = bridge_state.metrics();
            let probe = bridge_state.resource_probe();
            let action_id = next_action_id();
            let span = tracing::info_span!("ic.issuance", action_id = %action_id);
            spawn(async move {
                let wallet =
                    metered_app_wallet_for(network, metrics.clone(), probe.clone());
                let secret_store = RedbSecretStore::new(store, wallet_id);
                let vc_store = match RedbVcStore::open(vc_store_path()) {
                    Ok(v) => v,
                    Err(e) => {
                        err_msg.set(Some(format!("open vc store: {e}")));
                        busy.set(false);
                        return;
                    }
                };
                // Wrap the real reqwest client with the
                // metrics decorator so every /token and
                // /credential HTTP call lands in the
                // aggregator (host + status + duration_ms)
                // and the Logs tab (via TracingMetrics).
                let raw_http: Arc<dyn HttpClient> = Arc::new(ReqwestHttpClient::default());
                let http: Arc<dyn HttpClient> =
                    Arc::new(MeteredHttpClient::new(raw_http, metrics.clone()));
                let clock = SystemClock;
                // Bracket the whole OID4VCI flow with
                // `time_op` so the aggregator records one
                // top-level "issuance" sample carrying the
                // total wall-time + RSS / CPU deltas. The
                // HTTP decorator records the two sub-calls
                // separately — the difference highlights
                // whether the latency lives in the network
                // or in local crypto.
                let result = time_op(
                    &*metrics,
                    &*probe,
                    "issuance",
                    oid4vci_run_issuance(
                        &*http,
                        &clock,
                        &url,
                        &wallet,
                        &secret_store,
                        &did,
                        &vc_store,
                    ),
                )
                .await;
                match result {
                    Ok(vc_uri) => {
                        in_mem_metrics.incr("vcs.issued", 1);
                        ok_msg.set(Some(format!("issued {vc_uri}")));
                    }
                    Err(e) => {
                        in_mem_metrics.incr("vcs.issuance_failed", 1);
                        err_msg.set(Some(format!("issue failed: {e}")));
                    }
                }
                busy.set(false);
            }.instrument(span));
        }
    };

    let scan = {
        let mut url_input = url_input;
        let mut err_msg = err_msg;
        move |_| {
            err_msg.set(None);
            spawn(async move {
                let Some(bridge) = eval_bridge::global_bridge() else {
                    err_msg.set(Some(
                        "JS bridge not installed yet (js-bridge feature off?)"
                            .into(),
                    ));
                    return;
                };
                match eval_bridge::scan_qr(&*bridge).await {
                    Ok(url) => url_input.set(url),
                    Err(wallet_core::js_bridge::JsBridgeError::Transport(msg))
                        if msg == "cancelled" => {}
                    Err(e) => err_msg.set(Some(format!("scan failed: {e}"))),
                }
            });
        }
    };

    // See identity_centre OID4VP card — same rationale, iOS
    // needs an explicit clipboard-read entry point.
    let paste = {
        let mut url_input = url_input;
        let mut err_msg = err_msg;
        move |_| {
            err_msg.set(None);
            spawn(async move {
                let Some(bridge) = eval_bridge::global_bridge() else {
                    err_msg.set(Some(
                        "JS bridge not installed yet (js-bridge feature off?)"
                            .into(),
                    ));
                    return;
                };
                match eval_bridge::paste_text(&*bridge).await {
                    Ok(text) => url_input.set(text),
                    Err(e) => err_msg.set(Some(format!("paste failed: {e}"))),
                }
            });
        }
    };

    rsx! {
        div { class: "card",
            div { class: "card-header", "Request VC (OID4VCI)" }
            textarea {
                value: "{url_input.read()}",
                oninput: move |e| url_input.set(e.value()),
                placeholder: "paste openid-credential-offer:// URL here",
                rows: "3",
                style: "width: 100%; padding: 6px 8px; background: var(--surface-2); color: var(--text); border: 1px solid var(--border); border-radius: 6px; font-family: ui-monospace, monospace; font-size: 11px;"
            }
            div { class: "row",
                button {
                    class: "cta",
                    disabled: *busy.read(),
                    onclick: request_vc,
                    {if *busy.read() { "Requesting…" } else { "Get credential" }}
                }
                button {
                    disabled: *busy.read(),
                    onclick: paste,
                    "📋 Paste"
                }
                button {
                    disabled: *busy.read(),
                    onclick: scan,
                    "📷 Scan QR"
                }
            }
            if let Some(msg) = ok_msg.read().as_ref() {
                div { class: "wizard-outcome ok",
                    div { class: "row label", "Issued" }
                    div { class: "seed-blob", "{msg}" }
                }
            }
            if let Some(msg) = err_msg.read().as_ref() {
                div { class: "wizard-outcome err",
                    div { class: "row label", "Failed" }
                    div { class: "seed-blob", "{msg}" }
                }
            }
        }
    }
}

// ─── Section 4 ─────────────────────────────────────────────────────

/// Per-row verification outcome shown next to each VC. Mirrors the
/// `last_verify_outcome` text format `self_verify_and_cache` writes
/// into metadata, with an extra `Unknown` slot for rows that haven't
/// been verified yet this session.
#[derive(Clone, PartialEq, Eq)]
enum VerifyBadge {
    Unknown,
    Valid,
    Invalid(String),
    Error(String),
}

impl VerifyBadge {
    fn from_result(r: &SelfVerifyResult) -> Self {
        match r {
            SelfVerifyResult::Valid { .. } => VerifyBadge::Valid,
            SelfVerifyResult::Invalid(reason) => VerifyBadge::Invalid(format!("{reason:?}")),
            SelfVerifyResult::Error(msg) => VerifyBadge::Error(msg.clone()),
        }
    }

    fn label(&self) -> String {
        match self {
            VerifyBadge::Unknown => "Unknown".into(),
            VerifyBadge::Valid => "Valid".into(),
            VerifyBadge::Invalid(r) => format!("Invalid: {r}"),
            VerifyBadge::Error(e) => format!("Error: {e}"),
        }
    }

    fn css(&self) -> &'static str {
        match self {
            VerifyBadge::Unknown => "wizard-outcome",
            VerifyBadge::Valid => "wizard-outcome ok",
            _ => "wizard-outcome err",
        }
    }
}

#[component]
fn VcInventorySection(network: Network, bridge_state: BridgeState) -> Element {
    // Trigger value: bumped after every successful verify so the
    // list resource re-runs. Without this the list would render
    // stale + the new verify badges wouldn't show until the panel
    // unmounted.
    let refresh_tick = use_signal(|| 0u64);

    let vcs = use_resource(move || {
        let _ = refresh_tick.read();
        async move {
            match RedbVcStore::open(vc_store_path()) {
                Ok(s) => s.list_ordered().map_err(|e| e.to_string()),
                Err(e) => Err(format!("open vc store: {e}")),
            }
        }
    });

    // Per-VC verify badge map. Keyed by `vc_uri`. Updates on each
    // self-verify click.
    let badges = use_signal::<std::collections::HashMap<String, VerifyBadge>>(Default::default);

    rsx! {
        div { class: "card",
            div { class: "card-header", "VC inventory" }
            match &*vcs.read_unchecked() {
                None => rsx! { div { class: "detail-empty", "Loading…" } },
                Some(Err(e)) => rsx! {
                    div { class: "wizard-outcome err",
                        div { class: "row label", "List failed" }
                        div { class: "seed-blob", "{e}" }
                    }
                },
                Some(Ok(list)) if list.is_empty() => rsx! {
                    div { class: "detail-empty",
                        "No VCs yet. Request one from Section 3 first."
                    }
                },
                Some(Ok(list)) => rsx! {
                    for vc in list.iter().cloned() {
                        {render_vc_dispatch(network, bridge_state.clone(), vc, badges, refresh_tick)}
                    }
                },
            }

            // Dev-only sample inserter. Lets us validate the
            // digital-passport card without depending on the
            // upstream issuer minting `digital-passport:v1` VCs
            // yet. Stripped from release builds via cfg gate.
            {render_sample_digital_passport_inserter(refresh_tick)}
        }
    }
}

/// Dispatch a VC row to either the schema-specific card component
/// (when we have a view for that family) or the generic fallback
/// row. Keeps `render_vc_row` ignorant of view-module specifics so
/// future schema additions are a single match arm here.
fn render_vc_dispatch(
    network: Network,
    bridge_state: BridgeState,
    vc: StoredVc,
    badges: Signal<std::collections::HashMap<String, VerifyBadge>>,
    refresh_tick: Signal<u64>,
) -> Element {
    if crate::vc_views::digital_passport::is_digital_passport(&vc) {
        render_digital_passport_dispatch(vc, badges, refresh_tick)
    } else {
        render_vc_row(network, bridge_state, vc, badges, refresh_tick)
    }
}

/// Wire `DigitalPassportCard` to the redb-backed opening store
/// and the existing verify-badge map. Keeps the card itself
/// storage-agnostic — see `vc_views/digital_passport.rs` for the
/// extraction rationale.
fn render_digital_passport_dispatch(
    vc: StoredVc,
    badges: Signal<std::collections::HashMap<String, VerifyBadge>>,
    refresh_tick: Signal<u64>,
) -> Element {
    use crate::vc_views::digital_passport::DigitalPassportCard;

    // Open the redb VC store once per render and capture in an Rc
    // so the closure can hand `Option<VcOpening>` lookups to the
    // card. `RedbVcStore` internally holds `Arc<Database>` so the
    // open is cheap; failures (corrupt file, disk full) fall
    // through as "no openings", which the card renders as
    // "(no opening stored)" rows.
    let store_opt =
        std::rc::Rc::new(RedbVcStore::open(vc_store_path()).ok());
    let vc_uri = vc.vc_uri.clone();
    let fetch_opening: std::rc::Rc<dyn Fn(&str) -> Option<VcOpening>> = {
        let store_opt = store_opt.clone();
        let vc_uri = vc_uri.clone();
        std::rc::Rc::new(move |path: &str| -> Option<VcOpening> {
            store_opt
                .as_ref()
                .as_ref()?
                .get_opening(&vc_uri, path)
                .ok()
                .flatten()
        })
    };

    let badge_label = badges
        .read()
        .get(&vc_uri)
        .cloned()
        .map(|b| b.label());

    // Wire Delete to the local redb store via the same path the
    // sample-insertion button uses. Local-only — there's no
    // chain-side equivalent in Phase 1; the holder simply forgets
    // a credential. Errors are logged at info level (best-effort
    // demo cleanup, not a hard fail path).
    let on_delete = EventHandler::new({
        let mut refresh_tick = refresh_tick;
        move |uri_to_delete: String| {
            spawn(async move {
                let store_path = vc_store_path();
                let res = tokio::task::spawn_blocking(move || {
                    RedbVcStore::open(store_path)
                        .map_err(|e| format!("open: {e}"))
                        .and_then(|s| {
                            s.delete_vc(&uri_to_delete)
                                .map_err(|e| format!("delete: {e}"))
                        })
                })
                .await;
                match res {
                    Ok(Ok(())) => {
                        tracing::info!(
                            target: "dioxus_wallet::identity_centre",
                            "vc.delete: removed",
                        );
                    }
                    Ok(Err(e)) => {
                        tracing::warn!(
                            target: "dioxus_wallet::identity_centre",
                            error = %e,
                            "vc.delete: store error",
                        );
                    }
                    Err(e) => {
                        tracing::warn!(
                            target: "dioxus_wallet::identity_centre",
                            error = %e,
                            "vc.delete: task panicked",
                        );
                    }
                }
                let next = *refresh_tick.read() + 1;
                refresh_tick.set(next);
            });
        }
    });

    DigitalPassportCard(vc, fetch_opening, badge_label, Some(on_delete))
}

/// Insert a hard-coded `digital-passport:v1` sample into the redb
/// store on click. Body is a placeholder byte string (not a valid
/// CBOR-encoded credential) — self-verify against this row will
/// fail, but the card renders against the openings + envelope only
/// so the privacy-tier visualisation is exercised end-to-end.
///
/// Gated on `debug_assertions` so release builds (which the demo
/// would actually ship) don't expose the inserter. Listed last in
/// the inventory card to keep it out of the way.
#[cfg(debug_assertions)]
fn render_sample_digital_passport_inserter(
    mut refresh_tick: Signal<u64>,
) -> Element {
    let mut busy = use_signal(|| false);
    let mut last_msg = use_signal::<Option<String>>(|| None);

    let insert = move |_| {
        if *busy.read() {
            return;
        }
        busy.set(true);
        spawn(async move {
            let result =
                tokio::task::spawn_blocking(insert_sample_digital_passport)
                    .await;
            let msg = match result {
                Ok(Ok(uri)) => format!("Inserted {uri}"),
                Ok(Err(e)) => format!("Insert failed: {e}"),
                Err(e) => format!("Insert task panicked: {e}"),
            };
            last_msg.set(Some(msg));
            let next = *refresh_tick.read() + 1;
            refresh_tick.set(next);
            busy.set(false);
        });
    };

    rsx! {
        div { class: "row",
            button {
                disabled: *busy.read(),
                onclick: insert,
                {if *busy.read() { "Inserting…" } else { "Insert sample Digital Passport" }}
            }
        }
        if let Some(msg) = last_msg.read().as_ref() {
            div { class: "detail-empty", "{msg}" }
        }
    }
}

#[cfg(not(debug_assertions))]
fn render_sample_digital_passport_inserter(
    _refresh_tick: Signal<u64>,
) -> Element {
    rsx! {}
}

/// Synchronous body of the dev-only sample inserter. Builds a
/// canonical `digital-passport:v1` envelope with the three
/// expected openings under their JSON-Pointer paths, stamps a
/// `display_order` that pushes it to the bottom of the list, and
/// writes everything atomically. Returns the assigned `vc_uri` on
/// success.
#[cfg(debug_assertions)]
fn insert_sample_digital_passport() -> Result<String, String> {
    use crate::vc_views::digital_passport::{
        CLAIM_DATE_OF_BIRTH, CLAIM_FIRST_NAME, CLAIM_LAST_NAME,
    };
    use std::time::{SystemTime, UNIX_EPOCH};

    let store = RedbVcStore::open(vc_store_path())
        .map_err(|e| format!("open vc store: {e}"))?;

    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let vc_uri = format!("urn:vc:digital-passport:sample-{suffix}");
    let issued_at_ms = suffix;

    let vc = StoredVc {
        vc_uri: vc_uri.clone(),
        issuer_did: "did:midnight:demo-issuer-sample".into(),
        holder_did: "did:midnight:demo-holder-sample".into(),
        format: "midnight-vc-compact".into(),
        // Placeholder body. The card renders against the envelope
        // + openings; the body is opaque CBOR in production and we
        // don't decode it here.
        body: b"<sample digital-passport placeholder body>".to_vec(),
        issued_at_ms,
    };

    // Text-pad first/last name to 64 bytes (the schema's
    // `Bytes<64>` representation).
    let first = pad_to_64(b"Alice");
    let last = pad_to_64(b"Liddell");

    // `dateOfBirth` as days-since-epoch for 1990-01-01.
    // 7305 days = 1970-01-01 + 20 years (5 leap days).
    let dob_days: u32 = 7305;
    let dob_bytes = dob_days.to_le_bytes().to_vec();

    let openings = vec![
        VcOpening {
            vc_uri: vc_uri.clone(),
            claim_path: CLAIM_FIRST_NAME.into(),
            plaintext: first,
            opening: vec![0u8; 32],
        },
        VcOpening {
            vc_uri: vc_uri.clone(),
            claim_path: CLAIM_LAST_NAME.into(),
            plaintext: last,
            opening: vec![0u8; 32],
        },
        VcOpening {
            vc_uri: vc_uri.clone(),
            claim_path: CLAIM_DATE_OF_BIRTH.into(),
            plaintext: dob_bytes,
            opening: vec![0u8; 32],
        },
    ];

    store
        .insert_vc_with_openings(&vc, &openings)
        .map_err(|e| format!("insert: {e}"))?;
    store
        .update_metadata(&vc_uri, |m| {
            // Push sample rows to the end of the list so they
            // don't reorder real issuer-minted VCs.
            m.display_order = u32::MAX - 1;
        })
        .map_err(|e| format!("update metadata: {e}"))?;
    Ok(vc_uri)
}

/// Right-pad a byte slice with zeros up to 64 bytes. Used by the
/// dev-only sample inserter to mirror the schema's text-padded
/// `Bytes<64>` representation of first/last name claims.
#[cfg(debug_assertions)]
fn pad_to_64(s: &[u8]) -> Vec<u8> {
    let mut out = vec![0u8; 64];
    let n = s.len().min(64);
    out[..n].copy_from_slice(&s[..n]);
    out
}

/// Render a single VC row. Plain helper (not a `#[component]`)
/// because `StoredVc` doesn't implement `PartialEq` — the
/// `#[component]` macro requires Eq props.
fn render_vc_row(
    network: Network,
    bridge_state: BridgeState,
    vc: StoredVc,
    mut badges: Signal<std::collections::HashMap<String, VerifyBadge>>,
    mut refresh_tick: Signal<u64>,
) -> Element {
    let mut busy = use_signal(|| false);
    let vc_uri = vc.vc_uri.clone();
    let issuer_did = vc.issuer_did.clone();
    let body_len = vc.body.len();

    let verify = {
        let bridge_state = bridge_state.clone();
        let vc = vc.clone();
        let vc_uri_for_set = vc_uri.clone();
        move |_| {
            if *busy.read() {
                return;
            }
            let Some(store) = bridge_state.store().cloned() else {
                let mut b = badges.read().clone();
                b.insert(
                    vc_uri_for_set.clone(),
                    VerifyBadge::Error("wallet store not opened yet".into()),
                );
                badges.set(b);
                return;
            };
            let Some(wallet_id) = bridge_state.active_wallet_id() else {
                let mut b = badges.read().clone();
                b.insert(
                    vc_uri_for_set.clone(),
                    VerifyBadge::Error("no active wallet".into()),
                );
                badges.set(b);
                return;
            };
            busy.set(true);
            let vc = vc.clone();
            let vc_uri = vc_uri_for_set.clone();
            let metrics = bridge_state.metrics_dyn();
            let in_mem_metrics = bridge_state.metrics();
            let probe = bridge_state.resource_probe();
            let action_id = next_action_id();
            let span = tracing::info_span!("ic.self_verify", action_id = %action_id);
            spawn(async move {
                let wallet =
                    metered_app_wallet_for(network, metrics.clone(), probe.clone());
                let secret_store = RedbSecretStore::new(store, wallet_id);
                let vc_store = match RedbVcStore::open(vc_store_path()) {
                    Ok(v) => v,
                    Err(e) => {
                        let mut b = badges.read().clone();
                        b.insert(
                            vc_uri.clone(),
                            VerifyBadge::Error(format!("open vc store: {e}")),
                        );
                        badges.set(b);
                        busy.set(false);
                        return;
                    }
                };
                let clock = SystemClock;
                // Bracket the verify call with `time_op_simple`
                // (it returns a non-Result `SelfVerifyResult`)
                // so the aggregator records latency + RSS /
                // CPU delta per click. Outcomes are
                // independently broken out via counters below.
                let r = time_op_simple(
                    &*metrics,
                    &*probe,
                    "self_verify",
                    self_verify_and_cache(
                        &vc,
                        &wallet,
                        &secret_store,
                        &vc_store,
                        &clock,
                    ),
                )
                .await;
                match &r {
                    wallet_core::SelfVerifyResult::Valid { .. } => {
                        in_mem_metrics.incr("verifies.valid", 1);
                    }
                    wallet_core::SelfVerifyResult::Invalid(_) => {
                        in_mem_metrics.incr("verifies.invalid", 1);
                    }
                    wallet_core::SelfVerifyResult::Error(_) => {
                        in_mem_metrics.incr("verifies.error", 1);
                    }
                }
                let mut b = badges.read().clone();
                b.insert(vc_uri.clone(), VerifyBadge::from_result(&r));
                badges.set(b);
                let next = *refresh_tick.read() + 1;
                refresh_tick.set(next);
                busy.set(false);
            }.instrument(span));
        }
    };

    let badge = badges
        .read()
        .get(&vc_uri)
        .cloned()
        .unwrap_or(VerifyBadge::Unknown);

    // Per-row Delete handler — local-only redb removal. Same
    // pattern as the digital-passport card's delete: spawn a
    // blocking task, log the outcome at info/warn, then bump the
    // refresh tick so the inventory re-renders without the row.
    let delete = {
        let vc_uri_for_delete = vc_uri.clone();
        let mut refresh_tick = refresh_tick;
        move |_| {
            let uri = vc_uri_for_delete.clone();
            spawn(async move {
                let store_path = vc_store_path();
                let res = tokio::task::spawn_blocking(move || {
                    RedbVcStore::open(store_path)
                        .map_err(|e| format!("open: {e}"))
                        .and_then(|s| {
                            s.delete_vc(&uri).map_err(|e| format!("delete: {e}"))
                        })
                })
                .await;
                let outcome = match res {
                    Ok(Ok(())) => None,
                    Ok(Err(e)) => Some(e),
                    Err(e) => Some(format!("task panicked: {e}")),
                };
                if let Some(msg) = outcome {
                    tracing::warn!(
                        target: "dioxus_wallet::identity_centre",
                        error = %msg,
                        "vc.delete (generic row): failed",
                    );
                }
                let next = *refresh_tick.read() + 1;
                refresh_tick.set(next);
            });
        }
    };

    rsx! {
        div { class: "row label", "VC" }
        div { class: "seed-blob", "{truncate_did(&vc_uri)}" }
        div { class: "row label", "Issuer" }
        div { class: "seed-blob", "{truncate_did(&issuer_did)}" }
        div { class: "detail-empty", "body: {body_len} bytes" }
        div { class: "row",
            button {
                disabled: *busy.read(),
                onclick: verify,
                {if *busy.read() { "Verifying…" } else { "Self-verify" }}
            }
            button {
                class: "vc-row-delete-btn",
                onclick: delete,
                "Delete"
            }
            div { class: "{badge.css()}",
                div { class: "seed-blob", "{badge.label()}" }
            }
        }
    }
}
