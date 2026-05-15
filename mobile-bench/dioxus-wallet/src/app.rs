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

/// Top-level tabs. Wallet shows identity + balance; DIDs holds the
/// create/resolve/load flow plus session activity; Diagnostics
/// surfaces probes + proof-server URL + raw seed/keys for power
/// users.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Wallet,
    Dids,
    Diagnostics,
}

impl Tab {
    fn label(&self) -> &'static str {
        match self {
            Tab::Wallet => "Wallet",
            Tab::Dids => "DIDs",
            Tab::Diagnostics => "Diagnostics",
        }
    }
}

/// One entry in the in-memory session activity log. Sized for the
/// log panel — we keep just the fields a user would want to see at
/// a glance plus copy-paste-able hashes.
#[derive(Clone, PartialEq, Eq)]
enum SessionEvent {
    Deploy {
        did: String,
        tx_hash: [u8; 32],
        block_hash: [u8; 32],
    },
    Resolve {
        did: String,
        counter: u32,
    },
    LoadCircuit {
        did: String,
        circuit: String,
        tx_hash: [u8; 32],
        block_hash: [u8; 32],
    },
    /// A DID circuit invocation the user prepared in the UI. The
    /// wallet does NOT submit these yet — see DidOperationsPanel
    /// for the local-only flow. Once the Compact-runtime bridge
    /// lands, the same operation will be turned into a real
    /// `ContractCall` transaction.
    OperationDrafted {
        did: String,
        operation: DidOperation,
    },
}

/// One DID circuit invocation, drafted in the UI. Shape mirrors
/// the corresponding Compact circuit in
/// `mobile-bench/wallet-core/contracts/midnight-did/did.compact`.
#[derive(Clone, PartialEq, Eq)]
enum DidOperation {
    AddAlsoKnownAs { value: String },
    RemoveAlsoKnownAs { value: String },
    AddVerificationMethod(VerificationMethodInput),
    UpdateVerificationMethod(VerificationMethodInput),
    RemoveVerificationMethod { id: String },
    AddVerificationMethodRelation { relation: String, method_id: String },
    RemoveVerificationMethodRelation { relation: String, method_id: String },
    AddService(ServiceInput),
    UpdateService(ServiceInput),
    RemoveService { id: String },
    Deactivate,
}

