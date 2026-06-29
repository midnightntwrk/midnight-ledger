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

use std::sync::atomic::{AtomicU64, Ordering};

use dioxus::prelude::*;
use tracing::Instrument;

// HTTP / OID4VCI / `time_op` imports moved to `worker/handlers.rs`
// along with the OID4VP + OID4VCI flows (worker plan Tasks 3 & 4).
// What stays here is the VC-inventory + self-verify surface that
// still runs inline.
use wallet_core::{
    Metrics, Network, RedbVcStore, SelfVerifyResult, StoredVc, SystemClock,
    self_verify_and_cache, time_op_simple,
};

/// Monotonic per-session counter for Identity Centre action IDs.
/// Rendered as hex so a 4-char string keeps the Logs-tab prefix
/// short while still being unique across the ~thousand-click
/// session ceiling we'd ever realistically hit. Wraps at u64::MAX
/// after roughly 18 quintillion clicks.
static NEXT_ACTION_ID: AtomicU64 = AtomicU64::new(0);

/// Translate a raw bootstrap-error string into a user-facing message.
///
/// Wallet-core errors arrive as opaque strings ("submit: RPC error:
/// User error: Invalid Transaction (1010)") that mean nothing to a
/// demo operator. Pattern-match the well-known signatures and
/// substitute a sentence describing what happened + what the next
/// useful action is. Fall back to the raw string when we don't
/// recognise it so power users / log readers still see the truth.
///
/// This is a tactical fix until the proper "mobile-friendly error
/// surface" feature (see `docs/BACKLOG.md`) is built. The proper
/// version groups errors by category, shows a one-line headline +
/// optional details disclosure, and threads correlation IDs through
/// to the Logs tab — none of which fits in a `String` substitution.
pub(crate) fn humanize_bootstrap_error(raw: &str) -> String {
    // 1010 / Custom error 196 = LedgerLackOfDust on standalone.
    // The wallet seed is fresh; per-block DUST emission hasn't yet
    // accrued enough to pay for the createDID contract write.
    if raw.contains("Invalid Transaction (1010)")
        || raw.contains("Custom error: 196")
        || raw.to_lowercase().contains("lack of dust")
    {
        return "Not enough DUST yet — the test chain needs ~60-120 \
                seconds of block emission before your wallet can mint \
                a DID. Wait a minute and tap Bootstrap again.\n\n\
                (Technical: chain returned `Invalid Transaction (1010)` \
                = LedgerLackOfDust. The standalone Midnight env only \
                pre-funds the genesis seed; every other seed accrues \
                DUST one block at a time.)"
            .to_string();
    }
    // 1010 / Custom error 200 family covers BadProof / ProofVerificationFailed.
    if raw.contains("Custom error: 200") || raw.contains("BadProof") {
        return "Proof verification failed on chain. This usually \
                means the wallet is talking to a chain running newer \
                LedgerParameters than the build expects.\n\n\
                Try: tap Bootstrap again (the wallet refetches tip \
                params on each retry). If it keeps failing, the \
                APK probably needs to be rebuilt against the current \
                workspace develop.\n\n\
                (Technical: chain returned `Custom error: 200` = \
                BadProof.)"
            .to_string();
    }
    // Network-level reachability failures.
    if raw.contains("Connection refused")
        || raw.contains("dns error")
        || raw.contains("connect timeout")
        || raw.contains("Network is unreachable")
    {
        return format!(
            "Couldn't reach the Midnight indexer / proof-server. \
             Check that:\n\
             • the laptop's docker-compose stack is up \
             (`./scripts/run-demo.sh env-status`)\n\
             • your phone is on the same tailnet as the laptop\n\
             • the URLs in Diagnostics → Endpoints point at the \
             laptop's tailnet IP (not localhost)\n\n\
             (Technical: {raw})"
        );
    }
    // No recognised pattern — pass through with a soft framing.
    format!("Bootstrap failed.\n\n(Technical: {raw})")
}

/// Cross-component "VC inventory changed" tick.
///
/// [`VcInventorySection`] subscribes to this in its
/// `use_resource` so any flow that mutates `vcs.redb` (the redb
/// VC store) can bump the counter and force the next render to
/// re-read the rows.
///
/// Why a `GlobalSignal` and not the existing per-component
/// `refresh_tick`: OID4VCI issuance can run from the
/// Diagnostics → Bootstrap paste-URL flow (whose component tree
/// doesn't include `VcInventorySection`) OR from the
/// Credentials → Scan QR flow (which mounts the inventory but
/// dispatches via [`run_oid4vci_request`] in a sibling scope
/// that has no handle to the local `refresh_tick`). A module-
/// level [`GlobalSignal`] is the smallest fix that bridges
/// across both component trees AND survives navigation without
/// re-instantiating.
///
/// The original per-component `refresh_tick` is unchanged —
/// kept for the cheap in-place bumps (post-verify badge
/// refresh, sample-VC inserter) so an empty round-trip through
/// the global doesn't churn the resource on every badge tick.
pub(crate) static VC_INVENTORY_TICK: GlobalSignal<u64> =
    GlobalSignal::new(|| 0);

/// Bump the global VC-inventory tick. Call this after any
/// mutation to `vcs.redb` that originated outside
/// `VcInventorySection`'s own scope. Cheap: a single atomic
/// increment + Dioxus subscription wake.
pub(crate) fn bump_vc_inventory_tick() {
    let next = *VC_INVENTORY_TICK.read() + 1;
    *VC_INVENTORY_TICK.write() = next;
}

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
use wallet_core::secret_storage::{SecretStorage, redb_secret_store::RedbSecretStore};

