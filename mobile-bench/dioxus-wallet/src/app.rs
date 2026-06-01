use dioxus::prelude::*;
use crate::format::{
    DUST_DECIMALS, NIGHT_DECIMALS, format_atomic_dust, format_atomic_night,
    format_balance, format_int, format_log_timestamp, format_ms, short_keyref,
};
use crate::bench_stage;
use crate::proc_stats::{CLK_TCK, proc_per_core_stats, proc_self_stats};
use wallet_core::{
    ChainTipInfo, HttpIndexerClient, Network, NodeStatus, ProbeResult, SubxtNodeClient, Wallet,
    probe_connectivity,
};

use crate::bridge::{BridgeState, run_bridge_loop, spawn_proof_server};

/// CSS is bundled into the binary at compile time via `include_str!` —
/// belt-and-braces vs. the asset! macro, which can drop on certain
/// release-mode bundling paths. The file lives next to `assets/`
/// where Android packaging still finds it.
const STYLES: &str = include_str!("../assets/styles.css");

/// Midnight wordmark — rendered in the App header in place of
/// the previous `<h1>Midnight Wallet</h1>`. Inlined via
/// `include_str!` so it ships in the binary with no extra fetch.
pub(crate) const LOGO_SVG: &str = include_str!("../assets/logo.svg");

/// Stacked variant — used as the splash-screen content for the
/// first ~1.5 s after launch.
pub(crate) const LOGO_SPLASH_SVG: &str = include_str!("../assets/logo-splash.svg");

/// Compact monogram — used as the platform window icon on
/// desktop. The rasterisation to RGBA bytes happens in `lib.rs`
/// via `resvg` before the window is built; Android has no
/// concept of a per-process window icon (the launcher uses the
/// APK's `mipmap-*` resources instead), so the const is gated to
/// non-Android targets to keep `#![deny(warnings)]` quiet.
#[cfg(all(not(target_os = "android"), not(target_os = "ios")))]
pub(crate) const LOGO_ICON_SVG: &str = include_str!("../assets/logo-icon.svg");

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
/// users. `Test` holds the dev-only probes (JS bridge spike,
/// per-stage timings) so they don't clutter the main flow.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Wallet,
    Dids,
    Keys,
    Diagnostics,
    Metrics,
    /// `Benchmark` runs the `contract-benchmark` crate's parameterised
    /// dummy contract at varying `k` and reports prove timings per row.
    /// Inserted between `Metrics` and `Test` so the menu ordering reads
    /// Wallet → DIDs → Keys → Diagnostics → Metrics → Benchmark → Test
    /// → Logs → Settings.
    Benchmark,
    Test,
    Logs,
    Settings,
    /// Identity Centre Phase 1 — drives the four shipped wallet-core
    /// flows (bootstrap DID + VC keys, OID4VP authenticate, OID4VCI
    /// issue, self-verify VCs) as paste-URL buttons. The plan's
    /// carousel + FAB structure is deferred to Phase 2; this tab is
    /// the pragmatic linear page that exposes the same end-to-end
    /// contract against the running issuer-mock.
    Identity,
    /// Operator/dev setup tab. Holds the heavy-weight one-time
    /// `bootstrap_did_with_keys` flow (mint a fresh issuer-/holder-
    /// facing DID + Ed25519 + Jubjub keys) and the manual paste-URL
    /// `OID4VP` authenticate path. The everyday holder-facing surface
    /// (Identity Centre) stays scan-first; this tab is where you'd
    /// go to seed a new wallet or debug the protocol step-by-step.
    Bootstrap,
}

impl Tab {
    fn label(&self) -> &'static str {
        match self {
            Tab::Wallet => "Wallet",
            Tab::Dids => "DIDs",
            Tab::Keys => "Keys",
            Tab::Diagnostics => "Diagnostics",
            Tab::Metrics => "Metrics",
            Tab::Benchmark => "Benchmark",
            Tab::Test => "Test",
            Tab::Logs => "Logs",
            Tab::Settings => "Settings",
            Tab::Identity => "Identity Centre",
            Tab::Bootstrap => "Bootstrap",
        }
    }

    /// Stable u8 encoding for the persistent session row.
    /// New variants must claim the next free integer; never
    /// reorder existing ones.
    fn to_persist(self) -> u8 {
        match self {
            Tab::Wallet => 0,
            Tab::Dids => 1,
            Tab::Diagnostics => 2,
            Tab::Settings => 3,
            Tab::Keys => 4,
            Tab::Logs => 5,
            Tab::Test => 6,
            Tab::Metrics => 7,
            Tab::Benchmark => 8,
            Tab::Identity => 9,
            Tab::Bootstrap => 10,
        }
    }

    /// Decode the persisted u8 back to a variant. Unknown
    /// values (e.g. a downgrade from a future binary that had
    /// more tabs) fall back to `Wallet` — the safest
    /// "where was I" default.
    fn from_persist(b: u8) -> Self {
        match b {
            0 => Tab::Wallet,
            1 => Tab::Dids,
            2 => Tab::Diagnostics,
            3 => Tab::Settings,
            4 => Tab::Keys,
            // Logs / Test / Metrics / Benchmark used to be top-level
            // tabs (codes 5/6/7/8 in older builds). They have moved
            // inside the Diagnostics carousel — redirect there so a
            // user resuming from an older session doesn't land on an
            // unreachable variant.
            5 | 6 | 7 | 8 => Tab::Diagnostics,
            9 => Tab::Identity,
            10 => Tab::Bootstrap,
            _ => Tab::Wallet,
        }
    }
}

/// One entry in the in-memory session activity log. Sized for the
/// log panel — we keep just the fields a user would want to see at
/// a glance plus copy-paste-able hashes.
#[derive(Clone, PartialEq, Eq)]
enum SessionEvent {
    /// Emitted by `CreateDidWizard` when a deploy completes. The
    /// wizard is currently unmounted (preprod-live ships with three
    /// pre-seeded DIDs and Create DID is redundant alongside the
    /// inventory `Open` + detail-view `Update`), so this variant
    /// has no live producer. The component is still defined and can
    /// be re-mounted; kept on the enum so older session-log
    /// snapshots still deserialise.
    #[allow(dead_code)]
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
    /// A DID circuit invocation the user prepared in the UI.
    /// Emitted by the now-removed `DidOperationsPanel` (the
    /// "draft only" pre-submit-was-wired UI). Kept on the enum
    /// so older `SessionLogPanel` snapshots still deserialise;
    /// no live producer.
    #[allow(dead_code)]
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
    /// Schnorr-Jubjub path. The redesigned `did.compact` (upstream
    /// commit `6274cff feat!: Redesign DID verification method
    /// storage`) keeps Jubjub VMs in a dedicated
    /// `schnorrJubjubVerificationMethods: Map<...>` ledger slot
    /// because `assertSupportedVerificationMethod` REJECTS
    /// `kty=EC, crv=Jubjub` from the JWK map with
    /// "EC keys must use P-256 or secp256k1; use SchnorrJubjub
    /// methods for Jubjub". The Operation Builder routes Add/Update
    /// VMs with `curve == "Jubjub"` here automatically; Remove is
    /// an explicit dropdown entry (id alone doesn't disambiguate
    /// which map the VM lives in).
    AddSchnorrJubjubVerificationMethod(VerificationMethodInput),
    UpdateSchnorrJubjubVerificationMethod(VerificationMethodInput),
    RemoveSchnorrJubjubVerificationMethod { id: String },
    AddVerificationMethodRelation { relation: String, method_id: String },
    RemoveVerificationMethodRelation { relation: String, method_id: String },
    AddService(ServiceInput),
    UpdateService(ServiceInput),
    RemoveService { id: String },
    /// `deactivate` circuit. The dedicated "Deactivate" button on
    /// the DID detail view drives this; not used inside the
    /// batch Operation Builder (which would only ever queue one
    /// of these as the LAST entry, by design).
    #[allow(dead_code)]
    Deactivate,
}

/// Compact `MapMutation` enum variant tags — used by
/// `setVerificationMethod`, `setSchnorrJubjubVerificationMethod`,
/// `setService`. `Undefined=0` is rejected by the contract's
/// `assertMapMutationDefined`.
const MAP_MUTATION_INSERT: i32 = 1;
const MAP_MUTATION_UPDATE: i32 = 2;
/// Compact `SetMutation` enum variant tags — used by
/// `setAlsoKnownAs` and `setVerificationMethodRelation`.
/// `Undefined=0` is rejected by `assertSetMutationDefined`.
const SET_MUTATION_INSERT: i32 = 1;
const SET_MUTATION_REMOVE: i32 = 2;