impl DidOperation {
    fn circuit(&self) -> &'static str {
        match self {
            Self::AddAlsoKnownAs { .. } => "addAlsoKnownAs",
            Self::RemoveAlsoKnownAs { .. } => "removeAlsoKnownAs",
            Self::AddVerificationMethod(_) => "addVerificationMethod",
            Self::UpdateVerificationMethod(_) => "updateVerificationMethod",
            Self::RemoveVerificationMethod { .. } => "removeVerificationMethod",
            Self::AddVerificationMethodRelation { .. } => "addVerificationMethodRelation",
            Self::RemoveVerificationMethodRelation { .. } => "removeVerificationMethodRelation",
            Self::AddService(_) => "addService",
            Self::UpdateService(_) => "updateService",
            Self::RemoveService { .. } => "removeService",
            Self::Deactivate => "deactivate",
        }
    }

    /// Single-line human-readable summary for the session log.
    fn summary(&self) -> String {
        match self {
            Self::AddAlsoKnownAs { value } | Self::RemoveAlsoKnownAs { value } => {
                format!("value: {value}")
            }
            Self::AddVerificationMethod(vm) | Self::UpdateVerificationMethod(vm) => {
                format!("id: {} · {}/{}", vm.id, vm.key_type, vm.curve)
            }
            Self::RemoveVerificationMethod { id } | Self::RemoveService { id } => {
                format!("id: {id}")
            }
            Self::AddVerificationMethodRelation { relation, method_id }
            | Self::RemoveVerificationMethodRelation { relation, method_id } => {
                format!("{relation} ← {method_id}")
            }
            Self::AddService(s) | Self::UpdateService(s) => {
                format!("id: {} · {} → {}", s.id, s.typ, s.endpoint)
            }
            Self::Deactivate => "—".to_string(),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
struct VerificationMethodInput {
    id: String,
    key_type: String,
    curve: String,
    pk_x: String,
    pk_y: String,
}

#[derive(Clone, PartialEq, Eq, Debug)]
struct ServiceInput {
    id: String,
    typ: String,
    endpoint: String,
}

/// Timing snapshot for one completed pipeline run. Built by the
/// receiver side: each `WizardStage` arrival timestamps with
/// `Instant::now()`; durations are the deltas between consecutive
/// timestamps + the implicit "start → first stage" leg.
#[derive(Clone, PartialEq, Eq)]
struct TimingRun {
    /// "create_did" or "load_did_circuit:<circuit>" — what the
    /// pipeline was doing.
    label: String,
    /// Per-stage duration in milliseconds. Indexed by `PIPELINE`
    /// order; entries past the last reached stage are left at 0.
    per_stage_ms: [u64; 6],
    /// End-to-end duration from spawn to terminal (Done or Failed).
    total_ms: u64,
    /// Whether the run ended in Done (true) or Failed (false).
    succeeded: bool,
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
    // Latest NIGHT subunit total from `Wallet::sync_unshielded()`.
    // None = never synced or sync in flight; Some(0) = synced, no
    // funds. The `unshielded_balance` future kicks off after a
    // successful Connect (see below).
    let mut night_subunits = use_signal::<Option<u128>>(|| None);
    // Last DID id this session deployed via CreateDidWizard.
    // ResolveDidPanel pre-populates its input from this so the
    // user can immediately verify their freshly-created DID.
    let mut last_did_id = use_signal::<Option<String>>(|| None);
    // Last `(did, maintenance_counter)` ResolveDidPanel surfaced.
    // LoadCircuitPanel consumes this to pre-fill its counter input
    // so the user doesn't have to track the counter manually
    // between maintenance updates.
    let mut last_resolved = use_signal::<Option<(String, u32)>>(|| None);
    // Top-of-page tab selection. Default to Wallet so first-time
    // users see the address + balance immediately.
    let mut active_tab = use_signal(|| Tab::Wallet);
    // Chronological log of session-scoped events: each deploy,
    // resolve, and circuit load gets one entry. Persisted in
    // memory only; cleared when the user reloads the page.
    let mut session_log = use_signal::<Vec<SessionEvent>>(Vec::new);
    // Per-pipeline timing snapshots, newest last. Shown in the
    // Diagnostics tab as a stacked bar / breakdown per run.
    let mut timing_log = use_signal::<Vec<TimingRun>>(Vec::new);

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
        night_subunits.set(None);

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

            // After a successful Connect, snapshot the unshielded
            // UTXO set so BalancesCard can render the real NIGHT
            // total. We deliberately use `Wallet::demo(net)` to
            // match the seed shown in the address card; the
            // generate flow currently shares the same demo path.
            // A snapshot failure is non-fatal — the UI stays on
            // "—" rather than reverting the Synced phase.
            if errs.is_empty() {
                let w = Wallet::demo(net);
                match w.sync_unshielded().await {
                    Ok(set) => {
                        // NIGHT's raw 64-char token type is 32 zero bytes
                        // (per the example-counter MIGRATION_GUIDE — the
                        // v4 `nativeToken()` tagged form would silently
                        // miss the balance).
                        let night = set.total_for(
                            &wallet_core::TokenType(vec![0u8; 32]),
                        );
                        night_subunits.set(Some(night));
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "unshielded snapshot failed");
                    }
                }
            }
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

        // Tab navigation. Each button sets active_tab; rendering
        // below is a single match on the current value.
        div { class: "tab-nav",
            for t in [Tab::Wallet, Tab::Dids, Tab::Diagnostics] {
                button {
                    class: if *active_tab.read() == t { "tab-btn active" } else { "tab-btn" },
                    onclick: move |_| active_tab.set(t),
                    "{t.label()}"
                }
            }
        }

        match *active_tab.read() {
            Tab::Wallet => rsx! {
                if let Some(w) = wallet.read().as_ref() {
                    AddressCard { address: w.address.clone() }
                }

                BalancesCard {
                    connected: matches!(*phase.read(), SyncPhase::Synced),
                    night_subunits: *night_subunits.read(),
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

                div { class: "row",
                    button { onclick: move |_| load_demo(), "Reload demo" }
                    button { onclick: move |_| generate(), "Random wallet" }
                }

                BalancePanel { network: *network.read() }
            },
            Tab::Dids => rsx! {
                CreateDidWizard {
                    network: *network.read(),
                    on_done: move |o: wallet_core::DeployOutcome| {
                        let did = o.did_id.to_did_string();
                        last_did_id.set(Some(did.clone()));
                        let mut log = session_log.read().clone();
                        log.push(SessionEvent::Deploy {
                            did,
                            tx_hash: o.tx_hash,
                            block_hash: o.block_hash,
                        });
                        session_log.set(log);
                    },
                    on_timing: move |run: TimingRun| {
                        let mut log = timing_log.read().clone();
                        log.push(run);
                        timing_log.set(log);
                    },
                }
                ResolveDidPanel {
                    network: *network.read(),
                    seed_did: last_did_id.read().clone(),
                    on_resolved: move |(did, counter): (String, u32)| {
                        last_resolved.set(Some((did.clone(), counter)));
                        let mut log = session_log.read().clone();
                        log.push(SessionEvent::Resolve { did, counter });
                        session_log.set(log);
                    },
                }
                LoadCircuitPanel {
                    network: *network.read(),
                    seed_did: last_did_id.read().clone(),
                    seed_counter: last_resolved.read().as_ref().map(|(_, c)| *c),
                    on_done: move |(did, circuit, o): (String, String, wallet_core::DeployOutcome)| {
                        let mut log = session_log.read().clone();
                        log.push(SessionEvent::LoadCircuit {
                            did,
                            circuit,
                            tx_hash: o.tx_hash,
                            block_hash: o.block_hash,
                        });
                        session_log.set(log);
                    },
                    on_timing: move |run: TimingRun| {
                        let mut log = timing_log.read().clone();
                        log.push(run);
                        timing_log.set(log);
                    },
                }
                DidOperationsPanel {
                    seed_did: last_did_id.read().clone(),
                    on_drafted: move |(did, op): (String, DidOperation)| {
                        let mut log = session_log.read().clone();
                        log.push(SessionEvent::OperationDrafted { did, operation: op });
                        session_log.set(log);
                    },
                }
                SessionLogPanel { events: session_log.read().clone() }
            },
            Tab::Diagnostics => rsx! {
                JsBridgePanel {}
                TimingsPanel { runs: timing_log.read().clone() }
                if let Some(w) = wallet.read().as_ref() {
                    div { class: "row", "Seed (hex):" }
                    div { class: "seed-blob", "{w.seed_hex}" }
                    div { class: "row", "Coin PK:" }
                    div { class: "seed-blob", "{w.coin_pk_hex}" }
                    div { class: "row", "Encryption PK:" }
                    div { class: "seed-blob", "{w.enc_pk_hex}" }
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
            },
        }
    }
}

/// Fixed pipeline order — used to render a checklist with one row
/// per stage. Done/Failed sit outside this list as terminal states.
const PIPELINE: &[&str] = &[
    "Syncing DUST",
    "Composing",
    "Balancing fees",
    "Proving",
    "Submitting",
    "Confirming inclusion",
];

/// State of a single pipeline row at a given moment.
#[derive(Clone, Copy, PartialEq, Eq)]
enum StepStatus {
    Pending,
    Active,
    Done,
    /// Reached after a Failed terminal — show the step that was in
    /// flight as the failure point, others stay Pending.
    FailedHere,
}

/// Map an index in `PIPELINE` to its current status given the
/// stream's last seen stage and any terminal state.
fn step_status(idx: usize, stages: &[wallet_core::WizardStage]) -> StepStatus {
    use wallet_core::WizardStage as W;

    // Project each WizardStage to a pipeline index, or to a terminal.
    let mut last_pipeline_idx: Option<usize> = None;
    let mut terminal_done = false;
    let mut terminal_failed_at: Option<usize> = None;
    for stage in stages {
        match stage {
            W::SyncingDust => last_pipeline_idx = Some(0),
            W::Composing => last_pipeline_idx = Some(1),
            W::Balancing => last_pipeline_idx = Some(2),
            W::Proving => last_pipeline_idx = Some(3),
            W::Submitting => last_pipeline_idx = Some(4),
            W::Confirming => last_pipeline_idx = Some(5),
            W::Done(_) => {
                terminal_done = true;
                last_pipeline_idx = Some(PIPELINE.len() - 1);
            }
            W::Failed(_) => {
                terminal_failed_at = last_pipeline_idx;
            }
        }
    }

    if terminal_done {
        return StepStatus::Done;
    }
    if let Some(failed_at) = terminal_failed_at {
        if idx == failed_at {
            return StepStatus::FailedHere;
        }
        if idx < failed_at {
            return StepStatus::Done;
        }
        return StepStatus::Pending;
    }
    match last_pipeline_idx {
        None => StepStatus::Pending,
        Some(cur) if idx < cur => StepStatus::Done,
        Some(cur) if idx == cur => StepStatus::Active,
        _ => StepStatus::Pending,
    }
}

/// Map a `WizardStage` to its 0-based slot in `PIPELINE`, or
/// `None` for terminal stages (Done / Failed).
fn stage_pipeline_idx(s: &wallet_core::WizardStage) -> Option<usize> {
    use wallet_core::WizardStage as W;
    Some(match s {
        W::SyncingDust => 0,
        W::Composing => 1,
        W::Balancing => 2,
        W::Proving => 3,
        W::Submitting => 4,
        W::Confirming => 5,
        W::Done(_) | W::Failed(_) => return None,
    })
}

/// Compute a `TimingRun` from a sequence of `(stage_idx, arrival_time)`
/// observations plus the terminal timestamp. Each stage's duration is
/// "next arrival - own arrival"; the last reached stage uses `t_end`
/// as its "next". Stages never reached stay at 0 ms.
fn build_timing(
    label: String,
    observations: &[(usize, std::time::Instant)],
    t_end: std::time::Instant,
    succeeded: bool,
) -> TimingRun {
    let mut per_stage_ms = [0u64; 6];
    for win in observations.windows(2) {
        let (i0, t0) = win[0];
        let (_, t1) = win[1];
        per_stage_ms[i0] = t1.saturating_duration_since(t0).as_millis() as u64;
    }
    if let Some(&(last_idx, last_t)) = observations.last() {
        per_stage_ms[last_idx] = t_end.saturating_duration_since(last_t).as_millis() as u64;
    }
    let total_ms = observations
        .first()
        .map(|&(_, t0)| t_end.saturating_duration_since(t0).as_millis() as u64)
        .unwrap_or(0);
    TimingRun {
        label,
        per_stage_ms,
        total_ms,
        succeeded,
    }
}

/// Final outcome from the stages stream, if any.
fn terminal(stages: &[wallet_core::WizardStage]) -> Option<TerminalView<'_>> {
    use wallet_core::WizardStage as W;
    stages.iter().rev().find_map(|s| match s {
        W::Done(o) => Some(TerminalView::Done(o)),
        W::Failed(msg) => Some(TerminalView::Failed(msg.as_str())),
        _ => None,
    })
}

enum TerminalView<'a> {
    Done(&'a wallet_core::DeployOutcome),
    Failed(&'a str),
}

#[component]
fn CreateDidWizard(
    network: Network,
    on_done: EventHandler<wallet_core::DeployOutcome>,
    on_timing: EventHandler<TimingRun>,
) -> Element {
    use wallet_core::WizardStage;

    let mut stages = use_signal::<Vec<WizardStage>>(Vec::new);
    let mut running = use_signal(|| false);

    let start = move |_| {
        if *running.read() {
            return;
        }
        running.set(true);
        stages.set(Vec::new());
        let on_done = on_done.clone();
        let on_timing = on_timing.clone();
        spawn(async move {
            use futures::StreamExt;
            let w = Wallet::demo(network);
            let mut stream = std::pin::pin!(w.create_did());
            let mut observations: Vec<(usize, std::time::Instant)> = Vec::new();
            while let Some(stage) = stream.next().await {
                let now = std::time::Instant::now();
                if let Some(idx) = stage_pipeline_idx(&stage) {
                    observations.push((idx, now));
                } else {
                    let succeeded = matches!(&stage, WizardStage::Done(_));
                    on_timing.call(build_timing(
                        "create_did".to_string(),
                        &observations,
                        now,
                        succeeded,
                    ));
                }
                let mut current = stages.read().clone();
                if let WizardStage::Done(o) = &stage {
                    on_done.call(o.clone());
                }
                current.push(stage);
                stages.set(current);
            }
            running.set(false);
        });
    };

    let stages_snapshot = stages.read().clone();
    let term = terminal(&stages_snapshot);
    let has_started = !stages_snapshot.is_empty();
    let button_label = match (*running.read(), &term) {
        (true, _) => "Submitting…",
        (false, Some(TerminalView::Failed(_))) => "Retry",
        (false, Some(TerminalView::Done(_))) => "Create another",
        (false, None) => "Create DID",
    };

    rsx! {
        div { class: "wizard-header", "Create DID" }
        div { class: "row",
            button {
                disabled: *running.read(),
                onclick: start,
                "{button_label}"
            }
        }

        if has_started {
            ul { class: "wizard-steps",
                for (idx , label) in PIPELINE.iter().enumerate() {
                    {render_step_row(idx, label, step_status(idx, &stages_snapshot))}
                }
            }
        }

        if let Some(TerminalView::Done(outcome)) = &term {
            div { class: "wizard-outcome ok",
                div { class: "row label", "DID" }
                div { class: "seed-blob", "{outcome.did_id.to_did_string()}" }
                div { class: "row label", "Tx hash" }
                div { class: "seed-blob", "0x{hex::encode(outcome.tx_hash)}" }
                div { class: "row label", "Block hash" }
                div { class: "seed-blob", "0x{hex::encode(outcome.block_hash)}" }
            }
        } else if let Some(TerminalView::Failed(msg)) = &term {
            div { class: "wizard-outcome err",
                div { class: "row label", "Failed" }
                div { class: "seed-blob", "{msg}" }
            }
        }
    }
}

fn render_step_row(_idx: usize, label: &str, status: StepStatus) -> Element {
    let (glyph, class) = match status {
        StepStatus::Pending => ("○", "wizard-step pending"),
        StepStatus::Active => ("●", "wizard-step active"),
        StepStatus::Done => ("✓", "wizard-step done"),
        StepStatus::FailedHere => ("✗", "wizard-step failed"),
    };
    rsx! {
        li { class: "{class}",
            span { class: "wizard-glyph", "{glyph}" }
            span { class: "wizard-label", "{label}" }
            if matches!(status, StepStatus::Active) {
                span { class: "wizard-active-tag", "…" }
            }
        }
    }
}

#[component]
fn BalancePanel(network: Network) -> Element {
    let mut result = use_signal::<Option<Result<String, String>>>(|| None);
    let mut pending = use_signal(|| false);

    let sync = move |_| {
        if *pending.read() {
            return;
        }
        pending.set(true);
        result.set(None);
        spawn(async move {
            let w = Wallet::demo(network);
            let r = match w.sync_unshielded().await {
                Ok(set) => {
                    let mut lines = Vec::new();
                    lines.push(format!("utxos: {}", set.len()));
                    for (token, value) in set.balance_by_token() {
                        lines.push(format!("  {}: {}", hex::encode(&token.0), value));
                    }
                    Ok(lines.join("\n"))
                }
                Err(e) => Err(e.to_string()),
            };
            result.set(Some(r));
            pending.set(false);
        });
    };

    rsx! {
        div { class: "row", "Balance" }
        div { class: "row",
            button {
                disabled: *pending.read(),
                onclick: sync,
                {if *pending.read() { "Syncing…" } else { "Sync balance" }}
            }
        }
        if let Some(res) = result.read().as_ref() {
            match res {
                Ok(text) => rsx! { div { class: "seed-blob", "{text}" } },
                Err(e) => rsx! { div { class: "seed-blob", style: "color: var(--error);", "{e}" } },
            }
        }
    }
}

/// Successful resolve outcome — what the ResolveDidPanel displays
/// after a chain round-trip. The document JSON is computed lazily
/// for the toggle so we don't burn cycles rendering it when collapsed.
#[derive(Clone)]
struct ResolveView {
    counter: u32,
    last_block_height: Option<i64>,
    last_tx_hash: String,
    deactivated: bool,
    vm_count: usize,
    service_count: usize,
    document_json: String,
}

#[component]
fn ResolveDidPanel(
    network: Network,
    seed_did: Option<String>,
    on_resolved: EventHandler<(String, u32)>,
) -> Element {
    let mut input = use_signal(|| seed_did.clone().unwrap_or_default());
    // Re-seed the input whenever a new `seed_did` arrives — e.g.
    // the wizard just deployed a fresh DID. We only OVERWRITE
    // when the seed actually changes to avoid clobbering the
    // user's manual typing.
    use_effect(move || {
        if let Some(seed) = seed_did.clone() {
            if *input.read() != seed {
                input.set(seed);
            }
        }
    });
    let mut result = use_signal::<Option<Result<ResolveView, String>>>(|| None);
    let mut pending = use_signal(|| false);
    let mut show_json = use_signal(|| false);

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
        let on_resolved = on_resolved.clone();
        spawn(async move {
            let w = Wallet::demo(network);
            let r: Result<ResolveView, String> = match w.resolve_did_full(&did_str).await {
                Ok(resolved) => {
                    let did_string = resolved.document.id.to_did_string();
                    let json = serde_json::to_string_pretty(&resolved.document)
                        .unwrap_or_else(|e| format!("serialise: {e}"));
                    let view = ResolveView {
                        counter: resolved.maintenance_counter,
                        last_block_height: resolved.last_block_height,
                        last_tx_hash: resolved.last_tx_hash,
                        deactivated: resolved.document.deactivated,
                        vm_count: resolved.document.verification_method.len(),
                        service_count: resolved.document.service.len(),
                        document_json: json,
                    };
                    on_resolved.call((did_string, resolved.maintenance_counter));
                    Ok(view)
                }
                Err(e) => Err(e.to_string()),
            };
            result.set(Some(r));
            pending.set(false);
        });
    };

    rsx! {
        div { class: "wizard-header", "Resolve DID" }
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
                {if *pending.read() { "Resolving…" } else { "Resolve" }}
            }
        }
        if let Some(res) = result.read().as_ref() {
            match res {
                Ok(view) => {
                    let status_class = if view.deactivated { "wizard-outcome err" } else { "wizard-outcome ok" };
                    let status_label = if view.deactivated { "Deactivated" } else { "Active" };
                    let block = view
                        .last_block_height
                        .map(|h| format_int(h))
                        .unwrap_or_else(|| "—".into());
                    rsx! {
                        div { class: "{status_class}",
                            div { class: "row label", "{status_label}" }
                            div { class: "did-meta-grid",
                                div { class: "did-meta-cell",
                                    span { class: "label", "Counter" }
                                    span { class: "value", "{view.counter}" }
                                }
                                div { class: "did-meta-cell",
                                    span { class: "label", "VMs" }
                                    span { class: "value", "{view.vm_count}" }
                                }
                                div { class: "did-meta-cell",
                                    span { class: "label", "Services" }
                                    span { class: "value", "{view.service_count}" }
                                }
                                div { class: "did-meta-cell",
                                    span { class: "label", "Last block" }
                                    span { class: "value", "{block}" }
                                }
                            }
                            div { class: "row label", "Last tx" }
                            div { class: "seed-blob", "0x{view.last_tx_hash}" }
                            div { class: "row",
                                button {
                                    onclick: move |_| {
                                        let cur = *show_json.read();
                                        show_json.set(!cur);
                                    },
                                    {if *show_json.read() { "Hide document" } else { "Show document JSON" }}
                                }
                            }
                            if *show_json.read() {
                                div { class: "seed-blob", "{view.document_json}" }
                            }
                        }
                    }
                }
                Err(e) => rsx! {
                    div { class: "wizard-outcome err",
                        div { class: "row label", "Failed" }
                        div { class: "seed-blob", "{e}" }
                    }
                },
            }
        }
    }
}