use crate::app::{metered_app_wallet_for, truncate_did, wallet_store_path};
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
///
/// `pub(crate)` so the worker module's OID4VCI handler can
/// reach the same path the UI uses (otherwise a worker-side
/// `RedbVcStore::open` would land in a different file from the
/// `VcInventorySection` reader's).
pub(crate) fn vc_store_path() -> std::path::PathBuf {
    let mut p = wallet_store_path();
    p.set_file_name(VC_STORE_FILENAME);
    p
}

/// Run the OID4VP authentication flow with a known DID + URL.
/// Shared by `Oid4vpSection` (paste flow) and `ScanQrSection`
/// (scan flow); each call site invokes via `require_did` so the
/// DID has already been picked by the time we land here.
///
/// Sets `busy = true` synchronously and clears it after the
/// spawned task finishes; writes the protocol outcome to
/// `ok_msg` / `err_msg`.
fn run_oid4vp_authenticate(
    network: Network,
    bridge_state: BridgeState,
    did_str: String,
    url: String,
    mut err_msg: Signal<Option<String>>,
    mut ok_msg: Signal<Option<String>>,
    mut busy: Signal<bool>,
) {
    // Parse the DID synchronously — anything malformed is a
    // user / UI bug that doesn't deserve a worker round-trip.
    let did = match wallet_core::DidId::parse(&did_str) {
        Ok(d) => d,
        Err(e) => {
            err_msg.set(Some(format!("did parse: {e}")));
            return;
        }
    };
    // Defensive: these reads also happen on the worker side;
    // catching them here lets us surface a precise error before
    // a WorkMsg flies and avoids a needless busy flicker.
    if bridge_state.store().is_none() {
        err_msg.set(Some("wallet store not opened yet".into()));
        return;
    }
    if bridge_state.active_wallet_id().is_none() {
        err_msg.set(Some("no active wallet".into()));
        return;
    }
    busy.set(true);
    err_msg.set(None);
    ok_msg.set(None);

    // Reach the worker. If the bridge state hasn't installed one
    // yet (shouldn't happen post-App-boot), surface a clean
    // error rather than silently spinning.
    let Some(worker) = bridge_state.worker().cloned() else {
        err_msg.set(Some("wallet worker not ready".into()));
        busy.set(false);
        return;
    };
    let in_mem_metrics = bridge_state.metrics();
    let action_id = crate::worker::next_action_id();

    // Thread-affinity wrap — see the OID4VCI counterpart for the
    // full rationale. The bootstrap path's commit fdba2182 fix
    // applies here too: `router::register` MUST run on the same
    // Dioxus task-pool thread as the outcome-pump's `take`, or
    // the worker's outcome is silently dropped.
    spawn(async move {
        // Register the outcome handler BEFORE sending so a very fast
        // worker can't race the registration. The closure captures
        // the local signals + metrics; it runs inside the outcome-
        // pump `use_future` (Dioxus scope) so signal writes are
        // valid. The metrics counter increments mirror the pre-
        // worker inline behaviour.
        crate::worker::router::register(
            action_id,
            Box::new(move |outcome| {
                match outcome {
                    crate::worker::WorkOutcome::Oid4vpOk {
                        session_id,
                        status,
                        ..
                    } => {
                        in_mem_metrics.incr("oid4vp.ok", 1);
                        if let Ok(mut w) = ok_msg.try_write() {
                            *w = Some(format!(
                                "session_id={session_id} status={status}",
                            ));
                        }
                    }
                    crate::worker::WorkOutcome::Err { msg, .. } => {
                        in_mem_metrics.incr("oid4vp.failed", 1);
                        if let Ok(mut w) = err_msg.try_write() {
                            *w = Some(format!("authenticate failed: {msg}"));
                        }
                    }
                    // Worker only emits Oid4vpOk / Err for an
                    // Oid4vpAuthenticate action_id; log defensively
                    // if a future variant routes wrongly.
                    other => {
                        tracing::warn!(
                            target: "wallet_worker",
                            action_id,
                            ?other,
                            "OID4VP handler received unexpected outcome",
                        );
                    }
                }
                if let Ok(mut w) = busy.try_write() {
                    *w = false;
                }
            }),
        );

        worker.send(crate::worker::WorkMsg::Oid4vpAuthenticate {
            action_id,
            network,
            did,
            qr_url: url,
        });
    });
}

