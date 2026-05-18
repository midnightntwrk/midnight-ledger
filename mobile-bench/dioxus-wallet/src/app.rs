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

    /// Translate the drafted operation into the JSON `args` array
    /// expected by `Wallet::call_did_circuit` (which hands it to
    /// the JS harness verbatim). Mirrors the per-circuit shapes
    /// exercised in `tests/js_inspect_circuits.rs`:
    /// - bigints are tagged as `{ "$bigint": "<n>" }` so the
    ///   harness revives them as JS BigInt (JSON has no native
    ///   bigint, JS Number tops out at 2^53);
    /// - enum tags match the `.compact` source order — see
    ///   `KeyType`, `CurveType`, `VerificationMethodType`,
    ///   `VerificationMethodRelation` declarations in
    ///   `contracts/midnight-did/did.compact`.
    fn args_json(&self) -> serde_json::Value {
        match self {
            Self::AddAlsoKnownAs { value } | Self::RemoveAlsoKnownAs { value } => {
                serde_json::json!([value])
            }
            Self::AddVerificationMethod(vm) | Self::UpdateVerificationMethod(vm) => {
                serde_json::json!([vm_to_json(vm)])
            }
            Self::RemoveVerificationMethod { id } | Self::RemoveService { id } => {
                serde_json::json!([id])
            }
            Self::AddVerificationMethodRelation { relation, method_id }
            | Self::RemoveVerificationMethodRelation { relation, method_id } => {
                serde_json::json!([relation_tag(relation), method_id])
            }
            Self::AddService(s) | Self::UpdateService(s) => serde_json::json!([{
                "id": s.id,
                "typ": s.typ,
                "serviceEndpoint": s.endpoint,
            }]),
            Self::Deactivate => serde_json::json!([]),
        }
    }
}

/// Look up an enum tag by name from a `&[&str]` table whose order
/// matches the contract's `.compact` declaration order. Returns
/// the offset; 0-based for `KeyType`/`CurveType`, callers add 1
/// for `VerificationMethodRelation` (whose declaration starts with
/// `Undefined` which we don't surface in the UI).
fn enum_tag(table: &[&str], name: &str) -> i32 {
    table.iter().position(|s| *s == name).unwrap_or(0) as i32
}

fn relation_tag(name: &str) -> i32 {
    // Contract enum: Undefined=0, Authentication=1, …, CapabilityDelegation=5.
    // Our UI table `RELATIONS` skips Undefined, so add 1.
    enum_tag(RELATIONS, name) + 1
}

/// Build the JSON `VerificationMethod` struct payload — `typ` is
/// always `JsonWebKey` (the only variant the contract accepts);
/// `publicKeyJwk.x/y` are bigints expressed as decimal strings so
/// `BigInt(str)` revives them in JS. A "0x…" prefix also works
/// because `BigInt("0x…")` is well-defined.
fn vm_to_json(vm: &VerificationMethodInput) -> serde_json::Value {
    serde_json::json!({
        "id": vm.id,
        // VerificationMethodType.JsonWebKey = 1
        "typ": 1,
        "publicKeyJwk": {
            "kty": enum_tag(KEY_TYPES, &vm.key_type),
            "crv": enum_tag(CURVE_TYPES, &vm.curve),
            "x": serde_json::json!({ "$bigint": vm.pk_x.clone() }),
            "y": serde_json::json!({ "$bigint": vm.pk_y.clone() }),
        }
    })
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

/// One row in the session-scoped DID inventory. A DID enters the
/// inventory via a deploy (status `Pending` until resolved) or a
/// resolve (status comes from the on-chain state). Subsequent
/// resolves of the same DID update the row in place — counter +
/// vm/service counts + last-seen block are kept fresh so the
/// table always reflects the most recent observation.
#[derive(Clone, PartialEq, Eq, Debug)]
struct DidInventoryEntry {
    /// `did:midnight:<network>:<address>` — primary key.
    did: String,
    network_label: String,
    status: DidInventoryStatus,
    /// `None` for a freshly-deployed DID that hasn't been
    /// resolved yet (we don't know the counter chain-side until
    /// the indexer catches up).
    counter: Option<u32>,
    vm_count: Option<usize>,
    service_count: Option<usize>,
    last_block_height: Option<i64>,
}

/// Status badge for [`DidInventoryEntry`]. `Pending` is what we
/// show between deploy and first successful resolve; afterwards
/// the resolve reports `Active` or `Deactivated`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum DidInventoryStatus {
    Pending,
    Active,
    Deactivated,
}