#[component]
fn LoadCircuitPanel(
    network: Network,
    seed_did: Option<String>,
    seed_counter: Option<u32>,
    on_done: EventHandler<(String, String, wallet_core::DeployOutcome)>,
    on_timing: EventHandler<TimingRun>,
) -> Element {
    use wallet_core::WizardStage;

    // DID input — auto-seeded from the most recent deploy.
    let mut input = use_signal(|| seed_did.clone().unwrap_or_default());
    use_effect(move || {
        if let Some(seed) = seed_did.clone() {
            if *input.read() != seed {
                input.set(seed);
            }
        }
    });

    let circuit_names = wallet_core::did_circuit_names();
    // Default to `addVerificationMethod` — the most common first
    // step after a fresh deploy. Sits at a known position in the
    // registry; we look it up so a reordering doesn't silently
    // change the default.
    let default_idx = circuit_names
        .iter()
        .position(|n| *n == "addVerificationMethod")
        .unwrap_or(0);
    let mut circuit_idx = use_signal(|| default_idx);
    // Initial counter: whatever the parent resolved most recently,
    // or 0 (first maintenance after a fresh deploy).
    let mut counter_str = use_signal(|| seed_counter.map(|c| c.to_string()).unwrap_or_else(|| "0".to_string()));
    // Re-seed the counter whenever a new resolve completes, but
    // only if the user hasn't manually edited away from the prior
    // seed.
    let mut last_seed = use_signal::<Option<u32>>(|| seed_counter);
    use_effect(move || {
        if let Some(c) = seed_counter {
            let last = *last_seed.read();
            let current_text = counter_str.read().clone();
            let current_matches_last = last
                .map(|p| current_text == p.to_string())
                .unwrap_or(true);
            if Some(c) != last && current_matches_last {
                counter_str.set(c.to_string());
                last_seed.set(Some(c));
            } else if Some(c) != last {
                last_seed.set(Some(c));
            }
        }
    });

    let mut stages = use_signal::<Vec<WizardStage>>(Vec::new);
    let mut running = use_signal(|| false);
    // Parse error from invalid DID / counter input — surfaced as a
    // local failure without going through the wizard's terminal
    // state, since we don't even attempt the network if inputs are
    // malformed.
    let mut input_error = use_signal::<Option<String>>(|| None);

    let start = move |_| {
        if *running.read() {
            return;
        }
        let did_str = input.read().clone();
        if did_str.is_empty() {
            input_error.set(Some("enter a did:midnight:… string".into()));
            return;
        }
        let did_id = match wallet_core::DidId::parse(&did_str) {
            Ok(d) => d,
            Err(e) => {
                input_error.set(Some(format!("parse DID: {e}")));
                return;
            }
        };
        let counter: u32 = match counter_str.read().trim().parse() {
            Ok(c) => c,
            Err(e) => {
                input_error.set(Some(format!("counter must be a u32: {e}")));
                return;
            }
        };
        let name = circuit_names[*circuit_idx.read()].to_string();
        let did_for_log = did_str.clone();
        input_error.set(None);
        running.set(true);
        stages.set(Vec::new());
        let on_done = on_done.clone();
        let on_timing = on_timing.clone();
        let timing_label = format!("load_did_circuit:{name}");
        spawn(async move {
            use futures::StreamExt;
            let w = Wallet::demo(network);
            let mut stream = std::pin::pin!(w.load_did_circuit(did_id, name.clone(), counter));
            let mut observations: Vec<(usize, std::time::Instant)> = Vec::new();
            while let Some(stage) = stream.next().await {
                let now = std::time::Instant::now();
                if let Some(idx) = stage_pipeline_idx(&stage) {
                    observations.push((idx, now));
                } else {
                    let succeeded = matches!(&stage, WizardStage::Done(_));
                    on_timing.call(build_timing(
                        timing_label.clone(),
                        &observations,
                        now,
                        succeeded,
                    ));
                }
                let mut current = stages.read().clone();
                if let WizardStage::Done(o) = &stage {
                    on_done.call((did_for_log.clone(), name.clone(), o.clone()));
                }
                current.push(stage);
                stages.set(current);
            }
            running.set(false);
        });
    };

    let stages_snapshot = stages.read().clone();
    let term = terminal(&stages_snapshot);
    let has_started = !stages_snapshot.is_empty();
    let button_label = match (*running.read(), &term) {
        (true, _) => "Submitting…",
        (false, Some(TerminalView::Failed(_))) => "Retry",
        (false, Some(TerminalView::Done(_))) => "Load another",
        (false, None) => "Load circuit",
    };

    let current_idx = *circuit_idx.read();
    rsx! {
        div { class: "wizard-header", "Load circuit verifier key" }
        div { class: "row",
            input {
                r#type: "text",
                placeholder: "did:midnight:undeployed:…",
                value: "{input.read()}",
                oninput: move |e| input.set(e.value()),
                style: "flex: 1; padding: 6px 8px; background: var(--surface-2); color: var(--text); border: 1px solid var(--border); border-radius: 6px; font-family: ui-monospace, monospace; font-size: 11px;"
            }
        }
        div { class: "row",
            label { style: "min-width: 80px;", "Circuit" }
            select {
                onchange: move |e| {
                    if let Ok(idx) = e.value().parse::<usize>() {
                        circuit_idx.set(idx);
                    }
                },
                style: "flex: 1; padding: 6px 8px; background: var(--surface-2); color: var(--text); border: 1px solid var(--border); border-radius: 6px;",
                for (idx , name) in circuit_names.iter().enumerate() {
                    option {
                        value: "{idx}",
                        selected: idx == current_idx,
                        "{name}"
                    }
                }
            }
        }
        div { class: "row",
            label { style: "min-width: 80px;", "Counter" }
            input {
                r#type: "number",
                min: "0",
                value: "{counter_str.read()}",
                oninput: move |e| counter_str.set(e.value()),
                style: "width: 120px; padding: 6px 8px; background: var(--surface-2); color: var(--text); border: 1px solid var(--border); border-radius: 6px; font-family: ui-monospace, monospace; font-size: 11px;"
            }
            span { style: "font-size: 11px; color: var(--text-muted);",
                "Defaults to 0 (first maintenance after deploy)."
            }
        }
        div { class: "row",
            button {
                disabled: *running.read(),
                onclick: start,
                "{button_label}"
            }
        }

        if let Some(msg) = input_error.read().as_ref() {
            div { class: "wizard-outcome err",
                div { class: "row label", "Input" }
                div { class: "seed-blob", "{msg}" }
            }
        }

        if has_started {
            ul { class: "wizard-steps",
                for (idx , label) in PIPELINE.iter().enumerate() {
                    {render_step_row(idx, label, step_status(idx, &stages_snapshot))}
                }
            }
        }

        if let Some(TerminalView::Done(outcome)) = &term {
            div { class: "wizard-outcome ok",
                div { class: "row label", "Tx hash" }
                div { class: "seed-blob", "0x{hex::encode(outcome.tx_hash)}" }
                div { class: "row label", "Block hash" }
                div { class: "seed-blob", "0x{hex::encode(outcome.block_hash)}" }
            }
        } else if let Some(TerminalView::Failed(msg)) = &term {
            div { class: "wizard-outcome err",
                div { class: "row label", "Failed" }
                div { class: "seed-blob", "{msg}" }
            }
        }
    }
}