/// Run the OID4VCI issuance flow with a known DID + URL. Counterpart
/// to `run_oid4vp_authenticate` for the credential-offer side.
///
/// Routed through the wallet-worker thread (worker plan Task
/// 4). The click site only registers an outcome handler +
/// sends a `WorkMsg::Oid4vciIssuance`; the heavy state machine
/// (Wallet + indexer + http + JWS PoP + credential POST +
/// redb insert) runs on the worker's 8 MiB stack instead of
/// the Chromium WebView dispatch thread's 256 KiB.
fn run_oid4vci_request(
    network: Network,
    bridge_state: BridgeState,
    did_str: String,
    url: String,
    mut err_msg: Signal<Option<String>>,
    mut ok_msg: Signal<Option<String>>,
    mut busy: Signal<bool>,
) {
    let did = match wallet_core::DidId::parse(&did_str) {
        Ok(d) => d,
        Err(e) => {
            err_msg.set(Some(format!("did parse: {e}")));
            return;
        }
    };
    // Defensive: surface a precise error here so we don't fire
    // a WorkMsg the worker would immediately reject. The worker
    // side re-checks for safety.
    if bridge_state.store().is_none() {
        err_msg.set(Some("wallet store not opened yet".into()));
        return;
    }
    if bridge_state.active_wallet_id().is_none() {
        err_msg.set(Some("no active wallet".into()));
        return;
    }
    busy.set(true);
    err_msg.set(None);
    ok_msg.set(None);

    let Some(worker) = bridge_state.worker().cloned() else {
        err_msg.set(Some("wallet worker not ready".into()));
        busy.set(false);
        return;
    };
    let in_mem_metrics = bridge_state.metrics();
    let action_id = crate::worker::next_action_id();

    // Thread-affinity invariant of `worker::router`: the
    // register-side and the take-side must execute on the same
    // Dioxus task-pool thread, because the router stores
    // handlers in `thread_local!` storage. The outcome-pump
    // (`use_future` in the parent App) runs on the task pool;
    // this function is called from the WebView dispatch thread,
    // so a bare `register + send` here lands the handler on the
    // wrong thread, the worker emits its outcome, and the
    // pump's `take` returns `None` ("outcome dropped — no
    // registered handler" — see worker/router.rs).
    //
    // Wrap both register + send in `spawn(async move {…})` so
    // the work runs on the task pool, matching where the pump
    // pulls the outcome from. Same fix as the bootstrap path
    // (commit fdba2182) — keeping the OID4VP/OID4VCI paths
    // structurally aligned.
    spawn(async move {
        // Outcome handler captures the per-component signals +
        // metrics counters + the cross-component VC inventory tick.
        // Runs inside the App's outcome-pump `use_future` (Dioxus
        // scope), so signal writes + the global-signal bump are
        // valid.
        crate::worker::router::register(
            action_id,
            Box::new(move |outcome| {
                match outcome {
                    crate::worker::WorkOutcome::Oid4vciOk { vc_uri, .. } => {
                        in_mem_metrics.incr("vcs.issued", 1);
                        // Cross-tab refresh signal: the worker
                        // persisted a row into vcs.redb; the
                        // Credentials → inventory `use_resource`
                        // resubscribes via this bump and reads the
                        // new row on the next paint (commit
                        // ea972c42 added this for the pre-worker
                        // path; preserved here).
                        bump_vc_inventory_tick();
                        if let Ok(mut w) = ok_msg.try_write() {
                            *w = Some(format!("issued {vc_uri}"));
                        }
                    }
                    crate::worker::WorkOutcome::Err { msg, .. } => {
                        in_mem_metrics.incr("vcs.issuance_failed", 1);
                        if let Ok(mut w) = err_msg.try_write() {
                            *w = Some(msg);
                        }
                    }
                    other => {
                        tracing::warn!(
                            target: "wallet_worker",
                            action_id,
                            ?other,
                            "OID4VCI handler received unexpected outcome",
                        );
                    }
                }
                if let Ok(mut w) = busy.try_write() {
                    *w = false;
                }
            }),
        );

        worker.send(crate::worker::WorkMsg::Oid4vciIssuance {
            action_id,
            network,
            did,
            qr_url: url,
        });
    });
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
    did_inventory: Signal<
        std::collections::BTreeMap<String, crate::app::DidInventoryEntry>,
    >,
    on_did_minted: EventHandler<(String, Network)>,
) -> Element {
    // The "current Identity Centre DID" — populated either from a
    // fresh bootstrap or by scanning the secret store for a key
    // whose `kid` carries the `#key-auth` fragment. Held at panel
    // scope so all four sections share it.
    let ic_did = use_signal::<Option<String>>(|| None);

    // Picker state — `Some` while the DID-picker modal is up.
    // Shared by every flow that needs an explicit "act as which
    // DID?" decision: the Scan QR button (this panel) and the
    // OID4VCI request (this panel). The Bootstrap tab owns its
    // own copy for the OID4VP paste-URL flow.
    let pending_pick =
        use_signal::<Option<crate::did_picker::PickerState>>(|| None);

    // `on_did_minted` is the bridge between this panel and `app.rs`'s
    // `did_inventory` signal. It travels with the BootstrapPanel
    // (where the actual minting happens) rather than the
    // IdentityCentrePanel now, but the props are kept here so older
    // call sites (and the C3 picker work coming next) can compile
    // unchanged.
    let _ = on_did_minted;

    rsx! {
        // Scan-only Credentials tab. The OID4VCI paste-URL section
        // (and its OID4VP sibling) live under Diagnostics →
        // Bootstrap now; the everyday holder-facing surface is the
        // scan-QR hero + the credential inventory underneath.
        ScanQrSection {
            network,
            bridge_state: bridge_state.clone(),
            ic_did,
            did_inventory,
            pending_pick,
        }

        VcInventorySection {
            network,
            bridge_state,
        }

        // Modal — renders when pending_pick is `Some`, returns
        // empty rsx otherwise. Sits at panel-bottom so the
        // backdrop overlays everything above (CSS handles the
        // z-index + full-viewport sizing).
        crate::did_picker::DidPickerModal { pending_pick }
    }
}

// ─── Top-level Scan QR (C2) ────────────────────────────────────────