impl DidInventoryStatus {
    fn label(&self) -> &'static str {
        match self {
            Self::Pending => "Pending",
            Self::Active => "Active",
            Self::Deactivated => "Deactivated",
        }
    }
    fn badge_class(&self) -> &'static str {
        match self {
            Self::Pending => "did-badge pending",
            Self::Active => "did-badge active",
            Self::Deactivated => "did-badge deactivated",
        }
    }
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
    // Per-session DID inventory keyed by DID string. Adopts the
    // UI/UX bundle's "DID-first inventory" pattern — every DID
    // we touch (deploy, resolve) appears as a row in the inventory
    // panel with its current best-known status + counter.
    let mut did_inventory =
        use_signal::<std::collections::BTreeMap<String, DidInventoryEntry>>(Default::default);
    // Which DID, if any, is currently "open" in the detail view.
    // `None` → render the flat panels (Create / Resolve / etc.);
    // `Some(did)` → render `DidDetailView` for that DID.
    let mut open_did = use_signal::<Option<String>>(|| None);
    // Cache of the most recent successful resolve for each DID.
    // `DidDetailView` reads from this so opening / switching tabs
    // doesn't have to re-query the indexer; a manual "Resolve
    // latest" button refreshes it.
    let mut resolved_cache =
        use_signal::<std::collections::HashMap<String, wallet_core::ResolvedDid>>(
            Default::default,
        );
    // Penultimate resolve per DID — populated by snapshotting the
    // current `resolved_cache` entry just before it's overwritten.
    // The Resolver tab consumes this to render a "what changed
    // since the previous resolve" diff card (per UI/UX bundle's
    // Resolver inspector).
    let mut previous_resolved_cache =
        use_signal::<std::collections::HashMap<String, wallet_core::ResolvedDid>>(
            Default::default,
        );
    // Per-pipeline timing snapshots, newest last. Shown in the
    // Diagnostics tab as a stacked bar / breakdown per run.
    let mut timing_log = use_signal::<Vec<TimingRun>>(Vec::new);

    // ── JS bridge + embedded proof-server ─────────────────────────
    // BridgeState is cheap-clone (Arc<OnceCell<String>>); we keep a
    // copy in a signal for UI display and pass another into the
    // background spawn / bridge loop.
    let bridge_state = use_signal(BridgeState::new);
    let mut proof_server = use_signal::<Option<String>>(|| None);

    // Open the persistent wallet store once at startup and
    // hand the handle to BridgeState. Failures are logged but
    // don't crash the app — a missing store means controller
    // secrets stay session-scoped (the previous behaviour).
    //
    // Default passphrase for the prototype: a fixed dev
    // string. A future slice will surface an unlock prompt and
    // let the user set / rotate this.
    use_future(move || {
        let state = bridge_state.read().clone();
        let net = *network.read();
        async move {
            let path = wallet_store_path();
            match wallet_core::store::WalletStore::open(&path, DEV_STORE_PASSPHRASE) {
                Ok(store) => {
                    state.set_store(store.clone());
                    let n = state.hydrate_controller_secrets(net);
                    // Bulk-load the DID inventory rows for the
                    // current network into the UI signal.
                    let mut inv_map: std::collections::BTreeMap<
                        String,
                        DidInventoryEntry,
                    > = Default::default();
                    let mut inv_count = 0usize;
                    if let Ok(rows) = store.list_did_inventory(net) {
                        for row in rows {
                            inv_count += 1;
                            inv_map.insert(
                                row.did.clone(),
                                DidInventoryEntry {
                                    did: row.did,
                                    network_label: net.label().to_string(),
                                    status: status_from_store(row.status),
                                    counter: row.counter,
                                    vm_count: row.vm_count.map(|v| v as usize),
                                    service_count: row.service_count.map(|v| v as usize),
                                    last_block_height: row.last_block_height,
                                },
                            );
                        }
                    }
                    if !inv_map.is_empty() {
                        did_inventory.set(inv_map);
                    }
                    // Hydrate the resolved-cache map so the
                    // detail tabs still have content after a
                    // reload (the cross-resolve diff card needs
                    // *something* to diff against on first
                    // resolve of a session — the prior on-disk
                    // entry plays that role until a fresh
                    // resolve overwrites it).
                    let mut cache_map = std::collections::HashMap::new();
                    let mut cache_count = 0usize;
                    if let Ok(rows) = store.list_resolved_cache(net) {
                        for (did, json, _at) in rows {
                            if let Ok(r) =
                                serde_json::from_str::<wallet_core::ResolvedDid>(&json)
                            {
                                cache_map.insert(did, r);
                                cache_count += 1;
                            }
                        }
                    }
                    if !cache_map.is_empty() {
                        resolved_cache.set(cache_map);
                    }
                    tracing::info!(
                        path=%path.display(),
                        hydrated_controller_secrets=n,
                        hydrated_inventory=inv_count,
                        hydrated_cache=cache_count,
                        "wallet store opened",
                    );
                }
                Err(e) => tracing::warn!(error=%e, path=%path.display(), "wallet store open failed"),
            }
        }
    });

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
                        // Stash the per-DID random controller secret in
                        // the shared bridge store so subsequent JS-driven
                        // circuit calls can look it up via the
                        // `getControllerSecretKey` RPC.
                        bridge_state.read().remember_controller_secret(
                            *network.read(),
                            did.clone(),
                            o.controller_sk,
                        );
                        last_did_id.set(Some(did.clone()));
                        // Drop into the inventory as Pending — the next
                        // Resolve flips it to Active/Deactivated and
                        // populates the counters.
                        let entry = DidInventoryEntry {
                            did: did.clone(),
                            network_label: o.did_id.network.label().to_string(),
                            status: DidInventoryStatus::Pending,
                            counter: None,
                            vm_count: None,
                            service_count: None,
                            last_block_height: None,
                        };
                        let mut inv = did_inventory.read().clone();
                        inv.insert(did.clone(), entry.clone());
                        did_inventory.set(inv);
                        persist_inventory_entry(
                            &bridge_state.read(),
                            *network.read(),
                            &entry,
                        );
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
                if let Some(did_open) = open_did.read().clone() {
                    // Detail mode: full 8-tab view of one DID.
                    DidDetailView {
                        network: *network.read(),
                        did: did_open.clone(),
                        cached: resolved_cache.read().get(&did_open).cloned(),
                        previous_cached: previous_resolved_cache
                            .read()
                            .get(&did_open)
                            .cloned(),
                        controller_secret: bridge_state
                            .read()
                            .controller_secret_for_on(*network.read(), &did_open),
                        session_log: session_log.read().clone(),
                        on_back: move |_| open_did.set(None),
                        on_resolved: move |resolved: wallet_core::ResolvedDid| {
                            let did_string = resolved.document.id.to_did_string();
                            // Inventory row stays in sync.
                            let entry = DidInventoryEntry {
                                did: did_string.clone(),
                                network_label: resolved.document.id.network.label().to_string(),
                                status: if resolved.document.deactivated {
                                    DidInventoryStatus::Deactivated
                                } else {
                                    DidInventoryStatus::Active
                                },
                                counter: Some(resolved.maintenance_counter),
                                vm_count: Some(resolved.document.verification_method.len()),
                                service_count: Some(resolved.document.service.len()),
                                last_block_height: resolved.last_block_height,
                            };
                            let mut inv = did_inventory.read().clone();
                            inv.insert(did_string.clone(), entry.clone());
                            did_inventory.set(inv);
                            persist_inventory_entry(
                                &bridge_state.read(),
                                *network.read(),
                                &entry,
                            );
                            // Snapshot the current resolve into the
                            // penultimate slot before overwriting it
                            // — the Resolver tab diffs the two so
                            // the user sees what changed.
                            let cache_snap = resolved_cache.read().clone();
                            if let Some(prev) = cache_snap.get(&did_string) {
                                let mut prev_map = previous_resolved_cache.read().clone();
                                prev_map.insert(did_string.clone(), prev.clone());
                                previous_resolved_cache.set(prev_map);
                            }
                            // Cache the full resolve for the detail tabs.
                            let mut cache = cache_snap;
                            cache.insert(did_string.clone(), resolved.clone());
                            resolved_cache.set(cache);
                            persist_resolved_cache(
                                &bridge_state.read(),
                                *network.read(),
                                &did_string,
                                &resolved,
                            );
                            // Session log gets a Resolve event.
                            let mut log = session_log.read().clone();
                            log.push(SessionEvent::Resolve {
                                did: did_string,
                                counter: resolved.maintenance_counter,
                            });
                            session_log.set(log);
                            // The maintenance counter feeds the
                            // load-circuit auto-fill, same as before.
                            last_resolved.set(Some((
                                resolved.document.id.to_did_string(),
                                resolved.maintenance_counter,
                            )));
                        },
                        on_deactivated: move |(did, outcome): (String, wallet_core::DeployOutcome)| {
                            let mut log = session_log.read().clone();
                            log.push(SessionEvent::LoadCircuit {
                                did,
                                circuit: "deactivate".to_string(),
                                tx_hash: outcome.tx_hash,
                                block_hash: outcome.block_hash,
                            });
                            session_log.set(log);
                        },
                        on_timing: move |run: TimingRun| {
                            let mut log = timing_log.read().clone();
                            log.push(run);
                            timing_log.set(log);
                        },
                        on_event: move |ev: SessionEvent| {
                            let mut log = session_log.read().clone();
                            log.push(ev);
                            session_log.set(log);
                        },
                    }
                } else {
                    // Browse mode: inventory + flat panels.
                    DidInventoryPanel {
                        entries: did_inventory.read().values().cloned().collect(),
                        on_select: move |did: String| {
                            last_did_id.set(Some(did.clone()));
                            open_did.set(Some(did));
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
                        on_seen: move |entry: DidInventoryEntry| {
                            let mut inv = did_inventory.read().clone();
                            inv.insert(entry.did.clone(), entry.clone());
                            did_inventory.set(inv);
                            persist_inventory_entry(
                                &bridge_state.read(),
                                *network.read(),
                                &entry,
                            );
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
                }
            },
            Tab::Diagnostics => rsx! {
                JsBridgePanel { seed_did: last_did_id.read().clone() }
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
                div { class: "row label",
                    "Controller secret (save this — without it you cannot update or deactivate this DID)"
                }
                div { class: "seed-blob", "0x{hex::encode(outcome.controller_sk)}" }
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
    /// Fires *after* a successful resolve with the full inventory
    /// row. Parent feeds this into `did_inventory` to keep the
    /// DID inventory panel in sync.
    on_seen: EventHandler<DidInventoryEntry>,
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
        let on_seen = on_seen.clone();
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
                        last_tx_hash: resolved.last_tx_hash.clone(),
                        deactivated: resolved.document.deactivated,
                        vm_count: resolved.document.verification_method.len(),
                        service_count: resolved.document.service.len(),
                        document_json: json,
                    };
                    on_resolved.call((did_string.clone(), resolved.maintenance_counter));
                    on_seen.call(DidInventoryEntry {
                        did: did_string,
                        network_label: resolved.document.id.network.label().to_string(),
                        status: if resolved.document.deactivated {
                            DidInventoryStatus::Deactivated
                        } else {
                            DidInventoryStatus::Active
                        },
                        counter: Some(resolved.maintenance_counter),
                        vm_count: Some(resolved.document.verification_method.len()),
                        service_count: Some(resolved.document.service.len()),
                        last_block_height: resolved.last_block_height,
                    });
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

/// Buildable circuit kinds — every variant except `Deactivate`,
/// which has its own dedicated button in the detail header (it
/// closes the DID for further updates; not a thing to batch).
const BUILDABLE_OPS: &[OpKind] = &[
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
];

/// Status of a queued operation as the batch flows through
/// `Wallet::call_did_circuit`. Drives the per-row indicator in
/// the preview pane.
#[derive(Clone, PartialEq, Eq)]
enum QueueStatus {
    Pending,
    /// Currently running. `phase` is the human-readable
    /// substep — "Loading VK", "Calling circuit". The auto-load
    /// path may dwell on "Loading VK" for the duration of one
    /// MaintenanceUpdate (deploy + balance + prove + submit +
    /// confirm + indexer-settle wait) before transitioning.
    Running { phase: String },
    Done {
        tx_hex: String,
        /// If the wallet auto-loaded the circuit's verifier key
        /// just before the call, this is the tx hash of that
        /// `MaintenanceUpdate`. The preview pane surfaces it
        /// inline so the user understands the two transactions
        /// landed for one queued op.
        loaded_tx_hex: Option<String>,
    },
    Failed { err: String },
    /// A later op in the batch never ran because an earlier op
    /// failed. We stop on the first failure rather than press on
    /// against unknown state.
    Skipped,
}

/// 3-pane Operation Builder (palette / form / preview) adopted
/// from `midnight-did-uiux-bundle`. Drafts ride one batch queue;
/// `Submit batch` iterates them through `Wallet::call_did_circuit`
/// sequentially, awaiting each call's terminal `WizardStage`
/// before starting the next. State changes between maintenance
/// updates so we must serialize.
///
/// `Deactivate` is intentionally NOT buildable here — the detail
/// header has its own button for it. The builder is for
/// composing / mutating the DID document; deactivate is
/// terminal.
#[component]
fn DidOperationBuilder(
    network: Network,
    did: String,
    controller_secret: [u8; 32],
    /// Circuits whose verifier key is already registered on
    /// `ContractState.operations`. Drives the auto-load step:
    /// any queued op whose `circuit()` isn't in this set gets a
    /// preceding `Wallet::load_did_circuit` MaintenanceUpdate
    /// before its `ContractCall`. The set is local to the
    /// component — the spawned submit closure clones it and
    /// extends it as it runs, so successive ops in the same
    /// batch reuse a single load.
    loaded_circuits: Vec<String>,
    /// Current `maintenance_authority.counter` for the contract.
    /// Every `MaintenanceUpdate` the auto-load path emits must
    /// use this exact counter (chain rejects mismatches with
    /// `InvalidMaintenanceUpdate`); the closure bumps it locally
    /// after each accepted load. Subsequent loads in the same
    /// batch use the bumped value.
    initial_counter: u32,
    on_back: EventHandler<()>,
    on_event: EventHandler<SessionEvent>,
    on_resolved: EventHandler<wallet_core::ResolvedDid>,
) -> Element {
    let mut op_idx = use_signal(|| 0usize);

    // Per-circuit form fields. Same single-set-of-signals
    // pattern as `DidOperationsPanel` — the fields not relevant
    // to the current op carry stale state but are inert.
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
    let mut form_error = use_signal::<Option<String>>(|| None);

    // Queue + execution state.
    let mut queue = use_signal::<Vec<(DidOperation, QueueStatus)>>(Vec::new);
    let mut running = use_signal(|| false);
    let mut batch_error = use_signal::<Option<String>>(|| None);

    let on_add_to_batch = move |_| {
        let op = BUILDABLE_OPS[*op_idx.read()];
        let drafted = match op {
            OpKind::AddAlsoKnownAs => {
                let v = f_value.read().trim().to_string();
                if v.is_empty() {
                    form_error.set(Some("value is required".into()));
                    return;
                }
                DidOperation::AddAlsoKnownAs { value: v }
            }
            OpKind::RemoveAlsoKnownAs => {
                let v = f_value.read().trim().to_string();
                if v.is_empty() {
                    form_error.set(Some("value is required".into()));
                    return;
                }
                DidOperation::RemoveAlsoKnownAs { value: v }
            }
            OpKind::AddVerificationMethod | OpKind::UpdateVerificationMethod => {
                let id = f_id.read().trim().to_string();
                let pk_x = f_pk_x.read().trim().to_string();
                let pk_y = f_pk_y.read().trim().to_string();
                if id.is_empty() || pk_x.is_empty() || pk_y.is_empty() {
                    form_error.set(Some("id, pk.x, pk.y are required".into()));
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
                    form_error.set(Some("id is required".into()));
                    return;
                }
                DidOperation::RemoveVerificationMethod { id }
            }
            OpKind::AddVerificationMethodRelation => {
                let method_id = f_method_id.read().trim().to_string();
                if method_id.is_empty() {
                    form_error.set(Some("method_id is required".into()));
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
                    form_error.set(Some("method_id is required".into()));
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
                    form_error.set(Some("id, type, endpoint are required".into()));
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
                    form_error.set(Some("id is required".into()));
                    return;
                }
                DidOperation::RemoveService { id }
            }
            OpKind::Deactivate => unreachable!("Deactivate not buildable here"),
        };
        form_error.set(None);
        let mut q = queue.read().clone();
        q.push((drafted, QueueStatus::Pending));
        queue.set(q);
    };

    let did_for_submit = did.clone();
    let sk_for_submit = controller_secret;
    let on_submit_batch = move |_| {
        if *running.read() {
            return;
        }
        let snapshot: Vec<DidOperation> = queue
            .read()
            .iter()
            .filter_map(|(op, st)| match st {
                QueueStatus::Pending | QueueStatus::Failed { .. } => Some(op.clone()),
                _ => None,
            })
            .collect();
        if snapshot.is_empty() {
            batch_error.set(Some("queue is empty".into()));
            return;
        }
        let Ok(did_id) = wallet_core::DidId::parse(&did_for_submit) else {
            batch_error.set(Some(format!("parse DID: {}", did_for_submit)));
            return;
        };
        // Reset queue to all-pending so we don't carry stale ✓
        // markers from a previous run.
        let reset: Vec<(DidOperation, QueueStatus)> = queue
            .read()
            .iter()
            .map(|(op, _)| (op.clone(), QueueStatus::Pending))
            .collect();
        queue.set(reset);
        batch_error.set(None);
        running.set(true);

        let did_for_log = did_for_submit.clone();
        let on_event = on_event.clone();
        let on_resolved = on_resolved.clone();
        let mut loaded_set: std::collections::HashSet<String> =
            loaded_circuits.iter().cloned().collect();
        let mut counter_cursor: u32 = initial_counter;
        spawn(async move {
            use futures::StreamExt;
            use wallet_core::WizardStage;
            let wallet = Wallet::demo(network);
            let total = queue.read().len();
            for i in 0..total {
                let op = {
                    let q = queue.read();
                    q[i].0.clone()
                };
                let circuit = op.circuit().to_string();

                // ── Phase 1: auto-load VK if not on-chain ─────
                let mut loaded_tx_hex: Option<String> = None;
                if !loaded_set.contains(&circuit) {
                    {
                        let mut q = queue.read().clone();
                        q[i].1 = QueueStatus::Running {
                            phase: format!("Loading VK ({circuit})"),
                        };
                        queue.set(q);
                    }
                    let mut load_stream = std::pin::pin!(wallet.load_did_circuit(
                        did_id.clone(),
                        circuit.clone(),
                        counter_cursor,
                    ));
                    let mut load_terminal: Option<WizardStage> = None;
                    while let Some(stage) = load_stream.next().await {
                        if matches!(&stage, WizardStage::Done(_) | WizardStage::Failed(_)) {
                            load_terminal = Some(stage);
                            break;
                        }
                    }
                    match load_terminal {
                        Some(WizardStage::Done(o)) => {
                            let load_tx = hex::encode(o.tx_hash);
                            loaded_tx_hex = Some(load_tx.clone());
                            loaded_set.insert(circuit.clone());
                            counter_cursor = counter_cursor.saturating_add(1);
                            on_event.call(SessionEvent::LoadCircuit {
                                did: did_for_log.clone(),
                                circuit: format!("{circuit} (auto-load VK)"),
                                tx_hash: o.tx_hash,
                                block_hash: o.block_hash,
                            });
                            // Give the indexer a beat to pick the
                            // new VK up before the ContractCall
                            // tries to look it up. The live batch
                            // test settles 30s between writes;
                            // the auto-load path is the same
                            // shape so use the same floor.
                            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                        }
                        Some(WizardStage::Failed(msg)) => {
                            let mut q = queue.read().clone();
                            q[i].1 = QueueStatus::Failed {
                                err: format!("auto-load {circuit}: {msg}"),
                            };
                            for j in (i + 1)..total {
                                q[j].1 = QueueStatus::Skipped;
                            }
                            queue.set(q);
                            break;
                        }
                        _ => {
                            let mut q = queue.read().clone();
                            q[i].1 = QueueStatus::Failed {
                                err: format!(
                                    "auto-load {circuit}: stream ended without terminal stage",
                                ),
                            };
                            for j in (i + 1)..total {
                                q[j].1 = QueueStatus::Skipped;
                            }
                            queue.set(q);
                            break;
                        }
                    }
                }

                // ── Phase 2: ContractCall the circuit ─────────
                {
                    let mut q = queue.read().clone();
                    q[i].1 = QueueStatus::Running {
                        phase: format!("Calling {circuit}"),
                    };
                    queue.set(q);
                }
                let mut stream = std::pin::pin!(wallet.call_did_circuit(
                    did_id.clone(),
                    circuit.clone(),
                    op.args_json(),
                    sk_for_submit,
                ));
                let mut terminal: Option<WizardStage> = None;
                while let Some(stage) = stream.next().await {
                    if matches!(&stage, WizardStage::Done(_) | WizardStage::Failed(_)) {
                        terminal = Some(stage);
                        break;
                    }
                }
                match terminal {
                    Some(WizardStage::Done(o)) => {
                        let tx_hex = hex::encode(o.tx_hash);
                        let block_hash = o.block_hash;
                        let mut q = queue.read().clone();
                        q[i].1 = QueueStatus::Done {
                            tx_hex: tx_hex.clone(),
                            loaded_tx_hex: loaded_tx_hex.clone(),
                        };
                        queue.set(q);
                        on_event.call(SessionEvent::LoadCircuit {
                            did: did_for_log.clone(),
                            circuit: circuit.clone(),
                            tx_hash: o.tx_hash,
                            block_hash,
                        });
                        // Settle between ops so the next call's
                        // `prepareUnprovenCallTx` reads fresh
                        // state. ContractCall doesn't bump the
                        // maintenance counter, but does change
                        // `version` + the operations transcript
                        // — both feed back into the harness.
                        if i + 1 < total {
                            tokio::time::sleep(std::time::Duration::from_secs(15)).await;
                        }
                    }
                    Some(WizardStage::Failed(msg)) => {
                        let mut q = queue.read().clone();
                        q[i].1 = QueueStatus::Failed { err: msg };
                        // Mark every later op as skipped so the
                        // user sees that the batch was aborted.
                        for j in (i + 1)..total {
                            q[j].1 = QueueStatus::Skipped;
                        }
                        queue.set(q);
                        break;
                    }
                    _ => {
                        let mut q = queue.read().clone();
                        q[i].1 = QueueStatus::Failed {
                            err: "stream ended without terminal stage".into(),
                        };
                        for j in (i + 1)..total {
                            q[j].1 = QueueStatus::Skipped;
                        }
                        queue.set(q);
                        break;
                    }
                }
            }
            // Re-resolve the DID so the surrounding detail view
            // reflects the new state (counter, vm count, etc.).
            match wallet.resolve_did_full(&did_for_log).await {
                Ok(r) => on_resolved.call(r),
                Err(e) => tracing::warn!(error=%e, "post-batch resolve failed"),
            }
            running.set(false);
        });
    };

    let on_clear_batch = move |_: dioxus::events::MouseEvent| {
        if *running.read() {
            return;
        }
        queue.set(Vec::new());
        batch_error.set(None);
    };

    let cur_idx = *op_idx.read();
    let cur_op = BUILDABLE_OPS[cur_idx];
    let cur_kt = *f_key_type_idx.read();
    let cur_cv = *f_curve_idx.read();
    let cur_rel = *f_relation_idx.read();
    let queue_len = queue.read().len();
    let is_running = *running.read();

    rsx! {
        div { class: "detail-back-row",
            button { onclick: move |_| on_back.call(()),
                "← Back to detail"
            }
        }
        div { class: "op-builder",
            // ── Pane 1 : palette ──────────────────────────────
            div { class: "op-pane palette",
                h3 { "Operations" }
                ul { class: "op-list",
                    for (i , kind) in BUILDABLE_OPS.iter().enumerate() {
                        li {
                            class: if i == cur_idx { "op-item active" } else { "op-item" },
                            onclick: move |_| op_idx.set(i),
                            "{kind.circuit_name()}"
                        }
                    }
                }
            }

            // ── Pane 2 : form ─────────────────────────────────
            div { class: "op-pane form",
                h3 { "{cur_op.circuit_name()}" }
                match cur_op {
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
                            placeholder: "field element (decimal or 0x… hex)",
                        }
                        FormRow {
                            label: "pk.y",
                            value: f_pk_y.read().clone(),
                            on_change: move |s: String| f_pk_y.set(s),
                            placeholder: "field element (decimal or 0x… hex)",
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
                    OpKind::AddVerificationMethodRelation
                    | OpKind::RemoveVerificationMethodRelation => rsx! {
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
                        div { class: "detail-empty",
                            "Deactivate has its own button in the header."
                        }
                    },
                }

                div { class: "row",
                    button {
                        disabled: is_running,
                        onclick: on_add_to_batch,
                        "Add to batch"
                    }
                }
                if let Some(msg) = form_error.read().as_ref() {
                    div { class: "wizard-outcome err",
                        div { class: "row label", "Validation" }
                        div { class: "seed-blob", "{msg}" }
                    }
                }
            }

            // ── Pane 3 : preview / queue ──────────────────────
            div { class: "op-pane preview",
                h3 { "Batch ({queue_len})" }
                if queue_len == 0 {
                    div { class: "detail-empty",
                        "Nothing queued yet. Configure an op on the left, then \"Add to batch\"."
                    }
                } else {
                    ol { class: "op-queue",
                        for (i , entry) in queue.read().iter().enumerate() {
                            li { class: "op-queue-row",
                                span { class: queue_status_class(&entry.1),
                                    "{queue_status_label(&entry.1)}"
                                }
                                span { class: "op-queue-name", "{i + 1}. {entry.0.circuit()}" }
                                span { class: "op-queue-summary", "{entry.0.summary()}" }
                                if let QueueStatus::Running { phase } = &entry.1 {
                                    div { class: "op-queue-phase", "{phase}…" }
                                }
                                if let QueueStatus::Done { tx_hex, loaded_tx_hex } = &entry.1 {
                                    if let Some(load_tx) = loaded_tx_hex {
                                        div { class: "op-queue-tx muted",
                                            "auto-load VK · tx 0x{load_tx}"
                                        }
                                    }
                                    div { class: "op-queue-tx", "tx 0x{tx_hex}" }
                                }
                                if let QueueStatus::Failed { err } = &entry.1 {
                                    div { class: "op-queue-err", "{err}" }
                                }
                            }
                        }
                    }
                    div { class: "row",
                        button {
                            class: "btn-primary",
                            disabled: is_running,
                            onclick: on_submit_batch,
                            {if is_running { "Submitting…" } else { "Submit batch" }}
                        }
                        button {
                            disabled: is_running,
                            onclick: on_clear_batch,
                            "Clear"
                        }
                    }
                }
                if let Some(msg) = batch_error.read().as_ref() {
                    div { class: "wizard-outcome err",
                        div { class: "row label", "Batch error" }
                        div { class: "seed-blob", "{msg}" }
                    }
                }
            }
        }
    }
}

fn queue_status_label(s: &QueueStatus) -> &'static str {
    match s {
        QueueStatus::Pending => "•",
        QueueStatus::Running { .. } => "…",
        QueueStatus::Done { .. } => "✓",
        QueueStatus::Failed { .. } => "✗",
        QueueStatus::Skipped => "—",
    }
}

fn queue_status_class(s: &QueueStatus) -> &'static str {
    match s {
        QueueStatus::Pending => "op-stat pending",
        QueueStatus::Running { .. } => "op-stat running",
        QueueStatus::Done { .. } => "op-stat done",
        QueueStatus::Failed { .. } => "op-stat failed",
        QueueStatus::Skipped => "op-stat skipped",
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

/// Result of a `bridgeWitnessTest` round-trip — Rust → JS → Rust →
/// JS → Rust. Verifies the witness-callback chain we need before
/// real circuit execution.
#[derive(Clone, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize, Debug)]
#[serde(rename_all = "camelCase", default)]
struct WitnessTestResult {
    source_length: i64,
    controller_pk_public: String,
    secret_hex_first8: String,
    elapsed_ms: i64,
    error: Option<String>,
}

#[component]
fn JsBridgePanel(seed_did: Option<String>) -> Element {
    let mut message = use_signal(|| "hello from rust".to_string());
    let mut result = use_signal::<Option<Result<BridgeProbeResult, String>>>(|| None);
    let mut pending = use_signal(|| false);
    let mut witness_did = use_signal(|| seed_did.clone().unwrap_or_default());
    use_effect(move || {
        if let Some(seed) = seed_did.clone() {
            if *witness_did.read() != seed {
                witness_did.set(seed);
            }
        }
    });
    let mut witness_result = use_signal::<Option<Result<WitnessTestResult, String>>>(|| None);
    let mut witness_pending = use_signal(|| false);

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

    let probe_witness = move |_| {
        if *witness_pending.read() {
            return;
        }
        let did = witness_did.read().trim().to_string();
        if did.is_empty() {
            witness_result.set(Some(Err("enter a DID created in this session first".into())));
            return;
        }
        witness_pending.set(true);
        witness_result.set(None);
        let did_json = serde_json::to_string(&did).unwrap_or_else(|_| "\"\"".into());
        // Nested chain: this eval calls `bridgeWitnessTest` which
        // internally awaits `window.midnightWallet.getControllerSecretKey({ did })`
        // — i.e. JS → Rust → JS → continued execution → final return.
        // Verifies the witness-callback chain we need for ContractCall.
        let snippet = format!(
            r#"if (!window.midnightDidBundle?.bridgeWitnessTest) {{
                return {{ error: "bridgeWitnessTest not loaded" }};
            }}
            try {{
                const r = await window.midnightDidBundle.bridgeWitnessTest({{ did: {did_json} }});
                return r;
            }} catch (e) {{
                return {{ error: String(e?.message ?? e) }};
            }}"#
        );
        spawn(async move {
            let r: Result<WitnessTestResult, String> = match document::eval(&snippet).await {
                Ok(v) => serde_json::from_value::<WitnessTestResult>(v)
                    .map_err(|e| format!("decode: {e}")),
                Err(e) => Err(format!("eval failed: {e}")),
            };
            witness_result.set(Some(r));
            witness_pending.set(false);
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
        div { class: "row",
            input {
                r#type: "text",
                placeholder: "did:midnight:undeployed:… (witness lookup)",
                value: "{witness_did.read()}",
                oninput: move |e| witness_did.set(e.value()),
                style: "flex: 1; padding: 6px 8px; background: var(--surface-2); color: var(--text); border: 1px solid var(--border); border-radius: 6px; font-family: ui-monospace, monospace; font-size: 11px;"
            }
            button {
                disabled: *witness_pending.read(),
                onclick: probe_witness,
                {if *witness_pending.read() { "Witness…" } else { "Witness test" }}
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
        if let Some(r) = witness_result.read().as_ref() {
            match r {
                Ok(w) => {
                    if let Some(err) = w.error.as_ref() {
                        rsx! {
                            div { class: "wizard-outcome err",
                                div { class: "row label", "Witness JS-side error" }
                                div { class: "seed-blob", "{err}" }
                            }
                        }
                    } else {
                        rsx! {
                            div { class: "wizard-outcome ok",
                                div { class: "row label", "Witness round-trip OK (JS → Rust → JS chain works)" }
                                div { class: "did-meta-grid",
                                    div { class: "did-meta-cell",
                                        span { class: "label", "Secret prefix" }
                                        span { class: "value", "{w.secret_hex_first8}…" }
                                    }
                                    div { class: "did-meta-cell",
                                        span { class: "label", "Length" }
                                        span { class: "value", "{w.source_length} bytes" }
                                    }
                                    div { class: "did-meta-cell",
                                        span { class: "label", "Elapsed" }
                                        span { class: "value", "{w.elapsed_ms} ms" }
                                    }
                                    div { class: "did-meta-cell",
                                        span { class: "label", "Controller pk" }
                                        span { class: "value", "{w.controller_pk_public}…" }
                                    }
                                }
                            }
                        }
                    }
                }
                Err(e) => rsx! {
                    div { class: "wizard-outcome err",
                        div { class: "row label", "Witness eval error" }
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

/// Which detail-page tab is currently visible.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum DetailTab {
    Overview,
    Document,
    Methods,
    Relationships,
    Services,
    Operations,
    Sign,
    Resolver,
    RawState,
}

impl DetailTab {
    const ALL: &'static [DetailTab] = &[
        DetailTab::Overview,
        DetailTab::Document,
        DetailTab::Methods,
        DetailTab::Relationships,
        DetailTab::Services,
        DetailTab::Operations,
        DetailTab::Sign,
        DetailTab::Resolver,
        DetailTab::RawState,
    ];

    fn label(&self) -> &'static str {
        match self {
            DetailTab::Overview => "Overview",
            DetailTab::Document => "DID Document",
            DetailTab::Methods => "Methods",
            DetailTab::Relationships => "Relationships",
            DetailTab::Services => "Services",
            DetailTab::Operations => "Operations",
            DetailTab::Sign => "Sign",
            DetailTab::Resolver => "Resolver",
            DetailTab::RawState => "Raw state",
        }
    }
}

/// Centerpiece DID-detail view — adopts
/// `midnight-did-uiux-bundle/06-wireframes.md` (line 48–61). Lives
/// in the DIDs tab when an inventory row is "open" and renders an
/// 8-tab panel for the picked DID: Overview, DID Document,
/// Methods, Relationships, Services, Operations, Resolver,
/// Raw state.
///
/// Reads from `cached: Option<ResolvedDid>` — the parent passes
/// the most-recent resolve from `resolved_cache`. The "Resolve
/// latest" header button re-fetches from the chain and bubbles
/// the new `ResolvedDid` via `on_resolved`, which the parent
/// writes back into the cache.
///
/// The "Deactivate" button fires
/// `Wallet::call_did_circuit("deactivate", [])` — only enabled
/// if `controller_known` (the per-DID random sk is in the
/// session's `BridgeState.controller_secrets`).
///
/// `on_back` returns to the inventory/browse view; the parent
/// clears its `open_did` signal.
#[component]
fn DidDetailView(
    network: Network,
    did: String,
    cached: Option<wallet_core::ResolvedDid>,
    /// The resolve immediately preceding `cached`, if any. The
    /// Resolver tab diffs these two to surface "what changed
    /// since the previous resolve" (counter / VM / service /
    /// alsoKnownAs / loaded-VKs deltas). `None` on the first
    /// successful resolve of a DID this session.
    previous_cached: Option<wallet_core::ResolvedDid>,
    /// Per-DID random sk if this session has it (the wallet
    /// minted the DID here). `None` means the user resolved a
    /// DID created elsewhere — Deactivate is disabled.
    controller_secret: Option<[u8; 32]>,
    session_log: Vec<SessionEvent>,
    on_back: EventHandler<()>,
    on_resolved: EventHandler<wallet_core::ResolvedDid>,
    on_deactivated: EventHandler<(String, wallet_core::DeployOutcome)>,
    on_timing: EventHandler<TimingRun>,
    on_event: EventHandler<SessionEvent>,
) -> Element {
    use wallet_core::WizardStage;

    let mut tab = use_signal(|| DetailTab::Overview);
    let mut resolving = use_signal(|| false);
    let mut resolve_error = use_signal::<Option<String>>(|| None);
    let mut deactivating = use_signal::<Vec<WizardStage>>(Vec::new);
    let mut deactivate_error = use_signal::<Option<String>>(|| None);
    // When true, render `DidOperationBuilder` instead of the
    // 8-tab view. Toggled by the "Update DID" button (which is
    // disabled unless we have the controller secret for this DID
    // — write circuits need it for the `localSecretKey()`
    // witness).
    let mut builder_mode = use_signal(|| false);
    let controller_known = controller_secret.is_some();

    // Click handler for "Resolve latest".
    let did_for_resolve = did.clone();
    let resolve_latest = move |_| {
        if *resolving.read() {
            return;
        }
        resolving.set(true);
        resolve_error.set(None);
        let did_str = did_for_resolve.clone();
        let on_resolved = on_resolved.clone();
        spawn(async move {
            let w = Wallet::demo(network);
            match w.resolve_did_full(&did_str).await {
                Ok(r) => on_resolved.call(r),
                Err(e) => resolve_error.set(Some(e.to_string())),
            }
            resolving.set(false);
        });
    };

    // Click handler for "Deactivate". Drives the full
    // Wallet::call_did_circuit("deactivate") pipeline, surfacing
    // each WizardStage so the user sees the progress.
    let did_for_deactivate = did.clone();
    let sk_for_deactivate = controller_secret;
    let deactivate = move |_| {
        let Some(sk) = sk_for_deactivate else {
            deactivate_error.set(Some(
                "controller secret not in session — was this DID created here?".into(),
            ));
            return;
        };
        if !deactivating.read().is_empty()
            && !matches!(
                deactivating.read().last(),
                Some(WizardStage::Done(_)) | Some(WizardStage::Failed(_))
            )
        {
            return; // already in flight
        }
        deactivate_error.set(None);
        deactivating.set(Vec::new());
        let did_str = did_for_deactivate.clone();
        let on_deactivated = on_deactivated.clone();
        let on_timing = on_timing.clone();
        spawn(async move {
            use futures::StreamExt;
            let w = Wallet::demo(network);
            let did_id = match wallet_core::DidId::parse(&did_str) {
                Ok(d) => d,
                Err(e) => {
                    deactivate_error.set(Some(format!("parse DID: {e}")));
                    return;
                }
            };
            let timing_label = "call_did_circuit:deactivate".to_string();
            let mut observations: Vec<(usize, std::time::Instant)> = Vec::new();
            let mut stream = std::pin::pin!(w.call_did_circuit(
                did_id,
                "deactivate".to_string(),
                serde_json::json!([]),
                sk,
            ));
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
                let mut current = deactivating.read().clone();
                if let WizardStage::Done(o) = &stage {
                    on_deactivated.call((did_str.clone(), o.clone()));
                } else if let WizardStage::Failed(msg) = &stage {
                    deactivate_error.set(Some(msg.clone()));
                }
                current.push(stage);
                deactivating.set(current);
            }
        });
    };

    // Auto-resolve on first mount if we don't have anything
    // cached yet — saves the user a click.
    let mut auto_resolve_done = use_signal(|| false);
    {
        let did_for_auto = did.clone();
        let cached_some = cached.is_some();
        use_effect(move || {
            if !cached_some && !*auto_resolve_done.read() {
                auto_resolve_done.set(true);
                let did_str = did_for_auto.clone();
                resolving.set(true);
                spawn(async move {
                    let w = Wallet::demo(network);
                    match w.resolve_did_full(&did_str).await {
                        Ok(r) => on_resolved.call(r),
                        Err(e) => resolve_error.set(Some(e.to_string())),
                    }
                    resolving.set(false);
                });
            }
        });
    }

    let did_short = truncate_did(&did);
    let did_full = did.clone();
    let status_label = match cached.as_ref() {
        None => "Resolving…",
        Some(r) => {
            if r.document.deactivated {
                "Deactivated"
            } else {
                "Active"
            }
        }
    };
    let status_class = match cached.as_ref() {
        None => "did-badge pending",
        Some(r) => {
            if r.document.deactivated {
                "did-badge deactivated"
            } else {
                "did-badge active"
            }
        }
    };
    let version = cached
        .as_ref()
        .map(|r| format!("v{}", r.document.version))
        .unwrap_or_else(|| "—".to_string());
    let cur_tab = *tab.read();

    // Builder mode short-circuits the 8-tab render. We still
    // require the controller secret (UI guards this on the
    // toggle) — if we somehow ended up here without one, drop
    // back to tabs.
    if *builder_mode.read() {
        if let Some(sk) = controller_secret {
            let did_for_builder = did.clone();
            // Pull the on-chain VK set + counter from the cached
            // resolve. If we haven't resolved yet, the builder
            // gets an empty set and counter 0 — every queued op
            // will be auto-loaded (counter starts at 0 on a
            // fresh deploy, so this is also correct).
            let (loaded_circuits, initial_counter) = cached
                .as_ref()
                .map(|r| (r.loaded_circuits.clone(), r.maintenance_counter))
                .unwrap_or_else(|| (Vec::new(), 0));
            return rsx! {
                DidOperationBuilder {
                    network,
                    did: did_for_builder,
                    controller_secret: sk,
                    loaded_circuits,
                    initial_counter,
                    on_back: move |_| builder_mode.set(false),
                    on_event,
                    on_resolved,
                }
            };
        }
        // Defensive fallback if we lost the secret somehow.
        builder_mode.set(false);
    }

    let deactivated_now = cached
        .as_ref()
        .map(|r| r.document.deactivated)
        .unwrap_or(false);
    let update_disabled = !controller_known || deactivated_now;
    let update_title = if !controller_known {
        "Controller secret unknown — was this DID created in another session?"
    } else if deactivated_now {
        "DID is deactivated; no further updates accepted"
    } else {
        "Open the Operation Builder (palette / form / preview)"
    };

    rsx! {
        div { class: "detail-back-row",
            button { onclick: move |_| on_back.call(()), "← Back to inventory" }
        }
        div { class: "detail-header",
            div { class: "did-line",
                span { class: "{status_class}", "{status_label}" }
                span { class: "version", "{version}" }
                span {
                    class: "did-text",
                    title: "{did_full}",
                    "{did_short}"
                }
            }
            div { class: "actions",
                button {
                    class: "btn-primary",
                    disabled: update_disabled,
                    title: "{update_title}",
                    onclick: move |_| builder_mode.set(true),
                    "Update DID"
                }
                button {
                    disabled: *resolving.read(),
                    onclick: resolve_latest,
                    {if *resolving.read() { "Resolving…" } else { "Resolve latest" }}
                }
                button {
                    class: "btn-danger",
                    disabled: !controller_known
                        || cached.as_ref().map(|r| r.document.deactivated).unwrap_or(false),
                    title: if controller_known {
                        "Submit a deactivate ContractCall via the JS bridge"
                    } else {
                        "Controller secret unknown — was this DID created in another session?"
                    },
                    onclick: deactivate,
                    "Deactivate"
                }
            }
            if let Some(err) = resolve_error.read().as_ref() {
                div { class: "wizard-outcome err",
                    div { class: "row label", "Resolve failed" }
                    div { class: "seed-blob", "{err}" }
                }
            }
            if let Some(err) = deactivate_error.read().as_ref() {
                div { class: "wizard-outcome err",
                    div { class: "row label", "Deactivate failed" }
                    div { class: "seed-blob", "{err}" }
                }
            }
            {
                let stages_snap = deactivating.read().clone();
                if !stages_snap.is_empty() {
                    let term = terminal(&stages_snap);
                    rsx! {
                        ul { class: "wizard-steps",
                            for (idx , label) in PIPELINE.iter().enumerate() {
                                {render_step_row(idx, label, step_status(idx, &stages_snap))}
                            }
                        }
                        if let Some(TerminalView::Done(o)) = &term {
                            div { class: "wizard-outcome ok",
                                div { class: "row label", "Deactivate landed" }
                                div { class: "seed-blob", "tx 0x{hex::encode(o.tx_hash)}" }
                                div { class: "seed-blob", "block 0x{hex::encode(o.block_hash)}" }
                            }
                        }
                    }
                } else {
                    rsx! { "" }
                }
            }
        }
        div { class: "detail-tabs",
            for t in DetailTab::ALL {
                button {
                    class: if cur_tab == *t { "detail-tab active" } else { "detail-tab" },
                    onclick: move |_| tab.set(*t),
                    "{t.label()}"
                }
            }
        }
        div { class: "detail-pane",
            {render_detail_tab(
                cur_tab,
                network,
                did.clone(),
                cached.as_ref(),
                previous_cached.as_ref(),
                controller_secret,
                &session_log,
            )}
        }
    }
}

fn render_detail_tab(
    tab: DetailTab,
    network: Network,
    did: String,
    resolved: Option<&wallet_core::ResolvedDid>,
    previous: Option<&wallet_core::ResolvedDid>,
    controller_secret: Option<[u8; 32]>,
    session_log: &[SessionEvent],
) -> Element {
    // Sign tab is the only one we let the user open before
    // the first successful resolve — the keypair derivation
    // doesn't depend on chain state. Every other tab needs
    // `resolved` to have any content.
    if tab == DetailTab::Sign {
        return rsx! {
            SignTab {
                network,
                did: did.clone(),
                controller_secret,
            }
        };
    }
    let Some(r) = resolved else {
        return rsx! {
            div { class: "detail-empty",
                "No resolved snapshot yet. Click \"Resolve latest\" or wait for the auto-resolve."
            }
        };
    };
    match tab {
        DetailTab::Overview => render_overview_tab(r),
        DetailTab::Document => render_document_tab(r),
        DetailTab::Methods => render_methods_tab(r),
        DetailTab::Relationships => render_relationships_tab(r),
        DetailTab::Services => render_services_tab(r),
        DetailTab::Operations => render_operations_tab(&did, session_log),
        DetailTab::Sign => unreachable!("handled above"),
        DetailTab::Resolver => render_resolver_tab(&did, r, previous),
        DetailTab::RawState => render_raw_state_tab(r),
    }
}

fn render_overview_tab(r: &wallet_core::ResolvedDid) -> Element {
    let counter = r.maintenance_counter;
    let vms = r.document.verification_method.len();
    let services = r.document.service.len();
    let block = r
        .last_block_height
        .map(|h| format_int(h))
        .unwrap_or_else(|| "—".into());
    let last_tx = if r.last_tx_hash.is_empty() {
        "—".to_string()
    } else {
        format!("0x{}", r.last_tx_hash)
    };
    rsx! {
        h3 { "Summary" }
        div { class: "did-meta-grid",
            div { class: "did-meta-cell",
                span { class: "label", "Version" }
                span { class: "value", "{r.document.version}" }
            }
            div { class: "did-meta-cell",
                span { class: "label", "Maintenance counter" }
                span { class: "value", "{counter}" }
            }
            div { class: "did-meta-cell",
                span { class: "label", "Methods" }
                span { class: "value", "{vms}" }
            }
            div { class: "did-meta-cell",
                span { class: "label", "Services" }
                span { class: "value", "{services}" }
            }
            div { class: "did-meta-cell",
                span { class: "label", "Last block" }
                span { class: "value", "{block}" }
            }
            div { class: "did-meta-cell",
                span { class: "label", "Last tx" }
                span { class: "value", title: "{last_tx}", "{last_tx}" }
            }
        }
    }
}

fn render_document_tab(r: &wallet_core::ResolvedDid) -> Element {
    let json = serde_json::to_string_pretty(&r.document)
        .unwrap_or_else(|e| format!("serialise: {e}"));
    rsx! {
        h3 { "DID Document" }
        div { class: "seed-blob", "{json}" }
    }
}

fn render_methods_tab(r: &wallet_core::ResolvedDid) -> Element {
    if r.document.verification_method.is_empty() {
        return rsx! {
            h3 { "Verification methods" }
            div { class: "detail-empty",
                "This DID has no verification methods. Add one via the Operation Builder (coming soon)."
            }
        };
    }
    rsx! {
        h3 { "Verification methods" }
        table { class: "detail-table",
            thead {
                tr {
                    th { "ID" }
                    th { "Type" }
                    th { "Curve" }
                    th { "" }
                }
            }
            tbody {
                for vm in r.document.verification_method.iter() {
                    {
                        // Type/curve names come straight from the JWK
                        let kty = format!("{:?}", vm.public_key_jwk.kty);
                        let crv = format!("{:?}", vm.public_key_jwk.crv);
                        let id = vm.id.clone();
                        rsx! {
                            tr {
                                td { "{vm.id}" }
                                td { class: "muted", "{kty}" }
                                td { class: "muted", "{crv}" }
                                td { {copy_btn(id, "Copy DID URL")} }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn render_relationships_tab(r: &wallet_core::ResolvedDid) -> Element {
    // Rows = each verification method id; columns = relations.
    // Cells show ✓ when the method is in that relation's set,
    // — otherwise.
    if r.document.verification_method.is_empty() {
        return rsx! {
            h3 { "Relationships" }
            div { class: "detail-empty",
                "Add a verification method first to see the relationship matrix."
            }
        };
    }
    // Strip the DID prefix from VM ids if present so the matrix
    // shows fragment ids only (matches the on-chain raw form).
    let method_ids: Vec<String> = r
        .document
        .verification_method
        .iter()
        .map(|vm| vm.id.rsplit('#').next().unwrap_or(&vm.id).to_string())
        .collect();
    let auth = &r.authentication_ids;
    let assr = &r.assertion_method_ids;
    let ka = &r.key_agreement_ids;
    let ci = &r.capability_invocation_ids;
    let cd = &r.capability_delegation_ids;
    rsx! {
        h3 { "Verification relationships" }
        table { class: "relmat",
            thead {
                tr {
                    th { "Method" }
                    th { "Auth" }
                    th { "Assert" }
                    th { "KeyAgr" }
                    th { "CapInv" }
                    th { "CapDel" }
                }
            }
            tbody {
                for mid in method_ids.iter() {
                    {render_relation_row(mid, auth, assr, ka, ci, cd)}
                }
            }
        }
    }
}

fn render_relation_row(
    mid: &str,
    auth: &[String],
    assr: &[String],
    ka: &[String],
    ci: &[String],
    cd: &[String],
) -> Element {
    let cell = |present: bool| {
        if present {
            rsx! { td { class: "relcheck", "✓" } }
        } else {
            rsx! { td { class: "reldash", "—" } }
        }
    };
    rsx! {
        tr {
            td { "{mid}" }
            {cell(auth.iter().any(|x| x == mid))}
            {cell(assr.iter().any(|x| x == mid))}
            {cell(ka.iter().any(|x| x == mid))}
            {cell(ci.iter().any(|x| x == mid))}
            {cell(cd.iter().any(|x| x == mid))}
        }
    }
}

fn render_services_tab(r: &wallet_core::ResolvedDid) -> Element {
    if r.document.service.is_empty() {
        return rsx! {
            h3 { "Services" }
            div { class: "detail-empty",
                "This DID exposes no service endpoints."
            }
        };
    }
    rsx! {
        h3 { "Services" }
        table { class: "detail-table",
            thead {
                tr {
                    th { "ID" }
                    th { "Type" }
                    th { "Endpoint" }
                    th { "" }
                }
            }
            tbody {
                for s in r.document.service.iter() {
                    {
                        let endpoint = match &s.service_endpoint {
                            wallet_core::ServiceEndpoint::Uri(u) => u.clone(),
                            wallet_core::ServiceEndpoint::Object(v) => v.to_string(),
                        };
                        let id = s.id.clone();
                        let endpoint_clip = endpoint.clone();
                        rsx! {
                            tr {
                                td { "{s.id}" }
                                td { class: "muted", "{s.typ}" }
                                td { class: "muted", "{endpoint}" }
                                td {
                                    {copy_btn(id, "Copy service id")}
                                    {copy_btn(endpoint_clip, "Copy endpoint")}
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn render_operations_tab(did: &str, session_log: &[SessionEvent]) -> Element {
    // Operations history for this DID — filter the global
    // session log down to events that reference it. Renders the
    // same row component the SessionLogPanel uses.
    let matches: Vec<(usize, &SessionEvent)> = session_log
        .iter()
        .enumerate()
        .filter(|(_, e)| match e {
            SessionEvent::Deploy { did: d, .. } => d == did,
            SessionEvent::Resolve { did: d, .. } => d == did,
            SessionEvent::LoadCircuit { did: d, .. } => d == did,
            SessionEvent::OperationDrafted { did: d, .. } => d == did,
        })
        .collect();
    if matches.is_empty() {
        return rsx! {
            h3 { "Operations" }
            div { class: "detail-empty",
                "No operations on this DID in the current session yet."
            }
        };
    }
    rsx! {
        h3 { "Operations" }
        ul { class: "session-log",
            for (idx , event) in matches.iter().rev() {
                {render_session_entry(*idx, event)}
            }
        }
    }
}

fn render_resolver_tab(
    did: &str,
    r: &wallet_core::ResolvedDid,
    previous: Option<&wallet_core::ResolvedDid>,
) -> Element {
    // Resolver diagnostics — adopts the bundle's "Resolve DID"
    // diagnostics card (prototype/index.html line 112-113).
    let id = &r.document.id;
    let net = id.network.label();
    let block = r
        .last_block_height
        .map(|h| format_int(h))
        .unwrap_or_else(|| "—".into());
    let addr_hex = id.contract_address_hex();
    // Raw-state size in bytes (hex string length / 2) and a
    // short fingerprint — first 8 + last 4 chars of the hex —
    // so the user can eyeball whether two resolves of the same
    // DID hit the same on-chain state without diffing the full
    // ~kB blob.
    let raw_bytes = r.raw_state_hex.len() / 2;
    let raw_fp = state_fingerprint(&r.raw_state_hex);
    let loaded = r.loaded_circuits.len();
    let loaded_summary = if loaded == 0 {
        "—".to_string()
    } else {
        r.loaded_circuits.join(", ")
    };
    rsx! {
        h3 { "Resolver diagnostics" }
        div { class: "detail-kv",
            div { class: "k", "DID syntax" }
            div { class: "v", "Valid · parsed by wallet_core::DidId::parse" }
            div { class: "k", "Network" }
            div { class: "v", "{net}" }
            div { class: "k", "Contract address" }
            div { class: "v", "0x{addr_hex}" }
            div { class: "k", "Status" }
            div { class: "v",
                {if r.document.deactivated { "Deactivated" } else { "Active" }}
            }
            div { class: "k", "Version" }
            div { class: "v", "{r.document.version}" }
            div { class: "k", "Maintenance counter" }
            div { class: "v", "{r.maintenance_counter}" }
            div { class: "k", "Resolver latency" }
            div { class: "v", "{r.resolve_latency_ms} ms" }
            div { class: "k", "Last indexed block" }
            div { class: "v", "{block}" }
            div { class: "k", "Raw state size" }
            div { class: "v", "{raw_bytes} bytes" }
            div { class: "k", "Raw state fingerprint" }
            div { class: "v", "{raw_fp}" }
            div { class: "k", "Loaded VKs" }
            div { class: "v", "{loaded} ({loaded_summary})" }
            div { class: "k", "DID input" }
            div { class: "v", "{did}" }
        }
        {render_resolve_diff(r, previous)}
    }
}

/// Render the "what changed since the previous resolve" diff
/// card. `None` previous → render a placeholder so the user
/// understands the card exists. Otherwise enumerate the deltas
/// we care about: counter, version, deactivated, vm + service
/// counts, alsoKnownAs, services, loaded VKs, raw state size /
/// fingerprint.
fn render_resolve_diff(
    cur: &wallet_core::ResolvedDid,
    prev: Option<&wallet_core::ResolvedDid>,
) -> Element {
    let Some(prev) = prev else {
        return rsx! {
            h3 { "Cross-resolve diff" }
            div { class: "detail-empty",
                "Only one resolve recorded this session. Click \"Resolve latest\" again to compare."
            }
        };
    };

    let mut rows: Vec<(String, String, String)> = Vec::new();
    let push = |rows: &mut Vec<(String, String, String)>, k: &str, prev: String, cur: String| {
        if prev != cur {
            rows.push((k.to_string(), prev, cur));
        }
    };
    push(
        &mut rows,
        "Version",
        prev.document.version.to_string(),
        cur.document.version.to_string(),
    );
    push(
        &mut rows,
        "Counter",
        prev.maintenance_counter.to_string(),
        cur.maintenance_counter.to_string(),
    );
    push(
        &mut rows,
        "Deactivated",
        prev.document.deactivated.to_string(),
        cur.document.deactivated.to_string(),
    );
    push(
        &mut rows,
        "Methods",
        prev.document.verification_method.len().to_string(),
        cur.document.verification_method.len().to_string(),
    );
    push(
        &mut rows,
        "Services",
        prev.document.service.len().to_string(),
        cur.document.service.len().to_string(),
    );
    push(
        &mut rows,
        "alsoKnownAs",
        prev.document.also_known_as.len().to_string(),
        cur.document.also_known_as.len().to_string(),
    );
    push(
        &mut rows,
        "Loaded VKs",
        prev.loaded_circuits.len().to_string(),
        cur.loaded_circuits.len().to_string(),
    );
    push(
        &mut rows,
        "Last block",
        prev.last_block_height
            .map(|h| format_int(h))
            .unwrap_or_else(|| "—".into()),
        cur.last_block_height
            .map(|h| format_int(h))
            .unwrap_or_else(|| "—".into()),
    );
    push(
        &mut rows,
        "Last tx",
        format!(
            "0x{}",
            short_hex_or_dash(&prev.last_tx_hash),
        ),
        format!(
            "0x{}",
            short_hex_or_dash(&cur.last_tx_hash),
        ),
    );
    let prev_fp = state_fingerprint(&prev.raw_state_hex);
    let cur_fp = state_fingerprint(&cur.raw_state_hex);
    push(
        &mut rows,
        "Raw state fingerprint",
        prev_fp,
        cur_fp,
    );
    // VKs newly loaded since the previous resolve — useful to
    // confirm an auto-load step in the Operation Builder
    // actually landed.
    let prev_set: std::collections::HashSet<&str> =
        prev.loaded_circuits.iter().map(String::as_str).collect();
    let new_vks: Vec<&str> = cur
        .loaded_circuits
        .iter()
        .map(String::as_str)
        .filter(|c| !prev_set.contains(*c))
        .collect();
    if !new_vks.is_empty() {
        rows.push((
            "Newly loaded VKs".to_string(),
            "—".to_string(),
            new_vks.join(", "),
        ));
    }

    if rows.is_empty() {
        return rsx! {
            h3 { "Cross-resolve diff" }
            div { class: "detail-empty",
                "No fields changed between the previous and current resolve."
            }
        };
    }
    rsx! {
        h3 { "Cross-resolve diff" }
        table { class: "detail-table",
            thead {
                tr {
                    th { "Field" }
                    th { "Previous" }
                    th { "Current" }
                }
            }
            tbody {
                for (k , prev_v , cur_v) in rows.into_iter() {
                    tr {
                        td { "{k}" }
                        td { class: "muted", "{prev_v}" }
                        td { "{cur_v}" }
                    }
                }
            }
        }
    }
}

/// Short fingerprint of an opaque hex blob — first 8 + "…" +
/// last 4 chars. Cheap to glance, enough to spot when two
/// resolves see the same on-chain state without comparing the
/// full ~kB hex string.
fn state_fingerprint(hex: &str) -> String {
    let h = hex.trim_start_matches("0x");
    if h.len() <= 12 {
        h.to_string()
    } else {
        format!("{}…{}", &h[..8], &h[h.len() - 4..])
    }
}

fn short_hex_or_dash(hex: &str) -> String {
    let h = hex.trim_start_matches("0x");
    if h.is_empty() {
        "—".to_string()
    } else if h.len() <= 12 {
        h.to_string()
    } else {
        format!("{}…{}", &h[..8], &h[h.len() - 4..])
    }
}

fn render_raw_state_tab(r: &wallet_core::ResolvedDid) -> Element {
    let n = r.raw_state_hex.len() / 2;
    let full_hex = format!("0x{}", r.raw_state_hex);
    rsx! {
        h3 { "Raw ledger state ({n} bytes)" }
        div { class: "row",
            {copy_btn(full_hex.clone(), "Copy raw state hex")}
            span { style: "color: var(--text-muted); font-size: 11px;",
                "fingerprint {state_fingerprint(&r.raw_state_hex)}"
            }
        }
        div { class: "seed-blob", "{full_hex}" }
    }
}


/// `Sign` tab — demonstrates the in-tree Jubjub Schnorr port
/// against a user-typed payload. The signing key is derived
/// deterministically from `(controller_secret, did)` (see
/// `sign_tab_seed`), so the same DID always signs with the
/// same key. Local verify is instant; on-chain verify spawns
/// a one-shot Node harness, calls `schnorrVerify[Digest]`,
/// and surfaces the result — proving the Rust signature still
/// passes the upstream Compact circuit.
#[component]
fn SignTab(
    network: Network,
    did: String,
    controller_secret: Option<[u8; 32]>,
) -> Element {
    use wallet_core::secret_storage::jubjub_schnorr;

    let mut payload = use_signal(|| String::from("hello, did"));
    // Three results — one per verify path. `None` until the
    // user clicks; `Some(Ok(true|false))` after a clean run;
    // `Some(Err)` if the bridge / decoder failed.
    let mut local_result = use_signal::<Option<bool>>(|| None);
    let mut bridge_result = use_signal::<Option<Result<bool, String>>>(|| None);
    let mut upstream_result = use_signal::<Option<Result<bool, String>>>(|| None);
    let mut bridge_in_flight = use_signal(|| false);

    let Some(controller_secret) = controller_secret else {
        return rsx! {
            h3 { "Sign with Jubjub Schnorr" }
            div { class: "detail-empty",
                "Controller secret unknown — signing needs the wallet that created this DID."
            }
        };
    };

    // One round trip through the wallet-core diagnostic
    // helper does everything: derive pk, hash to digest, sign,
    // encode both wire forms. Re-deriving on every render is
    // cheap (~1ms) and keeps the component a pure function of
    // its inputs.
    let seed = jubjub_schnorr::seed_from_controller_and_did(&controller_secret, &did);
    let payload_bytes = payload.read().as_bytes().to_vec();
    let diag = jubjub_schnorr::sign_payload_diagnostic(&seed, &payload_bytes);
    let pk_x_dec = diag.pk_x_decimal.clone();
    let pk_y_dec = diag.pk_y_decimal.clone();
    let sig_compact_hex = diag.compact_hex.clone();
    let sig_upstream_hex = diag.upstream_hex.clone();
    let digest_dec = diag.digest_decimal.clone();

    let on_verify_local = {
        let seed = seed;
        let payload_bytes = payload_bytes.clone();
        let compact_bytes = hex::decode(&diag.compact_hex).expect("hex from sign diag");
        move |_| {
            local_result.set(Some(jubjub_schnorr::verify_payload_with_seed(
                &seed,
                &payload_bytes,
                &compact_bytes,
            )));
        }
    };

    // Reusable JSON builders for the two bridge methods. The
    // decimal-string fields come straight from the diagnostic.
    let digest_json: Vec<serde_json::Value> = diag
        .digest_decimal
        .iter()
        .map(|d| serde_json::json!({ "$bigint": d }))
        .collect();
    let pk_json = serde_json::json!({
        "x": { "$bigint": diag.pk_x_decimal },
        "y": { "$bigint": diag.pk_y_decimal },
    });
    let bridge_request_compact = serde_json::json!({
        "announcement": {
            "x": { "$bigint": diag.announcement_x_decimal },
            "y": { "$bigint": diag.announcement_y_decimal },
        },
        "publicKey": pk_json.clone(),
        "digest": digest_json.clone(),
        "response": { "$bigint": diag.response_decimal },
    });
    let bridge_request_upstream = serde_json::json!({
        "signatureHex": sig_upstream_hex.clone(),
        "publicKey": pk_json,
        "digest": digest_json,
    });

    let on_verify_bridge = {
        let req = bridge_request_compact.clone();
        move |_| {
            if *bridge_in_flight.read() {
                return;
            }
            bridge_in_flight.set(true);
            bridge_result.set(None);
            let req = req.clone();
            spawn(async move {
                let outcome = call_bridge_verify(&req, "schnorrVerify").await;
                bridge_result.set(Some(outcome));
                bridge_in_flight.set(false);
            });
        }
    };
    let on_verify_bridge_upstream = {
        let req = bridge_request_upstream.clone();
        move |_| {
            if *bridge_in_flight.read() {
                return;
            }
            bridge_in_flight.set(true);
            upstream_result.set(None);
            let req = req.clone();
            spawn(async move {
                let outcome = call_bridge_verify(&req, "schnorrVerifyUpstreamEncoded").await;
                upstream_result.set(Some(outcome));
                bridge_in_flight.set(false);
            });
        }
    };

    // Silence unused-variable lints if we ever drop a verify path.
    let _ = network;

    rsx! {
        h3 { "Sign with Jubjub Schnorr" }
        div { style: "color: var(--text-muted); font-size: 11px; margin-bottom: 10px;",
            "Key derived deterministically from "
            code { "SHA-256(\"midnight-did:wallet:sign-tab:v1\" || controller_sk || did)" }
            ". Identical across reloads of the same wallet."
        }
        div { class: "row",
            label { style: "min-width: 80px;", "Payload" }
            input {
                r#type: "text",
                value: "{payload.read()}",
                oninput: move |e| {
                    payload.set(e.value());
                    // Clear stale verify results — they refer to
                    // the old payload's signature.
                    local_result.set(None);
                    bridge_result.set(None);
                    upstream_result.set(None);
                },
                style: "flex: 1; padding: 6px 8px; background: var(--surface-2); color: var(--text); border: 1px solid var(--border); border-radius: 6px; font-family: ui-monospace, monospace; font-size: 11px;"
            }
        }
        h3 { "Public key (Jubjub subgroup)" }
        div { class: "detail-kv",
            div { class: "k", "pk.x" }
            div { class: "v", "{pk_x_dec}" }
            div { class: "k", "pk.y" }
            div { class: "v", "{pk_y_dec}" }
        }
        h3 { "Digest (4-limb Fr)" }
        div { class: "detail-kv",
            div { class: "k", "d[0]" } div { class: "v", "{digest_dec[0]}" }
            div { class: "k", "d[1]" } div { class: "v", "{digest_dec[1]}" }
            div { class: "k", "d[2]" } div { class: "v", "{digest_dec[2]}" }
            div { class: "k", "d[3]" } div { class: "v", "{digest_dec[3]}" }
        }
        h3 { "Signature" }
        div { class: "detail-kv",
            div { class: "k", "Compact (64B)" }
            div { class: "v",
                {copy_btn(sig_compact_hex.clone(), "Copy compact hex")}
                "0x{sig_compact_hex}"
            }
            div { class: "k", "Upstream (96B)" }
            div { class: "v",
                {copy_btn(sig_upstream_hex.clone(), "Copy upstream hex")}
                "0x{sig_upstream_hex}"
            }
        }
        h3 { "Verify" }
        div { class: "row",
            button { onclick: on_verify_local, "Verify locally" }
            button {
                disabled: *bridge_in_flight.read(),
                onclick: on_verify_bridge,
                "Verify via on-chain circuit"
            }
            button {
                disabled: *bridge_in_flight.read(),
                onclick: on_verify_bridge_upstream,
                "Verify via decodeJubjubSignature"
            }
        }
        {render_sign_verify_result("Local Rust", local_result.read().as_ref().copied().map(Ok))}
        {render_sign_verify_result("On-chain circuit", bridge_result.read().clone())}
        {render_sign_verify_result("Upstream decode + circuit", upstream_result.read().clone())}
    }
}

/// Helper for the three verify outcomes the Sign tab can show.
/// `None` → not yet clicked; `Some(Ok(true))` → accepted;
/// `Some(Ok(false))` → rejected (algebraic failure);
/// `Some(Err)` → bridge / decode error.
fn render_sign_verify_result(label: &str, state: Option<Result<bool, String>>) -> Element {
    match state {
        None => rsx! {},
        Some(Ok(true)) => rsx! {
            div { class: "wizard-outcome ok",
                div { class: "row label", "{label} verify" }
                div { class: "seed-blob", "accepted ✓" }
            }
        },
        Some(Ok(false)) => rsx! {
            div { class: "wizard-outcome err",
                div { class: "row label", "{label} verify" }
                div { class: "seed-blob", "rejected ✗" }
            }
        },
        Some(Err(e)) => rsx! {
            div { class: "wizard-outcome err",
                div { class: "row label", "{label} verify" }
                div { class: "seed-blob", "bridge error: {e}" }
            }
        },
    }
}

/// One-shot spawn of `NodeChildBridge`, fire the given method
/// with the given JSON, return `Ok(verified)` or `Err(message)`.
/// The harness child is dropped at function return — fine for
/// the Sign tab's button-click cadence; if we ever surface a
/// heavier verify flow we'd want a long-lived bridge handle.
async fn call_bridge_verify(
    req: &serde_json::Value,
    method: &str,
) -> Result<bool, String> {
    use wallet_core::js_bridge::{JsBridge, NodeChildBridge};
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct VerifyOut {
        verified: bool,
        error: Option<String>,
    }
    let bridge = NodeChildBridge::spawn(&NodeChildBridge::default_harness_path())
        .map_err(|e| format!("spawn harness: {e}"))?;
    let out: VerifyOut = bridge
        .call(method, req.clone())
        .await
        .map_err(|e| format!("{method}: {e}"))?;
    if let Some(err) = out.error {
        // Surface circuit asserts as `Ok(false)` (the signature
        // is structurally valid but algebraically rejected),
        // not as `Err` — the user wants to see "rejected" for
        // a tampered sig, not "bridge error".
        if err.contains("Invalid Jubjub Schnorr signature") {
            return Ok(false);
        }
        return Err(err);
    }
    Ok(out.verified)
}

#[component]
fn DidInventoryPanel(
    entries: Vec<DidInventoryEntry>,
    /// Fires when the user clicks "Open" — parent uses this to
    /// re-seed the Resolve / LoadCircuit panels so the operator
    /// can drive the next step on that DID.
    on_select: EventHandler<String>,
) -> Element {
    if entries.is_empty() {
        return rsx! {
            div { class: "wizard-header", "DIDs" }
            div { class: "session-log-empty",
                "No DIDs in this session yet. Create one or resolve an existing one to populate the inventory."
            }
        };
    }
    rsx! {
        div { class: "wizard-header", "DIDs ({entries.len()})" }
        div { class: "did-inventory",
            div { class: "did-inventory-row did-inventory-header",
                span { class: "did-inventory-cell status", "Status" }
                span { class: "did-inventory-cell network", "Network" }
                span { class: "did-inventory-cell did", "DID" }
                span { class: "did-inventory-cell counter", "Counter" }
                span { class: "did-inventory-cell vms", "VMs" }
                span { class: "did-inventory-cell services", "Services" }
                span { class: "did-inventory-cell action", "" }
            }
            for entry in entries.iter() {
                {render_inventory_row(entry, on_select.clone())}
            }
        }
    }
}

fn render_inventory_row(entry: &DidInventoryEntry, on_select: EventHandler<String>) -> Element {
    let did_short = truncate_did(&entry.did);
    let did_full = entry.did.clone();
    let counter = entry
        .counter
        .map(|c| c.to_string())
        .unwrap_or_else(|| "—".into());
    let vms = entry
        .vm_count
        .map(|n| n.to_string())
        .unwrap_or_else(|| "—".into());
    let services = entry
        .service_count
        .map(|n| n.to_string())
        .unwrap_or_else(|| "—".into());
    let badge_class = entry.status.badge_class();
    let status_label = entry.status.label();
    let did_for_click = did_full.clone();
    rsx! {
        div {
            key: "{did_full}",
            class: "did-inventory-row",
            span { class: "did-inventory-cell status",
                span { class: "{badge_class}", "{status_label}" }
            }
            span { class: "did-inventory-cell network", "{entry.network_label}" }
            span {
                class: "did-inventory-cell did",
                title: "{did_full}",
                "{did_short}"
            }
            span { class: "did-inventory-cell counter", "{counter}" }
            span { class: "did-inventory-cell vms", "{vms}" }
            span { class: "did-inventory-cell services", "{services}" }
            span { class: "did-inventory-cell action",
                button {
                    onclick: move |_| on_select.call(did_for_click.clone()),
                    "Open"
                }
            }
        }
    }
}

/// Truncate a DID for table display — keeps the `did:midnight:net:`
/// prefix and the last 6 chars of the address so it's still
/// recognisable but doesn't blow out the column. Full DID lives on
/// the row's `title` attribute for hover.
fn truncate_did(did: &str) -> String {
    let parts: Vec<&str> = did.splitn(4, ':').collect();
    if parts.len() < 4 {
        return did.to_string();
    }
    let prefix = parts[..3].join(":");
    let addr = parts[3];
    if addr.len() <= 10 {
        return did.to_string();
    }
    format!("{prefix}:{}…{}", &addr[..4], &addr[addr.len() - 4..])
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

/// Translate the wallet-core `InventoryStatus` (persisted)
/// to the dioxus-wallet `DidInventoryStatus` (in-memory).
/// Both enums carry the same variant names; the mapping is
/// purely a type-system bridge.
fn status_from_store(s: wallet_core::store::InventoryStatus) -> DidInventoryStatus {
    match s {
        wallet_core::store::InventoryStatus::Pending => DidInventoryStatus::Pending,
        wallet_core::store::InventoryStatus::Active => DidInventoryStatus::Active,
        wallet_core::store::InventoryStatus::Deactivated => DidInventoryStatus::Deactivated,
    }
}

fn status_to_store(s: DidInventoryStatus) -> wallet_core::store::InventoryStatus {
    match s {
        DidInventoryStatus::Pending => wallet_core::store::InventoryStatus::Pending,
        DidInventoryStatus::Active => wallet_core::store::InventoryStatus::Active,
        DidInventoryStatus::Deactivated => wallet_core::store::InventoryStatus::Deactivated,
    }
}

/// Write-through helper — pushes the latest UI-side inventory
/// state into the persistent store. Best-effort; a store error
/// is logged but doesn't fail the in-memory update, so an
/// unhealthy disk doesn't break the current session.
fn persist_inventory_entry(
    bridge_state: &BridgeState,
    network: Network,
    entry: &DidInventoryEntry,
) {
    let Some(store) = bridge_state.store() else {
        return;
    };
    let row = wallet_core::store::DidInventoryEntry {
        did: entry.did.clone(),
        network,
        status: status_to_store(entry.status),
        counter: entry.counter,
        vm_count: entry.vm_count.map(|v| v as u32),
        service_count: entry.service_count.map(|v| v as u32),
        last_block_height: entry.last_block_height,
        created_at: 0,
        updated_at: 0,
    };
    if let Err(e) = store.put_did_inventory(row) {
        tracing::warn!(error=%e, did=%entry.did, "persist did inventory failed");
    }
}

/// Write-through helper — caches the resolved JSON snapshot
/// under `(network, did)` so the detail tabs survive a reload.
fn persist_resolved_cache(
    bridge_state: &BridgeState,
    network: Network,
    did: &str,
    resolved: &wallet_core::ResolvedDid,
) {
    let Some(store) = bridge_state.store() else {
        return;
    };
    let Ok(json) = serde_json::to_string(resolved) else {
        return;
    };
    if let Err(e) = store.put_resolved_cache(network, did, json) {
        tracing::warn!(error=%e, did=%did, "persist resolved cache failed");
    }
}

/// Dev-only passphrase for the persistent wallet store. A
/// future slice will surface an unlock prompt and let the user
/// set / rotate this — until then the prototype just uses a
/// fixed string so the file-on-disk decrypts across runs
/// without user input.
const DEV_STORE_PASSPHRASE: &str = "midnight-did-wallet-dev:v1";

/// Path the persistent wallet store lives at. Uses the OS
/// data-dir (`~/Library/Application Support/...` on macOS,
/// `~/.local/share/...` on Linux, `%APPDATA%/...` on Windows)
/// so multiple wallets on the same machine don't fight. Falls
/// back to a `./wallet.redb` next to the binary if the dirs
/// crate can't resolve a data dir.
fn wallet_store_path() -> std::path::PathBuf {
    if let Some(base) = dirs::data_dir() {
        let dir = base.join("midnight-did-wallet");
        let _ = std::fs::create_dir_all(&dir);
        return dir.join("wallet.redb");
    }
    std::path::PathBuf::from("wallet.redb")
}

/// Small inline ⧉ button that copies `value` to the system
/// clipboard on click. Used in the Methods + Services tables and
/// the Raw State pane so every long, hand-typeable string has a
/// one-click extract. No "copied!" feedback — that needs signal
/// state which only works inside `#[component]` fns and these
/// render helpers are plain `fn`s. The button is small enough
/// that the silent copy is the right trade.
fn copy_btn(value: String, title: &'static str) -> Element {
    rsx! {
        button {
            class: "copy-btn inline",
            title: "{title}",
            onclick: move |_| {
                let _ = copy_to_clipboard(&value);
            },
            "⧉"
        }
    }
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