/// Variants of the 11-circuit dropdown. Order matches the
/// dropdown's display order; numeric tag is the `<select>` value
/// we round-trip through `e.value().parse()`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum OpKind {
    AddAlsoKnownAs,
    RemoveAlsoKnownAs,
    AddVerificationMethod,
    UpdateVerificationMethod,
    RemoveVerificationMethod,
    AddVerificationMethodRelation,
    RemoveVerificationMethodRelation,
    AddService,
    UpdateService,
    RemoveService,
    Deactivate,
}

impl OpKind {
    const ALL: &'static [OpKind] = &[
        OpKind::AddAlsoKnownAs,
        OpKind::RemoveAlsoKnownAs,
        OpKind::AddVerificationMethod,
        OpKind::UpdateVerificationMethod,
        OpKind::RemoveVerificationMethod,
        OpKind::AddVerificationMethodRelation,
        OpKind::RemoveVerificationMethodRelation,
        OpKind::AddService,
        OpKind::UpdateService,
        OpKind::RemoveService,
        OpKind::Deactivate,
    ];

    fn circuit_name(&self) -> &'static str {
        match self {
            Self::AddAlsoKnownAs => "addAlsoKnownAs",
            Self::RemoveAlsoKnownAs => "removeAlsoKnownAs",
            Self::AddVerificationMethod => "addVerificationMethod",
            Self::UpdateVerificationMethod => "updateVerificationMethod",
            Self::RemoveVerificationMethod => "removeVerificationMethod",
            Self::AddVerificationMethodRelation => "addVerificationMethodRelation",
            Self::RemoveVerificationMethodRelation => "removeVerificationMethodRelation",
            Self::AddService => "addService",
            Self::UpdateService => "updateService",
            Self::RemoveService => "removeService",
            Self::Deactivate => "deactivate",
        }
    }
}