impl DidOperation {
    /// Circuit entry-point name that `Wallet::load_did_circuit` /
    /// `Wallet::call_did_circuit` consume. Maps to the artifact
    /// registry in `wallet-core/contracts/midnight-did/*.{prover,
    /// verifier,bzkir,zkir}`. Updated 2026-05-28 for the upstream
    /// `feat!: Redesign DID verification method storage`
    /// refactor: every Add/Update/Remove circuit was replaced by
    /// a single `set*(payload, MapMutation|SetMutation)` form;
    /// Jubjub VMs moved to their own
    /// `setSchnorrJubjubVerificationMethod` circuit.
    fn circuit(&self) -> &'static str {
        match self {
            Self::AddAlsoKnownAs { .. } | Self::RemoveAlsoKnownAs { .. } => {
                "setAlsoKnownAs"
            }
            Self::AddVerificationMethod(_) | Self::UpdateVerificationMethod(_) => {
                "setVerificationMethod"
            }
            Self::RemoveVerificationMethod { .. } => "removeVerificationMethod",
            Self::AddSchnorrJubjubVerificationMethod(_)
            | Self::UpdateSchnorrJubjubVerificationMethod(_) => {
                "setSchnorrJubjubVerificationMethod"
            }
            Self::RemoveSchnorrJubjubVerificationMethod { .. } => {
                "removeSchnorrJubjubVerificationMethod"
            }
            Self::AddVerificationMethodRelation { .. }
            | Self::RemoveVerificationMethodRelation { .. } => {
                "setVerificationMethodRelation"
            }
            Self::AddService(_) | Self::UpdateService(_) => "setService",
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
            Self::AddSchnorrJubjubVerificationMethod(vm)
            | Self::UpdateSchnorrJubjubVerificationMethod(vm) => {
                format!("id: {} · Schnorr-Jubjub", vm.id)
            }
            Self::RemoveVerificationMethod { id }
            | Self::RemoveSchnorrJubjubVerificationMethod { id }
            | Self::RemoveService { id } => {
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
            Self::AddAlsoKnownAs { value } => {
                serde_json::json!([value, SET_MUTATION_INSERT])
            }
            Self::RemoveAlsoKnownAs { value } => {
                serde_json::json!([value, SET_MUTATION_REMOVE])
            }
            Self::AddVerificationMethod(vm) => {
                serde_json::json!([vm_to_jwk_json(vm), MAP_MUTATION_INSERT])
            }
            Self::UpdateVerificationMethod(vm) => {
                serde_json::json!([vm_to_jwk_json(vm), MAP_MUTATION_UPDATE])
            }
            Self::RemoveVerificationMethod { id }
            | Self::RemoveSchnorrJubjubVerificationMethod { id }
            | Self::RemoveService { id } => serde_json::json!([id]),
            Self::AddSchnorrJubjubVerificationMethod(vm) => {
                serde_json::json!([vm_to_schnorr_jubjub_json(vm), MAP_MUTATION_INSERT])
            }
            Self::UpdateSchnorrJubjubVerificationMethod(vm) => {
                serde_json::json!([vm_to_schnorr_jubjub_json(vm), MAP_MUTATION_UPDATE])
            }
            Self::AddVerificationMethodRelation { relation, method_id } => {
                serde_json::json!([relation_tag(relation), method_id, SET_MUTATION_INSERT])
            }
            Self::RemoveVerificationMethodRelation { relation, method_id } => {
                serde_json::json!([relation_tag(relation), method_id, SET_MUTATION_REMOVE])
            }
            Self::AddService(s) => serde_json::json!([
                {
                    "id": s.id,
                    "typ": s.typ,
                    "serviceEndpoint": s.endpoint,
                },
                MAP_MUTATION_INSERT,
            ]),
            Self::UpdateService(s) => serde_json::json!([
                {
                    "id": s.id,
                    "typ": s.typ,
                    "serviceEndpoint": s.endpoint,
                },
                MAP_MUTATION_UPDATE,
            ]),
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

/// Build the JSON `VerificationMethod` struct payload for the
/// `setVerificationMethod` circuit. The redesigned contract
/// (upstream commit `6274cff feat!: Redesign DID verification
/// method storage`) reverted `publicKeyJwk.x/y` from `Bytes<32>`
/// back to `Opaque<"string">` (base64url-encoded JWK
/// coordinates, per W3C JWK). The wire shape is now a plain
/// JSON string the runtime passes straight through to the
/// contract's `Opaque<"string">` slot — no `$bytes` wrap.
///
/// The upstream API validates that x/y are decode-able to
/// exactly 32 bytes via `decodeBase64UrlBytes32` before the call
/// (see `~/iohk/midnight-did/packages/api/src/ledger-mappers.ts`);
/// we mirror that by encoding our hex/decimal input through a
/// 32-byte buffer first.
///
/// `y` is empty (`""`) for `OKP` keys (Ed25519, X25519) — the
/// contract's `assertSupportedVerificationMethod` rejects OKP
/// keys carrying a `y` coordinate. For `EC` keys (P-256, secp256k1)
/// both x and y are required. **Jubjub MUST NOT come through
/// this path** — `vm_to_schnorr_jubjub_json` is the right one.
/// The contract's `assertSupportedVerificationMethod` rejects
/// `kty=EC, crv=Jubjub` at runtime with a clear error.
fn vm_to_jwk_json(vm: &VerificationMethodInput) -> serde_json::Value {
    let kty = enum_tag(KEY_TYPES, &vm.key_type);
    let is_okp = vm.key_type == "OKP";
    let y_b64 = if is_okp {
        String::new()
    } else {
        pk_to_base64url(&vm.pk_y)
    };
    serde_json::json!({
        "id": vm.id,
        // VerificationMethodType.JsonWebKey = 1
        "typ": 1,
        "publicKeyJwk": {
            "kty": kty,
            "crv": crv_to_contract_tag(&vm.curve),
            "x": pk_to_base64url(&vm.pk_x),
            "y": y_b64,
        }
    })
}

/// Build the JSON `SchnorrJubjubVerificationMethod` struct
/// payload for the `setSchnorrJubjubVerificationMethod` circuit.
/// Shape mirrors upstream
/// `schnorrJubjubVerificationMethodToLedger` — id is the
/// fragment-bound method id, publicKey is a `JubjubPoint =
/// {x: bigint, y: bigint}` carrying the two field elements
/// directly (NOT base64url, NOT JWK).
///
/// The runtime's `reviveBigints` revives `{"$bigint": "<dec>"}`
/// into a JS `BigInt`, which the contract's
/// `CompactTypeJubjubPoint.toValue` accepts as the x/y inputs.
fn vm_to_schnorr_jubjub_json(vm: &VerificationMethodInput) -> serde_json::Value {
    serde_json::json!({
        "id": vm.id,
        "publicKey": {
            "x": serde_json::json!({ "$bigint": pk_to_bigint_decimal(&vm.pk_x) }),
            "y": serde_json::json!({ "$bigint": pk_to_bigint_decimal(&vm.pk_y) }),
        }
    })
}

/// Convert the UI form's hex-or-decimal `pk_x` / `pk_y` string
/// into a base64url-encoded 32-byte big-endian string — the
/// wire format the redesigned JWK contract slot expects. Empty
/// input → 32 zero bytes encoded as `"AAAA…"`.
fn pk_to_base64url(input: &str) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(pk_to_bytes32_buf(input))
}

/// Convert the UI form's hex-or-decimal `pk_x` / `pk_y` string
/// into a decimal-string representation of the 32-byte BE buffer
/// interpreted as a big-endian non-negative integer. Used by
/// `vm_to_schnorr_jubjub_json` so the `$bigint` tag carries a
/// value the harness's `reviveBigints` can convert into a JS
/// BigInt for the `JubjubPoint.x/y` field elements.
fn pk_to_bigint_decimal(input: &str) -> String {
    let bytes = pk_to_bytes32_buf(input);
    bigint_decimal_from_be_bytes(&bytes)
}

/// Strip the format-detection logic out of `pk_to_bytes32_hex`
/// so the same parser can feed both the base64url and decimal
/// representations. Returns 32 BE bytes; truncates / zero-pads
/// as needed.
fn pk_to_bytes32_buf(input: &str) -> [u8; 32] {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return [0u8; 32];
    }
    let maybe_hex = trimmed.strip_prefix("0x").unwrap_or(trimmed);
    if !maybe_hex.is_empty() && maybe_hex.chars().all(|c| c.is_ascii_hexdigit()) {
        let mut s = maybe_hex.to_string();
        if s.len() % 2 == 1 {
            s.insert(0, '0');
        }
        let bytes = hex::decode(&s).unwrap_or_default();
        let mut buf = [0u8; 32];
        if bytes.len() <= 32 {
            let offset = 32 - bytes.len();
            buf[offset..].copy_from_slice(&bytes);
        } else {
            buf.copy_from_slice(&bytes[bytes.len() - 32..]);
        }
        return buf;
    }
    // Decimal path.
    let mut buf = [0u8; 32];
    for ch in trimmed.chars() {
        let Some(d) = ch.to_digit(10) else { continue };
        let mut carry = d as u16;
        for byte in buf.iter_mut().rev() {
            let v = (*byte as u16) * 10 + carry;
            *byte = (v & 0xff) as u8;
            carry = v >> 8;
        }
    }
    buf
}

/// Convert a 32-byte BE buffer into a decimal string (no leading
/// zeros). Used to feed `$bigint`-tagged JubjubPoint coords to
/// `reviveBigints`. Zero buffer → `"0"`. Implementation is a
/// repeated divide-by-10 with no external dep — same `[u8; 32]`
/// shape we already produce, just in the other direction.
fn bigint_decimal_from_be_bytes(bytes: &[u8; 32]) -> String {
    // Output buffer; we'll reverse at the end.
    let mut out: Vec<u8> = Vec::with_capacity(78);
    let mut work = *bytes;
    loop {
        let mut all_zero = true;
        let mut rem: u32 = 0;
        for byte in work.iter_mut() {
            let cur = rem * 256 + *byte as u32;
            *byte = (cur / 10) as u8;
            rem = cur % 10;
            if *byte != 0 {
                all_zero = false;
            }
        }
        out.push(b'0' + rem as u8);
        if all_zero {
            break;
        }
    }
    out.reverse();
    // Strip leading zeros but always keep at least one digit.
    let first_nonzero = out.iter().position(|&b| b != b'0').unwrap_or(out.len() - 1);
    String::from_utf8(out[first_nonzero..].to_vec()).unwrap_or_else(|_| "0".to_string())
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
pub(crate) struct DidInventoryEntry {
    /// `did:midnight:<network>:<address>` — primary key.
    pub(crate) did: String,
    pub(crate) network_label: String,
    pub(crate) status: DidInventoryStatus,
    /// `None` for a freshly-deployed DID that hasn't been
    /// resolved yet (we don't know the counter chain-side until
    /// the indexer catches up).
    pub(crate) counter: Option<u32>,
    pub(crate) vm_count: Option<usize>,
    pub(crate) service_count: Option<usize>,
    pub(crate) last_block_height: Option<i64>,
}

/// Status badge for [`DidInventoryEntry`]. `Pending` is what we
/// show between deploy and first successful resolve; afterwards
/// the resolve reports `Active` or `Deactivated`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum DidInventoryStatus {
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

/// Cost snapshot for one completed pipeline run. Computed by
/// taking a `Wallet::balance_snapshot` immediately before
/// driving the stream and again right after `Done` arrives;
/// the deltas reported here are `before − after` clamped at
/// zero (DUST accrues continuously, so a positive `after − before`
/// just means "no transaction, just accrual" — surfaced as
/// zero cost).
#[derive(Clone, PartialEq, Eq)]
struct CostRun {
    /// "create_did", "load_did_circuit:<circuit>",
    /// "call_did_circuit:<circuit>", "batch:<n_ops>".
    label: String,
    /// **Net** DUST burnt for this run, in atomic units
    /// (10^-15 DUST). Computed as
    /// `max(before − after, 0)` — DUST continuously accrues
    /// from the wallet's NIGHT generators, so a strictly
    /// "what did this tx cost" number isn't directly
    /// observable here: a Done flow taking 30s also accrued
    /// some DUST during those seconds, which is netted out
    /// against the spent amount. The reported value is what
    /// the user's balance actually moved by, not the raw
    /// proving-time fee.
    dust_consumed: u128,
    /// NIGHT atomic units spent in this run (10^-6 NIGHT).
    /// NIGHT doesn't accrue so this number is exact —
    /// equal to the actual NIGHT moved by the transaction
    /// (typically `0` for DID flows; the chain takes its
    /// fee in DUST).
    night_consumed: u128,
    /// Wall-clock duration from the pre-snapshot to the
    /// post-snapshot. Bigger than `TimingRun.total_ms` by the
    /// two snapshot calls' overhead (~1–3s combined).
    duration_ms: u64,
    /// Whether the pipeline reached Done (true) or Failed
    /// (false). Cost is still surfaced on failure because
    /// some flows burn DUST during balancing even when the
    /// final submission rejects.
    succeeded: bool,
}

/// PreProd-live demo configuration. Compiled in only when
/// `--features preprod-live` is on. The seed + DIDs come from
/// the operator's local
/// `~/.midnight-did/profiles/preprod/preproad-default`
/// manager profile, checked into source per operator
/// instruction. Off by default — vanilla builds keep the
/// shared `app_wallet_for(PreProd)` seed.
#[cfg(feature = "preprod-live")]
mod preprod_live {
    pub const SEED_HEX: &str =
        "c1e8d986d10a2aff5d5f6fbf3d568f447b1cd46ccb190f838e0cf2707f5622a2";
    /// Contract addresses for the three DIDs in the manager
    /// profile. Pre-populated into the wallet's persistent
    /// inventory at unlock so the operator sees them
    /// immediately without typing DID strings into the
    /// Resolve panel.
    pub const DIDS: &[&str] = &[
        "6b6e06d6f9779b0e4a3596a02edba5539f5b435c07ff5c885f3855d8d8653801",
        "5914d2622abfb6f793c4b15c82692593500ecc481ae9b99a1655ad5e766dca4f",
        "ce785669eac7048652d239bd40286240bbe09f9f9c5d614631a3b256a2fec68a",
    ];
    /// Verification-method keys decrypted from the manager
    /// profile's `manager-secrets.json` via
    /// `scripts/dump-manager-keys.mjs`. Baked into the binary so
    /// the mobile build seeds its own wallet store on first
    /// unlock — `cargo test` can't reach the on-device redb
    /// like it can on desktop. These are dev/preprod material
    /// (hardcoded `midnight-dev-passphrase` upstream); not
    /// production-sensitive.
    pub const KEYS_JSON: &str = include_str!("../preprod_keys.json");
}

/// Build the wallet handle this App uses for a given network.
/// Vanilla builds delegate to `Wallet::demo` — same shared
/// test seed everyone else uses. `preprod-live` builds swap
/// in the operator's manager-profile seed when `net` is
/// PreProd; all other networks stay on `Wallet::demo`.
///
/// One place to centralise the swap so the rest of the App
/// doesn't need to know which build it's running under.
pub(crate) fn app_wallet_for(net: Network) -> Wallet {
    let base = {
        #[cfg(feature = "preprod-live")]
        {
            if matches!(net, Network::PreProd) {
                let bytes = hex::decode(preprod_live::SEED_HEX)
                    .expect("preprod_live::SEED_HEX is hex");
                let seed: [u8; 32] = bytes
                    .as_slice()
                    .try_into()
                    .expect("preprod_live::SEED_HEX is 32 bytes");
                Wallet::from_seed(seed, Network::PreProd)
            } else {
                wallet_core::Wallet::demo(net)
            }
        }
        #[cfg(not(feature = "preprod-live"))]
        {
            wallet_core::Wallet::demo(net)
        }
    };
    // If the App has booted an embedded proof-server (see
    // `BridgeState::spawn_proof_server`), thread the URL into the
    // wallet so `Wallet::call_did_circuit` / `create_did` /
    // `load_did_circuit` route per-preimage proving through the
    // release-built proof-server's `/prove` endpoint instead of
    // running the in-process zkir prover (slow on debug builds).
    // The static below is set by `set_proof_server_url` once at App
    // startup and read here on every wallet construction, so all
    // ~15 call sites of `app_wallet_for` transparently get the
    // proof-server-backed wallet without parameter threading.
    let with_url = if let Some(url) = PROOF_SERVER_URL.get() {
        tracing::info!(target: "dioxuswalletmain", proof_server_url = %url, "app_wallet_for: attaching proof-server URL");
        base.with_proof_server_url(url.clone())
    } else {
        tracing::info!(target: "dioxuswalletmain", "app_wallet_for: PROOF_SERVER_URL not set yet — will use LocalProvingProvider");
        base
    };
    // Layer 2 / Phase 3: attach the persisted DUST syncer if the
    // store has been opened. `Wallet::sync_dust` will then resume
    // from `last_id + 1` instead of replaying ~534k events on
    // every call. The static is populated by
    // `set_dust_syncer_for(network, syncer)` once per network
    // after the store opens; we look up the matching one here.
    let with_dust = if let Some(syncer) = dust_syncer_for(net) {
        with_url.with_dust_syncer(syncer)
    } else {
        with_url
    };
    // Phase D: under `--features js-bridge`, attach the
    // process-wide `DioxusEvalBridge` so `Wallet::call_did_circuit`
    // can drive `window.midnightDidBundle.prepareUnprovenCallTx` in
    // the embedded WebView instead of trying to spawn a Node child
    // (which fails fast on Android per `NodeChildBridge::spawn`).
    // The bridge handle is `None` until the App's `use_future`
    // driver runs `install_global` on first render; if a wallet is
    // constructed before that point it just falls back to the
    // legacy path. In practice the driver mounts on the first
    // frame, well before any UI button can fire a DID write.
    #[cfg(feature = "js-bridge")]
    {
        if let Some(bridge) = crate::eval_bridge::global_bridge() {
            with_dust.with_js_bridge(bridge)
        } else {
            tracing::info!(
                target: "dioxuswalletmain",
                "app_wallet_for: DioxusEvalBridge not yet installed — will fall back to NodeChildBridge",
            );
            with_dust
        }
    }
    #[cfg(not(feature = "js-bridge"))]
    {
        with_dust
    }
}

/// Process-wide map of `Network → Arc<DustSyncer>`. Populated by
/// `set_dust_syncer_for` from the App's "wallet store opened"
/// path. Read by `app_wallet_for` on every wallet construction.
static DUST_SYNCERS: std::sync::OnceLock<
    std::sync::Mutex<
        std::collections::HashMap<
            wallet_core::Network,
            std::sync::Arc<wallet_core::DustSyncer>,
        >,
    >,
> = std::sync::OnceLock::new();

fn dust_syncers_map() -> &'static std::sync::Mutex<
    std::collections::HashMap<
        wallet_core::Network,
        std::sync::Arc<wallet_core::DustSyncer>,
    >,
> {
    DUST_SYNCERS.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// Look up the DUST syncer for `network`, if one has been
/// registered.
pub fn dust_syncer_for(
    network: wallet_core::Network,
) -> Option<std::sync::Arc<wallet_core::DustSyncer>> {
    dust_syncers_map()
        .lock()
        .ok()
        .and_then(|m| m.get(&network).cloned())
}

/// Register a `DustSyncer` for `network`. Called once at App
/// startup after the wallet store opens; idempotent (later
/// registrations replace the previous entry, which is fine
/// because all consumers just look up the current one).
pub fn set_dust_syncer_for(
    network: wallet_core::Network,
    syncer: std::sync::Arc<wallet_core::DustSyncer>,
) {
    if let Ok(mut m) = dust_syncers_map().lock() {
        m.insert(network, syncer);
    }
}

/// Set the embedded proof-server URL. Called once at App startup
/// from `BridgeState::spawn_proof_server`; idempotent. Only used
/// when the `proof-server-http` feature spins up the embedded HTTP
/// proof-server (desktop-only) — every other build path proves
/// in-process.
#[cfg(feature = "proof-server-http")]
pub fn set_proof_server_url(url: String) {
    let _ = PROOF_SERVER_URL.set(url);
}

static PROOF_SERVER_URL: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// `preprod-live` only: stamp the operator's three DIDs into
/// the wallet's persistent inventory as `Pending`, and seed
/// the matching controller secrets so the Sign tab + write
/// flows are usable immediately. Auto-resolve at unlock then
/// promotes the rows to `Active` with real counters.
///
/// Idempotent across runs:
/// - `WalletStore::put_did_inventory` preserves
///   `created_at` on an existing row and just bumps
///   `updated_at`.
/// - `remember_controller_secret` overwrites in-place; same
///   value on every call.
#[cfg(feature = "preprod-live")]
fn seed_preprod_live_state(state: &BridgeState, store: &wallet_core::store::WalletStore) {
    use wallet_core::store::{DidInventoryEntry, InventoryStatus};

    let sk = wallet_core::upstream_demo_controller_secret();
    for addr in preprod_live::DIDS {
        let did = format!("did:midnight:preprod:{addr}");
        if let Err(e) = store.put_did_inventory(DidInventoryEntry {
            did: did.clone(),
            network: Network::PreProd,
            status: InventoryStatus::Pending,
            counter: None,
            vm_count: None,
            service_count: None,
            last_block_height: None,
            created_at: 0,
            updated_at: 0,
        }) {
            tracing::warn!(error=%e, did=%did, "preprod-live: seed inventory failed");
        }
        state.remember_controller_secret(Network::PreProd, did, sk);
    }
    tracing::info!(count = preprod_live::DIDS.len(), "preprod-live: seeded inventory + secrets");
}

/// `preprod-live` only: import the operator's verification-method
/// keys from `preprod_live::KEYS_JSON` into the active wallet's
/// secret store. Idempotent — keys whose `id` already exists in
/// the store are skipped, so re-running on every unlock is safe
/// and cheap.
///
/// On desktop the user can do the same via
/// `cargo test -p wallet-core --test import_manager_keys`. On
/// mobile that round-trip isn't possible (the redb lives in the
/// Android app sandbox), so we bake the keys into the binary
/// and import at unlock instead.
#[cfg(feature = "preprod-live")]
fn seed_preprod_live_keys(
    store: &wallet_core::store::WalletStore,
    wallet_id: wallet_core::store::WalletId,
) {
    use wallet_core::secret_storage::{
        ImportKeyInput, MidnightCurve, MidnightKeyType, SecretStorage,
        redb_secret_store::RedbSecretStore,
    };

    #[derive(serde::Deserialize)]
    struct PreprodKey {
        id: String,
        kty: String,
        crv: String,
        private_key_hex: String,
        /// Optional DID this key serves. Missing in the raw
        /// dumper output (the manager-secrets store doesn't
        /// encode this association), so the JSON ships with the
        /// field absent. Annotate by hand if you want the key to
        /// land in the secret store tagged with a DID — useful
        /// for the Keys-tab filter and for matching at sign time.
        /// A follow-up patch can auto-fill this by walking each
        /// resolved DID's `verificationMethod` array and
        /// matching `publicKeyJwk.x` against `public_jwk.x`.
        #[serde(default)]
        did: Option<String>,
        /// Optional purpose hint (`authentication`,
        /// `assertionMethod`, etc.) — same lifecycle as `did`,
        /// stays None until manually filled.
        #[serde(default)]
        purpose: Option<String>,
    }

    let keys: Vec<PreprodKey> = match serde_json::from_str(preprod_live::KEYS_JSON) {
        Ok(k) => k,
        Err(e) => {
            tracing::warn!(error=%e, "preprod-live: parse keys JSON failed");
            return;
        }
    };

    let mut store_handle = RedbSecretStore::new(store.clone(), wallet_id);
    let existing_rows: Vec<wallet_core::secret_storage::StoredKeyMeta> =
        match futures::executor::block_on(store_handle.list_keys(None)) {
            Ok(rows) => rows,
            Err(e) => {
                tracing::warn!(error=%e, "preprod-live: list existing keys failed");
                return;
            }
        };
    // id → (key_ref_uuid, current did) — lets us decide whether to
    // skip, re-tag, or import each JSON entry. `SecretKeyRef`
    // exposes the UUID handle separately from the kid; the
    // store APIs (`delete_key` etc.) take the UUID string.
    let existing: std::collections::HashMap<String, (String, Option<String>)> =
        existing_rows
            .iter()
            .map(|r| {
                (
                    r.id.clone(),
                    (r.key_ref.uuid().to_string(), r.did.clone()),
                )
            })
            .collect();

    let mut imported = 0u32;
    let mut retagged = 0u32;
    let mut skipped = 0u32;
    for k in keys {
        // If the same id already exists, two cases:
        //   1. existing.did == k.did              → skip (idempotent).
        //   2. existing.did differs from k.did    → delete + reimport
        //      so the stored meta picks up the new DID tag (the
        //      SecretStorage trait has no `update_did` hook, so
        //      delete-and-reimport is the only way to mutate the
        //      `did` field of an existing key).
        if let Some((key_ref, current_did)) = existing.get(&k.id) {
            if *current_did == k.did {
                skipped += 1;
                continue;
            }
            if let Err(e) = futures::executor::block_on(
                store_handle.delete_key(key_ref.as_str()),
            ) {
                tracing::warn!(
                    error=%e, id=%k.id,
                    "preprod-live: delete-for-retag failed; skipping"
                );
                skipped += 1;
                continue;
            }
            retagged += 1;
            // Fall through to import below with the new did set.
        }
        let kty = match k.kty.as_str() {
            "OKP" => MidnightKeyType::OKP,
            "EC" => MidnightKeyType::EC,
            other => {
                tracing::warn!(id = %k.id, kty = other, "preprod-live: unknown kty, skipping");
                continue;
            }
        };
        let crv = match k.crv.as_str() {
            "Ed25519" => MidnightCurve::Ed25519,
            "Jubjub" => MidnightCurve::Jubjub,
            "P-256" => MidnightCurve::P256,
            other => {
                tracing::warn!(id = %k.id, crv = other, "preprod-live: unknown crv, skipping");
                continue;
            }
        };
        let private_key = match hex::decode(&k.private_key_hex) {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(error=%e, id=%k.id, "preprod-live: hex decode failed");
                continue;
            }
        };
        // Fall back to "preprod-default" for the purpose tag so
        // every imported key carries provenance; per-key
        // overrides (if the JSON annotated them) win.
        let purpose = k.purpose.clone().unwrap_or_else(|| "preprod-default".into());
        match futures::executor::block_on(store_handle.import_key(ImportKeyInput {
            id: k.id.clone(),
            private_key,
            kty,
            crv,
            did: k.did.clone(),
            purpose: Some(purpose),
        })) {
            Ok(_) => imported += 1,
            Err(e) => tracing::warn!(error=%e, id=%k.id, "preprod-live: import key failed"),
        }
    }
    tracing::info!(
        imported, retagged, skipped,
        "preprod-live: keys import done",
    );
}

#[component]
pub fn App() -> Element {
    // Splash screen — shown for the first ~1.5 s of every launch.
    // Renders the stacked Midnight logo against a full-screen
    // dark backdrop; auto-hides via a tokio sleep timer the
    // moment the App component first mounts.
    let mut splash_visible = use_signal(|| true);
    use_future(move || async move {
        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
        splash_visible.set(false);
    });

    // Android-only: bring up the rustls platform-verifier once
    // `ndk-context` has been seeded by dioxus-mobile. We can't do
    // this from `lib.rs::main` because that runs before dioxus
    // initialises the JNI bridge. Poll-then-init: a short loop
    // (max ~3s) handles the seed delay; once `try_init_android_tls`
    // returns `Ok(true)` the future stops. Without this every
    // HTTPS call hits the panic in
    // `rustls-platform-verifier/src/android.rs:94`.
    #[cfg(target_os = "android")]
    use_future(|| async move {
        for _ in 0..30 {
            match crate::try_init_android_tls() {
                Ok(true) => {
                    tracing::info!("rustls-platform-verifier ready");
                    return;
                }
                Ok(false) => {}
                Err(e) => {
                    tracing::warn!(error = %e, "rustls init failed");
                    return;
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        tracing::warn!("rustls-platform-verifier: gave up waiting for ndk-context");
    });
    if *splash_visible.read() {
        return rsx! {
            style { "{STYLES}" }
            div { class: "splash",
                div { class: "splash-logo", dangerous_inner_html: "{LOGO_SPLASH_SVG}" }
            }
        };
    }

    let mut network = use_signal(|| Network::PreProd);
    let mut wallet = use_signal::<Option<WalletInfo>>(|| {
        Some(WalletInfo::from_wallet(&app_wallet_for(Network::PreProd)))
    });
    let mut phase = use_signal(|| SyncPhase::Idle);
    let mut chain = use_signal::<ChainSnapshot>(ChainSnapshot::default);
    let mut probe = use_signal::<Option<ProbeResult>>(|| None);
    // Latest NIGHT subunit total from `Wallet::sync_unshielded()`.
    // None = never synced or sync in flight; Some(0) = synced, no
    // funds. The `unshielded_balance` future kicks off after a
    // successful Connect (see below).
    let mut night_subunits = use_signal::<Option<u128>>(|| None);
    // Same shape for DUST — synced by `WalletSyncPane`'s
    // auto-triggered fold + cached snapshot. `None` until the
    // first DUST sync completes; `Some(0)` is a real "zero
    // balance" reading.
    let dust_subunits = use_signal::<Option<u128>>(|| None);
    // Monotonic tick that drives `WalletSyncPane` to re-fire both
    // NIGHT + DUST sync streams. Bumped by the Wallet-tab CTA's
    // `connect()` closure once the endpoint probe succeeds, so
    // the user clicks one button to bring everything online
    // instead of two (the top CTA used to start NIGHT only, the
    // pane-internal button DUST). 0 = never triggered → pane
    // sits idle showing "queued". Each bump kicks both syncs.
    let mut sync_trigger = use_signal::<u64>(|| 0);
    // Top-right menu dropdown open/closed. Toggled by the `≡`
    // button; closed automatically when the user picks a tab.
    let mut menu_open = use_signal(|| false);
    // Active sub-view inside the merged Diagnostics carousel
    // (0=Probes, 1=Metrics, 2=Benchmark, 3=Test, 4=Logs). Lives
    // at App scope so Dioxus's hook ordering stays consistent
    // across renders regardless of which tab is showing.
    let mut diag_view = use_signal::<u8>(|| 0);
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
    // Per-flow cost snapshots (dust + NIGHT consumed). Pushed
    // by the spawn handlers that wrap each Wizard pipeline.
    let mut cost_log = use_signal::<Vec<CostRun>>(Vec::new);

    // ── JS bridge + embedded proof-server ─────────────────────────
    // BridgeState is cheap-clone (Arc<OnceCell<String>>); we keep a
    // copy in a signal for UI display and pass another into the
    // background spawn / bridge loop.
    let bridge_state = use_signal(|| {
        let state = BridgeState::new();
        // Attach the process-global log capture handle the
        // tracing layer was installed against. The Logs tab
        // reads its in-memory ring via this same handle.
        if let Some(cap) = crate::logs::LOG_CAPTURE.get() {
            state.set_log_capture(cap.clone());
        }
        state
    });
    let mut proof_server = use_signal::<Option<String>>(|| None);

    // Persistent wallet store lifecycle. Locked at boot —
    // the user types a passphrase into the unlock card and
    // we drive the open + hydration chain off that. Default
    // value pre-fills as "midnight"; the input box lets the
    // user override if they're opening a store sealed with a
    // different passphrase.
    let mut unlock_state = use_signal(|| UnlockState::Locked);
    let mut passphrase_input = use_signal(|| DEV_STORE_PASSPHRASE.to_string());

    let mut on_unlock = move |entered: String| {
        let state = bridge_state.read().clone();
        let net = *network.read();
        let seed_for_wallet = wallet
            .read()
            .as_ref()
            .map(|w| w.seed_hex.clone());
        unlock_state.set(UnlockState::Opening);
        spawn(async move {
            let path = wallet_store_path();
            match wallet_core::store::WalletStore::open(&path, &entered) {
                Ok(store) => {
                    state.set_store(store.clone());

                    // Spawn the log persistence drainer once
                    // — it owns the receiver half of the
                    // channel `lib.rs::run()` set up, batches
                    // events, and writes them to the `logs`
                    // table. Subsequent unlock retries don't
                    // re-spawn (the receiver was already
                    // taken).
                    if let Some(slot) = crate::logs::LOG_RX.get() {
                        if let Some(rx) = slot.lock().ok().and_then(|mut g| g.take()) {
                            let store_for_drain = store.clone();
                            spawn(async move {
                                crate::logs::run_persist_drainer(store_for_drain, rx).await;
                            });
                            tracing::info!("log persist drainer spawned");
                        }
                    }

                    // Bind the active wallet for this
                    // network. Auto-creates a row if none
                    // exists yet (e.g. first launch, or
                    // first time the user opens this
                    // network).
                    let wallet_id = find_or_create_wallet_for_network(
                        &store,
                        net,
                        seed_for_wallet.as_deref(),
                    );
                    state.set_active_wallet_id(wallet_id);

                    // Layer 2 / Phase 3: register a DustSyncer
                    // for this network so every subsequent
                    // `app_wallet_for(net)` returns a wallet that
                    // resumes DUST sync from the persisted
                    // checkpoint instead of doing a full event
                    // replay. The syncer reads `DUST_SYNC` rows
                    // from the same store we just opened.
                    {
                        // The dust secret key is derived from
                        // the wallet seed; we need to build a
                        // temporary wallet to get it. This
                        // wallet itself doesn't need a syncer —
                        // we're constructing the syncer it'd use.
                        let tmp = app_wallet_for(net);
                        match tmp.dust_secret_key() {
                            Ok(sk) => {
                                let syncer = std::sync::Arc::new(
                                    wallet_core::DustSyncer::new(
                                        net,
                                        std::sync::Arc::new(store.clone()),
                                        sk,
                                    ),
                                );
                                set_dust_syncer_for(net, syncer);
                                tracing::info!(
                                    network=?net,
                                    "dust syncer registered (path B / phase 3)"
                                );
                            }
                            Err(e) => {
                                tracing::warn!(
                                    error = %e,
                                    "dust secret key derivation failed; DustSyncer not registered"
                                );
                            }
                        }
                    }

                    // PreProd-live demo: stamp the operator's
                    // three manager-profile DIDs into the
                    // inventory + seed their controller
                    // secrets so the Sign tab + Update DID
                    // flows are immediately usable. Idempotent
                    // — re-running just refreshes the
                    // timestamps.
                    #[cfg(feature = "preprod-live")]
                    if matches!(net, Network::PreProd) {
                        seed_preprod_live_state(&state, &store);
                        if let Some(wid) = wallet_id {
                            seed_preprod_live_keys(&store, wid);
                        } else {
                            tracing::warn!(
                                "preprod-live: no active wallet_id, skipping key import"
                            );
                        }
                    }

                    let n = state.hydrate_controller_secrets(net);
                    let inv_map = load_inventory_for_network(&store, net);
                    let inv_count = inv_map.len();
                    if !inv_map.is_empty() {
                        did_inventory.set(inv_map);
                    }
                    let cache_map = load_resolved_cache_for_network(&store, net);
                    let cache_count = cache_map.len();
                    if !cache_map.is_empty() {
                        resolved_cache.set(cache_map);
                    }
                    // Restore the last-session row. Falls back
                    // silently if the row is absent (first
                    // launch) or the network it captured no
                    // longer matches the user's current pick
                    // — we only apply tab / open_did / etc.
                    // when the snapshot's network agrees with
                    // the active one.
                    let mut restored_session = false;
                    if let Ok(Some(snap)) = store.get_session() {
                        if snap.network == net {
                            active_tab.set(Tab::from_persist(snap.active_tab));
                            open_did.set(snap.open_did);
                            last_did_id.set(snap.last_did_id);
                            last_resolved.set(snap.last_resolved);
                            restored_session = true;
                        }
                    }
                    tracing::info!(
                        path=%path.display(),
                        network=?net,
                        wallet_id=?wallet_id,
                        hydrated_controller_secrets=n,
                        hydrated_inventory=inv_count,
                        hydrated_cache=cache_count,
                        restored_session,
                        "wallet store opened",
                    );
                    unlock_state.set(UnlockState::Open);

                    // Auto-resolve every hydrated DID in the
                    // background. The persistent inventory
                    // table just carries the last snapshot of
                    // each DID's status / counter / vm count;
                    // those values can drift between sessions
                    // (someone calls a circuit while the
                    // wallet is closed, a deactivate happens
                    // from another client, etc.). Re-resolve
                    // them so the inventory list shows fresh
                    // values on the next paint.
                    //
                    // Done one task per DID so a slow
                    // indexer hit doesn't block the others;
                    // failures only log + leave the in-memory
                    // entry as-is.
                    let dids_to_refresh: Vec<String> =
                        did_inventory.read().keys().cloned().collect();
                    for did_str in dids_to_refresh {
                        let net = net;
                        let bridge = state.clone();
                        spawn(async move {
                            let w = app_wallet_for(net);
                            match w.resolve_did_full(&did_str).await {
                                Ok(resolved) => {
                                    let did_string =
                                        resolved.document.id.to_did_string();
                                    let entry = DidInventoryEntry {
                                        did: did_string.clone(),
                                        network_label: net.label().to_string(),
                                        status: if resolved.document.deactivated {
                                            DidInventoryStatus::Deactivated
                                        } else {
                                            DidInventoryStatus::Active
                                        },
                                        counter: Some(resolved.maintenance_counter),
                                        vm_count: Some(
                                            resolved.document.verification_method.len(),
                                        ),
                                        service_count: Some(resolved.document.service.len()),
                                        last_block_height: resolved.last_block_height,
                                    };
                                    let mut inv = did_inventory.read().clone();
                                    inv.insert(did_string.clone(), entry.clone());
                                    did_inventory.set(inv);
                                    persist_inventory_entry(&bridge, net, &entry);
                                    // Snapshot before overwrite
                                    // for the cross-resolve diff
                                    // card.
                                    let cache_snap = resolved_cache.read().clone();
                                    if let Some(prev) = cache_snap.get(&did_string) {
                                        let mut prev_map =
                                            previous_resolved_cache.read().clone();
                                        prev_map.insert(
                                            did_string.clone(),
                                            prev.clone(),
                                        );
                                        previous_resolved_cache.set(prev_map);
                                    }
                                    let mut cache = cache_snap;
                                    cache.insert(did_string.clone(), resolved.clone());
                                    resolved_cache.set(cache);
                                    persist_resolved_cache(
                                        &bridge,
                                        net,
                                        &did_string,
                                        &resolved,
                                    );
                                    tracing::debug!(
                                        did=%did_string,
                                        counter=resolved.maintenance_counter,
                                        "auto-resolved at unlock",
                                    );
                                }
                                Err(e) => {
                                    tracing::debug!(
                                        error=%e,
                                        did=%did_str,
                                        "auto-resolve at unlock failed (leaving cached snapshot)",
                                    );
                                }
                            }
                        });
                    }
                }
                Err(e) => {
                    let msg = e.to_string();
                    tracing::warn!(error=%msg, path=%path.display(), "wallet store open failed");
                    unlock_state.set(UnlockState::Failed(msg));
                }
            }
        });
    };

    use_future(move || {
        let state = bridge_state.read().clone();
        async move {
            match spawn_proof_server(&state).await {
                Ok(url) => proof_server.set(Some(url)),
                Err(e) => tracing::warn!(error=%e, "embedded proof-server unavailable"),
            }
        }
    });

    // Persist the session row whenever any of its component
    // signals changes. `use_effect` re-runs after every render
    // that touched at least one of the read signals, so this
    // approach is debounced "to the next render boundary" by
    // construction — fine for the rate of mutations the wallet
    // actually drives (clicks, not loops).
    use_effect(move || {
        let net = *network.read();
        let tab = *active_tab.read();
        let open = open_did.read().clone();
        let last_did = last_did_id.read().clone();
        let last_res = last_resolved.read().clone();
        let state = bridge_state.read().clone();
        // Only persist after the store has been attached. The
        // use_future above runs concurrently with the first
        // render, so this effect could fire before `set_store`.
        if state.store().is_some() {
            persist_session(&state, net, tab, open, last_did, last_res);
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

    // Long-lived driver task for the WebView-side JS bridge. The
    // `JsBridge` handle installed in `app_wallet_for` is just a
    // channel sender; **this** task is what owns the receiver and
    // calls `document::eval` on every request. Spawning it from a
    // `use_future` guarantees we're inside the Dioxus runtime so
    // `document::eval` has the `Eval` machinery to bind against.
    //
    // First caller wins via `install_global` — subsequent calls
    // (e.g. when the App re-runs on a network swap) drop their
    // receiver and bail. The bridge handle in the global slot stays
    // valid for the life of the process.
    #[cfg(feature = "js-bridge")]
    use_future(|| async move {
        if let Some(rx) = crate::eval_bridge::install_global() {
            crate::eval_bridge::run_driver(rx).await;
        } else {
            tracing::debug!(
                target: "eval-bridge",
                "global bridge already installed; this App instance skips spawning a driver",
            );
        }
    });

    let mut load_demo = move || {
        let w = app_wallet_for(*network.read());
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
                HttpIndexerClient::new(net)
                    .map_err(|e| e.to_string())?
                    .chain_tip()
                    .await
                    .map_err(|e| e.to_string())
            };
            let node_fut = async {
                SubxtNodeClient::connect(net)
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

            // After a successful endpoint probe, cascade into the
            // `WalletSyncPane` to bring NIGHT (UTXO snapshot) +
            // DUST (event-stream replay) online in one user
            // action. The pane owns both sync rows and runs the
            // same `Wallet::sync_unshielded()` we used to duplicate
            // here, plus the DUST stream — so this CTA now just
            // bumps the trigger and lets the pane do the work.
            // (Old behaviour: probe → snapshot NIGHT here, then
            // user had to click a second button inside the pane to
            // kick DUST.)
            if errs.is_empty() {
                let next = *sync_trigger.read() + 1;
                sync_trigger.set(next);
            }
        });
    };

    let busy = matches!(*phase.read(), SyncPhase::Connecting);

    // If the store hasn't been unlocked yet, render the
    // unlock card and short-circuit. The rest of the app
    // depends on a hydrated store for inventory / keys /
    // session restore, and locking the UI behind the gate is
    // both more honest and simpler than rendering half-empty
    // tabs.
    let cur_unlock = unlock_state.read().clone();
    if !matches!(cur_unlock, UnlockState::Open) {
        return rsx! {
            style { "{STYLES}" }
            div { class: "header",
                div { class: "logo", dangerous_inner_html: "{LOGO_SVG}" }
            }
            UnlockCard {
                state: cur_unlock,
                passphrase: passphrase_input.read().clone(),
                on_input: move |s: String| passphrase_input.set(s),
                on_unlock: move |_| {
                    let p = passphrase_input.read().clone();
                    on_unlock(p);
                },
            }
        };
    }

    rsx! {
        style { "{STYLES}" }
        // The midnight-did bundle is injected via `with_custom_head`
        // (see lib.rs::desktop_or_mobile_launch) so it runs at
        // page-parse time and ahead of the bridge JS shim.

        // Mobile-friendly nav: the `≡` button in the top-right
        // toggles a dropdown listing every tab. The horizontal
        // tab-strip is still rendered below as a fallback for
        // wider viewports — CSS hides it on mobile (`@media
        // (max-width: 480px)` rule). Both surfaces share the
        // `active_tab` signal so they stay in sync.
        //
        // The Midnight wordmark goes where the `<h1>` title used
        // to live. The current tab name lives on a thin line
        // below so the active state is still visible.
        div { class: "header",
            div { class: "logo", dangerous_inner_html: "{LOGO_SVG}" }
            button {
                class: "menu-btn",
                title: "Menu",
                onclick: move |_| {
                    let cur = *menu_open.read();
                    menu_open.set(!cur);
                },
                "≡"
            }
        }
        div { class: "header-subtitle", "{active_tab.read().label()}" }
        if *menu_open.read() {
            div { class: "menu-dropdown",
                // Metrics / Benchmark / Test / Logs were collapsed
                // into a carousel under Diagnostics — the top-level
                // tab bar no longer lists them.
                for t in [Tab::Wallet, Tab::Dids, Tab::Identity, Tab::Keys, Tab::Diagnostics, Tab::Bootstrap, Tab::Settings] {
                    button {
                        class: if *active_tab.read() == t { "menu-item active" } else { "menu-item" },
                        onclick: move |_| {
                            active_tab.set(t);
                            menu_open.set(false);
                        },
                        "{t.label()}"
                    }
                }
            }
        }

        StatusLine {
            phase: phase.read().clone(),
            network: *network.read(),
            tip_height: chain.read().tip.as_ref().map(|t| t.height),
        }

        // `WalletStoreBadge` now lives only on the Settings tab —
        // it added a permanent strip across every screen that the
        // operator doesn't need to see during normal use. The
        // component is mounted at the top of `SettingsTab` below.

        // Tab navigation. Each button sets active_tab; rendering
        // below is a single match on the current value. CSS hides
        // this row on narrow viewports — see `.tab-nav` rule.
        div { class: "tab-nav",
            for t in [Tab::Wallet, Tab::Dids, Tab::Identity, Tab::Keys, Tab::Diagnostics, Tab::Bootstrap, Tab::Metrics, Tab::Benchmark, Tab::Test, Tab::Logs, Tab::Settings] {
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
                    dust_subunits: *dust_subunits.read(),
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
                            let Some(n) = parse_network(&e.value()) else { return };
                            if n == *network.read() {
                                return;
                            }
                            network.set(n);
                            chain.set(ChainSnapshot::default());
                            phase.set(SyncPhase::Idle);
                            night_subunits.set(None);
                            let was_demo = wallet
                                .read()
                                .as_ref()
                                .map(|w| {
                                    w.seed_hex == wallet_core::DEMO_SEED_HEX
                                        || w.seed_hex
                                            == wallet_core::UNDEPLOYED_GENESIS_SEED_HEX
                                })
                                .unwrap_or(false);
                            // Always re-derive the WalletInfo
                            // for the new network — even if the
                            // user previously loaded a random
                            // wallet, the next sync needs the
                            // network-correct keys.
                            let new_wallet = app_wallet_for(n);
                            let seed_hex = new_wallet.seed_hex();
                            if was_demo || wallet.read().is_none() {
                                wallet.set(Some(WalletInfo::from_wallet(&new_wallet)));
                            }
                            // Re-hydrate every per-network
                            // signal so the inventory, cache,
                            // open-DID badge, and active wallet
                            // id match the new network.
                            let state = bridge_state.read().clone();
                            spawn(async move {
                                rehydrate_for_network(
                                    state,
                                    n,
                                    Some(seed_hex),
                                    did_inventory,
                                    resolved_cache,
                                    previous_resolved_cache,
                                    open_did,
                                    last_did_id,
                                    last_resolved,
                                )
                                .await;
                            });
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

                WalletSyncPane {
                    network: *network.read(),
                    night_subunits,
                    dust_subunits,
                    sync_trigger,
                }
            },
            Tab::Dids => rsx! {
                // `CreateDidWizard` was removed from this tab —
                // operator demos run against the 3 pre-seeded
                // preprod-live DIDs and the wizard was redundant
                // alongside the per-row `Open` + the detail-view
                // `Update DID` button. Wizard component still lives
                // in this file and can be re-mounted here in one
                // line if we need it back.
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
                        bridge_state: bridge_state.read().clone(),
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
                        on_cost: move |cost: CostRun| {
                            let mut log = cost_log.read().clone();
                            log.push(cost);
                            cost_log.set(log);
                        },
                        on_event: move |ev: SessionEvent| {
                            let mut log = session_log.read().clone();
                            log.push(ev);
                            session_log.set(log);
                        },
                    }
                } else {
                    // Browse mode: Create DID button + inventory +
                    // resolve panel.
                    //
                    // The wizard streams a `DeployOutcome` on
                    // success. We persist the controller secret
                    // (without it the operator can never update or
                    // deactivate the DID), insert a `Pending` row
                    // in the inventory, log the deploy, and kick
                    // off a background resolve so the row's
                    // counter / VM count / status badge converge
                    // on the on-chain truth as soon as the indexer
                    // catches up.
                    CreateDidWizard {
                        network: *network.read(),
                        on_done: move |outcome: wallet_core::DeployOutcome| {
                            let net = *network.read();
                            let did_string = outcome.did_id.to_did_string();
                            // Persist controller_sk first — without
                            // it the user can never update or
                            // deactivate this DID, and the wizard's
                            // success blob is the only place it's
                            // ever surfaced in the clear.
                            bridge_state.read().remember_controller_secret(
                                net,
                                did_string.clone(),
                                outcome.controller_sk,
                            );
                            // Pending inventory entry — counter +
                            // VMs unknown until the indexer catches
                            // up and the auto-resolve below
                            // overwrites it with real values.
                            let entry = DidInventoryEntry {
                                did: did_string.clone(),
                                network_label: net.label().to_string(),
                                status: DidInventoryStatus::Pending,
                                counter: None,
                                vm_count: None,
                                service_count: None,
                                last_block_height: None,
                            };
                            let mut inv = did_inventory.read().clone();
                            inv.insert(did_string.clone(), entry.clone());
                            did_inventory.set(inv);
                            persist_inventory_entry(
                                &bridge_state.read(),
                                net,
                                &entry,
                            );
                            // Session log entry.
                            let mut log = session_log.read().clone();
                            log.push(SessionEvent::Deploy {
                                did: did_string.clone(),
                                tx_hash: outcome.tx_hash,
                                block_hash: outcome.block_hash,
                            });
                            session_log.set(log);
                            // Resolve-panel pre-fill.
                            last_did_id.set(Some(did_string.clone()));
                            // Background auto-resolve to flip the
                            // Pending badge to Active and fill in
                            // counter / VM / service counts.
                            //
                            // **Retry loop** because the indexer
                            // has its own ingestion lag (a few
                            // seconds on standalone, longer on
                            // PreProd). Without retries the first
                            // resolve fires at <1 s post-deploy,
                            // the indexer 404s on the new
                            // address, and the entry stays
                            // Pending forever — even though the
                            // DID exists on chain. Pattern mirrors
                            // `wait_for_indexer_settle` in
                            // `wallet-core::did::bootstrap`:
                            // 10 attempts × 3 s back-off = ~30 s
                            // window matching the live bootstrap
                            // flow's settle deadline.
                            let bridge = bridge_state.read().clone();
                            let did_for_spawn = did_string.clone();
                            spawn(async move {
                                let w = app_wallet_for(net);
                                const MAX_ATTEMPTS: u32 = 10;
                                const BACKOFF: std::time::Duration =
                                    std::time::Duration::from_secs(3);
                                let mut last_err: Option<String> = None;
                                let mut resolved_opt = None;
                                for attempt in 1..=MAX_ATTEMPTS {
                                    match w.resolve_did_full(&did_for_spawn).await {
                                        Ok(r) => {
                                            resolved_opt = Some(r);
                                            break;
                                        }
                                        Err(e) => {
                                            let msg = e.to_string();
                                            // Indexer-lag failures are
                                            // expected on the first
                                            // few attempts — log at
                                            // DEBUG so we don't spam
                                            // WARN on the happy path.
                                            tracing::debug!(
                                                did=%did_for_spawn,
                                                attempt,
                                                error=%msg,
                                                "auto-resolve attempt failed; retrying",
                                            );
                                            last_err = Some(msg);
                                            if attempt < MAX_ATTEMPTS {
                                                tokio::time::sleep(BACKOFF).await;
                                            }
                                        }
                                    }
                                }
                                let Some(resolved) = resolved_opt else {
                                    tracing::warn!(
                                        did=%did_for_spawn,
                                        attempts=MAX_ATTEMPTS,
                                        last_error=%last_err.unwrap_or_default(),
                                        "auto-resolve after Create DID failed \
                                         after retries — entry stays Pending; \
                                         use Resolve panel to refresh later",
                                    );
                                    return;
                                };
                                match Ok::<_, ()>(resolved) {
                                    Ok(resolved) => {
                                        let resolved_did =
                                            resolved.document.id.to_did_string();
                                        let entry = DidInventoryEntry {
                                            did: resolved_did.clone(),
                                            network_label: net.label().to_string(),
                                            status: if resolved.document.deactivated {
                                                DidInventoryStatus::Deactivated
                                            } else {
                                                DidInventoryStatus::Active
                                            },
                                            counter: Some(resolved.maintenance_counter),
                                            vm_count: Some(
                                                resolved.document.verification_method.len(),
                                            ),
                                            service_count: Some(
                                                resolved.document.service.len(),
                                            ),
                                            last_block_height: resolved.last_block_height,
                                        };
                                        let mut inv = did_inventory.read().clone();
                                        inv.insert(resolved_did.clone(), entry.clone());
                                        did_inventory.set(inv);
                                        persist_inventory_entry(&bridge, net, &entry);
                                        let mut cache = resolved_cache.read().clone();
                                        cache.insert(resolved_did.clone(), resolved.clone());
                                        resolved_cache.set(cache);
                                        persist_resolved_cache(
                                            &bridge,
                                            net,
                                            &resolved_did,
                                            &resolved,
                                        );
                                    }
                                    Err(_) => unreachable!(
                                        "outer Result was synthesized from Ok(resolved) above",
                                    ),
                                }
                            });
                        },
                        on_timing: move |run: TimingRun| {
                            let mut log = timing_log.read().clone();
                            log.push(run);
                            timing_log.set(log);
                        },
                        on_cost: move |cost: CostRun| {
                            let mut log = cost_log.read().clone();
                            log.push(cost);
                            cost_log.set(log);
                        },
                    }
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
                    // `LoadCircuitPanel` (manual VK reload via
                    // MaintenanceUpdate) and `DidOperationsPanel`
                    // (offline draft-only) were placeholders from
                    // before the Operation Builder shipped. The
                    // batch flow now auto-loads any missing
                    // circuit's VK before the call, and submission
                    // is wired end-to-end — both panels are
                    // obsolete. Components left in source for
                    // reference but no longer rendered.
                    SessionLogPanel { events: session_log.read().clone() }
                }
            },
            // Diagnostics is now a 5-page horizontal carousel
            // (M3 swipe-and-snap pattern). Page 0 is the original
            // Probes content; pages 1–4 are the Metrics / Benchmark
            // / Test / Logs views that used to be top-level tabs.
            // Sub-nav chips above the carousel jump to a page;
            // touch-swipe also works because the container has
            // `scroll-snap-type: x mandatory`.
            Tab::Diagnostics => rsx! {
                div { class: "carousel-nav",
                    for (idx , label) in [
                        "Probes", "Metrics", "Benchmark", "Test", "Logs"
                    ].iter().enumerate() {
                        button {
                            class: if *diag_view.read() as usize == idx {
                                "carousel-nav-item active"
                            } else {
                                "carousel-nav-item"
                            },
                            onclick: move |_| {
                                diag_view.set(idx as u8);
                                // Smooth-scroll the carousel to
                                // the matching page. document::eval
                                // is the cross-platform handle Wry
                                // exposes; same call works on
                                // desktop / Android / iOS.
                                let snippet = format!(
                                    "document.getElementById('diag-page-{idx}')\
                                        ?.scrollIntoView({{behavior:'smooth',inline:'start',block:'nearest'}});"
                                );
                                let _ = dioxus::document::eval(&snippet);
                            },
                            "{label}"
                        }
                    }
                }
                div { class: "carousel", id: "diag-carousel",
                    // Swipe-driven sync: when the user swipes the
                    // carousel by hand the scroll-snap settles on a
                    // new page; this handler reads the resulting
                    // `scrollLeft / clientWidth` from JS and lifts
                    // it back into `diag_view` so the sub-nav chip
                    // highlight follows the gesture. Click-driven
                    // navigation (the chips) sets `diag_view`
                    // directly and `scrollIntoView`s; the resulting
                    // scroll fires this same handler, which just
                    // re-sets the signal to the same value — cheap
                    // and idempotent.
                    onscroll: move |_| {
                        spawn(async move {
                            let snippet = "\
                                const el = document.getElementById('diag-carousel');\
                                return Math.round(el.scrollLeft / Math.max(el.clientWidth, 1));\
                            ";
                            if let Ok(v) = dioxus::document::eval(snippet).await {
                                // Math.round returns an integer-shaped
                                // JS number; serde_json may decode it
                                // as either u64 or f64 depending on
                                // serialisation. Try both shapes.
                                let idx = v
                                    .as_u64()
                                    .map(|n| n as u8)
                                    .or_else(|| v.as_f64().map(|f| f as u8));
                                if let Some(i) = idx {
                                    if i <= 4 {
                                        diag_view.set(i);
                                    }
                                }
                            }
                        });
                    },
                    // Page 0 — Probes (the original Diagnostics content)
                    div { class: "carousel-page", id: "diag-page-0",
                        TxCostPanel { runs: cost_log.read().clone() }
                        if let Some(w) = wallet.read().as_ref() {
                            div { class: "card",
                                div { class: "card-header", "Wallet identity" }
                                {kv_blob_row("Seed (hex)", &w.seed_hex)}
                                {kv_blob_row("Coin PK", &w.coin_pk_hex)}
                                {kv_blob_row("Encryption PK", &w.enc_pk_hex)}
                            }
                        }
                        if let Some(p) = probe.read().as_ref() {
                            div { class: "card",
                                div { class: "card-header", "Last probe — {p.network.label()}" }
                                ProbeRowCompact { name: "indexer http", url: p.indexer_http.url.clone(), reachable: p.indexer_http.reachable, latency: p.indexer_http.latency_ms, detail: p.indexer_http.detail.clone() }
                                ProbeRowCompact { name: "indexer ws",   url: p.indexer_ws.url.clone(),   reachable: p.indexer_ws.reachable,   latency: p.indexer_ws.latency_ms,   detail: p.indexer_ws.detail.clone() }
                                ProbeRowCompact { name: "node ws",      url: p.node_ws.url.clone(),      reachable: p.node_ws.reachable,      latency: p.node_ws.latency_ms,      detail: p.node_ws.detail.clone() }
                            }
                        }
                        if let Some(s) = chain.read().node.as_ref() {
                            div { class: "card",
                                div { class: "card-header", "Node" }
                                {kv_blob_row("Finalized head", &s.finalized_head_hash)}
                            }
                        }
                        if let Some(url) = proof_server.read().as_ref() {
                            div { class: "card",
                                div { class: "card-header",
                                    style: "display: flex; align-items: center; gap: 8px;",
                                    "Embedded proof-server"
                                    span { class: "status-pill success",
                                        span { class: "dot" }
                                        "active"
                                    }
                                }
                                {kv_blob_row("URL", url)}
                                {kv_blob_row(
                                    "Used by",
                                    "Wallet DID writes (Update / Deactivate / addAlsoKnownAs / removeAlsoKnownAs) — via tx::prove::prove_via_http"
                                )}
                                {kv_blob_row(
                                    "Not used by",
                                    "Benchmark tab — calls contract_benchmark::run_proof(k) directly through the halo2 library"
                                )}
                            }
                        }
                    }
                    // Page 1 — Metrics (+ Telemetry from the
                    // wallet-core `InMemoryMetrics` aggregator,
                    // see telemetry_panel.rs)
                    div { class: "carousel-page", id: "diag-page-1",
                        crate::telemetry_panel::TelemetryPanel {
                            bridge_state: bridge_state.read().clone(),
                        }
                        MetricsTab {
                            timings: timing_log.read().clone(),
                            costs: cost_log.read().clone(),
                        }
                    }
                    // Page 2 — Benchmark
                    div { class: "carousel-page", id: "diag-page-2",
                        BenchmarkTab {}
                    }
                    // Page 3 — Test / dev probes
                    div { class: "carousel-page", id: "diag-page-3",
                        JsBridgePanel { seed_did: last_did_id.read().clone() }
                        TimingsPanel { runs: timing_log.read().clone() }
                    }
                    // Page 4 — Logs
                    div { class: "carousel-page", id: "diag-page-4",
                        LogsTab { bridge_state: bridge_state.read().clone() }
                    }
                }
            },
            Tab::Keys => rsx! {
                KeysTab { bridge_state: bridge_state.read().clone() }
            },
            Tab::Metrics => rsx! {
                MetricsTab {
                    timings: timing_log.read().clone(),
                    costs: cost_log.read().clone(),
                }
            },
            Tab::Benchmark => rsx! {
                BenchmarkTab {}
            },
            Tab::Test => rsx! {
                JsBridgePanel { seed_did: last_did_id.read().clone() }
                TimingsPanel { runs: timing_log.read().clone() }
            },
            Tab::Logs => rsx! {
                LogsTab { bridge_state: bridge_state.read().clone() }
            },
            Tab::Settings => rsx! {
                SettingsTab { bridge_state: bridge_state.read().clone() }
            },
            Tab::Bootstrap => rsx! {
                crate::identity_centre::BootstrapPanel {
                    network: *network.read(),
                    bridge_state: bridge_state.read().clone(),
                    did_inventory,
                    // Same `on_did_minted` channel the Identity Centre
                    // used pre-C1 (and the Create-DID wizard before
                    // that) — inserts the new DID into the live
                    // inventory signal + persists. See `Tab::Identity`
                    // below for the full rationale.
                    on_did_minted: move |(did, net): (String, Network)| {
                        let entry = DidInventoryEntry {
                            did: did.clone(),
                            network_label: net.label().to_string(),
                            status: DidInventoryStatus::Active,
                            counter: None,
                            vm_count: Some(2),
                            service_count: Some(0),
                            last_block_height: None,
                        };
                        let mut inv = did_inventory.read().clone();
                        inv.insert(did.clone(), entry.clone());
                        did_inventory.set(inv);
                        persist_inventory_entry(
                            &bridge_state.read(),
                            net,
                            &entry,
                        );
                    },
                }
            },
            Tab::Identity => rsx! {
                crate::identity_centre::IdentityCentrePanel {
                    network: *network.read(),
                    bridge_state: bridge_state.read().clone(),
                    did_inventory,
                    // Callback fires after `bootstrap_did_with_keys`
                    // succeeds. Mirrors what the Create-DID wizard's
                    // `on_done` does: insert a Pending entry into the
                    // live `did_inventory` signal AND persist it to
                    // redb. The Dids tab renders from the signal, so
                    // this is what makes the new DID show up
                    // immediately (no app restart, no network switch).
                    //
                    // Today the Identity Centre tab itself doesn't
                    // bootstrap (Bootstrap tab does), but the prop is
                    // still threaded so the C2/C3 work — and any
                    // future in-Identity-Centre DID minting — can
                    // light up without rewiring the parent.
                    on_did_minted: move |(did, net): (String, Network)| {
                        let entry = DidInventoryEntry {
                            did: did.clone(),
                            network_label: net.label().to_string(),
                            // `bootstrap_did_with_keys` includes
                            // indexer-settle waits between every
                            // chain write, so by the time this
                            // callback fires the DID is fully
                            // active on chain — no need to leave
                            // the row in Pending and force the
                            // user to click Open to trigger an
                            // auto-resolve refresh. (The
                            // Create-DID Wizard path at L1924
                            // legitimately uses Pending because
                            // it submits + returns without
                            // settling.)
                            status: DidInventoryStatus::Active,
                            counter: None,
                            // bootstrap_did_with_keys attaches exactly
                            // two VMs (Ed25519 + Jubjub) before
                            // returning Ok; pre-seed the count so the
                            // Dids row reads "2 VMs · 0 services"
                            // immediately, even before Resolve.
                            vm_count: Some(2),
                            service_count: Some(0),
                            last_block_height: None,
                        };
                        let mut inv = did_inventory.read().clone();
                        inv.insert(did.clone(), entry.clone());
                        did_inventory.set(inv);
                        persist_inventory_entry(
                            &bridge_state.read(),
                            net,
                            &entry,
                        );
                    },
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
    /// Used by `CreateDidWizard`'s success panel. Mounted again
    /// on the DIDs tab so the operator can bootstrap a fresh DID
    /// against the active network (Undeployed / PreProd).
    Done(&'a wallet_core::DeployOutcome),
    Failed(&'a str),
}

// Mounted on the DIDs tab inside Tab::Dids's browse mode so the
// operator can create a fresh DID against the active network. The
// preprod-live seed path covers PreProd's 3 demo DIDs at unlock,
// but Undeployed and freshly-seeded sessions need the wizard to
// produce an actual DID before any of the other panels do
// anything useful.
#[component]
fn CreateDidWizard(
    network: Network,
    on_done: EventHandler<wallet_core::DeployOutcome>,
    on_timing: EventHandler<TimingRun>,
    on_cost: EventHandler<CostRun>,
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
        let on_cost = on_cost.clone();
        spawn(async move {
            use futures::StreamExt;
            let w = app_wallet_for(network);
            // Bracket the pipeline with balance snapshots so
            // we can compute the dust + NIGHT cost. A failure
            // to take the pre-snapshot disables cost
            // reporting for this run but doesn't block the
            // pipeline.
            let cost_start = std::time::Instant::now();
            let before = w.balance_snapshot().await.ok();
            let mut stream = std::pin::pin!(w.create_did());
            let mut observations: Vec<(usize, std::time::Instant)> = Vec::new();
            let mut succeeded = false;
            while let Some(stage) = stream.next().await {
                let now = std::time::Instant::now();
                if let Some(idx) = stage_pipeline_idx(&stage) {
                    observations.push((idx, now));
                } else {
                    succeeded = matches!(&stage, WizardStage::Done(_));
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
            if let (Some(before), Ok(after)) = (before, w.balance_snapshot().await) {
                on_cost.call(CostRun {
                    label: "create_did".to_string(),
                    dust_consumed: before.dust_atomic.saturating_sub(after.dust_atomic),
                    night_consumed: before.night_atomic.saturating_sub(after.night_atomic),
                    duration_ms: cost_start.elapsed().as_millis() as u64,
                    succeeded,
                });
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

    // The button itself carries the "Create DID" label so the
    // older `<div class="wizard-header">Create DID</div>` above
    // was redundant — two elements competing for the same row.
    // Dropped the header; the button-as-CTA is the single
    // affordance now.
    rsx! {
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
                {
                    let sk_hex = format!("0x{}", hex::encode(outcome.controller_sk));
                    rsx! {
                        div { class: "row",
                            div { class: "seed-blob",
                                style: "flex: 1; word-break: break-all;",
                                "{sk_hex}"
                            }
                            {copy_btn(sk_hex.clone(), "Copy controller secret (hex)")}
                        }
                    }
                }
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
            let w = app_wallet_for(network);
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

/// Status of one row inside `WalletSyncPane`. Three terminal
/// states + a "running with optional progress" middle state so
/// the UI can render a progress bar when one is available
/// (DUST) and a generic spinner when one isn't (NIGHT / UTXO
/// snapshot — fast enough that progress would flicker).
#[derive(Clone, Debug)]
enum SyncRow {
    Idle,
    /// Optional `(current, max)` for rows with a progress bar.
    Running {
        progress: Option<(i64, i64)>,
        note: String,
    },
    Done {
        summary: String,
    },
    Failed {
        err: String,
    },
}

/// Unified wallet-initialisation pane. Replaces the previous
/// separate "Sync balance" / "Sync DUST" buttons.
///
/// On mount it auto-kicks both syncs concurrently:
/// - **NIGHT** — `Wallet::sync_unshielded` (the indexer's
///   `unshieldedUtxoEvents` stream for the wallet's address).
///   Usually completes in < 1 s.
/// - **DUST** — `DustSyncer::sync()` (the Path-B persisted
///   snapshot + delta-resume from `5f7df14f → a7a10c5d`).
///   First run pays the full cold-replay cost (~10–15 min on
///   PreProd); subsequent runs are seconds.
///
/// Each row shows `name → status pill → progress bar (DUST only)`.
/// When a row reaches `Done`, the parent's `night_subunits` /
/// `dust_subunits` signal lands, which makes `BalancesCard`
/// switch from `"syncing…"` to the actual atomic-unit value.
///
/// Behaviour notes:
/// - The auto-trigger uses a `started` guard so re-renders
///   don't restart in-flight syncs. The button at the bottom
///   forces a re-sync (useful after a known on-chain change).
/// - The DUST sync runs on the same tokio executor as the
///   renderer; we yield every 128 events (see
///   `dust::syncer::PERSIST_EVERY_N_EVENTS`) so the UI stays
///   responsive even during the cold replay.
#[component]
fn WalletSyncPane(
    network: Network,
    night_subunits: Signal<Option<u128>>,
    dust_subunits: Signal<Option<u128>>,
    /// Monotonic trigger from the parent Wallet tab. When the
    /// value bumps from 0 → 1 → 2 → … the pane re-fires both
    /// NIGHT and DUST sync streams. Driven by the top-level
    /// `Connect` / `Reconnect` CTA so a single click covers
    /// everything; the pane no longer owns its own button.
    sync_trigger: Signal<u64>,
) -> Element {
    let night_row = use_signal::<SyncRow>(|| SyncRow::Idle);
    let dust_row = use_signal::<SyncRow>(|| SyncRow::Idle);
    // Latest DUST checkpoint event id surfaced under the rows. Held
    // outside `SyncRow::Done` so it survives across resyncs as a
    // standalone status line and isn't repeated inside the row's
    // summary.
    let dust_event_id = use_signal::<Option<i64>>(|| None);
    let started = use_signal(|| false);

    // Closure that re-fires both syncs from scratch.
    let kick = move || {
        let mut night_row = night_row;
        let mut dust_row = dust_row;
        let mut night_subunits = night_subunits;
        let mut dust_subunits = dust_subunits;
        let mut dust_event_id = dust_event_id;

        // NIGHT — fast, no progress bar.
        night_row.set(SyncRow::Running {
            progress: None,
            note: "snapshotting UTXOs…".to_string(),
        });
        night_subunits.set(None);
        spawn(async move {
            let w = app_wallet_for(network);
            match w.sync_unshielded().await {
                Ok(set) => {
                    let total: u128 = set
                        .iter()
                        .fold(0u128, |a, u| a.saturating_add(u.value));
                    // Surface the raw atomic count in logs so the
                    // formatter (lossy by design) can be sanity-checked
                    // against ground truth without re-reading source.
                    tracing::info!(
                        target: "balance",
                        atomic = %total,
                        utxos = set.len(),
                        "NIGHT balance synced",
                    );
                    night_subunits.set(Some(total));
                    let (compact, _) = format_balance(total, NIGHT_DECIMALS);
                    night_row.set(SyncRow::Done {
                        summary: format!(
                            "{} NIGHT ({} UTXO{})",
                            compact,
                            set.len(),
                            if set.len() == 1 { "" } else { "s" },
                        ),
                    });
                }
                Err(e) => {
                    night_row.set(SyncRow::Failed { err: e.to_string() });
                }
            }
        });

        // DUST — slow, has a progress stream.
        dust_row.set(SyncRow::Running {
            progress: None,
            note: "subscribing to dustLedgerEvents…".to_string(),
        });
        dust_subunits.set(None);
        let Some(syncer) = dust_syncer_for(network) else {
            dust_row.set(SyncRow::Failed {
                err: "syncer not initialised (unlock the wallet first)"
                    .to_string(),
            });
            return;
        };
        spawn(async move {
            use futures::StreamExt;
            let mut stream = std::pin::pin!(syncer.clone().sync());
            while let Some(p) = stream.next().await {
                match p {
                    Ok(prog) => dust_row.set(SyncRow::Running {
                        progress: Some((prog.current_id, prog.max_id)),
                        note: format!(
                            "indexing event {} / {} ({} processed)",
                            prog.current_id, prog.max_id, prog.events_processed,
                        ),
                    }),
                    Err(e) => {
                        dust_row.set(SyncRow::Failed { err: e.to_string() });
                        return;
                    }
                }
            }
            // Pull the freshly-persisted state to report the
            // current balance.
            let last_id = syncer
                .cached_state()
                .ok()
                .flatten()
                .map(|(_, id)| id);
            match syncer.current_balance_atomic() {
                Ok(Some(bal)) => {
                    tracing::info!(
                        target: "balance",
                        atomic = %bal,
                        "DUST balance synced",
                    );
                    dust_subunits.set(Some(bal));
                    dust_event_id.set(last_id);
                    let (compact, _) = format_balance(bal, DUST_DECIMALS);
                    dust_row.set(SyncRow::Done {
                        summary: format!("{} DUST", compact),
                    });
                }
                Ok(None) => {
                    dust_row.set(SyncRow::Failed {
                        err: "sync completed without persisting state".into(),
                    });
                }
                Err(e) => {
                    dust_row.set(SyncRow::Failed { err: e.to_string() });
                }
            }
        });
    };

    // Sync kicks ONLY when the parent bumps `sync_trigger` — that
    // happens at the end of the Wallet-tab CTA's `connect()` after
    // the endpoint probe succeeds, or when the operator clicks
    // the in-pane `Resync` button below (which bumps the same
    // signal). Earlier revisions had a self-owned `Connect`/`Resync`
    // button here, but that competed with the top CTA — the user
    // had to click both to get NIGHT + DUST online. One trigger,
    // one user action.
    //
    // The `DustSyncer` for the active network is still registered
    // eagerly on unlock + on every network switch (see `on_unlock`
    // and `rehydrate_for_network`) — only the *sync stream* itself
    // is gated behind the trigger. Cheap, idempotent registration
    // vs expensive subscription.
    use_effect({
        let kick = kick;
        let mut started = started;
        move || {
            let v = *sync_trigger.read();
            if v == 0 {
                // Initial mount — pane sits "queued" until first
                // bump. Skip the kick so we don't auto-sync.
                return;
            }
            started.set(true);
            kick();
        }
    });

    let any_running = matches!(*night_row.read(), SyncRow::Running { .. })
        || matches!(*dust_row.read(), SyncRow::Running { .. });

    let mut sync_trigger_mut = sync_trigger;
    let resync = move |_| {
        let next = *sync_trigger_mut.read() + 1;
        sync_trigger_mut.set(next);
    };

    let dust_event_id_val = *dust_event_id.read();
    rsx! {
        div { class: "card",
            div { class: "card-header", "Wallet sync" }
            {render_sync_row("NIGHT", &night_row.read())}
            {render_sync_row("DUST", &dust_row.read())}
            div { class: "row sync-foot",
                button {
                    class: "secondary",
                    disabled: any_running,
                    onclick: resync,
                    if any_running { "Syncing…" } else { "Resync" }
                }
                if let Some(id) = dust_event_id_val {
                    span { class: "sync-meta",
                        "event id: {id}"
                    }
                }
            }
        }
    }
}

/// One sync row: a status pill + an optional progress bar.
/// Pulled out so both NIGHT and DUST render identically.
fn render_sync_row(label: &'static str, row: &SyncRow) -> Element {
    let (icon, color, status_text) = match row {
        SyncRow::Idle => ("○", "var(--text-faint)", "queued".to_string()),
        SyncRow::Running { note, .. } => {
            ("◌", "var(--accent, #6ea8f8)", note.clone())
        }
        SyncRow::Done { summary } => {
            ("✓", "var(--success)", summary.clone())
        }
        SyncRow::Failed { err } => ("✗", "var(--error)", err.clone()),
    };
    let progress_pct: Option<f64> = match row {
        SyncRow::Running {
            progress: Some((cur, max)),
            ..
        } if *max > 0 => Some(
            ((*cur as f64 / *max as f64) * 100.0).clamp(0.0, 100.0),
        ),
        _ => None,
    };
    rsx! {
        div { class: "balance-row",
            span { class: "label",
                span { style: "display: inline-block; width: 1.2em; color: {color};",
                    "{icon}"
                }
                "{label}"
            }
            span { class: "value",
                style: "font-size: 12px; color: {color}; text-align: right;",
                "{status_text}"
            }
        }
        if let Some(pct) = progress_pct {
            div { class: "balance-row",
                div {
                    style: "width: 100%; height: 4px; background: var(--surface-2);\
                            border-radius: 2px; overflow: hidden;\
                            border: 1px solid var(--border-faint);",
                    div {
                        style: format!(
                            "width: {pct:.1}%; height: 100%;\
                             background: var(--accent, #6ea8f8);\
                             transition: width 200ms ease-out;"
                        ),
                    }
                }
            }
        }
    }
}

/// (Replaced by `WalletSyncPane`; kept until callers migrate —
/// referenced from older code paths still in flight.)
#[component]
#[allow(dead_code)]
fn DustSyncPanel(network: Network) -> Element {
    let mut progress = use_signal::<Option<wallet_core::SyncProgress>>(|| None);
    let mut running = use_signal(|| false);
    let mut error = use_signal::<Option<String>>(|| None);
    let mut last_id = use_signal::<Option<i64>>(|| None);

    // Hydrate the initial "last synced" hint from the persisted
    // snapshot if any. Reads from redb — cheap. Re-runs whenever
    // `network` changes (different cache row).
    use_effect(move || {
        let cur = network;
        if let Some(syncer) = dust_syncer_for(cur) {
            if let Ok(Some((_, id))) = syncer.cached_state() {
                last_id.set(Some(id));
            } else {
                last_id.set(None);
            }
        } else {
            last_id.set(None);
        }
    });

    let on_click = move |_| {
        if *running.read() {
            return;
        }
        let Some(syncer) = dust_syncer_for(network) else {
            error.set(Some(
                "DUST syncer not yet initialised — unlock the wallet first."
                    .into(),
            ));
            return;
        };
        running.set(true);
        error.set(None);
        progress.set(None);
        spawn(async move {
            use futures::StreamExt;
            let mut stream = std::pin::pin!(syncer.clone().sync());
            while let Some(p) = stream.next().await {
                match p {
                    Ok(prog) => progress.set(Some(prog)),
                    Err(e) => {
                        error.set(Some(e.to_string()));
                        running.set(false);
                        return;
                    }
                }
            }
            // Pick up the final checkpoint.
            if let Ok(Some((_, id))) = syncer.cached_state() {
                last_id.set(Some(id));
            }
            running.set(false);
        });
    };

    // `SyncProgress` is `Copy`, so we can deref the read guard into
    // a plain `Option<SyncProgress>` here. That lets us pattern-match
    // and run `.map`/`.filter` without juggling lifetimes inside the
    // rsx! arms below.
    let progress_val: Option<wallet_core::SyncProgress> = *progress.read();
    let progress_text = progress_val.map(|p| {
        format!(
            "Indexing… event {} / {} ({} processed this run)",
            p.current_id, p.max_id, p.events_processed,
        )
    });
    let progress_pct = progress_val.filter(|p| p.max_id > 0).map(|p| {
        ((p.current_id as f64 / p.max_id as f64) * 100.0).clamp(0.0, 100.0)
    });

    rsx! {
        div { class: "row", "DUST sync" }
        if let Some(text) = progress_text {
            div { class: "row", style: "font-size: 12px; color: var(--text-muted);",
                "{text}"
            }
            if let Some(pct) = progress_pct {
                div {
                    style: "width: 100%; height: 6px; background: var(--surface-2);\
                            border-radius: 3px; overflow: hidden;\
                            border: 1px solid var(--border-faint);",
                    div {
                        style: format!(
                            "width: {pct:.1}%; height: 100%; background: var(--accent, #6ea8f8);\
                             transition: width 200ms ease-out;"
                        ),
                    }
                }
            }
        } else if let Some(id) = *last_id.read() {
            div { class: "row", style: "font-size: 11px; color: var(--text-muted);",
                "Last synced: event id {id}"
            }
        } else {
            div { class: "row", style: "font-size: 11px; color: var(--text-faint);",
                "Not yet synced — first sync replays the full chain history (~10–15 min on PreProd)."
            }
        }
        div { class: "row",
            button {
                disabled: *running.read(),
                onclick: on_click,
                {if *running.read() { "Syncing…" } else { "Sync DUST" }}
            }
        }
        if let Some(e) = error.read().as_ref() {
            div { class: "seed-blob", style: "color: var(--error);", "{e}" }
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
            let w = app_wallet_for(network);
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
                            // "Last tx" gets its own card-style
                            // panel with a Copy button so the user
                            // can grab the hash for the explorer
                            // without selecting + copying by hand.
                            // Same `<pre>` styling the Document /
                            // Raw State tabs use so the long hex
                            // wraps cleanly inside the row.
                            div { class: "did-meta-cell",
                                style: "flex-direction: column; align-items: stretch; gap: 4px; margin-top: 8px;",
                                div { class: "row",
                                    style: "justify-content: space-between; align-items: center;",
                                    span { class: "label", "Last tx" }
                                    {copy_btn(format!("0x{}", view.last_tx_hash), "Copy")}
                                }
                                pre {
                                    style: "font-family: ui-monospace, 'SF Mono', 'JetBrains Mono', Menlo, monospace;\
                                            font-size: 11px;\
                                            color: var(--mono-tint, var(--text));\
                                            background: var(--surface-2);\
                                            border: 1px solid var(--border-faint);\
                                            border-radius: 8px;\
                                            padding: 8px 10px;\
                                            margin: 0;\
                                            white-space: pre-wrap;\
                                            word-break: break-all;",
                                    "0x{view.last_tx_hash}"
                                }
                            }
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
                                // Same `<pre>` formatting as the
                                // Detail-view Document tab — the
                                // `<div class="seed-blob">` was
                                // collapsing all whitespace, so the
                                // pretty-printed JSON came out as
                                // one wrapped line.
                                pre {
                                    style: "font-family: ui-monospace, 'SF Mono', 'JetBrains Mono', Menlo, monospace;\
                                            font-size: 11px;\
                                            color: var(--mono-tint, var(--text));\
                                            background: var(--surface-2);\
                                            border: 1px solid var(--border-faint);\
                                            border-radius: 8px;\
                                            padding: 12px;\
                                            margin: 8px 0 0 0;\
                                            white-space: pre-wrap;\
                                            word-break: break-word;\
                                            overflow-x: auto;",
                                    "{view.document_json}"
                                }
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


/// Variants of the 11-circuit dropdown. Order matches the
/// dropdown's display order; numeric tag is the `<select>` value
/// we round-trip through `e.value().parse()`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum OpKind {
    AddAlsoKnownAs,
    RemoveAlsoKnownAs,
    /// JWK verification method (Ed25519, X25519, P-256, secp256k1).
    /// If the operator picks `curve == "Jubjub"` in the form, the
    /// submit handler automatically routes to
    /// `AddSchnorrJubjubVerificationMethod` instead — the
    /// redesigned contract rejects Jubjub from the JWK map.
    AddVerificationMethod,
    UpdateVerificationMethod,
    RemoveVerificationMethod,
    /// Jubjub-only path. Distinct from `RemoveVerificationMethod`
    /// because the new contract keeps Jubjub VMs in a separate
    /// ledger map and the remove circuit is per-map; the id alone
    /// doesn't disambiguate which map the VM lives in.
    RemoveSchnorrJubjubVerificationMethod,
    AddVerificationMethodRelation,
    RemoveVerificationMethodRelation,
    AddService,
    UpdateService,
    RemoveService,
    /// `deactivate` circuit. Kept on the enum so exhaustive
    /// `match` arms in `DidOperationBuilder` still typecheck — the
    /// Operation Builder excludes it from `BUILDABLE_OPS`; the
    /// dedicated "Deactivate" button on the DID detail view is the
    /// only path that constructs the corresponding `DidOperation`.
    #[allow(dead_code)]
    Deactivate,
}

impl OpKind {
    /// Display string for the Operation-Builder dropdown. Updated
    /// 2026-05-28 to reflect the redesigned contract's circuit
    /// names (every `add*` / `update*` / `remove*` pair became a
    /// single `set*(payload, mutation)` circuit, plus a separate
    /// Schnorr-Jubjub path). The dropdown still surfaces the
    /// old action verbs (`add`/`update`/`remove`) since they're
    /// the right mental model for the operator — the
    /// add↔Insert / update↔Update / remove↔Remove mutation tag
    /// is plumbed in `DidOperation::args_json`.
    fn circuit_name(&self) -> &'static str {
        match self {
            Self::AddAlsoKnownAs => "addAlsoKnownAs",
            Self::RemoveAlsoKnownAs => "removeAlsoKnownAs",
            Self::AddVerificationMethod => "addVerificationMethod",
            Self::UpdateVerificationMethod => "updateVerificationMethod",
            Self::RemoveVerificationMethod => "removeVerificationMethod",
            Self::RemoveSchnorrJubjubVerificationMethod => {
                "removeSchnorrJubjubVerificationMethod"
            }
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
/// Curves the wallet UI offers in the Operation Builder's
/// `addVerificationMethod` form. Narrow set — the three we have
/// key derivation for. The contract's full curve enum
/// (`Ed25519, X25519, Jubjub, P256, Secp256k1` after the
/// 2026-05-27 refactor) is mapped separately at the encode
/// boundary via `crv_to_contract_tag`.
const CURVE_TYPES: &[&str] = &["Ed25519", "Jubjub", "P256"];

/// Translate the UI's curve name into the contract's runtime enum
/// tag (verified at runtime: `Ed25519=0, X25519=1, Jubjub=2,
/// P256=3, Secp256k1=4` per
/// `packages/contract/dist/managed/did/contract/index.js`). The UI
/// only ships the three we support; future X25519 / Secp256k1
/// would land here when we add their key derivation.
fn crv_to_contract_tag(curve_name: &str) -> i32 {
    match curve_name {
        "Ed25519" => 0,
        "X25519" => 1,
        "Jubjub" => 2,
        "P256" => 3,
        "Secp256k1" => 4,
        _ => 0, // fall back to Ed25519; the contract will reject
                // mismatched (kty, crv) pairs anyway.
    }
}
const RELATIONS: &[&str] = &[
    "Authentication",
    "AssertionMethod",
    "KeyAgreement",
    "CapabilityInvocation",
    "CapabilityDelegation",
];


/// Buildable circuit kinds — every variant except `Deactivate`,
/// which has its own dedicated button in the detail header (it
/// closes the DID for further updates; not a thing to batch).
const BUILDABLE_OPS: &[OpKind] = &[
    OpKind::AddAlsoKnownAs,
    OpKind::RemoveAlsoKnownAs,
    OpKind::AddVerificationMethod,
    OpKind::UpdateVerificationMethod,
    OpKind::RemoveVerificationMethod,
    OpKind::RemoveSchnorrJubjubVerificationMethod,
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

/// 3-pane Operation Builder (palette / form / commands history).
/// Adopted from `midnight-did-uiux-bundle`, with the original
/// "Add to batch / Submit batch" flow collapsed into a single
/// `Update DID` action: each click validates the form, appends
/// the op to the commands queue, and immediately drives the
/// submission pipeline (auto-load VK → ContractCall). The queue
/// surfaces history; clicking `Update DID` while a previous run
/// is in flight is a no-op, so the user can compose multiple
/// updates by issuing them one at a time.
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
    /// Persistent-store handle. Used to enumerate stored
    /// secret-storage keys for the verification-method
    /// picker; safe to pass even if the store hasn't loaded
    /// any keys yet (the picker just shows an empty list).
    bridge_state: BridgeState,
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
    /// Fragment ids of every verification method currently in the
    /// resolved DID document. Drives the `method_id` dropdown for
    /// `addVerificationMethodRelation` /
    /// `removeVerificationMethodRelation` — the operator can pick
    /// an existing VM instead of typing the fragment by hand.
    /// Empty before the first resolve, in which case the form
    /// falls back to a free-text input.
    method_ids: Vec<String>,
    on_back: EventHandler<()>,
    on_event: EventHandler<SessionEvent>,
    on_resolved: EventHandler<wallet_core::ResolvedDid>,
    on_cost: EventHandler<CostRun>,
    /// Lifted from the parent `DidDetailView` so the queue
    /// survives "Back to detail" → "Update DID" round trips.
    /// Previously a component-local `use_signal` here, which got
    /// dropped on unmount — the user lost any pending or
    /// completed-but-unsubmitted rows on every navigation.
    queue: Signal<Vec<(DidOperation, QueueStatus)>>,
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

    // Auto-pick the first known VM as `method_id` whenever the
    // user lands on a relation op and the current value isn't in
    // the resolved list. Without this the dropdown would render
    // with the first <option> visually selected but `f_method_id`
    // would still be `""`, and the submit handler would reject
    // the op with "method_id is required". The effect runs on
    // every render — `use_effect` in Dioxus 0.6 has no
    // dependency tracking, but the body is a no-op when the
    // value is already valid so this is fine.
    {
        let method_ids = method_ids.clone();
        use_effect(move || {
            let kind = BUILDABLE_OPS[*op_idx.read()];
            let needs_picker = matches!(
                kind,
                OpKind::AddVerificationMethodRelation
                    | OpKind::RemoveVerificationMethodRelation
            );
            if !needs_picker || method_ids.is_empty() {
                return;
            }
            let cur = f_method_id.read().clone();
            if !method_ids.iter().any(|m| m == &cur) {
                f_method_id.set(method_ids[0].clone());
            }
        });
    }

    // Queue + execution state. `queue` is the lifted Signal from
    // DidDetailView; rebind as mutable here to match the existing
    // call sites (`queue.set(...)` etc.).
    let mut queue = queue;
    let mut running = use_signal(|| false);
    let mut batch_error = use_signal::<Option<String>>(|| None);

    // Stored keys eligible to fill the addVerificationMethod /
    // updateVerificationMethod form. Loaded once at builder
    // mount; the picker just renders from this snapshot.
    // Empty when no wallet row exists yet OR no keys are
    // stored — the picker hides itself in those cases.
    let stored_vm_keys: Vec<StoredKeyForVm> = list_stored_vm_keys(&bridge_state);

    // Validate the current form into a `DidOperation`. Returns
    // `None` on validation failure (after pushing the message to
    // `form_error`). Shared between the per-click "Update DID"
    // path — there's no separate "Add to batch" stage any more.
    let mut draft_from_form = move || -> Option<DidOperation> {
        let op = BUILDABLE_OPS[*op_idx.read()];
        let drafted = match op {
            OpKind::AddAlsoKnownAs => {
                let v = f_value.read().trim().to_string();
                if v.is_empty() {
                    form_error.set(Some("value is required".into()));
                    return None;
                }
                DidOperation::AddAlsoKnownAs { value: v }
            }
            OpKind::RemoveAlsoKnownAs => {
                let v = f_value.read().trim().to_string();
                if v.is_empty() {
                    form_error.set(Some("value is required".into()));
                    return None;
                }
                DidOperation::RemoveAlsoKnownAs { value: v }
            }
            OpKind::AddVerificationMethod | OpKind::UpdateVerificationMethod => {
                let id = f_id.read().trim().to_string();
                let pk_x = f_pk_x.read().trim().to_string();
                let pk_y = f_pk_y.read().trim().to_string();
                if id.is_empty() || pk_x.is_empty() || pk_y.is_empty() {
                    form_error.set(Some("id, pk.x, pk.y are required".into()));
                    return None;
                }
                let vm = VerificationMethodInput {
                    id,
                    key_type: KEY_TYPES[*f_key_type_idx.read()].to_string(),
                    curve: CURVE_TYPES[*f_curve_idx.read()].to_string(),
                    pk_x,
                    pk_y,
                };
                // Auto-route Jubjub through the SchnorrJubjub
                // circuit family. The redesigned contract's
                // `assertSupportedVerificationMethod` rejects
                // Jubjub from the JWK map; the operator picking
                // curve=Jubjub clearly means they want the
                // dedicated Schnorr-Jubjub VM map.
                let is_jubjub = vm.curve == "Jubjub";
                match (op, is_jubjub) {
                    (OpKind::AddVerificationMethod, false) => {
                        DidOperation::AddVerificationMethod(vm)
                    }
                    (OpKind::UpdateVerificationMethod, false) => {
                        DidOperation::UpdateVerificationMethod(vm)
                    }
                    (OpKind::AddVerificationMethod, true) => {
                        DidOperation::AddSchnorrJubjubVerificationMethod(vm)
                    }
                    (OpKind::UpdateVerificationMethod, true) => {
                        DidOperation::UpdateSchnorrJubjubVerificationMethod(vm)
                    }
                    _ => unreachable!(),
                }
            }
            OpKind::RemoveVerificationMethod => {
                let id = f_id.read().trim().to_string();
                if id.is_empty() {
                    form_error.set(Some("id is required".into()));
                    return None;
                }
                DidOperation::RemoveVerificationMethod { id }
            }
            OpKind::RemoveSchnorrJubjubVerificationMethod => {
                let id = f_id.read().trim().to_string();
                if id.is_empty() {
                    form_error.set(Some("id is required".into()));
                    return None;
                }
                DidOperation::RemoveSchnorrJubjubVerificationMethod { id }
            }
            OpKind::AddVerificationMethodRelation => {
                let method_id = f_method_id.read().trim().to_string();
                if method_id.is_empty() {
                    form_error.set(Some("method_id is required".into()));
                    return None;
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
                    return None;
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
                    return None;
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
                    return None;
                }
                DidOperation::RemoveService { id }
            }
            OpKind::Deactivate => unreachable!("Deactivate not buildable here"),
        };
        form_error.set(None);
        Some(drafted)
    };

    let did_for_submit = did.clone();
    let sk_for_submit = controller_secret;
    // Single "Update DID" handler — validates the form, appends
    // the drafted op to the Commands queue, and kicks off the
    // submission pipeline (auto-load VK → ContractCall) for any
    // Pending / Failed rows. The two-step "Add to batch → Submit
    // batch" flow was confusing; the queue is now a history of
    // submitted commands, not a staging buffer.
    let on_update_did = move |_| {
        if *running.read() {
            return;
        }
        let Some(drafted) = draft_from_form() else { return };
        {
            let mut q = queue.read().clone();
            q.push((drafted, QueueStatus::Pending));
            queue.set(q);
        }

        // Only Pending rows participate in this submit. Failed rows
        // stay Failed (they need an explicit retry from the user —
        // re-running a duplicate `addAlsoKnownAs` for example would
        // hit the same `value already exists` chain rejection); Done
        // rows stay Done (their tx already landed). The previous
        // behaviour re-ran Failed alongside Pending which caused
        // double-submits after every chain-side rejection.
        let snapshot: Vec<DidOperation> = queue
            .read()
            .iter()
            .filter_map(|(op, st)| match st {
                QueueStatus::Pending => Some(op.clone()),
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
        // Preserve `Done` + `Failed` rows from earlier submits; only
        // reset rows that are about to participate in this run so we
        // don't leave stale `Running`/`Skipped` markers around.
        let reset: Vec<(DidOperation, QueueStatus)> = queue
            .read()
            .iter()
            .map(|(op, st)| {
                let new_status = match st {
                    QueueStatus::Pending
                    | QueueStatus::Running { .. }
                    | QueueStatus::Skipped => QueueStatus::Pending,
                    QueueStatus::Done { .. } | QueueStatus::Failed { .. } => st.clone(),
                };
                (op.clone(), new_status)
            })
            .collect();
        queue.set(reset);
        batch_error.set(None);
        running.set(true);

        let did_for_log = did_for_submit.clone();
        let on_event = on_event.clone();
        let on_resolved = on_resolved.clone();
        let on_cost = on_cost.clone();
        let mut loaded_set: std::collections::HashSet<String> =
            loaded_circuits.iter().cloned().collect();
        let mut counter_cursor: u32 = initial_counter;
        spawn(async move {
            use futures::StreamExt;
            use wallet_core::WizardStage;
            let wallet = app_wallet_for(network);
            let total = queue.read().len();
            // Bracket the whole batch (loads + calls) with
            // one cost snapshot pair. Per-op snapshots would
            // mean 2N+1 indexer round-trips which dominates
            // the wall-clock; the user-visible unit is the
            // batch anyway.
            let cost_start = std::time::Instant::now();
            let before = wallet.balance_snapshot().await.ok();
            let mut all_succeeded = true;
            for i in 0..total {
                // Skip rows already terminal from earlier submits.
                // Done = already on-chain; Failed = waiting on
                // explicit user retry (see filter in `on_update_did`).
                let row_status = queue.read()[i].1.clone();
                if matches!(
                    row_status,
                    QueueStatus::Done { .. } | QueueStatus::Failed { .. }
                ) {
                    continue;
                }
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
                    // Push every intermediate stage into the row's
                    // `phase` so the operator sees real progress.
                    // Without this update the row sits frozen on
                    // "Calling <circuit>" until the pipeline ends
                    // — that hid a 4-minute Update DID round trip
                    // on real S24 Ultra hardware as "stuck".
                    let label = match &stage {
                        WizardStage::SyncingDust => Some("Syncing DUST"),
                        WizardStage::Composing => Some("Composing tx"),
                        WizardStage::Balancing => Some("Balancing"),
                        WizardStage::Proving => Some("Proving"),
                        WizardStage::Submitting => Some("Submitting"),
                        WizardStage::Confirming => Some("Confirming on chain"),
                        _ => None,
                    };
                    if let Some(phase) = label {
                        let mut q = queue.read().clone();
                        q[i].1 = QueueStatus::Running {
                            phase: format!("{phase} ({circuit})"),
                        };
                        queue.set(q);
                    }
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
                        all_succeeded = false;
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
                        all_succeeded = false;
                        break;
                    }
                }
            }
            // Cost telemetry for the whole batch — dust burns
            // regardless of partial failures, so report it
            // even if the run aborted mid-way.
            if let (Some(before), Ok(after)) = (before, wallet.balance_snapshot().await) {
                on_cost.call(CostRun {
                    label: format!("batch:{total}"),
                    dust_consumed: before.dust_atomic.saturating_sub(after.dust_atomic),
                    night_consumed: before.night_atomic.saturating_sub(after.night_atomic),
                    duration_ms: cost_start.elapsed().as_millis() as u64,
                    succeeded: all_succeeded,
                });
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
                    OpKind::AddVerificationMethod | OpKind::UpdateVerificationMethod => {
                        // Snapshot per render so the click closure
                        // doesn't borrow into the outer fn.
                        let keys_for_render = stored_vm_keys.clone();
                        let keys_for_pick = stored_vm_keys.clone();
                        rsx! {
                            if !keys_for_render.is_empty() {
                                div { class: "row",
                                    label { style: "min-width: 80px;", "Use stored key" }
                                    select {
                                        onchange: move |e| {
                                            let Ok(idx) = e.value().parse::<usize>() else { return };
                                            if idx == 0 {
                                                // "— manual entry —"; leave the
                                                // form alone so any in-progress
                                                // typing is preserved.
                                                return;
                                            }
                                            if let Some(key) = keys_for_pick.get(idx - 1) {
                                                f_id.set(key.label.clone());
                                                f_key_type_idx.set(key.kty_idx);
                                                f_curve_idx.set(key.crv_idx);
                                                f_pk_x.set(key.pk_x_hex.clone());
                                                f_pk_y.set(key.pk_y_hex.clone());
                                            }
                                        },
                                        style: "flex: 1; padding: 6px 8px; background: var(--surface-2); color: var(--text); border: 1px solid var(--border); border-radius: 6px;",
                                        option {
                                            value: "0",
                                            selected: true,
                                            "— manual entry —"
                                        }
                                        for (i , k) in keys_for_render.iter().enumerate() {
                                            option {
                                                value: "{i + 1}",
                                                "{k.label} · {k.crv_label} ({short_keyref(&k.key_ref)})"
                                            }
                                        }
                                    }
                                }
                            }
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
                        }
                    },
                    OpKind::RemoveVerificationMethod
                    | OpKind::RemoveSchnorrJubjubVerificationMethod
                    | OpKind::RemoveService => rsx! {
                        FormRow {
                            label: "id",
                            value: f_id.read().clone(),
                            on_change: move |s: String| f_id.set(s),
                            placeholder: "fragment id to remove",
                        }
                    },
                    OpKind::AddVerificationMethodRelation
                    | OpKind::RemoveVerificationMethodRelation => {
                        let mids = method_ids.clone();
                        let cur_mid = f_method_id.read().clone();
                        rsx! {
                            FormSelect {
                                label: "relation",
                                options: RELATIONS,
                                selected_idx: cur_rel,
                                on_select: move |i: usize| f_relation_idx.set(i),
                            }
                            // When the DID document has at least one
                            // verification method, render a dropdown
                            // populated from the resolved cache so
                            // the operator can't fat-finger a
                            // non-existent fragment id. With zero VMs
                            // (e.g. a freshly bootstrapped DID
                            // before any addVerificationMethod
                            // landed) we fall back to a free-text
                            // input — the chain will still reject
                            // unknown ids but the user gets a path
                            // out of the impasse.
                            if mids.is_empty() {
                                FormRow {
                                    label: "method_id",
                                    value: cur_mid,
                                    on_change: move |s: String| f_method_id.set(s),
                                    placeholder: "existing verification-method fragment id",
                                }
                            } else {
                                {
                                    let mids_for_change = mids.clone();
                                    rsx! {
                                        div { class: "row",
                                            label { style: "min-width: 80px;", "method_id" }
                                            select {
                                                onchange: move |e| {
                                                    if let Ok(i) = e.value().parse::<usize>() {
                                                        if let Some(v) = mids_for_change.get(i) {
                                                            f_method_id.set(v.clone());
                                                        }
                                                    }
                                                },
                                                style: "flex: 1; padding: 6px 8px; background: var(--surface-2); color: var(--text); border: 1px solid var(--border); border-radius: 6px;",
                                                for (i , mid) in mids.iter().enumerate() {
                                                    option {
                                                        value: "{i}",
                                                        selected: mid == &cur_mid,
                                                        "{mid}"
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
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
                        class: "btn-primary",
                        disabled: is_running,
                        onclick: on_update_did,
                        {if is_running { "Submitting…" } else { "Update DID" }}
                    }
                }
                if let Some(msg) = form_error.read().as_ref() {
                    div { class: "wizard-outcome err",
                        div { class: "row label", "Validation" }
                        div { class: "seed-blob", "{msg}" }
                    }
                }
            }

            // ── Pane 3 : commands history ─────────────────────
            div { class: "op-pane preview",
                h3 { "Commands ({queue_len})" }
                if queue_len == 0 {
                    div { class: "detail-empty",
                        "No commands yet. Configure an op on the left, then \"Update DID\"."
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

/// Pre-boot unlock card. Shown until `WalletStore::open()`
/// succeeds; once unlocked, the App renders the full UI.
/// The passphrase input pre-fills with the
/// [`DEV_STORE_PASSPHRASE`] default ("midnight") so the
/// common case is one click; the user can retype before
/// hitting Unlock if they need to open a store sealed with
/// a different value.
#[component]
fn UnlockCard(
    state: UnlockState,
    passphrase: String,
    on_input: EventHandler<String>,
    on_unlock: EventHandler<()>,
) -> Element {
    let busy = matches!(state, UnlockState::Opening);
    let error_msg = match &state {
        UnlockState::Failed(m) => Some(m.clone()),
        _ => None,
    };
    rsx! {
        div { class: "card",
            div { class: "card-header", "Unlock wallet store" }
            div { style: "color: var(--text-muted); font-size: 11px; margin-bottom: 10px;",
                "The wallet keeps seeds, DID inventory, resolved snapshots, and "
                "session state in an AES-encrypted file. Type the passphrase to "
                "unlock it."
            }
            div { class: "row",
                label { style: "min-width: 80px;", "Passphrase" }
                input {
                    r#type: "password",
                    value: "{passphrase}",
                    oninput: move |e| on_input.call(e.value()),
                    onkeydown: move |e| {
                        if e.key() == dioxus::events::Key::Enter && !busy {
                            on_unlock.call(());
                        }
                    },
                    style: "flex: 1; padding: 6px 8px; background: var(--surface-2); color: var(--text); border: 1px solid var(--border); border-radius: 6px; font-family: ui-monospace, monospace; font-size: 11px;"
                }
            }
            div { class: "row",
                button {
                    class: "cta",
                    disabled: busy,
                    onclick: move |_| on_unlock.call(()),
                    {if busy { "Unlocking…" } else { "Unlock" }}
                }
            }
            if let Some(msg) = error_msg {
                div { class: "wizard-outcome err",
                    div { class: "row label", "Unlock failed" }
                    div { class: "seed-blob", "{msg}" }
                    div {
                        style: "color: var(--text-muted); font-size: 10px; margin-top: 6px;",
                        "If you previously unlocked the store with a different "
                        "passphrase, type it above. If this is the first launch, "
                        "the default is "
                        code { "midnight" }
                        "."
                    }
                }
            }
        }
    }
}

/// Keys tab — list every key the `RedbSecretStore` knows
/// about for the active wallet and let the user mint new
/// ones. Each row shows the curve, label, DID binding,
/// derivation flavour, key_ref, and a copy button for the
/// JWK. Generation uses the wallet's HD seed (each new key
/// claims the next free `Hkdf { account: 0, index, candidate }`).
#[component]
fn KeysTab(bridge_state: BridgeState) -> Element {
    use wallet_core::secret_storage::{
        GenerateKeyInput, MidnightCurve, MidnightKeyType, SecretStorage, StoredKeyMeta,
    };
    use wallet_core::secret_storage::redb_secret_store::RedbSecretStore;

    let mut keys = use_signal::<Vec<StoredKeyMeta>>(Vec::new);
    let mut label_input = use_signal(|| String::from("did-key"));
    let mut error = use_signal::<Option<String>>(|| None);
    let mut last_generated = use_signal::<Option<String>>(|| None);

    let store_handle = bridge_state.store().cloned();
    // Always read the App's pinned active wallet. Falls back
    // to the first wallet in the store if for some reason the
    // pin hasn't been set yet (race between unlock + first
    // tab click on Keys).
    let wallet_id = bridge_state.active_wallet_id().or_else(|| {
        store_handle
            .as_ref()
            .and_then(|s| s.list_wallet_ids().ok())
            .and_then(|ids| ids.into_iter().next())
    });

    let Some(store) = store_handle else {
        return rsx! {
            div { class: "card",
                div { class: "card-header", "Keys" }
                div { class: "detail-empty",
                    "Wallet store still opening. Try again in a moment."
                }
            }
        };
    };
    let Some(wallet_id) = wallet_id else {
        return rsx! {
            div { class: "card",
                div { class: "card-header", "Keys" }
                div { class: "detail-empty",
                    "No wallet row yet — connect first to mint one."
                }
            }
        };
    };

    // `refresh_keys` is called from multiple closures so we
    // wrap it in `Rc<RefCell<dyn FnMut>>` — Dioxus event
    // handlers want `FnMut` and our refresh closure captures
    // signal handles by move. The `Rc<RefCell<...>>` lets us
    // share that FnMut across the generate / refresh /
    // useEffect call sites without re-cloning the body.
    let refresh_keys = {
        let store = store.clone();
        std::rc::Rc::new(std::cell::RefCell::new(move || {
            let s = RedbSecretStore::new(store.clone(), wallet_id);
            let listed: Result<Vec<StoredKeyMeta>, _> = futures::executor::block_on(s.list_keys(None));
            match listed {
                Ok(ks) => keys.set(ks),
                Err(e) => error.set(Some(format!("list keys: {e}"))),
            }
        }))
    };

    use_effect({
        let refresh_keys = refresh_keys.clone();
        move || {
            (refresh_keys.borrow_mut())();
        }
    });

    let generate = {
        let store = store.clone();
        let refresh_keys = refresh_keys.clone();
        move |(kty, crv): (MidnightKeyType, MidnightCurve)| {
            let mut s = RedbSecretStore::new(store.clone(), wallet_id);
            let label = label_input.read().trim().to_string();
            if label.is_empty() {
                error.set(Some("label is required".into()));
                return;
            }
            let label_for_log = label.clone();
            match futures::executor::block_on(s.generate_key(GenerateKeyInput {
                id: label,
                kty,
                crv,
                did: None,
                purpose: None,
            })) {
                Ok((kref, _pk)) => {
                    error.set(None);
                    last_generated.set(Some(format!("{label_for_log} → {kref}")));
                    (refresh_keys.borrow_mut())();
                }
                Err(e) => error.set(Some(format!("generate: {e}"))),
            }
        }
    };

    rsx! {
        div { class: "card",
            div { class: "card-header", "Generate key" }
            div { class: "row",
                label { style: "min-width: 80px;", "Label" }
                input {
                    r#type: "text",
                    value: "{label_input.read()}",
                    oninput: move |e| label_input.set(e.value()),
                    style: "flex: 1; padding: 6px 8px; background: var(--surface-2); color: var(--text); border: 1px solid var(--border); border-radius: 6px; font-family: ui-monospace, monospace; font-size: 11px;"
                }
            }
            // Three curve choices grouped as a segmented capsule
            // (M3 single-select-shaped action group). The verb
            // "Generate" was redundant — the section header above
            // already says "Generate key". Refresh becomes a
            // dedicated icon button to free horizontal space so
            // the row never overflows on phone widths.
            div { class: "row", style: "align-items: center; gap: 8px;",
                div { class: "segmented", style: "flex: 1;",
                    button {
                        onclick: {
                            let g = generate.clone();
                            move |_| (g.clone())((MidnightKeyType::OKP, MidnightCurve::Ed25519))
                        },
                        "Ed25519"
                    }
                    button {
                        onclick: {
                            let g = generate.clone();
                            move |_| (g.clone())((MidnightKeyType::EC, MidnightCurve::P256))
                        },
                        "P-256"
                    }
                    button {
                        onclick: {
                            let g = generate.clone();
                            move |_| (g.clone())((MidnightKeyType::EC, MidnightCurve::Jubjub))
                        },
                        "Jubjub"
                    }
                }
                button {
                    class: "icon-btn",
                    title: "Refresh key list",
                    "aria-label": "Refresh key list",
                    onclick: {
                        let refresh_keys = refresh_keys.clone();
                        move |_| (refresh_keys.borrow_mut())()
                    },
                    "↻"
                }
            }
            if let Some(msg) = error.read().as_ref() {
                div { class: "wizard-outcome err",
                    div { class: "row label", "Failed" }
                    div { class: "seed-blob", "{msg}" }
                }
            } else if let Some(msg) = last_generated.read().as_ref() {
                div { class: "wizard-outcome ok",
                    div { class: "row label", "Minted" }
                    div { class: "seed-blob", "{msg}" }
                }
            }
        }

        div { class: "card",
            div { class: "card-header", "Stored keys ({keys.read().len()})" }
            if keys.read().is_empty() {
                div { class: "detail-empty",
                    "No keys yet. Generate one above — each new key claims the next HKDF index off the wallet seed."
                }
            } else {
                table { class: "detail-table",
                    thead {
                        tr {
                            th { "Label" }
                            th { "Curve" }
                            th { "Type" }
                            th { "Key ref" }
                            th { "" }
                        }
                    }
                    tbody {
                        for meta in keys.read().iter() {
                            {
                                let crv = format!("{:?}", meta.algorithm.crv);
                                let kty = format!("{:?}", meta.algorithm.kty);
                                // `SecretKeyRef` carries (uuid, kid);
                                // the UI shows the UUID handle —
                                // matches what the upstream JS lib
                                // exposes as `keyRef`.
                                let kref = meta.key_ref.uuid().to_string();
                                rsx! {
                                    tr {
                                        td { "{meta.id}" }
                                        td { class: "muted", "{crv}" }
                                        td { class: "muted", "{kty}" }
                                        td { class: "muted",
                                            title: "{kref}",
                                            "{short_keyref(&kref)}"
                                        }
                                        td { {copy_btn(kref.clone(), "Copy key_ref")} }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Jubjub-only view of a stored key, paired with the raw
/// 32-byte secret bytes the wallet uses to sign. The Sign
/// tab consumes this directly; the `seed` field is what
/// `jubjub_schnorr::sign_payload_diagnostic` ingests.
///
/// Stored in memory only for the duration of the SignTab
/// render. The vec is rebuilt on every render so a key
/// deleted in another tab disappears from the picker on the
/// next paint.
#[derive(Clone, Debug, PartialEq, Eq)]
struct StoredJubjubKeyEntry {
    key_ref: String,
    label: String,
    seed: [u8; 32],
}

/// Filter the secret store to Jubjub-only keys, pulling each
/// row's raw 32-byte secret out of the wallet via
/// `WalletStore::key_private_bytes`. Returns `Vec::new()` if
/// no wallet exists, the store is unreachable, or no Jubjub
/// keys are stored — the picker hides itself in those cases.
fn list_stored_jubjub_keys_for_sign(bridge_state: &BridgeState) -> Vec<StoredJubjubKeyEntry> {
    use wallet_core::secret_storage::MidnightCurve;

    let Some(store) = bridge_state.store() else {
        return Vec::new();
    };
    let Some(wallet_id) = bridge_state.active_wallet_id().or_else(|| {
        store
            .list_wallet_ids()
            .ok()
            .and_then(|ids| ids.into_iter().next())
    })
    else {
        return Vec::new();
    };
    let rows = match store.list_keys(wallet_id, None) {
        Ok(rs) => rs,
        Err(e) => {
            tracing::warn!(error=%e, "list stored keys for Sign picker");
            return Vec::new();
        }
    };
    rows.into_iter()
        .filter(|(_, row)| row.crv == MidnightCurve::Jubjub)
        .filter_map(|(key_ref, row)| {
            // `key_private_bytes` re-derives from the wallet
            // seed for `Hkdf` rows (cheap) or unwraps the
            // envelope for `Direct` rows (one scrypt). Both
            // paths produce the same 32-byte buffer that
            // `curve_support::sign` would pass to
            // `jubjub_schnorr::sign_payload_from_seed`.
            let bytes = store.key_private_bytes(wallet_id, &key_ref).ok()?;
            if bytes.len() != 32 {
                tracing::warn!(
                    key_ref=%key_ref,
                    "Jubjub stored secret length not 32; skipping",
                );
                return None;
            }
            let mut seed = [0u8; 32];
            seed.copy_from_slice(&bytes);
            Some(StoredJubjubKeyEntry {
                key_ref,
                label: row.label,
                seed,
            })
        })
        .collect()
}

/// One stored key in the shape the Operation Builder's
/// verification-method picker consumes. Pre-computes the
/// form-signal values so the click handler is a few
/// assignments and the row owns no references.
#[derive(Clone, Debug, PartialEq, Eq)]
struct StoredKeyForVm {
    key_ref: String,
    label: String,
    /// Index into `KEY_TYPES` (`"EC" | "RSA" | "oct" | "OKP"`).
    kty_idx: usize,
    /// Index into `CURVE_TYPES` (`"Ed25519" | "Jubjub" | "P256"`).
    crv_idx: usize,
    /// Human-readable label for the dropdown — e.g. "Jubjub".
    crv_label: &'static str,
    /// `pk.x` as 0x-prefixed BE hex. The Operation Builder's
    /// form treats this as the `{ "$bigint": <str> }` payload
    /// — `BigInt("0x…")` is valid JS.
    pk_x_hex: String,
    pk_y_hex: String,
}

/// Snapshot every key in the secret store for the active
/// wallet, normalised to the form the verification-method
/// picker needs. Returns `Vec::new()` if no wallet exists or
/// the secret store is unreachable — the picker hides itself
/// in those cases.
fn list_stored_vm_keys(bridge_state: &BridgeState) -> Vec<StoredKeyForVm> {
    use wallet_core::secret_storage::public_for_ledger;

    let Some(store) = bridge_state.store() else {
        return Vec::new();
    };
    let Some(wallet_id) = bridge_state.active_wallet_id().or_else(|| {
        store
            .list_wallet_ids()
            .ok()
            .and_then(|ids| ids.into_iter().next())
    })
    else {
        return Vec::new();
    };
    let rows = match store.list_keys(wallet_id, None) {
        Ok(rs) => rs,
        Err(e) => {
            tracing::warn!(error=%e, "list stored keys for VM picker");
            return Vec::new();
        }
    };
    rows.into_iter()
        .filter_map(|(key_ref, row)| {
            let ledger = public_for_ledger(&row.public_jwk).ok()?;
            let kty_idx = kty_to_idx(ledger.kty);
            let (crv_idx, crv_label) = crv_to_idx_label(ledger.crv);
            Some(StoredKeyForVm {
                key_ref,
                label: row.label.clone(),
                kty_idx,
                crv_idx,
                crv_label,
                pk_x_hex: format!("0x{}", hex::encode(ledger.x_be)),
                pk_y_hex: format!("0x{}", hex::encode(ledger.y_be)),
            })
        })
        .collect()
}

/// Map a `MidnightKeyType` to its position in the UI's
/// `KEY_TYPES` table. Must agree with the table declaration
/// in this file (`KEY_TYPES = ["EC", "RSA", "oct", "OKP"]`).
fn kty_to_idx(kty: wallet_core::secret_storage::MidnightKeyType) -> usize {
    use wallet_core::secret_storage::MidnightKeyType::*;
    match kty {
        EC => 0,
        OKP => 3,
    }
}

/// Map a `MidnightCurve` to its position in the UI's
/// `CURVE_TYPES` table plus a static label for the dropdown.
fn crv_to_idx_label(crv: wallet_core::secret_storage::MidnightCurve) -> (usize, &'static str) {
    use wallet_core::secret_storage::MidnightCurve::*;
    match crv {
        Ed25519 => (0, "Ed25519"),
        Jubjub => (1, "Jubjub"),
        P256 => (2, "P-256"),
    }
}

/// Logs tab — renders the in-memory ring buffer maintained
/// by `WalletLogLayer`. Updates whenever a new tracing event
/// fires (and we re-render). Older entries beyond the ring
/// capacity live in the redb `logs` table; the "Load older
/// from disk" button pulls another batch out of there into
/// the on-screen view.
///
/// Filters: a per-level toggle row (Error / Warn / Info /
/// Debug / Trace) lets the user narrow noise. Defaults to
/// Info+ so the tab doesn't drown in Debug noise on first
/// open.
#[component]
fn LogsTab(bridge_state: BridgeState) -> Element {
    use crate::logs::CapturedLog;

    // Per-level toggles. Default: Error/Warn/Info on, Debug
    // and Trace off — most users want signal not firehose.
    let mut show_error = use_signal(|| true);
    let mut show_warn = use_signal(|| true);
    let mut show_info = use_signal(|| true);
    let mut show_debug = use_signal(|| false);
    let mut show_trace = use_signal(|| false);
    // Free-text search (case-insensitive) across the
    // message + target fields.
    let mut search = use_signal(String::new);
    // Extra rows pulled from the on-disk archive via
    // "Load older from disk". Concatenated onto the ring
    // snapshot at render time so the user sees both sources
    // chronologically in one stream.
    let mut older: Signal<Vec<CapturedLog>> = use_signal(Vec::new);

    let capture = bridge_state.log_capture().cloned();

    // Live snapshot from the ring (newest-first).
    let live: Vec<CapturedLog> = capture
        .as_ref()
        .map(|c| c.snapshot())
        .unwrap_or_default();
    let older_snap = older.read().clone();

    let load_older = {
        let bs = bridge_state.clone();
        move |_| {
            let Some(store) = bs.store() else { return };
            // Pull the most-recent 200 rows from disk that
            // aren't already in the live ring.
            let live_min_ts = capture
                .as_ref()
                .map(|c| c.snapshot())
                .unwrap_or_default()
                .last()
                .map(|e| e.timestamp_ns);
            match store.list_logs_recent(500) {
                Ok(rows) => {
                    let mapped: Vec<CapturedLog> = rows
                        .into_iter()
                        .filter(|r| match live_min_ts {
                            Some(min) => r.timestamp_ns < min,
                            None => true,
                        })
                        .map(|r| CapturedLog {
                            timestamp_ns: r.timestamp_ns,
                            timestamp_ms: r.timestamp_ms,
                            level: r.level,
                            target: r.target,
                            message: r.message,
                        })
                        .collect();
                    older.set(mapped);
                }
                Err(e) => {
                    tracing::warn!(error=%e, "load older logs from store failed");
                }
            }
        }
    };

    let clear_ring = {
        let bs = bridge_state.clone();
        move |_| {
            if let Some(c) = bs.log_capture() {
                c.clear_ring();
            }
            older.set(Vec::new());
        }
    };

    let clear_archive = {
        let bs = bridge_state.clone();
        move |_| {
            let Some(store) = bs.store() else { return };
            if let Err(e) = store.clear_logs() {
                tracing::warn!(error=%e, "clear archive failed");
            }
            older.set(Vec::new());
        }
    };

    // Compose the full visible stream: live (newest-first)
    // followed by older from disk (also newest-first), then
    // apply level + search filters.
    let mut combined: Vec<CapturedLog> =
        Vec::with_capacity(live.len() + older_snap.len());
    combined.extend(live.into_iter());
    combined.extend(older_snap.into_iter());

    let want_error = *show_error.read();
    let want_warn = *show_warn.read();
    let want_info = *show_info.read();
    let want_debug = *show_debug.read();
    let want_trace = *show_trace.read();
    let search_str = search.read().trim().to_lowercase();
    let filtered: Vec<CapturedLog> = combined
        .into_iter()
        .filter(|e| match e.level {
            wallet_core::store::LogLevel::Error => want_error,
            wallet_core::store::LogLevel::Warn => want_warn,
            wallet_core::store::LogLevel::Info => want_info,
            wallet_core::store::LogLevel::Debug => want_debug,
            wallet_core::store::LogLevel::Trace => want_trace,
        })
        .filter(|e| {
            if search_str.is_empty() {
                true
            } else {
                e.message.to_lowercase().contains(&search_str)
                    || e.target.to_lowercase().contains(&search_str)
            }
        })
        .collect();

    let total_visible = filtered.len();

    let persist_state = if bridge_state.store().is_some() {
        "Persisting to ~/.midnight/wallet-prototype/wallet.redb"
    } else {
        "Live only — store not yet attached; entries vanish on reload until unlock completes"
    };

    rsx! {
        div { class: "card",
            div { class: "card-header",
                "Logs ({total_visible} visible)"
            }
            div { style: "color: var(--text-muted); font-size: 11px; margin-bottom: 10px;",
                "{persist_state}"
            }
            div { class: "row",
                LogLevelToggle { label: "Error", value: want_error,
                    on_toggle: move |v: bool| show_error.set(v) }
                LogLevelToggle { label: "Warn",  value: want_warn,
                    on_toggle: move |v: bool| show_warn.set(v) }
                LogLevelToggle { label: "Info",  value: want_info,
                    on_toggle: move |v: bool| show_info.set(v) }
                LogLevelToggle { label: "Debug", value: want_debug,
                    on_toggle: move |v: bool| show_debug.set(v) }
                LogLevelToggle { label: "Trace", value: want_trace,
                    on_toggle: move |v: bool| show_trace.set(v) }
            }
            div { class: "search-field",
                input {
                    r#type: "search",
                    value: "{search.read()}",
                    placeholder: "Search messages or targets",
                    oninput: move |e| search.set(e.value()),
                }
            }
            div { class: "row",
                button {
                    onclick: load_older,
                    "Load older from disk"
                }
                button {
                    onclick: clear_ring,
                    "Clear live"
                }
                button {
                    class: "btn-danger",
                    onclick: clear_archive,
                    "Clear archive"
                }
            }
            if filtered.is_empty() {
                div { class: "detail-empty",
                    "No log entries match the current filter. Adjust the level toggles or search."
                }
            } else {
                ul { class: "log-list",
                    for (idx , entry) in filtered.iter().enumerate() {
                        {render_log_entry(idx, entry)}
                    }
                }
            }
        }
    }
}

/// Single log-level pill + checkbox-style toggle. Click
/// flips the value through the supplied event handler.
#[component]
fn LogLevelToggle(label: &'static str, value: bool, on_toggle: EventHandler<bool>) -> Element {
    let class = if value {
        format!("log-level-pill {} active", label.to_lowercase())
    } else {
        format!("log-level-pill {}", label.to_lowercase())
    };
    rsx! {
        button {
            class: "{class}",
            onclick: move |_| on_toggle.call(!value),
            "{label}"
        }
    }
}

fn render_log_entry(idx: usize, entry: &crate::logs::CapturedLog) -> Element {
    let level = log_level_class(entry.level);
    let stamp = format_log_timestamp(entry.timestamp_ms);
    let target = entry.target.clone();
    let message = entry.message.clone();
    rsx! {
        li {
            key: "log-{idx}",
            class: "log-row {level}",
            span { class: "stamp", "{stamp}" }
            span { class: "level", "{log_level_label(entry.level)}" }
            span { class: "target", "{target}" }
            span { class: "message", "{message}" }
        }
    }
}

fn log_level_class(level: wallet_core::store::LogLevel) -> &'static str {
    use wallet_core::store::LogLevel::*;
    match level {
        Error => "level-error",
        Warn => "level-warn",
        Info => "level-info",
        Debug => "level-debug",
        Trace => "level-trace",
    }
}

fn log_level_label(level: wallet_core::store::LogLevel) -> &'static str {
    use wallet_core::store::LogLevel::*;
    match level {
        Error => "ERROR",
        Warn => "WARN",
        Info => "INFO",
        Debug => "DEBUG",
        Trace => "TRACE",
    }
}

/// Render a unix-ms timestamp as `HH:MM:SS.mmm` (UTC). Cheap
/// to compute, plenty of precision for tail-following.
/// Thin status badge that lives between the StatusLine and
/// the tab strip. Reports whether the persistent wallet store
/// is attached + the current row counts at a glance —
/// confirmation that the persistence layer is actually
/// running.
#[component]
fn WalletStoreBadge(state: BridgeState) -> Element {
    let stats = state.store().and_then(|s| s.stats().ok());
    // Resolve the active wallet's label so the badge tells
    // the user which wallet is currently bound. Falls back to
    // `—` if nothing's active yet.
    let active_label = state
        .store()
        .zip(state.active_wallet_id())
        .and_then(|(s, id)| s.wallet_meta(id).ok().flatten())
        .map(|m| m.label)
        .unwrap_or_else(|| "—".to_string());
    let (label, n_wallets, n_dids, n_keys): (&'static str, u64, u64, u64) = match &stats {
        Some(s) => ("Store · ok", s.wallets, s.did_inventory, s.keys),
        None => ("Store · opening…", 0, 0, 0),
    };
    let class = if stats.is_some() {
        "store-badge ok"
    } else {
        "store-badge pending"
    };
    rsx! {
        div { class: "{class}",
            span { class: "label", "{label}" }
            span { class: "kv",
                "active: " strong { "{active_label}" }
            }
            span { class: "kv",
                strong { "{n_wallets}" }
                " wallet"
                {if n_wallets == 1 { "" } else { "s" }}
            }
            span { class: "kv",
                strong { "{n_dids}" }
                " DID"
                {if n_dids == 1 { "" } else { "s" }}
            }
            span { class: "kv",
                strong { "{n_keys}" }
                " key"
                {if n_keys == 1 { "" } else { "s" }}
            }
        }
    }
}

/// Settings tab — render the wallet store's diagnostics + a
/// quick visual breakdown of every persisted table's row
/// counts. Refreshes the snapshot on mount; the user can hit
/// the "Refresh" button after a wallet operation to see new
/// counts.
///
/// `bridge_state` is cloned in so the component owns its own
/// `BridgeState` handle for the duration; the underlying
/// `WalletStore` arc is cheap to share.
#[component]
fn SettingsTab(bridge_state: BridgeState) -> Element {
    let mut stats =
        use_signal::<Option<Result<wallet_core::store::StoreStats, String>>>(|| None);
    let path = wallet_store_path();
    let path_display = path.display().to_string();

    let mut load_stats = {
        let state = bridge_state.clone();
        move || {
            let snap = state.store().and_then(|s| s.stats().ok());
            stats.set(Some(match snap {
                Some(s) => Ok(s),
                None => Err("wallet store not yet open".to_string()),
            }));
        }
    };

    use_effect({
        let mut load_stats = load_stats.clone();
        move || {
            load_stats();
        }
    });

    let snap = stats.read().clone();
    rsx! {
        // Moved here from the global header — gives at-a-glance store
        // health to operators who actually want it (Settings is where
        // you go to check on persistence) without painting the
        // wallet/DIDs/Benchmark screens with a permanent strip.
        WalletStoreBadge { state: bridge_state.clone() }

        div { class: "card",
            div { class: "card-header", "Persistent wallet store" }
            div { class: "detail-kv",
                div { class: "k", "File path" }
                div { class: "v", "{path_display}" }
                div { class: "k", "Status" }
                div { class: "v",
                    {match &snap {
                        Some(Ok(_)) => "Open · healthy".to_string(),
                        Some(Err(e)) => format!("Unhealthy · {e}"),
                        None => "Loading…".to_string(),
                    }}
                }
            }
            {match &snap {
                Some(Ok(s)) => rsx! {
                    h3 { "Schema" }
                    div { class: "detail-kv",
                        div { class: "k", "Version" }
                        div { class: "v", "v{s.schema_version}" }
                    }
                    h3 { "Row counts" }
                    table { class: "detail-table",
                        thead { tr { th { "Table" } th { "Rows" } } }
                        tbody {
                            tr { td { "wallets" }              td { "{s.wallets}" } }
                            tr { td { "keys" }                 td { "{s.keys}" } }
                            tr { td { "controller_secrets" }   td { "{s.controller_secrets}" } }
                            tr { td { "did_inventory" }        td { "{s.did_inventory}" } }
                            tr { td { "resolved_cache" }       td { "{s.resolved_cache}" } }
                            tr { td { "sessions" }             td { "{s.sessions}" } }
                            tr { td { "logs" }                 td { "{s.logs}" } }
                        }
                    }
                },
                Some(Err(e)) => rsx! {
                    div { class: "wizard-outcome err",
                        div { class: "row label", "Store unhealthy" }
                        div { class: "seed-blob", "{e}" }
                    }
                },
                None => rsx! { div { class: "detail-empty", "Loading store stats…" } },
            }}
            div { class: "row",
                button {
                    onclick: move |_| load_stats(),
                    "Refresh"
                }
                {copy_btn(path_display.clone(), "Copy file path")}
            }
        }

        div { class: "card",
            div { class: "card-header", "Crypto suites" }
            div { style: "color: var(--text-muted); font-size: 11px; margin-bottom: 10px;",
                "Three curves are wired through the RedbSecretStore:"
            }
            table { class: "detail-table",
                thead {
                    tr { th { "Curve" } th { "JWK kty" } th { "Derivation" } th { "Use" } }
                }
                tbody {
                    tr {
                        td { "Ed25519" } td { class: "muted", "OKP" }
                        td { class: "muted", "BIP32 → HKDF-SHA256" }
                        td { class: "muted", "DID authentication" }
                    }
                    tr {
                        td { "P-256" } td { class: "muted", "EC" }
                        td { class: "muted", "BIP32 → HKDF, normalised" }
                        td { class: "muted", "DID assertion / authn" }
                    }
                    tr {
                        td { "Jubjub Schnorr" } td { class: "muted", "EC" }
                        td { class: "muted", "BIP32 → HKDF, raw scalar" }
                        td { class: "muted", "Midnight on-chain auth" }
                    }
                }
            }
        }

        WalletBackupCard { bridge_state: bridge_state.clone() }
    }
}

/// Controller-secret card at the top of the DID detail view.
/// Two modes depending on whether the wallet knows this DID's
/// 32-byte controller secret (`localSecretKey()` witness for
/// every write circuit):
///
/// - **Known** — shows a `Reveal` toggle + `Copy` button so the
///   operator can save the hex elsewhere (Files-app, password
///   manager, paper). Hidden by default so a shoulder-surfer can't
///   read it from the screen.
/// - **Unknown** — shows a hex input + `Save` button. Lets an
///   operator who already has the sk from a previous session,
///   the wizard-success banner, or an export file restore the
///   key into `BridgeState` (in-memory cache) + `WalletStore`
///   (persistent redb) so the Update / Deactivate buttons light
///   up. Validates: hex parses, 32 bytes.
#[component]
fn ControllerSecretCard(
    network: Network,
    did: String,
    current_secret: Option<[u8; 32]>,
    bridge_state: BridgeState,
) -> Element {
    let mut revealed = use_signal(|| false);
    let mut input = use_signal(String::new);
    let mut status =
        use_signal::<Option<Result<String, String>>>(|| None);

    if let Some(sk) = current_secret {
        let hex_full = format!("0x{}", hex::encode(sk));
        let is_revealed = *revealed.read();
        let display = if is_revealed {
            hex_full.clone()
        } else {
            "•••••••• click Reveal to show".to_string()
        };
        rsx! {
            div { class: "card",
                div { class: "card-header", "Controller secret" }
                div { style: "color: var(--success); font-size: 11px; margin-bottom: 8px;",
                    "Known — stored on this device. Save the hex if you \
                     want to restore Update / Deactivate access on a fresh \
                     install."
                }
                div { class: "row",
                    div { class: "seed-blob",
                        style: "flex: 1; word-break: break-all; font-family: monospace;",
                        "{display}"
                    }
                    button {
                        onclick: move |_| revealed.set(!is_revealed),
                        {if is_revealed { "Hide" } else { "Reveal" }}
                    }
                    {copy_btn(hex_full, "Copy controller secret (hex)")}
                }
            }
        }
    } else {
        let did_for_save = did.clone();
        let state_for_save = bridge_state.clone();
        let save = move |_| {
            let raw = input.read().clone();
            let hex_str =
                raw.trim().trim_start_matches("0x").trim_start_matches("0X");
            let bytes = match hex::decode(hex_str) {
                Ok(b) => b,
                Err(e) => {
                    status.set(Some(Err(format!("invalid hex: {e}"))));
                    return;
                }
            };
            if bytes.len() != 32 {
                status.set(Some(Err(format!(
                    "expected 32 bytes (64 hex chars), got {}",
                    bytes.len()
                ))));
                return;
            }
            let mut sk = [0u8; 32];
            sk.copy_from_slice(&bytes);
            state_for_save.remember_controller_secret(
                network,
                did_for_save.clone(),
                sk,
            );
            status.set(Some(Ok(
                "Saved. Update + Deactivate are now enabled \
                 (you may need to navigate back + forward to refresh the badge).".into(),
            )));
            input.set(String::new());
        };
        let status_snap = status.read().clone();
        rsx! {
            div { class: "card",
                div { class: "card-header", "Controller secret" }
                div { style: "color: var(--warning); font-size: 11px; margin-bottom: 8px;",
                    "Unknown — this DID was minted in another session or by \
                     a different wallet. Paste the 32-byte hex sk to enable \
                     Update / Deactivate. Without it, this DID is \
                     read-only from here on."
                }
                div { class: "row",
                    input {
                        style: "flex: 1; font-family: monospace; \
                                font-size: 12px; padding: 6px 10px;",
                        placeholder: "0x... (64 hex chars)",
                        value: "{input.read()}",
                        oninput: move |e| input.set(e.value()),
                    }
                    button { onclick: save, "Save" }
                }
                {match &status_snap {
                    Some(Ok(msg)) => rsx! {
                        div { class: "wizard-outcome ok",
                            div { class: "seed-blob", "{msg}" }
                        }
                    },
                    Some(Err(msg)) => rsx! {
                        div { class: "wizard-outcome err",
                            div { class: "seed-blob", "{msg}" }
                        }
                    },
                    None => rsx! { Fragment {} },
                }}
            }
        }
    }
}

/// Export/import card under Settings. Lets the operator dump the
/// two irrecoverable tables (wallet HD seeds + per-DID controller
/// secrets) into a JSON file outside the per-app sandbox, and
/// restore from one. See `wallet_core::store::backup` for the
/// file format + encryption story (existing scrypt envelopes
/// carry through verbatim — same unlock passphrase decrypts).
#[component]
fn WalletBackupCard(bridge_state: BridgeState) -> Element {
    use wallet_core::store::WalletBackup;

    let backup_dir = wallet_backup_dir();
    let mut import_path_input =
        use_signal(|| backup_dir.display().to_string());
    let mut status =
        use_signal::<Option<Result<String, String>>>(|| None);

    let do_export = {
        let state = bridge_state.clone();
        let dir = backup_dir.clone();
        move |_| {
            let Some(store) = state.store() else {
                status.set(Some(Err(
                    "wallet store not opened yet — unlock first"
                        .to_string(),
                )));
                return;
            };
            let backup = match store.export_backup() {
                Ok(b) => b,
                Err(e) => {
                    status.set(Some(Err(format!("export failed: {e}"))));
                    return;
                }
            };
            // Timestamp file name to YYYYMMDD-HHMMSS so the
            // operator's `ls backups/` sorts chronologically.
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let fname = format!("wallet-{now}.mwallet.json");
            let path = dir.join(&fname);
            let json = match serde_json::to_vec_pretty(&backup) {
                Ok(j) => j,
                Err(e) => {
                    status.set(Some(Err(format!("serialise failed: {e}"))));
                    return;
                }
            };
            if let Err(e) = std::fs::write(&path, &json) {
                status.set(Some(Err(format!("write {}: {e}", path.display()))));
                return;
            }
            status.set(Some(Ok(format!(
                "exported {} wallet seed(s) + {} controller secret(s) → {}",
                backup.wallets.len(),
                backup.controller_secrets.len(),
                path.display()
            ))));
        }
    };

    let do_import = {
        let state = bridge_state.clone();
        move |_| {
            let Some(store) = state.store() else {
                status.set(Some(Err(
                    "wallet store not opened yet — unlock first".to_string(),
                )));
                return;
            };
            let path_str = import_path_input.read().clone();
            let path = std::path::PathBuf::from(path_str.trim());
            if path.is_dir() {
                status.set(Some(Err(format!(
                    "{} is a directory; type the full path to a .mwallet.json file",
                    path.display()
                ))));
                return;
            }
            let bytes = match std::fs::read(&path) {
                Ok(b) => b,
                Err(e) => {
                    status.set(Some(Err(format!("read {}: {e}", path.display()))));
                    return;
                }
            };
            let backup: WalletBackup = match serde_json::from_slice(&bytes) {
                Ok(b) => b,
                Err(e) => {
                    status.set(Some(Err(format!("parse {}: {e}", path.display()))));
                    return;
                }
            };
            let summary = match store.import_backup(&backup) {
                Ok(s) => s,
                Err(e) => {
                    status.set(Some(Err(format!("import failed: {e}"))));
                    return;
                }
            };
            status.set(Some(Ok(format!(
                "imported {} from {}: {} wallet(s) inserted / {} overwritten, \
                 {} controller secret(s) inserted / {} overwritten{}",
                backup.format,
                path.display(),
                summary.wallets_inserted,
                summary.wallets_overwritten,
                summary.controller_secrets_inserted,
                summary.controller_secrets_overwritten,
                if summary.controller_secrets_skipped_bad_network > 0 {
                    format!(
                        " (skipped {} controller_secret rows with unknown network)",
                        summary.controller_secrets_skipped_bad_network
                    )
                } else {
                    String::new()
                },
            ))));
        }
    };

    let backup_dir_display = backup_dir.display().to_string();
    let status_snap = status.read().clone();

    rsx! {
        div { class: "card",
            div { class: "card-header", "Backup & restore" }
            div { style: "color: var(--text-muted); font-size: 11px; margin-bottom: 10px;",
                "Exports your wallet seeds + per-DID controller secrets to a \
                 JSON file (existing AES-GCM envelopes ride through; same unlock \
                 passphrase decrypts). Survives app reinstall, simulator \
                 device delete, anything that wipes the live store. Keep the \
                 file somewhere private — it carries everything needed to \
                 control DIDs you minted on this wallet."
            }
            div { class: "detail-kv",
                div { class: "k", "Backup directory" }
                div { class: "v", "{backup_dir_display}" }
            }

            h3 { "Export" }
            div { style: "color: var(--text-muted); font-size: 11px; margin-bottom: 6px;",
                "Writes a timestamped .mwallet.json file into the backup \
                 directory above. On iOS simulator use \
                 `xcrun simctl pull` to copy it out of the sandbox."
            }
            div { class: "row",
                button { onclick: do_export, "Export wallet" }
                {copy_btn(backup_dir_display.clone(), "Copy directory path")}
            }

            h3 { "Import" }
            div { style: "color: var(--text-muted); font-size: 11px; margin-bottom: 6px;",
                "Paste the full file path to a .mwallet.json file. Existing \
                 rows with the same wallet_id / (network, did) are \
                 overwritten; rows in the live store but not in the file are \
                 left alone."
            }
            div { class: "row",
                input {
                    style: "flex: 1; padding: 6px 10px; font-family: monospace; \
                            font-size: 12px;",
                    value: "{import_path_input.read()}",
                    oninput: move |e| import_path_input.set(e.value()),
                }
                button { onclick: do_import, "Import wallet" }
            }

            {match &status_snap {
                Some(Ok(msg)) => rsx! {
                    div { class: "wizard-outcome ok",
                        div { class: "row label", "OK" }
                        div { class: "seed-blob", "{msg}" }
                    }
                },
                Some(Err(msg)) => rsx! {
                    div { class: "wizard-outcome err",
                        div { class: "row label", "Failed" }
                        div { class: "seed-blob", "{msg}" }
                    }
                },
                None => rsx! { Fragment {} },
            }}
        }
    }
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
fn TxCostPanel(runs: Vec<CostRun>) -> Element {
    if runs.is_empty() {
        return rsx! {
            div { class: "session-log-empty",
                "Run a Create DID, Load circuit, Deactivate, or batched submit "
                "to capture per-flow dust + NIGHT cost."
            }
        };
    }
    // Aggregate totals across every run so the user sees the
    // session's running tab.
    let total_dust: u128 = runs.iter().map(|r| r.dust_consumed).sum();
    let total_night: u128 = runs.iter().map(|r| r.night_consumed).sum();
    rsx! {
        div { class: "wizard-header",
            "Transaction costs · "
            "Σ dust {format_atomic_dust(total_dust)} · "
            "Σ NIGHT {format_atomic_night(total_night)}"
        }
        ul { class: "session-log",
            for (idx , run) in runs.iter().enumerate().rev() {
                {render_cost_entry(idx, run)}
            }
        }
    }
}

fn render_cost_entry(idx: usize, run: &CostRun) -> Element {
    let outcome = if run.succeeded { "ok" } else { "err" };
    let total = format_ms(run.duration_ms);
    let dust = format_atomic_dust(run.dust_consumed);
    let night = format_atomic_night(run.night_consumed);
    rsx! {
        li {
            key: "cost-{idx}",
            class: "session-log-entry timing {outcome}",
            div { class: "head",
                span { class: "kind", "{run.label}" }
                span { class: "when", "#{idx + 1} · total {total}" }
            }
            div { class: "detail-kv",
                div { class: "k", "DUST" }
                div { class: "v", "{dust}" }
                div { class: "k", "NIGHT" }
                div { class: "v", "{night}" }
            }
        }
    }
}

/// Aggregated metrics for one operation kind across the
/// session. Built from `TimingRun` / `CostRun` entries grouped by
/// their label prefix — e.g. `"load_did_circuit:addAlsoKnownAs"`
/// and `"load_did_circuit:addService"` collapse into one row.
struct MetricRow {
    /// Human-readable label, e.g. "Create DID" or "Load circuit".
    label: String,
    runs: u32,
    failures: u32,
    /// Wall-clock duration totals (ms). `total` doubles as the
    /// numerator for the average; `min`/`max` are over all runs.
    total_ms: u64,
    min_ms: u64,
    max_ms: u64,
    /// Cost totals. `dust` may be zero when only `TimingRun`
    /// rows exist for this kind (no cost snapshot was taken).
    total_dust: u128,
    total_night: u128,
    /// Number of cost samples — used to average DUST / NIGHT
    /// separately from the timing-run count.
    cost_samples: u32,
}

impl MetricRow {
    fn new(label: String) -> Self {
        Self {
            label,
            runs: 0,
            failures: 0,
            total_ms: 0,
            min_ms: u64::MAX,
            max_ms: 0,
            total_dust: 0,
            total_night: 0,
            cost_samples: 0,
        }
    }

    fn record_timing(&mut self, ms: u64, ok: bool) {
        self.runs += 1;
        if !ok {
            self.failures += 1;
        }
        self.total_ms += ms;
        self.min_ms = self.min_ms.min(ms);
        self.max_ms = self.max_ms.max(ms);
    }

    fn record_cost(&mut self, dust: u128, night: u128) {
        self.total_dust = self.total_dust.saturating_add(dust);
        self.total_night = self.total_night.saturating_add(night);
        self.cost_samples += 1;
    }

    fn avg_ms(&self) -> u64 {
        if self.runs == 0 { 0 } else { self.total_ms / self.runs as u64 }
    }

    fn avg_dust(&self) -> u128 {
        if self.cost_samples == 0 {
            0
        } else {
            self.total_dust / self.cost_samples as u128
        }
    }

    fn avg_night(&self) -> u128 {
        if self.cost_samples == 0 {
            0
        } else {
            self.total_night / self.cost_samples as u128
        }
    }
}

/// Map a `TimingRun.label` / `CostRun.label` to its display
/// category. Labels are colon-prefixed by convention
/// (`"load_did_circuit:addService"`), so the part before `:` is
/// what we group on. Unknown prefixes fall through as-is.
fn metrics_label(raw: &str) -> &'static str {
    let prefix = raw.split(':').next().unwrap_or(raw);
    match prefix {
        "create_did" => "Create DID",
        "load_did_circuit" => "Load circuit",
        "call_did_circuit" => "Call circuit",
        "batch" => "Batch submit",
        "deactivate_did" => "Deactivate DID",
        _ => "Other",
    }
}

/// Aggregate `timings` + `costs` into one row per operation
/// kind. Rows are sorted by total wall-clock time descending so
/// the most expensive kind floats to the top.
fn aggregate_metrics(timings: &[TimingRun], costs: &[CostRun]) -> Vec<MetricRow> {
    use std::collections::BTreeMap;
    let mut by_kind: BTreeMap<&'static str, MetricRow> = BTreeMap::new();
    for t in timings {
        let key = metrics_label(&t.label);
        by_kind
            .entry(key)
            .or_insert_with(|| MetricRow::new(key.to_string()))
            .record_timing(t.total_ms, t.succeeded);
    }
    for c in costs {
        let key = metrics_label(&c.label);
        by_kind
            .entry(key)
            .or_insert_with(|| MetricRow::new(key.to_string()))
            .record_cost(c.dust_consumed, c.night_consumed);
    }
    let mut rows: Vec<MetricRow> = by_kind.into_values().collect();
    rows.sort_by(|a, b| b.total_ms.cmp(&a.total_ms));
    rows
}

/// Metrics tab — session totals + per-operation aggregates for
/// time-consuming flows (Create DID, Load circuit, Call circuit,
/// Batch submit, …). Empty until at least one timing or cost
/// run has been recorded.
#[component]
fn MetricsTab(timings: Vec<TimingRun>, costs: Vec<CostRun>) -> Element {
    if timings.is_empty() && costs.is_empty() {
        return rsx! {
            div { class: "session-log-empty",
                "No metrics yet. Drive a Create DID, Load circuit, or batch \
                 submit to populate aggregated timings + cost."
            }
        };
    }

    let rows = aggregate_metrics(&timings, &costs);
    let total_runs: u32 = rows.iter().map(|r| r.runs).sum();
    let total_failures: u32 = rows.iter().map(|r| r.failures).sum();
    let total_dust: u128 = rows.iter().map(|r| r.total_dust).sum();
    let total_night: u128 = rows.iter().map(|r| r.total_night).sum();
    let total_ms: u64 = rows.iter().map(|r| r.total_ms).sum();

    rsx! {
        div { class: "card",
            div { class: "card-header", "Session totals" }
            div { class: "metrics-summary",
                div { class: "metric-pill",
                    span { class: "k", "Runs" }
                    span { class: "v", "{total_runs}" }
                }
                div { class: "metric-pill",
                    span { class: "k", "Failed" }
                    span { class: "v", "{total_failures}" }
                }
                div { class: "metric-pill",
                    span { class: "k", "Wall-clock" }
                    span { class: "v", "{format_ms(total_ms)}" }
                }
                div { class: "metric-pill",
                    span { class: "k", "Σ DUST" }
                    span { class: "v", "{format_atomic_dust(total_dust)}" }
                }
                div { class: "metric-pill",
                    span { class: "k", "Σ NIGHT" }
                    span { class: "v", "{format_atomic_night(total_night)}" }
                }
            }
        }

        div { class: "card",
            div { class: "card-header", "Per operation" }
            table { class: "detail-table metrics-table",
                thead {
                    tr {
                        th { "Operation" }
                        th { class: "num", "Runs" }
                        th { class: "num", "Avg" }
                        th { class: "num", "Min" }
                        th { class: "num", "Max" }
                        th { class: "num", "Avg DUST" }
                        th { class: "num", "Avg NIGHT" }
                    }
                }
                tbody {
                    for row in rows.iter() {
                        {render_metric_row(row)}
                    }
                }
            }
        }
    }
}

fn render_metric_row(row: &MetricRow) -> Element {
    let runs_text = if row.failures == 0 {
        row.runs.to_string()
    } else {
        format!("{} ({} fail)", row.runs, row.failures)
    };
    let min_text = if row.min_ms == u64::MAX {
        "—".to_string()
    } else {
        format_ms(row.min_ms)
    };
    let avg_dust = if row.cost_samples == 0 {
        "—".to_string()
    } else {
        format_atomic_dust(row.avg_dust())
    };
    let avg_night = if row.cost_samples == 0 {
        "—".to_string()
    } else {
        format_atomic_night(row.avg_night())
    };
    rsx! {
        tr { key: "{row.label}",
            td { "{row.label}" }
            td { class: "num", "{runs_text}" }
            td { class: "num", "{format_ms(row.avg_ms())}" }
            td { class: "num", "{min_text}" }
            td { class: "num", "{format_ms(row.max_ms)}" }
            td { class: "num", "{avg_dust}" }
            td { class: "num", "{avg_night}" }
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
// ───────────────────────────────────────────────────────────────────
// Benchmark tab
// ───────────────────────────────────────────────────────────────────

/// One row in the Benchmark tab — the `k` value, its run state, and (if
/// finished) the captured timings from `contract_benchmark::RunStats`.
#[derive(Clone, Default, PartialEq)]
struct BenchRow {
    /// Last terminal state for this row. `None` while idle / pending.
    last: Option<BenchOutcome>,
    /// True while a `run_proof(k)` invocation is in flight.
    running: bool,
}

#[derive(Clone, PartialEq)]
enum BenchOutcome {
    Ok {
        realized_k: u32,
        prove_ms: u64,
        keygen_ms: u64,
        verify_ms: Option<u64>,
        verified: Option<bool>,
        rows: u64,
        chain: u32,
        proof_bytes: usize,
    },
    Err(String),
}

/// Number of rows the Benchmark tab renders. Matches the public
/// `contract_benchmark::MAX_K`. We hard-code rather than re-export so the
/// UI can render before the user clicks `Run` (no need to consult the
/// crate at startup).
const BENCH_MAX_K: u32 = contract_benchmark::MAX_K;
const BENCH_MIN_K: u32 = contract_benchmark::MIN_K;
const BENCH_MAX_VERIFIABLE_K: u32 = contract_benchmark::MAX_VERIFIABLE_K;
/// Empirically-safe default for the "Run all" upper bound on real
/// mobile hardware. S24 Ultra OOMs on k=18 with the WebView resident
/// (see `mobile-bench/benchmark.md` §9). Desktop
/// runs can override via the number input next to the Run button.
const BENCH_DEFAULT_MAX_K: u32 = 17;

// `proc_self_stats` + `CLK_TCK` moved to `src/proc_stats.rs`
// where they ship with a parser-level unit-test suite. See
// the `use crate::proc_stats::{…}` import at the top of this
// file.

/// Benchmark tab — runs `contract-benchmark::run_proof(k)` for
/// `k ∈ MIN_K..=MAX_K` and shows per-row prove timings.
///
/// Implementation notes:
/// - Proving is CPU-heavy and would block the UI thread if invoked
///   synchronously. We push each run onto Dioxus' executor via
///   `spawn(...)`, which on desktop hands off to the same tokio runtime
///   the rest of the app uses.
/// - `Run All` chains rows sequentially (one `spawn` that awaits each
///   `k` in order). The spec is explicit: don't parallelise — parallel
///   proving on a desktop CPU skews wall-clock per row.
/// - The cache dir resolves via `MidnightDataProvider`'s defaults
///   (env `MIDNIGHT_PP`, then `XDG_CACHE_HOME`, then `$HOME/.cache`).
///   Higher `k` values fetch SRS from `srs.midnight.network` on
///   first invocation; this can take several seconds (`bls_midnight_2p14`
///   alone is ~100 MB) and dominates the first-call timing for that `k`.
/// - For `k > MAX_VERIFIABLE_K` (14), the embedded `PARAMS_VERIFIER`
///   cannot check the proof. The crate skips verification gracefully
///   and we surface that as `verified: None` in the row label.
#[component]
fn BenchmarkTab() -> Element {
    let mut rows = use_signal::<std::collections::BTreeMap<u32, BenchRow>>(|| {
        (BENCH_MIN_K..=BENCH_MAX_K)
            .map(|k| (k, BenchRow::default()))
            .collect()
    });
    // True while the "Run all" sweep is in flight. Prevents the user
    // from kicking off a second sweep on top — and disables individual
    // Run buttons while the sweep is running for the same reason.
    let mut sweeping = use_signal(|| false);
    // Upper bound for the "Run all" sweep — user-settable so the
    // empirically-safe `BENCH_DEFAULT_MAX_K` (=17 on S24 Ultra) can
    // be raised on roomier desktops or lowered on tighter devices
    // without rebuilding. Per-row Run buttons ignore this cap, so
    // the user can still attempt k > max manually if they want to
    // measure where it OOMs.
    let mut max_k = use_signal(|| BENCH_DEFAULT_MAX_K);
    // Live process stats sampled from `/proc/self/{status,stat}` on
    // Android/Linux. macOS/iOS leave these as `None` (no `/proc`).
    let mut rss_kb = use_signal::<Option<u64>>(|| None);
    let mut cpu_pct = use_signal::<Option<f32>>(|| None);
    // Per-core CPU load read from `/proc/stat`. Each entry is the
    // 0.0..=1.0 utilisation since the previous 500 ms tick. Empty
    // on platforms without `/proc`. Used by the per-core bar grid
    // at the bottom of the tab.
    let mut per_core_pct = use_signal::<Vec<f32>>(Vec::new);
    // Long-running sampler — ticks every 500 ms regardless of bench
    // state so the user sees background drift, not just spikes
    // during a sweep. The future lives for the component's lifetime;
    // Dioxus tears it down when the tab unmounts.
    use_future(move || async move {
        let mut prev: Option<(u64, std::time::Instant)> = None;
        let mut prev_per_core: Vec<(u64, u64)> = Vec::new();
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            if let Some((rss, jiffies)) = proc_self_stats() {
                rss_kb.set(Some(rss));
                let now = std::time::Instant::now();
                if let Some((prev_jiff, prev_t)) = prev {
                    let dt = (now - prev_t).as_secs_f32();
                    if dt > 0.0 {
                        let djiff = jiffies.saturating_sub(prev_jiff) as f32;
                        cpu_pct.set(Some(djiff / (CLK_TCK as f32 * dt) * 100.0));
                    }
                }
                prev = Some((jiffies, now));
            }
            // Per-core sample. /proc/stat is system-wide so we need
            // a separate read. The diff vs the previous sample gives
            // the busy fraction over the last 500 ms — that's what
            // the bar grid renders.
            if let Some(cores) = proc_per_core_stats() {
                if prev_per_core.len() == cores.len() && !cores.is_empty() {
                    let pct: Vec<f32> = cores
                        .iter()
                        .zip(prev_per_core.iter())
                        .map(|((b, t), (pb, pt))| {
                            let db = b.saturating_sub(*pb) as f32;
                            let dt = t.saturating_sub(*pt) as f32;
                            if dt > 0.0 { (db / dt * 100.0).min(100.0) } else { 0.0 }
                        })
                        .collect();
                    per_core_pct.set(pct);
                }
                prev_per_core = cores;
            }
        }
    });

    // Helper: run `contract_benchmark::run_proof(k)` on the tokio
    // blocking thread pool. `run_proof` is `async fn`-shaped but the
    // halo2 prove inside is multi-second CPU work that never yields
    // — calling `.await` directly on the executor's worker threads
    // starves Dioxus' render loop and the eval bridge driver, so the
    // UI freezes for the duration. `spawn_blocking` parks the work
    // on the dedicated blocking pool, which has its own threads and
    // can absorb the load without blocking the runtime.
    async fn run_bench(k: u32) -> Result<contract_benchmark::RunStats, String> {
        tokio::task::spawn_blocking(move || {
            tokio::runtime::Handle::current()
                .block_on(contract_benchmark::run_proof(k))
        })
        .await
        .map_err(|e| format!("join: {e}"))
        .and_then(|r| r.map_err(|e| format!("{e}")))
    }

    fn stats_to_outcome(s: contract_benchmark::RunStats) -> BenchOutcome {
        BenchOutcome::Ok {
            realized_k: s.realized_k,
            prove_ms: s.prove.as_millis() as u64,
            keygen_ms: s.keygen.as_millis() as u64,
            verify_ms: s.verify.map(|d| d.as_millis() as u64),
            verified: s.verified,
            rows: s.rows,
            chain: s.hash_chain_len,
            proof_bytes: s.proof_bytes,
        }
    }

    // Helper closure: kicks off a single-k run and updates the row state
    // on completion. Used by both the per-row Run button and the
    // Run-all sweep.
    let run_one = move |k: u32| {
        // Mark the row as running synchronously so the spinner shows.
        rows.with_mut(|m| {
            if let Some(r) = m.get_mut(&k) {
                r.running = true;
            }
        });
        spawn(async move {
            let outcome = match run_bench(k).await {
                Ok(s) => stats_to_outcome(s),
                Err(e) => BenchOutcome::Err(e),
            };
            rows.with_mut(|m| {
                if let Some(r) = m.get_mut(&k) {
                    r.running = false;
                    r.last = Some(outcome);
                }
            });
        });
    };

    let run_all = move |_| {
        if *sweeping.read() {
            return;
        }
        let cap = (*max_k.read()).clamp(BENCH_MIN_K, BENCH_MAX_K);
        sweeping.set(true);
        // Mark every queued row (≤ cap) as pending up front so the
        // user sees the queue. Rows above the cap stay in whatever
        // state they were in — the sweep simply doesn't touch them.
        rows.with_mut(|m| {
            for (k, r) in m.iter_mut() {
                if *k <= cap {
                    r.running = true;
                    r.last = None;
                }
            }
        });
        spawn(async move {
            for k in BENCH_MIN_K..=cap {
                let outcome = match run_bench(k).await {
                    Ok(s) => stats_to_outcome(s),
                    Err(e) => BenchOutcome::Err(e),
                };
                rows.with_mut(|m| {
                    if let Some(r) = m.get_mut(&k) {
                        r.running = false;
                        r.last = Some(outcome);
                    }
                });
                // Yield between rows so the UI gets a chance to repaint.
                tokio::task::yield_now().await;
            }
            sweeping.set(false);
        });
    };

    // Compute summary (min / max / sum prove ms) over successful rows.
    let snapshot = rows.read().clone();
    let mut prove_times_ms: Vec<u64> = Vec::new();
    let mut total_prove_ms: u64 = 0;
    let mut succeeded: u32 = 0;
    let mut failed: u32 = 0;
    for r in snapshot.values() {
        if let Some(BenchOutcome::Ok { prove_ms, .. }) = &r.last {
            prove_times_ms.push(*prove_ms);
            total_prove_ms += *prove_ms;
            succeeded += 1;
        } else if matches!(&r.last, Some(BenchOutcome::Err(_))) {
            failed += 1;
        }
    }
    let min_ms = prove_times_ms.iter().copied().min();
    let max_ms = prove_times_ms.iter().copied().max();
    let is_sweeping = *sweeping.read();

    rsx! {
        div { class: "card",
            div { class: "card-header", "Contract benchmark" }
            p { class: "wizard-subtitle",
                "Runs the parameterised dummy contract at increasing circuit \
                 size (k = {BENCH_MIN_K}..={BENCH_MAX_K}). For k > {BENCH_MAX_VERIFIABLE_K} \
                 the embedded verifier cannot check the proof so verification \
                 is skipped. First call at a given k may stall on SRS \
                 download (srs.midnight.network)."
            }
            div { class: "row", style: "align-items: center; gap: 12px; flex-wrap: wrap;",
                button {
                    class: "cta",
                    disabled: is_sweeping,
                    onclick: run_all,
                    if is_sweeping { "Running…" } else { "Run all" }
                }
                label { style: "display: inline-flex; align-items: center; gap: 6px;",
                    "up to k ="
                    input {
                        r#type: "number",
                        min: "{BENCH_MIN_K}",
                        max: "{BENCH_MAX_K}",
                        value: "{max_k}",
                        disabled: is_sweeping,
                        style: "width: 4.5em;",
                        oninput: move |evt| {
                            if let Ok(v) = evt.value().parse::<u32>() {
                                max_k.set(v.clamp(BENCH_MIN_K, BENCH_MAX_K));
                            }
                        },
                    }
                }
            }

            // Live process stats. Always visible on Android/Linux so
            // the user sees baseline RSS before a sweep starts and the
            // spike during it. On macOS/iOS `proc_self_stats` returns
            // `None` and the pills stay hidden.
            if let Some(rss) = *rss_kb.read() {
                div { class: "metrics-summary",
                    div { class: "metric-pill",
                        span { class: "k", "RSS" }
                        span { class: "v", "{rss / 1024} MiB" }
                    }
                    if let Some(pct) = *cpu_pct.read() {
                        div { class: "metric-pill",
                            span { class: "k", "CPU" }
                            span { class: "v", "{pct:.0}%" }
                        }
                    }
                    if let Some(stage) = bench_stage::current_stage() {
                        div { class: "metric-pill",
                            span { class: "k", "Stage" }
                            span { class: "v", "{stage}" }
                        }
                    }
                }
            }

            // Per-core CPU load bar grid. Each bar represents one
            // core's busy fraction over the last 500 ms sample. On
            // the S24 Ultra this is a row of 10 bars (1 prime + 5
            // performance + 4 efficiency cores). On platforms
            // without `/proc/stat` the vector stays empty and the
            // block renders nothing.
            {
                let cores = per_core_pct.read();
                if !cores.is_empty() {
                    rsx! {
                        div { class: "cpu-cores",
                            for (i, pct) in cores.iter().enumerate() {
                                div { class: "cpu-core",
                                    title: "Core {i}: {pct:.0}%",
                                    div {
                                        class: "cpu-core-fill",
                                        style: "height: {pct.min(100.0)}%;",
                                    }
                                    span { class: "cpu-core-label", "C{i}" }
                                }
                            }
                        }
                    }
                } else {
                    rsx!()
                }
            }

            if succeeded > 0 || failed > 0 {
                div { class: "metrics-summary",
                    div { class: "metric-pill",
                        span { class: "k", "Done" }
                        span { class: "v", "{succeeded}" }
                    }
                    div { class: "metric-pill",
                        span { class: "k", "Failed" }
                        span { class: "v", "{failed}" }
                    }
                    div { class: "metric-pill",
                        span { class: "k", "Σ prove" }
                        span { class: "v", "{format_ms(total_prove_ms)}" }
                    }
                    if let Some(m) = min_ms {
                        div { class: "metric-pill",
                            span { class: "k", "Min" }
                            span { class: "v", "{format_ms(m)}" }
                        }
                    }
                    if let Some(m) = max_ms {
                        div { class: "metric-pill",
                            span { class: "k", "Max" }
                            span { class: "v", "{format_ms(m)}" }
                        }
                    }
                }
            }
        }

        div { class: "card scroll-x",
            table { class: "detail-table metrics-table bench-table",
                thead {
                    tr {
                        // `Realised k` and `Rows` were dropped per
                        // user request — for k ≥ 5 realised == k, and
                        // for k < 5 the floor (5/24 rows) is in the
                        // footnote in the architecture doc rather
                        // than the table. The remaining columns
                        // (Hashes, Keygen, Prove, Verify, Proof) are
                        // what actually moves between runs.
                        th { "k" }
                        // Abbreviated to "H" — saves ~30 px of column
                        // width on phone-class viewports where the
                        // bench-table is tight. The numeric value
                        // (chain length) is self-explanatory in context.
                        th { class: "num", "H" }
                        th { class: "num", "Keygen" }
                        th { class: "num", "Prove" }
                        th { class: "num", "Verify" }
                        th { class: "num", "Proof" }
                        th { class: "action", "" }
                    }
                }
                tbody {
                    for k in BENCH_MIN_K..=BENCH_MAX_K {
                        {render_bench_row(k, snapshot.get(&k).cloned().unwrap_or_default(), is_sweeping, run_one)}
                    }
                }
            }
        }
    }
}

fn render_bench_row(
    k: u32,
    row: BenchRow,
    sweeping: bool,
    mut run_one: impl FnMut(u32) + 'static + Copy,
) -> Element {
    let running = row.running;
    let busy = sweeping || running;
    // Realised k + rows columns removed per UX feedback; for k ≥ 5
    // realised always equals k, and the row count just tracks
    // hashes × constant. `error` survives as a row-spanning note
    // shown below the row on failure.
    let (chain, keygen, prove, verify, proof_cell, error_note) = match &row.last {
        None => (
            String::from("—"), String::from("—"), String::from("—"),
            String::from("—"), String::from("—"), String::new(),
        ),
        Some(BenchOutcome::Ok { realized_k: _, rows: _, chain, keygen_ms, prove_ms, verify_ms, verified, proof_bytes }) => {
            let verify_cell = match (verify_ms, verified) {
                (Some(v), Some(true))  => format!("{} ✓", format_ms(*v)),
                (Some(v), Some(false)) => format!("{} ✗", format_ms(v.clone())),
                _ => "skipped".to_string(),
            };
            // `keygen_ms == 0` means the key-cache short-circuited
            // (see `contract_benchmark::run_proof_with_opts` — when
            // `key_cache_lookup(k)` hits, `kg_start.elapsed()` is
            // essentially zero because no real keygen ran). Render
            // "cached" to avoid the "0 ms" mystery in the UI.
            let keygen_cell = if *keygen_ms == 0 {
                "cached".to_string()
            } else {
                format_ms(*keygen_ms)
            };
            (
                chain.to_string(),
                keygen_cell,
                format_ms(*prove_ms),
                verify_cell,
                format!("{} B", proof_bytes),
                String::new(),
            )
        }
        Some(BenchOutcome::Err(e)) => (
            String::from("—"), String::from("—"), String::from("—"),
            String::from("—"), String::from("—"),
            format!("error: {e}"),
        ),
    };

    let status_label = if running { "Running…" } else { "Run" };

    rsx! {
        tr { key: "bench-{k}",
            td { "{k}" }
            td { class: "num", "{chain}" }
            td { class: "num", "{keygen}" }
            td { class: "num", "{prove}" }
            td { class: "num", "{verify}" }
            td { class: "num", "{proof_cell}" }
            td { class: "action",
                button {
                    class: "bench-run-btn",
                    disabled: busy,
                    onclick: move |_| { run_one(k); },
                    "{status_label}"
                }
            }
        }
        if !error_note.is_empty() {
            tr { key: "bench-err-{k}", class: "bench-err-row",
                td { colspan: "7", "{error_note}" }
            }
        }
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
    /// Persistent store + cache handle. Threaded through so
    /// the Operation Builder's "use stored key" picker can
    /// list keys directly from the secret store.
    bridge_state: BridgeState,
    session_log: Vec<SessionEvent>,
    on_back: EventHandler<()>,
    on_resolved: EventHandler<wallet_core::ResolvedDid>,
    on_deactivated: EventHandler<(String, wallet_core::DeployOutcome)>,
    on_timing: EventHandler<TimingRun>,
    on_cost: EventHandler<CostRun>,
    on_event: EventHandler<SessionEvent>,
) -> Element {
    use wallet_core::WizardStage;

    let mut tab = use_signal(|| DetailTab::Overview);
    let mut resolving = use_signal(|| false);
    let mut resolve_error = use_signal::<Option<String>>(|| None);
    let mut deactivating = use_signal::<Vec<WizardStage>>(Vec::new);
    let mut deactivate_error = use_signal::<Option<String>>(|| None);
    // Modal-confirm gate for Deactivate. The Deactivate button
    // flips this to `true`; the actual deactivate flow only runs
    // when the user confirms in the dialog. Deactivate is
    // irreversible — the contract has no reactivation circuit —
    // so a confirm step is warranted.
    let mut confirm_deactivate = use_signal(|| false);
    // When true, render `DidOperationBuilder` instead of the
    // 8-tab view. Toggled by the "Update DID" button (which is
    // disabled unless we have the controller secret for this DID
    // — write circuits need it for the `localSecretKey()`
    // witness).
    let mut builder_mode = use_signal(|| false);
    // Queue lives here (not inside `DidOperationBuilder`) so it
    // survives the "Back to detail" → "Update DID" round trip.
    // Otherwise the user's pending + done rows are wiped every
    // time they navigate back to the detail view.
    let builder_queue =
        use_signal::<Vec<(DidOperation, QueueStatus)>>(Vec::new);
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
            let w = app_wallet_for(network);
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
    let mut deactivate = move |_| {
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
        let on_cost = on_cost.clone();
        spawn(async move {
            use futures::StreamExt;
            let w = app_wallet_for(network);
            let did_id = match wallet_core::DidId::parse(&did_str) {
                Ok(d) => d,
                Err(e) => {
                    deactivate_error.set(Some(format!("parse DID: {e}")));
                    return;
                }
            };
            let timing_label = "call_did_circuit:deactivate".to_string();
            let cost_label = timing_label.clone();
            let cost_start = std::time::Instant::now();
            let before = w.balance_snapshot().await.ok();
            let mut observations: Vec<(usize, std::time::Instant)> = Vec::new();
            let mut succeeded = false;
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
                    succeeded = matches!(&stage, WizardStage::Done(_));
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
            if let (Some(before), Ok(after)) = (before, w.balance_snapshot().await) {
                on_cost.call(CostRun {
                    label: cost_label,
                    dust_consumed: before.dust_atomic.saturating_sub(after.dust_atomic),
                    night_consumed: before.night_atomic.saturating_sub(after.night_atomic),
                    duration_ms: cost_start.elapsed().as_millis() as u64,
                    succeeded,
                });
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
                    let w = app_wallet_for(network);
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
            // Fragment ids of every VM currently in the resolved
            // document. Feeds the `method_id` dropdown for the
            // two relation ops so the operator picks from a real
            // list instead of typing a fragment. Strip any `did:…#`
            // prefix to match the on-chain raw form (same shape as
            // the relationships matrix builder).
            let method_ids: Vec<String> = cached
                .as_ref()
                .map(|r| {
                    r.document
                        .verification_method
                        .iter()
                        .map(|vm| vm_short_name(&vm.id).to_string())
                        .collect()
                })
                .unwrap_or_default();
            let bridge_state_for_builder = bridge_state.clone();
            return rsx! {
                DidOperationBuilder {
                    network,
                    did: did_for_builder,
                    controller_secret: sk,
                    bridge_state: bridge_state_for_builder,
                    loaded_circuits,
                    initial_counter,
                    method_ids,
                    on_back: move |_| builder_mode.set(false),
                    on_event,
                    on_resolved,
                    on_cost,
                    queue: builder_queue,
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
            // M3-style segmented capsule. Three related actions on
            // the same DID grouped into one pill-shaped container:
            // Update (primary fill), Resolve (neutral), Deactivate
            // (danger). The per-button colour rules under
            // `.detail-header .actions .btn-primary` / `.btn-danger`
            // are more specific than `.segmented > button`, so they
            // keep their semantic cues inside the capsule. Labels
            // are short ("Update", "Resolve", "Deactivate") — the
            // context (DID detail header) makes the noun obvious.
            div { class: "actions segmented",
                button {
                    class: "btn-primary",
                    disabled: update_disabled,
                    title: "{update_title}",
                    onclick: move |_| builder_mode.set(true),
                    "Update"
                }
                button {
                    disabled: *resolving.read(),
                    onclick: resolve_latest,
                    {if *resolving.read() { "Resolving…" } else { "Resolve" }}
                }
                button {
                    class: "btn-danger",
                    disabled: !controller_known
                        || cached.as_ref().map(|r| r.document.deactivated).unwrap_or(false),
                    title: if controller_known {
                        "Deactivate this DID — irreversible"
                    } else {
                        "Controller secret unknown — was this DID created in another session?"
                    },
                    // The Deactivate button now opens a confirm
                    // dialog instead of running deactivate inline.
                    // The dialog's Confirm action is the only place
                    // the `deactivate` closure is invoked from, so
                    // there's no ownership conflict.
                    onclick: move |_| confirm_deactivate.set(true),
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
        ControllerSecretCard {
            network,
            did: did.clone(),
            current_secret: controller_secret,
            bridge_state: bridge_state.clone(),
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
                bridge_state.clone(),
                &session_log,
            )}
        }
        // Modal confirm for Deactivate. Renders only while the
        // Deactivate button has flipped `confirm_deactivate` to
        // `true`. The scrim click + Cancel button restore the
        // gate; the Confirm button runs `deactivate(evt)` then
        // closes the dialog. `evt.stop_propagation()` on the
        // dialog card prevents an inside-card click from bubbling
        // up to the scrim and dismissing the dialog by accident.
        if *confirm_deactivate.read() {
            div { class: "dialog-scrim",
                onclick: move |_| confirm_deactivate.set(false),
                div { class: "dialog",
                    onclick: move |evt: Event<MouseData>| evt.stop_propagation(),
                    div { class: "dialog-title", "Deactivate DID?" }
                    div { class: "dialog-body",
                        "This action is permanent. Once a DID is deactivated it ",
                        b { "cannot be reactivated" }
                        " — the contract has no reactivation circuit, and the chain will reject any future update from this controller."
                    }
                    div { class: "dialog-actions",
                        button { class: "btn-text",
                            onclick: move |_| confirm_deactivate.set(false),
                            "Cancel"
                        }
                        button { class: "btn-danger",
                            onclick: move |evt| {
                                confirm_deactivate.set(false);
                                deactivate(evt);
                            },
                            "Deactivate"
                        }
                    }
                }
            }
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
    bridge_state: BridgeState,
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
                bridge_state,
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
    // `to_string_pretty` writes proper 2-space-indented JSON
    // with real newlines; render it inside `<pre>` so the
    // browser doesn't collapse whitespace. The previous version
    // used `.seed-blob` on a plain `<div>` which (a) didn't pick
    // up the styled rule (that's scoped to
    // `details .panel .seed-blob`) and (b) wouldn't preserve
    // newlines anyway — the result on screen was one wrapped
    // line of JSON with no indentation.
    let json = serde_json::to_string_pretty(&r.document)
        .unwrap_or_else(|e| format!("serialise: {e}"));
    rsx! {
        h3 { "DID Document" }
        pre {
            style: "font-family: ui-monospace, 'SF Mono', 'JetBrains Mono', Menlo, monospace;\
                    font-size: 11px;\
                    color: var(--mono-tint, var(--text));\
                    background: var(--surface-2);\
                    border: 1px solid var(--border-faint);\
                    border-radius: 8px;\
                    padding: 12px;\
                    margin: 0;\
                    white-space: pre-wrap;\
                    word-break: break-word;\
                    overflow-x: auto;",
            "{json}"
        }
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
                        let id_full = vm.id.clone();
                        // Display the human-readable name only so
                        // the column stays readable; the full
                        // `did:…#frag` URL stays on the `title`
                        // attribute (long-press / hover) and is
                        // what the Copy button yields.
                        let id_fragment = vm_short_name(&id_full).to_string();
                        rsx! {
                            tr {
                                td { title: "{id_full}", "{id_fragment}" }
                                td { class: "muted", "{kty}" }
                                td { class: "muted", "{crv}" }
                                td { {copy_btn(id_full.clone(), "Copy DID URL")} }
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
        .map(|vm| vm_short_name(&vm.id).to_string())
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

/// Helper: render a `label → value` row inside a card. The
/// value is a hex / opaque blob shown in mono with proper
/// wrapping (same `<pre>` pattern the Document tab uses, scaled
/// down for tight rows). Used by the Diagnostics tab to render
/// Seed / Coin PK / Encryption PK / Finalized head / proof-server
/// URL in consistent panels.
fn kv_blob_row(label: &str, value: &str) -> Element {
    rsx! {
        div { class: "balance-row",
            span { class: "label", "{label}" }
            pre {
                style: "flex: 1; margin: 0 0 0 12px;\
                        font-family: ui-monospace, 'SF Mono', 'JetBrains Mono', Menlo, monospace;\
                        font-size: 11px; color: var(--mono-tint, var(--text));\
                        white-space: pre-wrap; word-break: break-all;\
                        text-align: right;",
                "{value}"
            }
        }
    }
}

fn render_raw_state_tab(r: &wallet_core::ResolvedDid) -> Element {
    let n = r.raw_state_hex.len() / 2;
    let full_hex = format!("0x{}", r.raw_state_hex);
    // Same `<pre>` + inline-style pattern the Document tab uses
    // (see `render_document_tab`). The previous `<div class="seed-blob">`
    // didn't pick up the scoped `.seed-blob` rule outside a
    // `details .panel` container, so the hex string overflowed the
    // tab's container. `white-space: pre-wrap` + `word-break:
    // break-all` keeps the dump inside the panel and lets it wrap
    // at byte boundaries.
    rsx! {
        h3 { "Raw ledger state ({n} bytes)" }
        div { class: "row",
            {copy_btn(full_hex.clone(), "Copy raw state hex")}
            span { style: "color: var(--text-muted); font-size: 11px;",
                "fingerprint {state_fingerprint(&r.raw_state_hex)}"
            }
        }
        pre {
            style: "font-family: ui-monospace, 'SF Mono', 'JetBrains Mono', Menlo, monospace;\
                    font-size: 11px;\
                    color: var(--mono-tint, var(--text));\
                    background: var(--surface-2);\
                    border: 1px solid var(--border-faint);\
                    border-radius: 8px;\
                    padding: 12px;\
                    margin: 8px 0 0 0;\
                    white-space: pre-wrap;\
                    word-break: break-all;\
                    overflow-x: auto;",
            "{full_hex}"
        }
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
    /// Persistent store handle. Lets the Sign tab enumerate
    /// stored Jubjub keys so the user can pick which key the
    /// signature comes from. When the store isn't attached or
    /// has no Jubjub keys, only the "DID-derived" source is
    /// available — same as the original behaviour.
    bridge_state: BridgeState,
) -> Element {
    use wallet_core::secret_storage::jubjub_schnorr;

    let mut payload = use_signal(|| String::from("hello, did"));
    // 0 → DID-derived (the original
    // `seed_from_controller_and_did` path). 1..N → indexes
    // into `stored_jubjub_keys`. The picker re-keys whenever
    // the user selects a different row.
    let mut source_idx = use_signal(|| 0usize);
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

    // Stored Jubjub keys eligible to sign. Filtered from the
    // full secret-storage listing because the Sign tab's
    // pipeline is Jubjub-specific; Ed25519 / P-256 rows go to
    // other consumers (e.g. the Operation Builder's VM
    // picker).
    let stored_jubjub_keys: Vec<StoredJubjubKeyEntry> =
        list_stored_jubjub_keys_for_sign(&bridge_state);

    // Resolve the chosen seed for THIS render. DID-derived
    // path stays as before; stored-key path hands the row's
    // raw 32-byte secret to `sign_payload_diagnostic`, which
    // SHA-256s it the same way `curve_support::sign` does —
    // so the signature this tab renders is bit-identical to
    // what `RedbSecretStore::sign` would produce for the same
    // (key_ref, payload).
    let cur_source = *source_idx.read();
    let (seed, source_label) = if cur_source == 0 || cur_source > stored_jubjub_keys.len() {
        (
            jubjub_schnorr::seed_from_controller_and_did(&controller_secret, &did),
            "DID-derived (default)".to_string(),
        )
    } else {
        let entry = &stored_jubjub_keys[cur_source - 1];
        (
            entry.seed,
            format!("stored · {} ({})", entry.label, short_keyref(&entry.key_ref)),
        )
    };

    // One round trip through the wallet-core diagnostic
    // helper does everything: derive pk, hash to digest, sign,
    // encode both wire forms. Re-deriving on every render is
    // cheap (~1ms) and keeps the component a pure function of
    // its inputs.
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

    let keys_for_picker = stored_jubjub_keys.clone();
    rsx! {
        h3 { "Sign with Jubjub Schnorr" }
        div { style: "color: var(--text-muted); font-size: 11px; margin-bottom: 10px;",
            "Source: " strong { "{source_label}" } "."
            br {}
            "DID-derived keys live only in memory (deterministic from the DID's controller). "
            "Stored keys are HD-derived off the wallet seed and persist in "
            code { "~/.midnight/wallet-prototype/wallet.redb" } "."
        }
        div { class: "row",
            label { style: "min-width: 80px;", "Signing key" }
            select {
                onchange: move |e| {
                    if let Ok(i) = e.value().parse::<usize>() {
                        source_idx.set(i);
                        // Stored verify results refer to the
                        // previous key; wipe them so the user
                        // doesn't think a stale check still
                        // applies.
                        local_result.set(None);
                        bridge_result.set(None);
                        upstream_result.set(None);
                    }
                },
                style: "flex: 1; padding: 6px 8px; background: var(--surface-2); color: var(--text); border: 1px solid var(--border); border-radius: 6px;",
                option {
                    value: "0",
                    selected: cur_source == 0,
                    "DID-derived (controller_sk · did)"
                }
                for (i , k) in keys_for_picker.iter().enumerate() {
                    option {
                        value: "{i + 1}",
                        selected: cur_source == i + 1,
                        "stored · {k.label} ({short_keyref(&k.key_ref)})"
                    }
                }
            }
        }
        if stored_jubjub_keys.is_empty() {
            div { style: "color: var(--text-faint); font-size: 10px; margin-top: 4px;",
                "Tip: generate a Jubjub key in the Keys tab to make it selectable here."
            }
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
    use wallet_core::js_bridge::{JsBridgeExt, NodeChildBridge};
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
    // Compact inventory layout: status badge | Open button | short
    // DID. Network / Counter / VMs / Services columns removed —
    // the active network lives in the header, and the rest is
    // visible inside the DID detail view itself.
    rsx! {
        div { class: "wizard-header", "DIDs ({entries.len()})" }
        div { class: "did-inventory",
            div { class: "did-inventory-row did-inventory-header",
                span { class: "did-inventory-cell status", "Status" }
                span { class: "did-inventory-cell action", "" }
                span { class: "did-inventory-cell did", "DID" }
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
            span { class: "did-inventory-cell action",
                button {
                    onclick: move |_| on_select.call(did_for_click.clone()),
                    "Open"
                }
            }
            span {
                class: "did-inventory-cell did",
                title: "{did_full}",
                "{did_short}"
            }
        }
    }
}

/// Truncate a DID for table display — keeps the `did:midnight:net:`
/// prefix and the last 6 chars of the address so it's still
/// recognisable but doesn't blow out the column. Full DID lives on
/// the row's `title` attribute for hover.
/// Extract the human-readable "name" portion of a verification
/// method id for table / dropdown display.
///
/// `VerificationMethod.id` per DID Core is `<did>#<fragment>`,
/// and `ledger_to_domain` (`mobile-bench/wallet-core/src/did/contract.rs`)
/// promotes bare fragment ids into that full URL form on resolve.
/// But operators sometimes type the full DID URL **without** the
/// `#` separator into the Operation Builder's id field — when
/// that happens, `ledger_to_domain` sees a `#`-less id and leaves
/// it untouched, so a naive `rsplit('#').next()` shows the full
/// string.
///
/// Strategy, in order:
/// 1. After the last `#` if one exists — DID Core form.
/// 2. Otherwise, strip a `did:<method>:<network>:<address>`
///    prefix (3 colons) and return whatever's left, also
///    stripping any leading `#` / `:`.
/// 3. Otherwise return the input unchanged.
fn vm_short_name(id: &str) -> &str {
    if let Some(p) = id.rfind('#') {
        return &id[p + 1..];
    }
    // No '#': try to strip the `did:method:network:addr` prefix.
    // Index of the 3rd colon (0-indexed) — separator between the
    // address and any colon-style suffix.
    if let Some((p, _)) = id.match_indices(':').nth(2) {
        // After the 3rd colon is the address. Find the next non-
        // hex character; anything past it is the "name". If the
        // whole rest is hex, the id has no name and we return it
        // as-is.
        let after_addr = &id[p + 1..];
        if let Some(end) = after_addr.find(|c: char| !c.is_ascii_hexdigit()) {
            return after_addr[end..].trim_start_matches(['#', ':']);
        }
    }
    id
}

pub(crate) fn truncate_did(did: &str) -> String {
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

// Pure formatting helpers were moved to `src/format.rs` so they
// can be unit-tested in isolation; see the top-of-file
// `use crate::format::{…}` import.

#[component]
fn BalancesCard(
    connected: bool,
    night_subunits: Option<u128>,
    dust_subunits: Option<u128>,
) -> Element {
    // Three display states each:
    //   • not connected           → "—"
    //   • connected, sync pending → "syncing…"
    //   • connected, sync done    → compact whole-unit value
    //                               with the precise value below.
    let night = match (connected, night_subunits) {
        (false, _) => None,
        (true, None) => Some(BalanceCell::Syncing),
        (true, Some(n)) => {
            let (compact, exact) = format_balance(n, NIGHT_DECIMALS);
            Some(BalanceCell::Value { compact, exact })
        }
    };
    let dust = match (connected, dust_subunits) {
        (false, _) => None,
        (true, None) => Some(BalanceCell::Syncing),
        (true, Some(n)) => {
            let (compact, exact) = format_balance(n, DUST_DECIMALS);
            Some(BalanceCell::Value { compact, exact })
        }
    };

    rsx! {
        div { class: "card",
            div { class: "card-header", "Balances" }
            {render_balance_row("NIGHT", &night)}
            {render_balance_row("DUST", &dust)}
            div { class: "balance-row",
                span { class: "hint",
                    {match (connected, night_subunits, dust_subunits) {
                        (false, _, _) => "Connect to the network to see live balances.",
                        (true, None, _) | (true, _, None) => "Syncing wallet state from the indexer…",
                        (true, Some(0), _) => "No NIGHT yet. Send NIGHT to the address above.",
                        (true, Some(_), Some(_)) => "DUST accrues from registered NIGHT UTXOs.",
                    }}
                }
            }
        }
    }
}

/// Display state for one currency row in the Balances card.
enum BalanceCell {
    Syncing,
    Value { compact: String, exact: String },
}

/// Render one Balances-card row. Layout: `<label>` on the left,
/// stacked **exact** value + optional compact tag on the right.
///
/// Previously the compact form (e.g. "5K") was the prominent
/// number and the exact value sat in a 11 px gray line beneath.
/// In a wallet, users want to see the actual amount — the
/// compact form is now a small secondary tag (e.g. "≈ 5K") shown
/// only when the whole-unit part is large enough that the
/// abbreviation is informative (≥ 1,000). Below 1,000 the exact
/// value alone is shown.
fn render_balance_row(unit: &str, cell: &Option<BalanceCell>) -> Element {
    let (primary, secondary) = match cell {
        None => ("—".to_string(), None),
        Some(BalanceCell::Syncing) => ("syncing…".to_string(), None),
        Some(BalanceCell::Value { compact, exact }) => {
            // Only surface the compact tag when it differs from
            // the exact (i.e. abbreviation actually kicked in —
            // it suffixes K/M/B/T, exact never does).
            let tag = if compact != exact {
                Some(format!("≈ {compact}"))
            } else {
                None
            };
            (exact.clone(), tag)
        }
    };
    let label = unit.to_string();
    let unit = unit.to_string();
    rsx! {
        div { class: "balance-row",
            span { class: "label", "{label}" }
            div { class: "value-stack",
                div { class: "value-line",
                    span { class: "value", "{primary}" }
                    span { class: "unit", " {unit}" }
                }
                if let Some(s) = secondary {
                    div { class: "precise", "{s}" }
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

/// Cross-platform clipboard write. Desktop (macOS / Linux /
/// Windows) uses `arboard`; Android + iOS no-op for now —
/// platform-native paths (`ClipboardManager` via JNI on Android,
/// `UIPasteboard` via objc on iOS) land in a follow-up.
#[cfg(all(not(target_os = "android"), not(target_os = "ios")))]
fn copy_to_clipboard(s: &str) -> Result<(), String> {
    let mut cb = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    cb.set_text(s.to_string()).map_err(|e| e.to_string())
}

#[cfg(any(target_os = "android", target_os = "ios"))]
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

/// Find the wallet row matching `network` in the store, or
/// auto-create one. Returns `Some(WalletId)` on success or
/// `None` if no row exists and we couldn't mint one (e.g.
/// `seed_hex_opt` is `None` or doesn't decode).
fn find_or_create_wallet_for_network(
    store: &wallet_core::store::WalletStore,
    net: Network,
    seed_hex_opt: Option<&str>,
) -> Option<wallet_core::store::WalletId> {
    use wallet_core::store::NetworkTag;
    let target = NetworkTag::from(net);

    // Look first — `wallet_meta` is a cheap read txn.
    if let Ok(ids) = store.list_wallet_ids() {
        for id in &ids {
            if let Ok(Some(meta)) = store.wallet_meta(*id) {
                if meta.network == target {
                    return Some(*id);
                }
            }
        }
    }

    // Nothing yet — mint a row tagged with this network's
    // demo seed. The label encodes the network so the
    // wallet picker can render rows like "Demo · PreProd".
    let hex = seed_hex_opt?;
    let bytes = hex::decode(hex).ok()?;
    let seed: [u8; 32] = bytes.as_slice().try_into().ok()?;
    let label = format!("Demo · {}", net.label());
    match store.create_wallet(&label, net, &seed) {
        Ok(id) => {
            tracing::info!(wallet_id=%id, network=?net, "auto-created wallet row");
            Some(id)
        }
        Err(e) => {
            tracing::warn!(error=%e, network=?net, "auto-create wallet row failed");
            None
        }
    }
}

/// Bulk-load DID inventory rows for `net` into the UI's
/// in-memory map shape. Empty map on store error or empty
/// table; the caller can `.is_empty()` to decide whether to
/// even touch the signal.
fn load_inventory_for_network(
    store: &wallet_core::store::WalletStore,
    net: Network,
) -> std::collections::BTreeMap<String, DidInventoryEntry> {
    let mut map = std::collections::BTreeMap::new();
    let Ok(rows) = store.list_did_inventory(net) else {
        return map;
    };
    for row in rows {
        map.insert(
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
    map
}

/// Reset every per-network UI signal and re-hydrate from
/// the store for `new_net`. Used by the network switcher in
/// the Wallet tab — keeps the inventory + cache + active
/// wallet id in sync with whatever network the user just
/// picked.
///
/// `seed_hex` is the demo seed for the target network; needed
/// only if no wallet row exists for that network yet so we
/// can auto-create one.
///
/// `bridge_state` must already have an attached store —
/// nothing useful happens otherwise (the caller wouldn't
/// have gotten here without unlocking first).
async fn rehydrate_for_network(
    bridge_state: BridgeState,
    new_net: Network,
    seed_hex: Option<String>,
    mut did_inventory: Signal<std::collections::BTreeMap<String, DidInventoryEntry>>,
    mut resolved_cache: Signal<
        std::collections::HashMap<String, wallet_core::ResolvedDid>,
    >,
    mut previous_resolved_cache: Signal<
        std::collections::HashMap<String, wallet_core::ResolvedDid>,
    >,
    mut open_did: Signal<Option<String>>,
    mut last_did_id: Signal<Option<String>>,
    mut last_resolved: Signal<Option<(String, u32)>>,
) {
    let Some(store) = bridge_state.store().cloned() else {
        tracing::warn!("rehydrate: store not attached, skipping");
        return;
    };

    // Drop any cached "open this DID on the prior network"
    // state — the DID id from the old network won't exist
    // (or means something different) on the new one.
    open_did.set(None);
    last_did_id.set(None);
    last_resolved.set(None);
    did_inventory.set(Default::default());
    resolved_cache.set(Default::default());
    previous_resolved_cache.set(Default::default());

    // Wipe the in-memory controller-secret cache too —
    // hydration below repopulates it for the new network.
    if let Ok(mut g) = bridge_state.controller_secrets.lock() {
        g.clear();
    }

    let wallet_id =
        find_or_create_wallet_for_network(&store, new_net, seed_hex.as_deref());
    bridge_state.set_active_wallet_id(wallet_id);

    // Re-register a DustSyncer for the new network. The unlock
    // path does this once for the network active at unlock-time
    // (see `on_unlock` block above); without this re-registration
    // here, `dust_syncer_for(new_net)` returns `None` after the
    // user picks a different network from the header dropdown,
    // and the DUST sync row stays stuck on "syncer not
    // initialised (unlock the wallet first)" even though the
    // wallet is fully unlocked. The bug originally surfaced
    // when switching from PreProd → Undeployed on the standalone
    // env: NIGHT synced, DUST didn't. `set_dust_syncer_for` is
    // idempotent (overwrites the prior entry), so the path
    // remains safe to call on every switch including back to
    // PreProd.
    {
        let tmp = app_wallet_for(new_net);
        match tmp.dust_secret_key() {
            Ok(sk) => {
                let syncer = std::sync::Arc::new(wallet_core::DustSyncer::new(
                    new_net,
                    std::sync::Arc::new(store.clone()),
                    sk,
                ));
                set_dust_syncer_for(new_net, syncer);
                tracing::info!(
                    network=?new_net,
                    "dust syncer registered (network switch)"
                );
            }
            Err(e) => {
                tracing::warn!(
                    network=?new_net,
                    error=%e,
                    "dust secret key derivation failed; DustSyncer not re-registered"
                );
            }
        }
    }

    // Re-seed the PreProd-live demo state when switching INTO
    // PreProd (matches the unlock-time seeding). No-op on
    // other networks or in vanilla builds.
    #[cfg(feature = "preprod-live")]
    if matches!(new_net, Network::PreProd) {
        seed_preprod_live_state(&bridge_state, &store);
    }

    let secrets_count = bridge_state.hydrate_controller_secrets(new_net);
    let inv_map = load_inventory_for_network(&store, new_net);
    let inv_count = inv_map.len();
    if !inv_map.is_empty() {
        did_inventory.set(inv_map);
    }
    let cache_map = load_resolved_cache_for_network(&store, new_net);
    let cache_count = cache_map.len();
    if !cache_map.is_empty() {
        resolved_cache.set(cache_map);
    }
    tracing::info!(
        network=?new_net,
        wallet_id=?wallet_id,
        hydrated_controller_secrets=secrets_count,
        hydrated_inventory=inv_count,
        hydrated_cache=cache_count,
        "network switch: re-hydrated",
    );

    // Auto-resolve every hydrated DID in the background —
    // same shape as the unlock spawn's auto-resolve. A
    // network switch is the other moment the inventory's
    // counter / status badges could be stale.
    let dids_to_refresh: Vec<String> = did_inventory.read().keys().cloned().collect();
    for did_str in dids_to_refresh {
        let bridge = bridge_state.clone();
        spawn(async move {
            let w = app_wallet_for(new_net);
            if let Ok(resolved) = w.resolve_did_full(&did_str).await {
                let did_string = resolved.document.id.to_did_string();
                let entry = DidInventoryEntry {
                    did: did_string.clone(),
                    network_label: new_net.label().to_string(),
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
                persist_inventory_entry(&bridge, new_net, &entry);
                let cache_snap = resolved_cache.read().clone();
                if let Some(prev) = cache_snap.get(&did_string) {
                    let mut prev_map = previous_resolved_cache.read().clone();
                    prev_map.insert(did_string.clone(), prev.clone());
                    previous_resolved_cache.set(prev_map);
                }
                let mut cache = cache_snap;
                cache.insert(did_string.clone(), resolved.clone());
                resolved_cache.set(cache);
                persist_resolved_cache(&bridge, new_net, &did_string, &resolved);
            }
        });
    }
}

/// Bulk-load resolved-cache snapshots for `net`, decoded
/// from the on-disk JSON. Entries that fail to decode are
/// dropped silently — the next manual / auto resolve will
/// refresh them.
fn load_resolved_cache_for_network(
    store: &wallet_core::store::WalletStore,
    net: Network,
) -> std::collections::HashMap<String, wallet_core::ResolvedDid> {
    let mut map = std::collections::HashMap::new();
    let Ok(rows) = store.list_resolved_cache(net) else {
        return map;
    };
    for (did, json, _at) in rows {
        if let Ok(r) = serde_json::from_str::<wallet_core::ResolvedDid>(&json) {
            map.insert(did, r);
        }
    }
    map
}

/// Write-through helper for the single-row session table.
/// Pushes the active tab, current network, open DID, and the
/// last-resolved tuple. Silent on store-write errors — the
/// session row is purely a UX convenience, never a hard
/// dependency.
fn persist_session(
    bridge_state: &BridgeState,
    network: Network,
    active_tab: Tab,
    open_did: Option<String>,
    last_did_id: Option<String>,
    last_resolved: Option<(String, u32)>,
) {
    let Some(store) = bridge_state.store() else {
        return;
    };
    let snap = wallet_core::store::SessionSnapshot {
        network,
        active_tab: active_tab.to_persist(),
        open_did,
        last_did_id,
        last_resolved,
        updated_at: 0,
    };
    if let Err(e) = store.put_session(snap) {
        tracing::warn!(error=%e, "persist session failed");
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

/// Default passphrase shown in the unlock card. The user can
/// override before clicking Unlock; the value lives in the
/// `passphrase_input` signal until they do. Hardcoded to
/// "midnight" for the prototype — a future slice may add
/// rotation + a "remember on this machine" toggle.
const DEV_STORE_PASSPHRASE: &str = "midnight";

/// Tri-state used by the App-level unlock gate. `Locked` is
/// the boot state; `Opening` means the open + hydration task
/// is in flight; `Open` means the store is attached and the
/// rest of the app can render; `Failed(msg)` means the
/// last unlock attempt errored — the user can retype the
/// passphrase and try again.
#[derive(Clone, PartialEq, Eq, Debug)]
enum UnlockState {
    Locked,
    Opening,
    Open,
    Failed(String),
}

/// Path the persistent wallet store lives at — pinned to
/// `~/.midnight/wallet-prototype/wallet.redb` for the
/// prototype. Stays out of platform-specific data dirs
/// (`~/Library/Application Support/...` etc.) so all three
/// host OSes share the same conventional location for the
/// duration of the prototype work; a production build would
/// likely move back to the OS-idiomatic dir.
///
/// Falls back to a `./wallet.redb` next to the binary if the
/// home dir can't be resolved (unlikely on macOS / Linux /
/// Windows but defensive).
pub(crate) fn wallet_store_path() -> std::path::PathBuf {
    #[cfg(target_os = "android")]
    {
        // Android: the app sandbox can write to its own private
        // `files/` dir but not to `/data/local/tmp` (owned by the
        // shell user). Resolve the running app's package name from
        // `/proc/self/cmdline` (Android writes the package id there
        // for every app process) and write under
        // `/data/data/<package>/files/midnight-dx-wallet/`. Falls
        // back to a relative path if we can't read the package id —
        // the binary still launches even if persistence is broken.
        if let Some(pkg) = read_android_package_name() {
            let dir = std::path::PathBuf::from("/data/data")
                .join(&pkg)
                .join("files")
                .join("midnight-dx-wallet");
            let _ = std::fs::create_dir_all(&dir);
            return dir.join("wallet.redb");
        }
        return std::path::PathBuf::from("wallet.redb");
    }
    #[cfg(target_os = "ios")]
    {
        // iOS: the per-app sandbox is reachable via `$HOME` (which
        // the system points at the app's container root). Put
        // persistent user-visible data under `Documents/` so iCloud
        // backup picks it up if the app opts in. `dirs` isn't on
        // the iOS dep set — we resolve `$HOME` directly via
        // `std::env`.
        if let Some(home) = std::env::var_os("HOME") {
            let dir = std::path::PathBuf::from(home)
                .join("Documents")
                .join("midnight-dx-wallet");
            let _ = std::fs::create_dir_all(&dir);
            return dir.join("wallet.redb");
        }
        return std::path::PathBuf::from("wallet.redb");
    }
    #[cfg(all(not(target_os = "android"), not(target_os = "ios")))]
    {
        if let Some(home) = dirs::home_dir() {
            let dir = home.join(".midnight").join("wallet-prototype");
            let _ = std::fs::create_dir_all(&dir);
            return dir.join("wallet.redb");
        }
        std::path::PathBuf::from("wallet.redb")
    }
}

/// Directory where wallet backup files (`*.mwallet.json`) live.
/// Co-located with `wallet_store_path()`'s parent so the operator
/// finds them next to the live database, and on iOS the
/// backups land under the per-app `Documents/` tree —
/// reachable via `xcrun simctl pull` for the sim, and via
/// Files.app for the device (provided
/// `UIFileSharingEnabled=YES` ever gets flipped in
/// Info.plist).
pub(crate) fn wallet_backup_dir() -> std::path::PathBuf {
    let dir = wallet_store_path()
        .parent()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("backups");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Read the running app's package name from `/proc/self/cmdline`.
/// Android writes the package id there for every app process. This
/// avoids pulling JNI just to ask Android for `getPackageName()`.
#[cfg(target_os = "android")]
fn read_android_package_name() -> Option<String> {
    let raw = std::fs::read("/proc/self/cmdline").ok()?;
    // cmdline is NUL-separated; the first slot is the executable
    // (the package id, e.g. "io.iohk.midnight.wallet").
    let first = raw.split(|b| *b == 0).next()?;
    let s = std::str::from_utf8(first).ok()?.trim();
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
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

// Tests for the pure formatting helpers moved with them to
// `src/format.rs`. The remaining surface in this file is
// Dioxus-component-heavy and exercised via the integration
// tests in `tests/`.
