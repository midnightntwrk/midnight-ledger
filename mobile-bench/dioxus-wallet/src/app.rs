use dioxus::prelude::*;
use wallet_core::{
    ChainTipInfo, IndexerClient, Network, NodeClient, NodeStatus, ProbeResult, Wallet,
    probe_connectivity,
};

use crate::bridge::{BridgeState, run_bridge_loop, spawn_proof_server};

/// CSS is bundled into the binary at compile time via `include_str!` —
/// belt-and-braces vs. the asset! macro, which can drop on certain
/// release-mode bundling paths. The file lives next to `assets/`
/// where Android packaging still finds it.
const STYLES: &str = include_str!("../assets/styles.css");

// `MIDNIGHT_DID_JS` is consumed by `lib.rs::desktop_or_mobile_launch`
// via `with_custom_head` so the bundle runs at page-parse time. We
// keep the include_str! reference in lib.rs only; importing it here
// would be unused.

#[derive(Clone, PartialEq, Eq)]
struct WalletInfo {
    seed_hex: String,
    coin_pk_hex: String,
    enc_pk_hex: String,
    address: String,
    network: Network,
}

impl WalletInfo {
    fn from_wallet(w: &Wallet) -> Self {
        Self {
            seed_hex: w.seed_hex(),
            coin_pk_hex: w.coin_public_key_hex().unwrap_or_else(|e| e.to_string()),
            enc_pk_hex: w
                .encryption_public_key_hex()
                .unwrap_or_else(|e| e.to_string()),
            address: w
                .unshielded_address()
                .unwrap_or_else(|e| format!("(address error: {e})")),
            network: w.network(),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
enum SyncPhase {
    /// Default — neither connect attempted nor done.
    Idle,
    /// Probe in flight or queries pending.
    Connecting,
    /// All probes green and chain queries returned.
    Synced,
    /// Probe failed or query errored.
    Stalled(String),
}

#[derive(Clone, Default, PartialEq, Eq)]
struct ChainSnapshot {
    tip: Option<ChainTipInfo>,
    node: Option<NodeStatus>,
    last_error: Option<String>,
}

#[component]
pub fn App() -> Element {
    let mut network = use_signal(|| Network::PreProd);
    let mut wallet = use_signal::<Option<WalletInfo>>(|| {
        Some(WalletInfo::from_wallet(&Wallet::demo(Network::PreProd)))
    });
    let mut phase = use_signal(|| SyncPhase::Idle);
    let mut chain = use_signal::<ChainSnapshot>(ChainSnapshot::default);
    let mut probe = use_signal::<Option<ProbeResult>>(|| None);

    // ── JS bridge + embedded proof-server ─────────────────────────
    // BridgeState is cheap-clone (Arc<OnceCell<String>>); we keep a
    // copy in a signal for UI display and pass another into the
    // background spawn / bridge loop.
    let bridge_state = use_signal(BridgeState::new);
    let mut proof_server = use_signal::<Option<String>>(|| None);

    use_future(move || {
        let state = bridge_state.read().clone();
        async move {
            match spawn_proof_server(&state).await {
                Ok(url) => proof_server.set(Some(url)),
                Err(e) => tracing::warn!(error=%e, "embedded proof-server unavailable"),
            }
        }
    });

    use_future(move || {
        let state = bridge_state.read().clone();
        let seed_hex = wallet
            .read()
            .as_ref()
            .map(|w| w.seed_hex.clone())
            .unwrap_or_default();
        async move {
            run_bridge_loop(state, seed_hex).await;
        }
    });

    let mut load_demo = move || {
        let w = Wallet::demo(*network.read());
        wallet.set(Some(WalletInfo::from_wallet(&w)));
    };
    let mut generate = move || {
        let w = Wallet::new_random(*network.read());
        wallet.set(Some(WalletInfo::from_wallet(&w)));
    };

    let mut connect = move || {
        if matches!(*phase.read(), SyncPhase::Connecting) {
            return;
        }
        let net = *network.read();
        phase.set(SyncPhase::Connecting);
        chain.set(ChainSnapshot::default());

        spawn(async move {
            let probe_result = probe_connectivity(net).await;
            let probe_ok = probe_result.all_reachable();
            probe.set(Some(probe_result.clone()));
            if !probe_ok {
                let reasons = [&probe_result.indexer_http, &probe_result.indexer_ws, &probe_result.node_ws]
                    .iter()
                    .filter_map(|s| (!s.reachable).then(|| s.detail.clone().unwrap_or_default()))
                    .collect::<Vec<_>>()
                    .join("; ");
                phase.set(SyncPhase::Stalled(format!("endpoint unreachable: {reasons}")));
                return;
            }

            let tip_fut = async {
                IndexerClient::new(net)
                    .map_err(|e| e.to_string())?
                    .chain_tip()
                    .await
                    .map_err(|e| e.to_string())
            };
            let node_fut = async {
                NodeClient::connect(net)
                    .await
                    .map_err(|e| e.to_string())?
                    .status()
                    .await
                    .map_err(|e| e.to_string())
            };
            let (tip, node) = tokio::join!(tip_fut, node_fut);

            let mut snapshot = ChainSnapshot::default();
            let mut errs: Vec<String> = Vec::new();
            match tip {
                Ok(Some(t)) => snapshot.tip = Some(t),
                Ok(None) => errs.push("indexer: no blocks".into()),
                Err(e) => errs.push(format!("indexer: {e}")),
            }
            match node {
                Ok(s) => snapshot.node = Some(s),
                Err(e) => errs.push(format!("node: {e}")),
            }
            if !errs.is_empty() {
                snapshot.last_error = Some(errs.join("; "));
            }
            chain.set(snapshot.clone());

            phase.set(if errs.is_empty() {
                SyncPhase::Synced
            } else {
                SyncPhase::Stalled(errs.join("; "))
            });
        });
    };

    let busy = matches!(*phase.read(), SyncPhase::Connecting);

    rsx! {
        style { "{STYLES}" }
        // The midnight-did bundle is injected via `with_custom_head`
        // (see lib.rs::desktop_or_mobile_launch) so it runs at
        // page-parse time and ahead of the bridge JS shim.

        div { class: "header",
            h1 { "Midnight Wallet" }
            button { class: "menu-btn", title: "Advanced", "≡" }
        }

        StatusLine {
            phase: phase.read().clone(),
            network: *network.read(),
            tip_height: chain.read().tip.as_ref().map(|t| t.height),
        }

        if let Some(w) = wallet.read().as_ref() {
            AddressCard { address: w.address.clone() }
        }

        BalancesCard {
            connected: matches!(*phase.read(), SyncPhase::Synced),
        }

        button {
            class: "cta",
            disabled: busy,
            onclick: move |_| connect(),
            {match &*phase.read() {
                SyncPhase::Idle => "Connect".to_string(),
                SyncPhase::Connecting => "Connecting…".to_string(),
                SyncPhase::Synced => "Reconnect".to_string(),
                SyncPhase::Stalled(_) => "Retry".to_string(),
            }}
        }

        div { class: "row",
            div { class: "label", "Network" }
            select {
                onchange: move |e| {
                    if let Some(n) = parse_network(&e.value()) {
                        network.set(n);
                        chain.set(ChainSnapshot::default());
                        phase.set(SyncPhase::Idle);
                        // Demo wallets are network-aware: PreProd
                        // / mainnet / etc. share DEMO_SEED_HEX, but
                        // Undeployed uses UNDEPLOYED_GENESIS_SEED_HEX
                        // (the prefunded standalone genesis). If the
                        // user has either of those loaded, refresh
                        // to the right one for the new network. A
                        // user-generated random wallet stays put.
                        let was_demo = wallet
                            .read()
                            .as_ref()
                            .map(|w| {
                                w.seed_hex == wallet_core::DEMO_SEED_HEX
                                    || w.seed_hex
                                        == wallet_core::UNDEPLOYED_GENESIS_SEED_HEX
                            })
                            .unwrap_or(false);
                        if was_demo {
                            wallet.set(Some(WalletInfo::from_wallet(&Wallet::demo(n))));
                        }
                    }
                },
                for n in Network::ALL {
                    option {
                        value: "{network_value(n)}",
                        selected: *network.read() == n,
                        "{n.label()}"
                    }
                }
            }
        }

        details {
            summary { "Advanced" }
            div { class: "panel",
                if let Some(w) = wallet.read().as_ref() {
                    div { class: "row", "Seed (hex):" }
                    div { class: "seed-blob", "{w.seed_hex}" }
                    div { class: "row", "Coin PK:" }
                    div { class: "seed-blob", "{w.coin_pk_hex}" }
                    div { class: "row", "Encryption PK:" }
                    div { class: "seed-blob", "{w.enc_pk_hex}" }
                }
                div { class: "row",
                    button { onclick: move |_| load_demo(), "Reload demo" }
                    button { onclick: move |_| generate(), "Random wallet" }
                }
                if let Some(p) = probe.read().as_ref() {
                    div { class: "row", "Last probe — {p.network.label()}" }
                    ProbeRowCompact { name: "indexer http", url: p.indexer_http.url.clone(), reachable: p.indexer_http.reachable, latency: p.indexer_http.latency_ms, detail: p.indexer_http.detail.clone() }
                    ProbeRowCompact { name: "indexer ws",   url: p.indexer_ws.url.clone(),   reachable: p.indexer_ws.reachable,   latency: p.indexer_ws.latency_ms,   detail: p.indexer_ws.detail.clone() }
                    ProbeRowCompact { name: "node ws",      url: p.node_ws.url.clone(),      reachable: p.node_ws.reachable,      latency: p.node_ws.latency_ms,      detail: p.node_ws.detail.clone() }
                }
                if let Some(s) = chain.read().node.as_ref() {
                    div { class: "row", "Node finalized head:" }
                    div { class: "seed-blob", "{s.finalized_head_hash}" }
                }
                if let Some(url) = proof_server.read().as_ref() {
                    div { class: "row", "Embedded proof-server:" }
                    div { class: "seed-blob", "{url}" }
                }

                ResolveDidPanel { network: *network.read() }
                CreateDidPanel { network: *network.read() }
            }
        }
    }
}

#[component]
fn CreateDidPanel(network: Network) -> Element {
    let mut result = use_signal::<Option<Result<String, String>>>(|| None);
    let mut pending = use_signal(|| false);

    let create = move |_| {
        if *pending.read() {
            return;
        }
        pending.set(true);
        result.set(None);
        spawn(async move {
            let r = match Ok::<_, wallet_core::WalletError>(Wallet::demo(network)) {
                Ok(w) => match w.create_did().await {
                    Ok(id) => Ok(id.to_did_string()),
                    Err(e) => {
                        // Surface the controller pubkey alongside the
                        // error so the panel doubles as a diagnostic
                        // for the key-derivation half (which DOES work
                        // today; only the deploy/submit half is stubbed).
                        let pk = w
                            .did_controller_public_key()
                            .map(|b| hex::encode(b))
                            .unwrap_or_else(|e| format!("(err: {e})"));
                        Err(format!(
                            "{e}\n\ncontrollerPublicKey would be: {pk}"
                        ))
                    }
                },
                Err(e) => Err(e.to_string()),
            };
            result.set(Some(r));
            pending.set(false);
        });
    };

    rsx! {
        div { class: "row", "Create DID" }
        div { class: "row",
            button {
                disabled: *pending.read(),
                onclick: create,
                {if *pending.read() { "Creating…" } else { "Create DID (Phase 3 stub)" }}
            }
        }
        if let Some(res) = result.read().as_ref() {
            match res {
                Ok(did) => rsx! { div { class: "seed-blob", "{did}" } },
                Err(e) => rsx! { div { class: "seed-blob", style: "color: var(--error);", "{e}" } },
            }
        }
    }
}

#[component]
fn ResolveDidPanel(network: Network) -> Element {
    let mut input = use_signal(String::new);
    let mut result = use_signal::<Option<Result<String, String>>>(|| None);
    let mut pending = use_signal(|| false);

    let resolve = move |_| {
        if *pending.read() {
            return;
        }
        let did_str = input.read().clone();
        if did_str.is_empty() {
            result.set(Some(Err("enter a did:midnight:... string".into())));
            return;
        }
        pending.set(true);
        result.set(None);
        spawn(async move {
            let r = match Ok::<_, wallet_core::WalletError>(Wallet::demo(network)) {
                Ok(w) => match w.resolve_did(&did_str).await {
                    Ok(doc) => match serde_json::to_string_pretty(&doc) {
                        Ok(s) => Ok(s),
                        Err(e) => Err(format!("serialise: {e}")),
                    },
                    Err(e) => Err(e.to_string()),
                },
                Err(e) => Err(e.to_string()),
            };
            result.set(Some(r));
            pending.set(false);
        });
    };

    rsx! {
        div { class: "row", "Resolve DID" }
        div { class: "row",
            input {
                r#type: "text",
                placeholder: "did:midnight:preprod:…",
                value: "{input.read()}",
                oninput: move |e| input.set(e.value()),
                style: "flex: 1; padding: 6px 8px; background: var(--surface-2); color: var(--text); border: 1px solid var(--border); border-radius: 6px; font-family: ui-monospace, monospace; font-size: 11px;"
            }
            button {
                disabled: *pending.read(),
                onclick: resolve,
                {if *pending.read() { "…" } else { "Resolve" }}
            }
        }
        if let Some(res) = result.read().as_ref() {
            match res {
                Ok(json) => rsx! { div { class: "seed-blob", "{json}" } },
                Err(e) => rsx! { div { class: "seed-blob", style: "color: var(--error);", "{e}" } },
            }
        }
    }
}

#[component]
fn StatusLine(phase: SyncPhase, network: Network, tip_height: Option<i64>) -> Element {
    let (dot_class, label): (&'static str, String) = match phase {
        SyncPhase::Idle => ("muted", format!("{} · disconnected", network.label())),
        SyncPhase::Connecting => ("warn", format!("{} · connecting…", network.label())),
        SyncPhase::Synced => match tip_height {
            Some(h) => ("success", format!("{} · synced · block {}", network.label(), format_int(h))),
            None => ("success", format!("{} · synced", network.label())),
        },
        SyncPhase::Stalled(reason) => ("error", format!("{} · stalled · {reason}", network.label())),
    };
    rsx! {
        div { class: "status-line",
            span { class: "dot {dot_class}" }
            span { "{label}" }
        }
    }
}

#[component]
fn AddressCard(address: String) -> Element {
    let mut copied = use_signal(|| false);
    rsx! {
        div { class: "card",
            div { class: "card-header", "Address (NIGHT receive)" }
            div { class: "address-block",
                div { class: "text", "{address}" }
                button {
                    class: if *copied.read() { "copy-btn copied" } else { "copy-btn" },
                    title: "Copy address",
                    onclick: {
                        let address = address.clone();
                        move |_| {
                            let _ = copy_to_clipboard(&address);
                            copied.set(true);
                        }
                    },
                    {if *copied.read() { "✓" } else { "⧉" }}
                }
            }
        }
    }
}

#[component]
fn BalancesCard(connected: bool) -> Element {
    rsx! {
        div { class: "card",
            div { class: "card-header", "Balances" }
            div { class: "balance-row",
                span { class: "label", "NIGHT" }
                span { class: "value",
                    {if connected { "0.000 000" } else { "—" }}
                    span { class: "unit", " NIGHT" }
                }
            }
            div { class: "balance-row",
                span { class: "label", "Dust" }
                span { class: "value",
                    {if connected { "0.000 000" } else { "—" }}
                    span { class: "unit", " DUST" }
                }
            }
            // Hint row reminds the user how to make funds appear. Replaced with the
            // `dust-progress` bar in Phase B once a registered NIGHT UTXO exists.
            div { class: "balance-row",
                span { class: "hint",
                    {if connected {
                        "Send NIGHT to the address above. Register UTXOs to start accruing Dust."
                    } else {
                        "Connect to the network to see live balances."
                    }}
                }
            }
        }
    }
}

#[component]
fn ProbeRowCompact(
    name: String,
    url: String,
    reachable: bool,
    latency: u128,
    detail: Option<String>,
) -> Element {
    rsx! {
        div { class: "probe",
            div { class: if reachable { "ok" } else { "bad" }, "{name}" }
            div { class: "url", "{url}" }
            div { class: "latency", "{latency} ms" }
            if let Some(d) = detail {
                if !reachable {
                    div { class: "detail", "{d}" }
                }
            }
        }
    }
}

fn format_int(n: i64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(' ');
        }
        out.push(c);
    }
    out.chars().rev().collect()
}

/// Cross-platform clipboard write. Desktop uses `arboard`; Android
/// wires up `ClipboardManager` via JNI in Phase D.
#[cfg(not(target_os = "android"))]
fn copy_to_clipboard(s: &str) -> Result<(), String> {
    let mut cb = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    cb.set_text(s.to_string()).map_err(|e| e.to_string())
}

#[cfg(target_os = "android")]
fn copy_to_clipboard(_s: &str) -> Result<(), String> {
    Ok(())
}

fn network_value(n: Network) -> &'static str {
    match n {
        Network::Mainnet => "mainnet",
        Network::PreProd => "preprod",
        Network::Preview => "preview",
        Network::QaNet => "qanet",
        Network::DevNet => "devnet",
        Network::Undeployed => "undeployed",
    }
}

fn parse_network(s: &str) -> Option<Network> {
    match s {
        "mainnet" => Some(Network::Mainnet),
        "preprod" => Some(Network::PreProd),
        "preview" => Some(Network::Preview),
        "qanet" => Some(Network::QaNet),
        "devnet" => Some(Network::DevNet),
        "undeployed" => Some(Network::Undeployed),
        _ => None,
    }
}