const KEY_TYPES: &[&str] = &["EC", "RSA", "oct", "OKP"];
const CURVE_TYPES: &[&str] = &["Ed25519", "Jubjub", "P256"];
const RELATIONS: &[&str] = &[
    "Authentication",
    "AssertionMethod",
    "KeyAgreement",
    "CapabilityInvocation",
    "CapabilityDelegation",
];

#[component]
fn DidOperationsPanel(
    seed_did: Option<String>,
    on_drafted: EventHandler<(String, DidOperation)>,
) -> Element {
    let mut did_input = use_signal(|| seed_did.clone().unwrap_or_default());
    use_effect(move || {
        if let Some(seed) = seed_did.clone() {
            if *did_input.read() != seed {
                did_input.set(seed);
            }
        }
    });

    let mut op_idx = use_signal(|| 0usize);

    // All circuit-specific fields share one signal each. A
    // single panel surfaces fields conditionally on `op_idx`; the
    // ones not visible carry stale state but are inert.
    let mut f_value = use_signal(String::new);
    let mut f_id = use_signal(String::new);
    let mut f_key_type_idx = use_signal(|| 0usize);
    let mut f_curve_idx = use_signal(|| 0usize);
    let mut f_pk_x = use_signal(String::new);
    let mut f_pk_y = use_signal(String::new);
    let mut f_relation_idx = use_signal(|| 0usize);
    let mut f_method_id = use_signal(String::new);
    let mut f_typ = use_signal(String::new);
    let mut f_endpoint = use_signal(String::new);
    let mut error = use_signal::<Option<String>>(|| None);
    let mut last_drafted = use_signal::<Option<DidOperation>>(|| None);

    let on_draft = move |_| {
        let did_str = did_input.read().trim().to_string();
        if did_str.is_empty() {
            error.set(Some("enter a did:midnight:… string".into()));
            return;
        }
        if wallet_core::DidId::parse(&did_str).is_err() {
            error.set(Some(format!("not a valid DID: {did_str}")));
            return;
        }
        let op = OpKind::ALL[*op_idx.read()];
        let drafted = match op {
            OpKind::AddAlsoKnownAs => {
                let v = f_value.read().trim().to_string();
                if v.is_empty() {
                    error.set(Some("value is required".into()));
                    return;
                }
                DidOperation::AddAlsoKnownAs { value: v }
            }
            OpKind::RemoveAlsoKnownAs => {
                let v = f_value.read().trim().to_string();
                if v.is_empty() {
                    error.set(Some("value is required".into()));
                    return;
                }
                DidOperation::RemoveAlsoKnownAs { value: v }
            }
            OpKind::AddVerificationMethod | OpKind::UpdateVerificationMethod => {
                let id = f_id.read().trim().to_string();
                let pk_x = f_pk_x.read().trim().to_string();
                let pk_y = f_pk_y.read().trim().to_string();
                if id.is_empty() || pk_x.is_empty() || pk_y.is_empty() {
                    error.set(Some("id, pk_x, pk_y are required".into()));
                    return;
                }
                let vm = VerificationMethodInput {
                    id,
                    key_type: KEY_TYPES[*f_key_type_idx.read()].to_string(),
                    curve: CURVE_TYPES[*f_curve_idx.read()].to_string(),
                    pk_x,
                    pk_y,
                };
                match op {
                    OpKind::AddVerificationMethod => DidOperation::AddVerificationMethod(vm),
                    OpKind::UpdateVerificationMethod => DidOperation::UpdateVerificationMethod(vm),
                    _ => unreachable!(),
                }
            }
            OpKind::RemoveVerificationMethod => {
                let id = f_id.read().trim().to_string();
                if id.is_empty() {
                    error.set(Some("id is required".into()));
                    return;
                }
                DidOperation::RemoveVerificationMethod { id }
            }
            OpKind::AddVerificationMethodRelation => {
                let method_id = f_method_id.read().trim().to_string();
                if method_id.is_empty() {
                    error.set(Some("method_id is required".into()));
                    return;
                }
                DidOperation::AddVerificationMethodRelation {
                    relation: RELATIONS[*f_relation_idx.read()].to_string(),
                    method_id,
                }
            }
            OpKind::RemoveVerificationMethodRelation => {
                let method_id = f_method_id.read().trim().to_string();
                if method_id.is_empty() {
                    error.set(Some("method_id is required".into()));
                    return;
                }
                DidOperation::RemoveVerificationMethodRelation {
                    relation: RELATIONS[*f_relation_idx.read()].to_string(),
                    method_id,
                }
            }
            OpKind::AddService | OpKind::UpdateService => {
                let id = f_id.read().trim().to_string();
                let typ = f_typ.read().trim().to_string();
                let endpoint = f_endpoint.read().trim().to_string();
                if id.is_empty() || typ.is_empty() || endpoint.is_empty() {
                    error.set(Some("id, type, endpoint are required".into()));
                    return;
                }
                let s = ServiceInput { id, typ, endpoint };
                match op {
                    OpKind::AddService => DidOperation::AddService(s),
                    OpKind::UpdateService => DidOperation::UpdateService(s),
                    _ => unreachable!(),
                }
            }
            OpKind::RemoveService => {
                let id = f_id.read().trim().to_string();
                if id.is_empty() {
                    error.set(Some("id is required".into()));
                    return;
                }
                DidOperation::RemoveService { id }
            }
            OpKind::Deactivate => DidOperation::Deactivate,
        };
        error.set(None);
        last_drafted.set(Some(drafted.clone()));
        on_drafted.call((did_str, drafted));
    };

    let op = OpKind::ALL[*op_idx.read()];
    let cur_idx = *op_idx.read();
    let cur_kt = *f_key_type_idx.read();
    let cur_cv = *f_curve_idx.read();
    let cur_rel = *f_relation_idx.read();
    rsx! {
        div { class: "wizard-header", "DID operation (draft only)" }
        div { class: "session-log-empty",
            "Drafts capture intent locally. On-chain submission lands once the Compact-runtime JS bridge is wired (see bridge.rs TODOs)."
        }
        div { class: "row",
            input {
                r#type: "text",
                placeholder: "did:midnight:undeployed:…",
                value: "{did_input.read()}",
                oninput: move |e| did_input.set(e.value()),
                style: "flex: 1; padding: 6px 8px; background: var(--surface-2); color: var(--text); border: 1px solid var(--border); border-radius: 6px; font-family: ui-monospace, monospace; font-size: 11px;"
            }
        }
        div { class: "row",
            label { style: "min-width: 80px;", "Circuit" }
            select {
                onchange: move |e| {
                    if let Ok(idx) = e.value().parse::<usize>() {
                        op_idx.set(idx);
                    }
                },
                style: "flex: 1; padding: 6px 8px; background: var(--surface-2); color: var(--text); border: 1px solid var(--border); border-radius: 6px;",
                for (i , kind) in OpKind::ALL.iter().enumerate() {
                    option {
                        value: "{i}",
                        selected: i == cur_idx,
                        "{kind.circuit_name()}"
                    }
                }
            }
        }

        // Per-circuit form fields. The ones not matching `op` are
        // simply skipped — their signal state is irrelevant.
        match op {
            OpKind::AddAlsoKnownAs | OpKind::RemoveAlsoKnownAs => rsx! {
                FormRow {
                    label: "value",
                    value: f_value.read().clone(),
                    on_change: move |s: String| f_value.set(s),
                    placeholder: "https://alias.example.com or arbitrary identifier",
                }
            },
            OpKind::AddVerificationMethod | OpKind::UpdateVerificationMethod => rsx! {
                FormRow {
                    label: "id",
                    value: f_id.read().clone(),
                    on_change: move |s: String| f_id.set(s),
                    placeholder: "key-0 / authkey-2025-05",
                }
                FormSelect {
                    label: "key_type",
                    options: KEY_TYPES,
                    selected_idx: cur_kt,
                    on_select: move |i: usize| f_key_type_idx.set(i),
                }
                FormSelect {
                    label: "curve",
                    options: CURVE_TYPES,
                    selected_idx: cur_cv,
                    on_select: move |i: usize| f_curve_idx.set(i),
                }
                FormRow {
                    label: "pk.x",
                    value: f_pk_x.read().clone(),
                    on_change: move |s: String| f_pk_x.set(s),
                    placeholder: "field element (hex or decimal)",
                }
                FormRow {
                    label: "pk.y",
                    value: f_pk_y.read().clone(),
                    on_change: move |s: String| f_pk_y.set(s),
                    placeholder: "field element (hex or decimal)",
                }
            },
            OpKind::RemoveVerificationMethod | OpKind::RemoveService => rsx! {
                FormRow {
                    label: "id",
                    value: f_id.read().clone(),
                    on_change: move |s: String| f_id.set(s),
                    placeholder: "fragment id to remove",
                }
            },
            OpKind::AddVerificationMethodRelation | OpKind::RemoveVerificationMethodRelation => rsx! {
                FormSelect {
                    label: "relation",
                    options: RELATIONS,
                    selected_idx: cur_rel,
                    on_select: move |i: usize| f_relation_idx.set(i),
                }
                FormRow {
                    label: "method_id",
                    value: f_method_id.read().clone(),
                    on_change: move |s: String| f_method_id.set(s),
                    placeholder: "existing verification-method fragment id",
                }
            },
            OpKind::AddService | OpKind::UpdateService => rsx! {
                FormRow {
                    label: "id",
                    value: f_id.read().clone(),
                    on_change: move |s: String| f_id.set(s),
                    placeholder: "service fragment id",
                }
                FormRow {
                    label: "type",
                    value: f_typ.read().clone(),
                    on_change: move |s: String| f_typ.set(s),
                    placeholder: "e.g. LinkedDomains",
                }
                FormRow {
                    label: "endpoint",
                    value: f_endpoint.read().clone(),
                    on_change: move |s: String| f_endpoint.set(s),
                    placeholder: "https://example.com/.well-known/did-config",
                }
            },
            OpKind::Deactivate => rsx! {
                div { class: "row",
                    span { style: "color: var(--text-muted); font-size: 11px;",
                        "No input. Sets the DID inactive and prevents further updates."
                    }
                }
            },
        }

        div { class: "row",
            button { onclick: on_draft, "Draft operation" }
        }

        if let Some(msg) = error.read().as_ref() {
            div { class: "wizard-outcome err",
                div { class: "row label", "Validation" }
                div { class: "seed-blob", "{msg}" }
            }
        } else if let Some(op) = last_drafted.read().as_ref() {
            div { class: "wizard-outcome ok",
                div { class: "row label", "Drafted (logged)" }
                div { class: "seed-blob", "{op.circuit()} · {op.summary()}" }
            }
        }
    }
}