/// Prominent scan-and-route action at the top of the Identity
/// Centre. Single entry point for both protocols — replaces the
/// per-section "📷 Scan QR" buttons that lived in `Oid4vpSection`
/// and `Oid4vciSection` pre-C1.
///
/// Behaviour:
///
/// 1. Click → opens the WebView's full-viewport camera overlay.
/// 2. On a decoded payload, inspect the URI scheme:
///    - `openid4vp://...` → run OID4VP authentication
///    - `openid-credential-offer://...` → run OID4VCI issuance
///    - anything else → surface "unsupported QR payload" error.
/// 3. The flow code mirrors what `Oid4vpSection` / `Oid4vciSection`
///    do internally (same wallet-core entry points, same metering
///    decorators). DID picker integration lands in C3.
#[component]
fn ScanQrSection(
    network: Network,
    bridge_state: BridgeState,
    ic_did: Signal<Option<String>>,
    did_inventory: Signal<
        std::collections::BTreeMap<String, crate::app::DidInventoryEntry>,
    >,
    pending_pick: Signal<Option<crate::did_picker::PickerState>>,
) -> Element {
    let mut busy = use_signal(|| false);
    let mut err_msg = use_signal::<Option<String>>(|| None);
    let mut ok_msg = use_signal::<Option<String>>(|| None);

    let scan_and_dispatch = {
        let bridge_state = bridge_state.clone();
        let mut ic_did = ic_did;
        move |_| {
            if *busy.read() {
                return;
            }
            let bridge_state_outer = bridge_state.clone();
            let mut err_msg_outer = err_msg;
            // Pick the DID FIRST — saves the user cancelling the
            // camera scan if it turns out they don't want to
            // authenticate as the only DID they have.
            crate::did_picker::require_did(
                did_inventory,
                pending_pick,
                "Scan QR (auto-detect protocol)",
                move |chosen_did| {
                    ic_did.set(Some(chosen_did.clone()));
                    err_msg.set(None);
                    ok_msg.set(None);
                    busy.set(true);
                    let bridge_state = bridge_state_outer.clone();
                    spawn(async move {
                        // Platform-resolved scanner. On Android,
                        // `ActiveQrScanner = AndroidQrScanner`,
                        // which JNIs into ML Kit and ignores the
                        // JS bridge. On every other target it's
                        // `FallbackQrScanner`, which delegates to
                        // `eval_bridge::scan_qr` (the legacy
                        // jsQR/getUserMedia path). The wallet UI
                        // code below doesn't branch on platform.
                        let scanner = crate::ActiveQrScanner;
                        let url = match wallet_core::QrScanner::scan(&scanner).await {
                            Ok(u) => u,
                            Err(wallet_core::QrScanError::Cancelled) => {
                                busy.set(false);
                                return;
                            }
                            Err(wallet_core::QrScanError::Unavailable(msg)) => {
                                err_msg.set(Some(msg));
                                busy.set(false);
                                return;
                            }
                            Err(e) => {
                                err_msg.set(Some(format!("{e}")));
                                busy.set(false);
                                return;
                            }
                        };
                        if url.starts_with("openid4vp://") {
                            // `run_oid4vp_authenticate` re-sets
                            // busy=true synchronously so the inner
                            // helper owns the busy lifecycle from
                            // here on. We've already flipped it
                            // true above; the helper flipping it
                            // true again is a no-op.
                            run_oid4vp_authenticate(
                                network,
                                bridge_state.clone(),
                                chosen_did.clone(),
                                url,
                                err_msg,
                                ok_msg,
                                busy,
                            );
                        } else if url.starts_with("openid-credential-offer://") {
                            run_oid4vci_request(
                                network,
                                bridge_state.clone(),
                                chosen_did.clone(),
                                url,
                                err_msg,
                                ok_msg,
                                busy,
                            );
                        } else {
                            err_msg.set(Some(format!(
                                "Unsupported QR payload: expected openid4vp:// or \
                                 openid-credential-offer:// prefix, got: {}",
                                truncate_for_msg(&url, 80),
                            )));
                            busy.set(false);
                        }
                    });
                },
                move |msg| err_msg_outer.set(Some(msg)),
            );
        }
    };

    rsx! {
        section { class: "cta-card scan-hero",
            div { class: "cta-card__ambient cta-card__ambient--one" }
            div { class: "cta-card__ambient cta-card__ambient--two" }

            div { class: "scan-hero__body",
                div { class: "scan-hero__copy",
                    p { class: "cta-card__eyebrow", "Receive" }
                    h2 { class: "cta-card__title", "Scan to continue" }
                    p { class: "cta-card__sub",
                        "One tap — auto-detects OID4VP (authenticate) "
                        "vs OID4VCI (request credential)."
                    }
                    div { class: "cta-card__action",
                        button {
                            // `scan-btn` lays out the leading icon
                            // + label inline; without it the SVG
                            // would render as a block before the
                            // text. Defined in `assets/styles.css`
                            // next to `.btn-primary`.
                            class: "btn-primary scan-btn",
                            disabled: *busy.read(),
                            onclick: scan_and_dispatch,
                            if *busy.read() {
                                "Working…"
                            } else {
                                span {
                                    class: "scan-btn__icon",
                                    aria_hidden: "true",
                                    dangerous_inner_html: crate::app::LUCIDE_SCAN_LINE,
                                }
                                span { class: "scan-btn__label", "Scan QR" }
                            }
                        }
                    }
                }
                // QR-frame illustration. Pure CSS + inline SVG so the
                // wallet binary doesn't pull a new asset. Corner
                // brackets evoke a viewfinder; the centre is a faint
                // grid hint suggesting the QR module pattern. Hidden
                // on narrow viewports via the `.cta-card`
                // breakpoint-collapse below.
                div { class: "scan-hero__frame", aria_hidden: "true",
                    div { class: "scan-hero__frame-inner",
                        div { class: "scan-hero__frame-corner scan-hero__frame-corner--tl" }
                        div { class: "scan-hero__frame-corner scan-hero__frame-corner--tr" }
                        div { class: "scan-hero__frame-corner scan-hero__frame-corner--bl" }
                        div { class: "scan-hero__frame-corner scan-hero__frame-corner--br" }
                        div { class: "scan-hero__frame-grid" }
                    }
                }
            }

            if let Some(msg) = ok_msg.read().as_ref() {
                div { class: "outcome outcome--ok",
                    div { class: "outcome__title", "Done" }
                    div { class: "outcome__body", "{msg}" }
                }
            }
            if let Some(msg) = err_msg.read().as_ref() {
                div { class: "outcome outcome--err",
                    div { class: "outcome__title", "Failed" }
                    div { class: "outcome__body", "{msg}" }
                }
            }
        }
    }
}

/// Trim a URL for inclusion in a user-facing error message.
/// Keeps the head + tail like the digital-passport card does
/// elsewhere, just less ornate.
fn truncate_for_msg(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.into();
    }
    let head: String = s.chars().take(max / 2).collect();
    format!("{head}…")
}

/// Operator/dev setup panel. Renders for `Tab::Bootstrap`. Holds
/// the heavy-weight one-time DID minting flow (`bootstrap_did_with_keys`)
/// and the manual paste-URL `OID4VP` authenticate path — the two
/// surfaces a developer/operator uses to seed a wallet or debug
/// the OID4VP step-by-step without the camera-scan path.
///
/// The everyday holder-facing surface (`IdentityCentrePanel`) stays
/// scan-first and skips these heavier controls.
///
/// Mounted inside the Diagnostics carousel's "Bootstrap" page.
/// Settings mounts `DemoWalletControlsSection` directly — the
/// wallet-swap dev affordance isn't bootstrap-related.
#[component]
pub fn DiagBootstrapPanel(
    network: Network,
    bridge_state: BridgeState,
    did_inventory: Signal<
        std::collections::BTreeMap<String, crate::app::DidInventoryEntry>,
    >,
    on_did_minted: EventHandler<(String, Network)>,
) -> Element {
    let ic_did = use_signal::<Option<String>>(|| None);
    // Shared picker state — both the OID4VP-paste and the
    // OID4VCI-paste sections route their entry-point clicks
    // through `require_did(pending_pick, …)`. Modal renders
    // last in the rsx so its backdrop sits above the cards.
    let pending_pick =
        use_signal::<Option<crate::did_picker::PickerState>>(|| None);

    rsx! {
        header { class: "section-header",
            div {
                p { class: "section-header__eyebrow", "Bootstrap" }
                h2 { class: "section-header__title", "Setup & manual flows" }
                p { class: "section-header__sub",
                    "Mint a fresh DID + VC keys, or paste an OID4VP / "
                    "OID4VCI URL by hand. The everyday Credentials tab "
                    "drives the same flows from the scanner."
                }
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
            did_inventory,
            pending_pick,
        }

        Oid4vciSection {
            network,
            bridge_state,
            ic_did,
            did_inventory,
            pending_pick,
        }

        crate::did_picker::DidPickerModal { pending_pick }
    }
}

/// Demo-only wallet swap controls. Lets the operator flip between
/// the deterministic demo wallet (`app_wallet_for(network)` — same
/// seed across restarts) and a fresh `Wallet::new_random` for
/// negative-path scenarios. Lives on the Settings tab — it's a
/// wallet-state affordance, not part of the credential / DID
/// bootstrap flow.
#[component]
pub fn DemoWalletControlsSection(
    network: Network,
    wallet: Signal<Option<crate::app::WalletInfo>>,
) -> Element {
    let mut wallet = wallet;

    let reload_demo = move |_| {
        let w = crate::app::app_wallet_for(network);
        wallet.set(Some(crate::app::WalletInfo::from_wallet(&w)));
    };
    let randomise = move |_| {
        let w = wallet_core::Wallet::new_random(network);
        wallet.set(Some(crate::app::WalletInfo::from_wallet(&w)));
    };

    rsx! {
        div { class: "card",
            div { class: "card-header", "Demo wallet controls" }
            div { class: "detail-empty",
                "Swap the in-memory wallet without touching the chain. "
                "Use Reload demo to return to the deterministic "
                "genesis-funded wallet; use Random wallet to seed a "
                "fresh keypair for negative-path scenarios."
            }
            div { class: "row",
                button {
                    class: "btn-secondary",
                    onclick: reload_demo,
                    "Reload demo"
                }
                button {
                    class: "btn-secondary",
                    onclick: randomise,
                    "Random wallet"
                }
            }
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
    let busy = use_signal(|| false);
    let err_msg = use_signal::<Option<String>>(|| None);
    let ok_msg = use_signal::<Option<String>>(|| None);
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
        let mut busy_sig = busy;
        let mut err_msg_sig = err_msg;
        let mut ok_msg_sig = ok_msg;
        let on_did_minted_eh = on_did_minted;
        move |_| {
            if *busy_sig.read() {
                return;
            }
            busy_sig.set(true);
            err_msg_sig.set(None);
            ok_msg_sig.set(None);

            // Reach the worker. If it's not installed yet
            // (shouldn't happen post-App-boot), surface a clear
            // error rather than silently spinning the busy
            // indicator forever.
            let Some(worker) = bridge_state.worker().cloned() else {
                err_msg_sig.set(Some("wallet worker not ready".into()));
                busy_sig.set(false);
                return;
            };
            let in_mem_metrics = bridge_state.metrics();
            let action_id = crate::worker::next_action_id();

            // Thread-affinity invariant of `worker::router`: the
            // register-side and the take-side must execute on the
            // same Dioxus task-pool thread, because the router
            // stores handlers in `thread_local!` storage. The
            // outcome-pump (`use_future` in the parent App)
            // already runs on the task pool — typically
            // ThreadId(2). This `onclick` closure runs on the
            // WebView dispatch thread — typically ThreadId(4) —
            // so a bare `register + send` here lands the handler
            // on the wrong thread, the worker emits its outcome,
            // and the pump's `take` returns `None` ("outcome
            // dropped — no registered handler" — see
            // worker/router.rs).
            //
            // Wrap both register + send in `spawn(async move {…})`
            // so the work runs on the task pool, matching where
            // the pump pulls the outcome from. Same fix as the
            // Unlock path; lock-step with the Worker Task 5
            // remediation in commit fdba2182.
            spawn(async move {
                crate::worker::router::register(
                    action_id,
                    Box::new(move |outcome| {
                        match outcome {
                            crate::worker::WorkOutcome::BootstrapOk {
                                did_str, ..
                            } => {
                                in_mem_metrics.incr("dids.bootstrapped", 1);
                                on_did_minted_eh.call((did_str.clone(), network));
                                // Use try_write() instead of .set() to avoid
                                // ValueDroppedError panic when the component
                                // has already unmounted by the time the
                                // worker thread delivers the outcome.
                                if let Ok(mut w) = ic_did.try_write() {
                                    *w = Some(did_str.clone());
                                }
                                if let Ok(mut w) = ok_msg_sig.try_write() {
                                    *w = Some(format!(
                                        "Bootstrapped {did_str}. Switch to the Dids \
                                         tab to see it; click Resolve there to fill \
                                         in counter / VM counts.",
                                    ));
                                }
                            }
                            crate::worker::WorkOutcome::Err { msg, .. } => {
                                in_mem_metrics.incr("dids.bootstrap_failed", 1);
                                let friendly = humanize_bootstrap_error(&msg);
                                if let Ok(mut w) = err_msg_sig.try_write() {
                                    *w = Some(friendly);
                                }
                            }
                            // The worker only ever emits BootstrapOk
                            // / Err for a Bootstrap action_id; the
                            // other arms aren't reachable for this
                            // registration. Log defensively in case
                            // a future variant routes wrongly.
                            other => {
                                tracing::warn!(
                                    target: "wallet_worker",
                                    action_id,
                                    ?other,
                                    "Bootstrap handler received unexpected outcome",
                                );
                            }
                        }
                        if let Ok(mut w) = busy_sig.try_write() {
                            *w = false;
                        }
                    }),
                );

                worker.send(crate::worker::WorkMsg::Bootstrap {
                    action_id,
                    network,
                    seed: DEMO_IC_SEED,
                });
            });
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
                div { class: "ic-did-row",
                    div { class: "ic-did-label", "Active DID" }
                    div {
                        class: "ic-did-value mono",
                        title: "{did}",
                        "{truncate_did(did)}"
                    }
                }
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
    did_inventory: Signal<
        std::collections::BTreeMap<String, crate::app::DidInventoryEntry>,
    >,
    pending_pick: Signal<Option<crate::did_picker::PickerState>>,
) -> Element {
    let mut url_input = use_signal(String::new);
    let busy = use_signal(|| false);
    let mut err_msg = use_signal::<Option<String>>(|| None);
    let ok_msg = use_signal::<Option<String>>(|| None);

    let authenticate = {
        let bridge_state = bridge_state.clone();
        let mut ic_did = ic_did;
        move |_| {
            if *busy.read() {
                return;
            }
            let url = url_input.read().trim().to_string();
            if url.is_empty() {
                err_msg.set(Some("paste an openid4vp:// URL first".into()));
                return;
            }
            let bridge_state_outer = bridge_state.clone();
            let mut err_msg_outer = err_msg;
            // Route through the DID picker. 0 usable DIDs → error
            // surfaced via the on_error sink; 1 → continuation runs
            // immediately; >1 → modal opens and the continuation
            // runs once the user picks.
            crate::did_picker::require_did(
                did_inventory,
                pending_pick,
                "Authenticate via OID4VP",
                move |chosen_did| {
                    ic_did.set(Some(chosen_did.clone()));
                    run_oid4vp_authenticate(
                        network,
                        bridge_state_outer.clone(),
                        chosen_did,
                        url.clone(),
                        err_msg,
                        ok_msg,
                        busy,
                    );
                },
                move |msg| err_msg_outer.set(Some(msg)),
            );
        }
    };

    // Explicit paste button — iOS WKWebView's long-press / Cmd-V
    // paste into `<textarea>` is unreliable; the bundle's
    // `pasteText()` calls `navigator.clipboard.readText()` from a
    // button click (a valid user gesture) and works on every
    // supported target.
    //
    // Per-section Scan QR was removed in the C1 layout split — the
    // top-of-IdentityCentre Scan QR button (landed in C2) is the
    // single QR entry point now and dispatches by protocol prefix.
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
    did_inventory: Signal<
        std::collections::BTreeMap<String, crate::app::DidInventoryEntry>,
    >,
    pending_pick: Signal<Option<crate::did_picker::PickerState>>,
) -> Element {
    let mut url_input = use_signal(String::new);
    let busy = use_signal(|| false);
    let mut err_msg = use_signal::<Option<String>>(|| None);
    let ok_msg = use_signal::<Option<String>>(|| None);

    let request_vc = {
        let bridge_state = bridge_state.clone();
        let mut ic_did = ic_did;
        move |_| {
            if *busy.read() {
                return;
            }
            let url = url_input.read().trim().to_string();
            if url.is_empty() {
                err_msg.set(Some(
                    "paste an openid-credential-offer:// URL first".into(),
                ));
                return;
            }
            let bridge_state_outer = bridge_state.clone();
            let mut err_msg_outer = err_msg;
            crate::did_picker::require_did(
                did_inventory,
                pending_pick,
                "Request credential (OID4VCI)",
                move |chosen_did| {
                    ic_did.set(Some(chosen_did.clone()));
                    run_oid4vci_request(
                        network,
                        bridge_state_outer.clone(),
                        chosen_did,
                        url.clone(),
                        err_msg,
                        ok_msg,
                        busy,
                    );
                },
                move |msg| err_msg_outer.set(Some(msg)),
            );
        }
    };

    // See identity_centre OID4VP card — same rationale, iOS
    // needs an explicit clipboard-read entry point. Per-section
    // Scan QR was removed in the C1 layout split — the
    // top-of-IdentityCentre Scan QR button (landed in C2) is the
    // single QR entry point and dispatches by protocol prefix.
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
        // Subscribe to BOTH the per-component tick (verify
        // badge / sample-insert) AND the module-level
        // `VC_INVENTORY_TICK` (cross-component bumps from
        // OID4VCI issuance — see [`bump_vc_inventory_tick`]).
        // Dioxus re-runs the resource when EITHER signal
        // changes; reading both inside the closure registers
        // the dependency.
        let _ = refresh_tick.read();
        let _ = VC_INVENTORY_TICK.read();
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

    // Per-row state hoisted to the inventory scope so it survives
    // (and isolates) row inserts + deletes. Each map is keyed by
    // `vc_uri`. Hoisting is necessary because Dioxus's rules of
    // hooks forbid calling `use_signal` inside per-row helpers:
    // the helper is invoked once per VC in the inventory's render
    // loop, so any `use_signal` inside it joins the inventory's
    // hook list. When the row count changes (delete!) the hook
    // count changes and Dioxus panics with "Unable to retrieve
    // the hook that was initialized at this index". The maps
    // below give us the same per-row state without ever touching
    // a hook from inside the helpers.
    let reveal_first_set =
        use_signal::<std::collections::HashSet<String>>(Default::default);
    let reveal_last_set =
        use_signal::<std::collections::HashSet<String>>(Default::default);
    let threshold_map =
        use_signal::<std::collections::HashMap<String, u32>>(Default::default);
    let busy_set =
        use_signal::<std::collections::HashSet<String>>(Default::default);

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
                        {render_vc_dispatch(
                            network,
                            bridge_state.clone(),
                            vc,
                            badges,
                            refresh_tick,
                            reveal_first_set,
                            reveal_last_set,
                            threshold_map,
                            busy_set,
                        )}
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
///
/// All per-row state lives in the inventory-level signals (see
/// `VcInventorySection`). The helpers below are *pure presentation*
/// — they never call hooks, so the inventory's hook count stays
/// constant across inserts and deletes.
#[allow(clippy::too_many_arguments)]
fn render_vc_dispatch(
    network: Network,
    bridge_state: BridgeState,
    vc: StoredVc,
    badges: Signal<std::collections::HashMap<String, VerifyBadge>>,
    refresh_tick: Signal<u64>,
    reveal_first_set: Signal<std::collections::HashSet<String>>,
    reveal_last_set: Signal<std::collections::HashSet<String>>,
    threshold_map: Signal<std::collections::HashMap<String, u32>>,
    busy_set: Signal<std::collections::HashSet<String>>,
) -> Element {
    if crate::vc_views::digital_passport::is_digital_passport(&vc) {
        render_digital_passport_dispatch(
            network,
            bridge_state,
            vc,
            badges,
            refresh_tick,
            reveal_first_set,
            reveal_last_set,
            threshold_map,
            busy_set,
        )
    } else {
        render_vc_row(
            network,
            bridge_state,
            vc,
            badges,
            refresh_tick,
            busy_set,
        )
    }
}

/// Wire `DigitalPassportCard` to the redb-backed opening store
/// and the existing verify-badge map. Keeps the card itself
/// storage-agnostic — see `vc_views/digital_passport.rs` for the
/// extraction rationale.
#[allow(clippy::too_many_arguments)]
fn render_digital_passport_dispatch(
    network: Network,
    bridge_state: BridgeState,
    vc: StoredVc,
    badges: Signal<std::collections::HashMap<String, VerifyBadge>>,
    refresh_tick: Signal<u64>,
    reveal_first_set: Signal<std::collections::HashSet<String>>,
    reveal_last_set: Signal<std::collections::HashSet<String>>,
    threshold_map: Signal<std::collections::HashMap<String, u32>>,
    busy_set: Signal<std::collections::HashSet<String>>,
) -> Element {
    use crate::vc_views::digital_passport::{
        CLAIM_DATE_OF_BIRTH, CLAIM_FIRST_NAME, CLAIM_LAST_NAME,
        DigitalPassportCard,
    };

    // Open redb once + look up the three known openings. Failures
    // (corrupt file, disk full) degrade to "(no opening stored)"
    // rows inside the card.
    let store_opt = RedbVcStore::open(vc_store_path()).ok();
    let vc_uri = vc.vc_uri.clone();
    let opening_first = store_opt
        .as_ref()
        .and_then(|s| s.get_opening(&vc_uri, CLAIM_FIRST_NAME).ok().flatten());
    let opening_last = store_opt
        .as_ref()
        .and_then(|s| s.get_opening(&vc_uri, CLAIM_LAST_NAME).ok().flatten());
    let opening_dob = store_opt
        .as_ref()
        .and_then(|s| s.get_opening(&vc_uri, CLAIM_DATE_OF_BIRTH).ok().flatten());

    let badge_label = badges
        .read()
        .get(&vc_uri)
        .cloned()
        .map(|b| b.label());

    // Pull per-row toggle state out of the inventory-level maps.
    // Absent keys default to "hidden" / threshold = 18 so a freshly
    // listed VC starts in the privacy-preserving posture.
    let reveal_first = reveal_first_set.read().contains(&vc_uri);
    let reveal_last = reveal_last_set.read().contains(&vc_uri);
    let age_threshold = threshold_map.read().get(&vc_uri).copied().unwrap_or(18);

    // Toggle handlers mutate the inventory maps. EventHandler is a
    // Copy wrapper over an Rc so it's cheap to clone via re-call.
    let on_toggle_first = {
        let vc_uri = vc_uri.clone();
        let mut set = reveal_first_set;
        EventHandler::new(move |_| {
            let mut next = set.read().clone();
            if !next.insert(vc_uri.clone()) {
                next.remove(&vc_uri);
            }
            set.set(next);
        })
    };
    let on_toggle_last = {
        let vc_uri = vc_uri.clone();
        let mut set = reveal_last_set;
        EventHandler::new(move |_| {
            let mut next = set.read().clone();
            if !next.insert(vc_uri.clone()) {
                next.remove(&vc_uri);
            }
            set.set(next);
        })
    };
    let on_threshold_change = {
        let vc_uri = vc_uri.clone();
        let mut map = threshold_map;
        EventHandler::new(move |n: u32| {
            let mut next = map.read().clone();
            next.insert(vc_uri.clone(), n);
            map.set(next);
        })
    };

    // Wire Delete to the local redb store. Local-only — there's
    // no chain-side equivalent in Phase 1; the holder simply
    // forgets the credential.
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

    // Wire Self-verify for digital-passport VCs. Mirrors the same
    // flow used by `render_vc_row` for generic VCs: reads
    // secret store + wallet from bridge state, calls
    // `self_verify_and_cache`, then updates the badge map and
    // refreshes the inventory. The wallet's `js_bridge()` is
    // consulted internally by `self_verify` for compact-VC
    // verification (digital-passport path).
    let on_verify = {
        let bridge_state = bridge_state.clone();
        let vc = vc.clone();
        let vc_uri_for_set = vc_uri.clone();
        let mut badges = badges;
        let mut refresh_tick = refresh_tick;
        let mut busy_set = busy_set;
        EventHandler::new(move |()| {
            if busy_set.read().contains(&vc_uri_for_set) {
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
            {
                let mut next = busy_set.read().clone();
                next.insert(vc_uri_for_set.clone());
                busy_set.set(next);
            }
            let vc = vc.clone();
            let vc_uri = vc_uri_for_set.clone();
            let metrics = bridge_state.metrics_dyn();
            let in_mem_metrics = bridge_state.metrics();
            let probe = bridge_state.resource_probe();
            let action_id = next_action_id();
            let span = tracing::info_span!("ic.self_verify_dp", action_id = %action_id);
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
                        let mut next = busy_set.read().clone();
                        next.remove(&vc_uri);
                        busy_set.set(next);
                        return;
                    }
                };
                let clock = SystemClock;
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
                let mut next_busy = busy_set.read().clone();
                next_busy.remove(&vc_uri);
                busy_set.set(next_busy);
            }.instrument(span));
        })
    };

    let verifying = busy_set.read().contains(&vc_uri);

    DigitalPassportCard(
        vc,
        opening_first,
        opening_last,
        opening_dob,
        reveal_first,
        reveal_last,
        age_threshold,
        on_toggle_first,
        on_toggle_last,
        on_threshold_change,
        badge_label,
        Some(on_verify),
        verifying,
        Some(on_delete),
    )
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
    use wallet_core::VcOpening;

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
        proof: vec![],
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
///
/// **No hooks inside.** The "busy" state for a self-verify in
/// flight is owned by the inventory-level `busy_set` signal so
/// this helper can be invoked from a `for` loop with a variable
/// row count without violating rules of hooks.
fn render_vc_row(
    network: Network,
    bridge_state: BridgeState,
    vc: StoredVc,
    mut badges: Signal<std::collections::HashMap<String, VerifyBadge>>,
    mut refresh_tick: Signal<u64>,
    mut busy_set: Signal<std::collections::HashSet<String>>,
) -> Element {
    let vc_uri = vc.vc_uri.clone();
    let issuer_did = vc.issuer_did.clone();
    let body_len = vc.body.len();
    let busy = busy_set.read().contains(&vc_uri);

    let verify = {
        let bridge_state = bridge_state.clone();
        let vc = vc.clone();
        let vc_uri_for_set = vc_uri.clone();
        move |_| {
            if busy_set.read().contains(&vc_uri_for_set) {
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
            {
                let mut next = busy_set.read().clone();
                next.insert(vc_uri_for_set.clone());
                busy_set.set(next);
            }
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
                        let mut next = busy_set.read().clone();
                        next.remove(&vc_uri);
                        busy_set.set(next);
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
                let mut next_busy = busy_set.read().clone();
                next_busy.remove(&vc_uri);
                busy_set.set(next_busy);
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
                disabled: busy,
                onclick: verify,
                {if busy { "Verifying…" } else { "Self-verify" }}
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