#[component]
fn FormRow(
    label: &'static str,
    value: String,
    on_change: EventHandler<String>,
    placeholder: &'static str,
) -> Element {
    rsx! {
        div { class: "row",
            label { style: "min-width: 80px;", "{label}" }
            input {
                r#type: "text",
                value: "{value}",
                placeholder: "{placeholder}",
                oninput: move |e| on_change.call(e.value()),
                style: "flex: 1; padding: 6px 8px; background: var(--surface-2); color: var(--text); border: 1px solid var(--border); border-radius: 6px; font-family: ui-monospace, monospace; font-size: 11px;"
            }
        }
    }
}

#[component]
fn FormSelect(
    label: &'static str,
    options: &'static [&'static str],
    selected_idx: usize,
    on_select: EventHandler<usize>,
) -> Element {
    rsx! {
        div { class: "row",
            label { style: "min-width: 80px;", "{label}" }
            select {
                onchange: move |e| {
                    if let Ok(i) = e.value().parse::<usize>() {
                        on_select.call(i);
                    }
                },
                style: "flex: 1; padding: 6px 8px; background: var(--surface-2); color: var(--text); border: 1px solid var(--border); border-radius: 6px;",
                for (i , opt) in options.iter().enumerate() {
                    option {
                        value: "{i}",
                        selected: i == selected_idx,
                        "{opt}"
                    }
                }
            }
        }
    }
}

/// Result of a `bridgeProbe` round-trip. Mirrors the JS-side
/// payload (see `web/src/entry.ts::bridgeProbe`). `error` is the
/// only field populated on the JS-side error path (e.g. the bundle
/// hasn't loaded because we built without `--features js-bridge`).
#[derive(Clone, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize, Debug)]
#[serde(rename_all = "camelCase", default)]
struct BridgeProbeResult {
    echoed: String,
    version: String,
    bundle_ready: bool,
    contract_layer_loaded: bool,
    contract_exports: Vec<String>,
    compact_runtime_exports: Vec<String>,
    time_ms: i64,
    /// Only set on the JS-side error path. When this is `Some`,
    /// the other fields are stale defaults.
    error: Option<String>,
}

#[component]
fn JsBridgePanel() -> Element {
    let mut message = use_signal(|| "hello from rust".to_string());
    let mut result = use_signal::<Option<Result<BridgeProbeResult, String>>>(|| None);
    let mut pending = use_signal(|| false);

    let probe = move |_| {
        if *pending.read() {
            return;
        }
        pending.set(true);
        result.set(None);
        let msg = message.read().clone();
        let msg_json = serde_json::to_string(&msg).unwrap_or_else(|_| "\"\"".into());
        // Build a small async JS expression that defends against the
        // js-bridge feature being off (bundle absent) and any thrown
        // error inside the probe. Returning a plain object either
        // way means Rust always gets a parseable JSON payload.
        let snippet = format!(
            r#"if (!window.midnightDidBundle?.bridgeProbe) {{
                return {{ error: "midnightDidBundle.bridgeProbe not loaded — rebuild with --features js-bridge" }};
            }}
            try {{
                const r = await window.midnightDidBundle.bridgeProbe({{ message: {msg_json} }});
                return r;
            }} catch (e) {{
                return {{ error: String(e?.message ?? e) }};
            }}"#,
        );
        spawn(async move {
            let r: Result<BridgeProbeResult, String> = match document::eval(&snippet).await {
                Ok(v) => serde_json::from_value::<BridgeProbeResult>(v)
                    .map_err(|e| format!("decode probe result: {e}")),
                Err(e) => Err(format!("eval failed: {e}")),
            };
            result.set(Some(r));
            pending.set(false);
        });
    };

    rsx! {
        div { class: "wizard-header", "JS bridge spike" }
        div { class: "session-log-empty",
            "Round-trips a message through Dioxus eval → bundle.bridgeProbe → back. Requires --features js-bridge."
        }
        div { class: "row",
            input {
                r#type: "text",
                value: "{message.read()}",
                oninput: move |e| message.set(e.value()),
                style: "flex: 1; padding: 6px 8px; background: var(--surface-2); color: var(--text); border: 1px solid var(--border); border-radius: 6px; font-family: ui-monospace, monospace; font-size: 11px;"
            }
            button {
                disabled: *pending.read(),
                onclick: probe,
                {if *pending.read() { "Probing…" } else { "Probe bridge" }}
            }
        }
        if let Some(r) = result.read().as_ref() {
            match r {
                Ok(probe) => {
                    if let Some(err) = probe.error.as_ref() {
                        rsx! {
                            div { class: "wizard-outcome err",
                                div { class: "row label", "JS-side error" }
                                div { class: "seed-blob", "{err}" }
                            }
                        }
                    } else {
                        let exports_n = probe.contract_exports.len();
                        let runtime_n = probe.compact_runtime_exports.len();
                        rsx! {
                            div { class: "wizard-outcome ok",
                                div { class: "row label", "Round-trip OK" }
                                div { class: "did-meta-grid",
                                    div { class: "did-meta-cell",
                                        span { class: "label", "Echoed" }
                                        span { class: "value", "{probe.echoed}" }
                                    }
                                    div { class: "did-meta-cell",
                                        span { class: "label", "Bundle v" }
                                        span { class: "value", "{probe.version}" }
                                    }
                                    div { class: "did-meta-cell",
                                        span { class: "label", "Contract" }
                                        span { class: "value", {if probe.contract_layer_loaded { format!("loaded · {exports_n} exports") } else { "not loaded".to_string() }} }
                                    }
                                    div { class: "did-meta-cell",
                                        span { class: "label", "Runtime" }
                                        span { class: "value", "{runtime_n} exports" }
                                    }
                                }
                                div { class: "row label", "Contract exports" }
                                div { class: "seed-blob", "{probe.contract_exports.join(\", \")}" }
                            }
                        }
                    }
                }
                Err(e) => rsx! {
                    div { class: "wizard-outcome err",
                        div { class: "row label", "Eval error" }
                        div { class: "seed-blob", "{e}" }
                    }
                },
            }
        }
    }
}

#[component]
fn TimingsPanel(runs: Vec<TimingRun>) -> Element {
    if runs.is_empty() {
        return rsx! {
            div { class: "session-log-empty",
                "Run a Create DID or Load circuit to capture per-stage timings."
            }
        };
    }
    rsx! {
        div { class: "wizard-header", "Pipeline timings" }
        ul { class: "session-log",
            for (idx , run) in runs.iter().enumerate().rev() {
                {render_timing_entry(idx, run)}
            }
        }
    }
}

fn render_timing_entry(idx: usize, run: &TimingRun) -> Element {
    let outcome = if run.succeeded { "ok" } else { "err" };
    let total = format_ms(run.total_ms);
    // Find max stage duration so we can scale bars relatively.
    let max_stage = run.per_stage_ms.iter().copied().max().unwrap_or(0).max(1);
    let label = run.label.clone();
    rsx! {
        li {
            key: "timing-{idx}",
            class: "session-log-entry timing {outcome}",
            div { class: "head",
                span { class: "kind", "{label}" }
                span { class: "when", "#{idx + 1} · total {total}" }
            }
            ul { class: "timing-bars",
                for (i , label) in PIPELINE.iter().enumerate() {
                    {render_timing_bar(label, run.per_stage_ms[i], max_stage)}
                }
            }
        }
    }
}

fn render_timing_bar(label: &str, ms: u64, max_ms: u64) -> Element {
    // Bar width in percent — empty stages stay at 0% so the user
    // sees clearly that work didn't happen there.
    let pct = if max_ms == 0 { 0 } else { ((ms * 100) / max_ms).min(100) };
    rsx! {
        li { class: "timing-bar-row",
            span { class: "timing-bar-label", "{label}" }
            div { class: "timing-bar-track",
                div { class: "timing-bar-fill", style: "width: {pct}%;" }
            }
            span { class: "timing-bar-value", "{format_ms(ms)}" }
        }
    }
}

/// Compact human-readable duration: 850ms / 1.2s / 41.8s / 2m 03s.
fn format_ms(ms: u64) -> String {
    if ms < 1_000 {
        format!("{ms}ms")
    } else if ms < 10_000 {
        let s = ms as f64 / 1000.0;
        format!("{s:.2}s")
    } else if ms < 60_000 {
        let s = ms as f64 / 1000.0;
        format!("{s:.1}s")
    } else {
        let m = ms / 60_000;
        let s = (ms % 60_000) / 1_000;
        format!("{m}m {s:02}s")
    }
}

#[component]
fn SessionLogPanel(events: Vec<SessionEvent>) -> Element {
    if events.is_empty() {
        return rsx! {
            div { class: "session-log-empty",
                "Activity will appear here as you create, resolve, and load circuits."
            }
        };
    }
    rsx! {
        div { class: "wizard-header", "Session activity" }
        ul { class: "session-log",
            // Newest entries first — last appended event is the most recent.
            for (idx , event) in events.iter().enumerate().rev() {
                {render_session_entry(idx, event)}
            }
        }
    }
}

fn render_session_entry(idx: usize, event: &SessionEvent) -> Element {
    match event {
        SessionEvent::Deploy { did, tx_hash, block_hash } => rsx! {
            li {
                key: "{idx}",
                class: "session-log-entry deploy",
                div { class: "head",
                    span { class: "kind", "Created DID" }
                    span { class: "when", "#{idx + 1}" }
                }
                div { class: "detail", "{did}" }
                div { class: "detail", "tx 0x{hex::encode(tx_hash)}" }
                div { class: "detail", "block 0x{hex::encode(block_hash)}" }
            }
        },
        SessionEvent::Resolve { did, counter } => rsx! {
            li {
                key: "{idx}",
                class: "session-log-entry resolve",
                div { class: "head",
                    span { class: "kind", "Resolved" }
                    span { class: "when", "#{idx + 1} · counter {counter}" }
                }
                div { class: "detail", "{did}" }
            }
        },
        SessionEvent::LoadCircuit { did, circuit, tx_hash, block_hash } => rsx! {
            li {
                key: "{idx}",
                class: "session-log-entry circuit",
                div { class: "head",
                    span { class: "kind", "Loaded {circuit}" }
                    span { class: "when", "#{idx + 1}" }
                }
                div { class: "detail", "{did}" }
                div { class: "detail", "tx 0x{hex::encode(tx_hash)}" }
                div { class: "detail", "block 0x{hex::encode(block_hash)}" }
            }
        },
        SessionEvent::OperationDrafted { did, operation } => rsx! {
            li {
                key: "{idx}",
                class: "session-log-entry circuit",
                div { class: "head",
                    span { class: "kind", "Drafted {operation.circuit()}" }
                    span { class: "when", "#{idx + 1} · local-only" }
                }
                div { class: "detail", "{did}" }
                div { class: "detail", "{operation.summary()}" }
            }
        },
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

/// Render a u128 subunit count as a comma-grouped decimal string —
/// e.g. `250000000000000` → `"250,000,000,000,000"`. Matches
/// example-counter's `formatBalance` (`BigInt.toLocaleString()`)
/// so the displayed values agree between wallets.
fn format_subunits(n: u128) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out.chars().rev().collect()
}

#[component]
fn BalancesCard(connected: bool, night_subunits: Option<u128>) -> Element {
    // Three display states for NIGHT:
    //   • not connected           → "—"
    //   • connected, sync pending → "syncing…"
    //   • connected, sync done    → "<grouped subunit count>"
    // DUST stays on "—" with a hint — the dustGenerations
    // subscription is Phase B and not wired yet.
    let night_text = match (connected, night_subunits) {
        (false, _) => "—".to_string(),
        (true, None) => "syncing…".to_string(),
        (true, Some(n)) => format_subunits(n),
    };

    rsx! {
        div { class: "card",
            div { class: "card-header", "Balances" }
            div { class: "balance-row",
                span { class: "label", "NIGHT" }
                span { class: "value",
                    "{night_text}"
                    span { class: "unit", " NIGHT" }
                }
            }
            div { class: "balance-row",
                span { class: "label", "Dust" }
                span { class: "value",
                    // DUST stays on "—" — the dustGenerations
                    // subscription is Phase B. The hint row below
                    // tells the user why.
                    "—"
                    span { class: "unit", " DUST" }
                }
            }
            // Hint row replaces the `dust-progress` bar that will land in
            // Phase B (dustGenerations subscription + registered NIGHT UTXOs).
            div { class: "balance-row",
                span { class: "hint",
                    {match (connected, night_subunits) {
                        (false, _) => "Connect to the network to see live balances.",
                        (true, None) => "Snapshotting unshielded UTXOs from the indexer…",
                        (true, Some(0)) => "No NIGHT yet. Send NIGHT to the address above.",
                        (true, Some(_)) => "DUST tracking lands in Phase B — register NIGHT UTXOs to accrue.",
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
