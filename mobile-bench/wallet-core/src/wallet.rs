use std::sync::Arc;

use rand::{RngCore, SeedableRng, rngs::OsRng};
use rand_chacha::ChaCha20Rng;
use serialize::Serializable;
use zswap::keys::{Seed, SecretKeys};

use crate::chain;
use crate::js_bridge::{JsBridge, JsBridgeExt};
use crate::network::Network;

/// Shape of the `prepareUnprovenCallTx` harness response. Lives
/// here (rather than on a public type) because `call_did_circuit`
/// is the only consumer.
#[derive(serde::Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct PrepareUnprovenCallTxResult {
    unproven_tx_hex: String,
    #[allow(dead_code)] // surfaced for diagnostics
    elapsed_ms: i64,
}

/// Drive the bridge's `prepareUnprovenCallTx` and return the hex
/// blob. Pulled out so `call_did_circuit` reads top-down without
/// the JSON-ferrying noise. Accepts any `JsBridge` impl —
/// `NodeChildBridge` for desktop tests, the WebView eval bridge
/// installed by `dioxus-wallet` for Android (and production
/// desktop) builds.
#[allow(clippy::too_many_arguments)]
async fn call_prepare_unproven(
    bridge: &dyn JsBridge,
    did: String,
    circuit: String,
    circuit_args: serde_json::Value,
    contract_state_hex: String,
    contract_address_hex: String,
    zswap_chain_state_hex: Option<String>,
    ledger_parameters_hex: Option<String>,
    controller_sk: [u8; 32],
    coin_public_key_hex: String,
    encryption_public_key_hex: String,
    network_id: String,
) -> Result<String, crate::js_bridge::JsBridgeError> {
    let r: PrepareUnprovenCallTxResult = bridge
        .call(
            "prepareUnprovenCallTx",
            serde_json::json!({
                "did": did,
                "circuit": circuit,
                "circuitArgs": circuit_args,
                "contractStateHex": contract_state_hex,
                "contractAddressHex": contract_address_hex,
                "zswapChainStateHex": zswap_chain_state_hex,
                "ledgerParametersHex": ledger_parameters_hex,
                "controllerSecretHex": hex::encode(controller_sk),
                "coinPublicKeyHex": coin_public_key_hex,
                "encryptionPublicKeyHex": encryption_public_key_hex,
                "networkId": network_id,
            }),
        )
        .await?;
    Ok(r.unproven_tx_hex)
}

/// Drive the bridge's `prepareVaultCallTx` (the passport-vault analogue
/// of `prepareUnprovenCallTx`) and return the unproven tx hex. Used by
/// [`Wallet::call_contract_circuit`]. Unlike the DID shim there is no
/// `controllerSecretHex` — the circuit's witnesses (e.g. the holder
/// date-of-birth for `claimFunds`) are supplied via `private_state`;
/// `depositFunds` omits it (empty private state).
#[allow(clippy::too_many_arguments)]
async fn call_prepare_vault_unproven(
    bridge: &dyn JsBridge,
    circuit: String,
    circuit_args: serde_json::Value,
    private_state: Option<serde_json::Value>,
    contract_state_hex: String,
    contract_address_hex: String,
    zswap_chain_state_hex: Option<String>,
    ledger_parameters_hex: Option<String>,
    coin_public_key_hex: String,
    encryption_public_key_hex: String,
    network_id: String,
) -> Result<String, crate::js_bridge::JsBridgeError> {
    let r: PrepareUnprovenCallTxResult = bridge
        .call(
            "prepareVaultCallTx",
            serde_json::json!({
                "circuit": circuit,
                "circuitArgs": circuit_args,
                "privateState": private_state,
                "contractStateHex": contract_state_hex,
                "contractAddressHex": contract_address_hex,
                "zswapChainStateHex": zswap_chain_state_hex,
                "ledgerParametersHex": ledger_parameters_hex,
                "coinPublicKeyHex": coin_public_key_hex,
                "encryptionPublicKeyHex": encryption_public_key_hex,
                "networkId": network_id,
            }),
        )
        .await?;
    Ok(r.unproven_tx_hex)
}

/// Drive the bridge's `prepareVaultClaim` (the high-level claim compose
/// that deserialises the holder credential bundle, reads the on-chain
/// policy, builds the selective-disclosure presentation + age proof,
/// and composes `claimFunds`). Returns the unproven tx hex. Unlike the
/// deposit, the shielded input is the contract's escrow coin (built by
/// the compose); the holder only pays DUST.
#[allow(clippy::too_many_arguments)]
async fn call_prepare_vault_claim(
    bridge: &dyn JsBridge,
    lock_id: u64,
    bundle: serde_json::Value,
    amount_base_units: String,
    current_day: Option<String>,
    contract_state_hex: String,
    contract_address_hex: String,
    zswap_chain_state_hex: Option<String>,
    ledger_parameters_hex: Option<String>,
    coin_public_key_hex: String,
    encryption_public_key_hex: String,
    recipient_user_address_hex: String,
    network_id: String,
) -> Result<String, crate::js_bridge::JsBridgeError> {
    let r: PrepareUnprovenCallTxResult = bridge
        .call(
            "prepareVaultClaim",
            serde_json::json!({
                "lockId": lock_id.to_string(),
                "bundle": bundle,
                "amountBaseUnits": amount_base_units,
                "currentDay": current_day,
                "contractStateHex": contract_state_hex,
                "contractAddressHex": contract_address_hex,
                "zswapChainStateHex": zswap_chain_state_hex,
                "ledgerParametersHex": ledger_parameters_hex,
                "coinPublicKeyHex": coin_public_key_hex,
                "encryptionPublicKeyHex": encryption_public_key_hex,
                "recipientUserAddressHex": recipient_user_address_hex,
                "networkId": network_id,
            }),
        )
        .await?;
    Ok(r.unproven_tx_hex)
}

/// Stable hardcoded seed used by [`Wallet::demo`] for non-Undeployed
/// networks so the dev UI shows the *same* coin/encryption public
/// keys across launches. **Not a real wallet**: the bytes are
/// publicly committed, so anything funded against these keys is
/// everyone's money. The gsd-wallet-style W0–W3 genesis seeds for
/// other localnet flavours land in iter-2.
pub const DEMO_SEED_HEX: &str =
    "88b9e1f2a2bf22ec7e739e6d43abc16f593ebdc1460568cb16a7730700bda13c";

/// Pre-funded genesis seed for the standalone (`Undeployed`) Midnight
/// stack. Mirrors `GENESIS_MINT_WALLET_SEED` from
/// `midnightntwrk/example-counter/counter-cli/src/cli.ts` and the
/// upstream identity-examples standalone environments. The dev
/// chainspec (`CFG_PRESET=dev`) mints both NIGHT and DUST to this
/// wallet at genesis, so [`Wallet::demo`] auto-loads it whenever the
/// active network is [`crate::Network::Undeployed`] — that's the
/// only way to drive `Wallet::create_did` etc. against the local
/// stack without a manual top-up.
pub const UNDEPLOYED_GENESIS_SEED_HEX: &str =
    "0000000000000000000000000000000000000000000000000000000000000001";

#[derive(Debug, thiserror::Error)]
pub enum WalletError {
    #[error("invalid seed length: expected 32 bytes, got {0}")]
    InvalidSeedLen(usize),
    #[error("hex decode: {0}")]
    Hex(#[from] hex::FromHexError),
    #[error("serialize: {0}")]
    Serialize(#[from] std::io::Error),
    #[error("address: {0}")]
    Address(String),
    /// The `create_did` `WizardStage` stream emitted a `Failed(_)`
    /// stage, or terminated without ever reaching `Done` / `Failed`.
    /// Produced exclusively by the awaitable wrappers
    /// ([`Wallet::create_did_awaitable`],
    /// [`Wallet::add_verification_method`],
    /// [`Wallet::add_verification_method_relation`]) — the underlying
    /// stream itself just yields `WizardStage::Failed(String)`.
    #[error("create_did failed: {0}")]
    CreateDidFailed(String),
}

/// DUST + NIGHT balance snapshot, in atomic units. Returned
/// by [`Wallet::balance_snapshot`]; consumers compute the
/// per-flow cost as `before − after` after a wizard pipeline
/// reaches `Done`.
///
/// Notes for callers:
/// - **DUST accrues over time.** Dust generators inside the
///   chain pay out continuously from the wallet's NIGHT
///   holdings, so two snapshots taken seconds apart with no
///   transactions between them will see a small positive
///   delta. UI consumers should clamp `before − after` at
///   `0` rather than render negative costs.
/// - **NIGHT only changes on transactions.** Deltas here are
///   the actual NIGHT spent (or received) by the wallet
///   between snapshots.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BalanceSnapshot {
    /// DUST atomic units (10^-15 DUST). Equivalent to
    /// `DustLocalState::wallet_balance(now)` at the snapshot
    /// instant.
    pub dust_atomic: u128,
    /// NIGHT atomic units (10^-6 NIGHT). Sum of every UTXO
    /// in the wallet's unshielded set.
    pub night_atomic: u128,
}

/// Claim policy a locker pins when calling `createLock` on the
/// multi-lock passport vault. Mirrors the `createLock` circuit's
/// policy arguments (`passport-vault.compact`). For the initial UI
/// only `min_age` + `max_claim` are surfaced; the issuing-state /
/// document-number gates default to disabled.
#[derive(Clone, Debug)]
pub struct VaultLockPolicy {
    pub min_age: u8,
    pub require_issuing_state: bool,
    pub required_issuing_state: [u8; 32],
    pub require_document_number: bool,
    pub required_document_number: [u8; 32],
    pub max_claim: u128,
    pub verifier_challenge_hash: [u8; 32],
}

/// Result of a successful `createLock`: the submitted tx hash plus the
/// lock id the contract assigned (read from `lockCount` immediately
/// before the call — `createLock` returns the pre-increment counter).
#[derive(Clone, Debug)]
pub struct VaultCreateLockOutcome {
    pub tx_hash: String,
    pub lock_id: u64,
}

/// A bare wallet — seed + derived keys + which network it talks to.
/// Iter-1 step-1 has no balance, no UTXOs, no sync; that's iter-2.
pub struct Wallet {
    network: Network,
    keys: SecretKeys,
    seed_bytes: [u8; 32],
    /// Optional `midnight-proof-server` base URL (e.g.
    /// `http://127.0.0.1:57610`). When set, `call_did_circuit`,
    /// `create_did`, and `load_did_circuit` route per-preimage
    /// proving through that server's `/prove` endpoint via
    /// `HttpProvingProvider`. When `None`, the in-process
    /// `zkir_v2::LocalProvingProvider` runs the proofs locally —
    /// fine for standalone tests, much slower for PreProd writes
    /// from a debug-built wallet.
    proof_server_url: Option<String>,
    /// Optional persisted DUST snapshot + indexer-resume bridge.
    /// When attached, `sync_dust()` consults the cached state
    /// (loaded from redb's `DUST_SYNC` table) and only catches
    /// up the delta from `last_id + 1`; full replay of PreProd's
    /// ~534k events only happens once, on the first cold sync.
    /// When `None`, `sync_dust` falls back to the original full-
    /// replay path — fine for tests, prohibitively slow for
    /// PreProd repeat operations. See `BACKLOG.md` Path B for
    /// the design.
    dust_syncer: Option<std::sync::Arc<crate::dust::syncer::DustSyncer>>,
    /// Optional persisted SHIELDED (zswap) syncer. When present,
    /// `sync_shielded()` delta-syncs + caches the wallet's spendable
    /// coins (the deposit balancer spends from these). Same lifecycle
    /// as `dust_syncer`: the App constructs it (it has the store) and
    /// attaches it via `with_shielded_syncer`.
    shielded_syncer: Option<std::sync::Arc<crate::shielded::ShieldedSyncer>>,
    /// Optional WebView-side JS bridge handle. When present,
    /// `call_did_circuit` routes `prepareUnprovenCallTx` through
    /// this bridge instead of spawning a `NodeChildBridge` child
    /// process. The dioxus-wallet App installs a
    /// `DioxusEvalBridge` here at startup so DID write flows work
    /// on Android (no Node.js in the APK) and on desktop without
    /// the harness-installation prerequisite. `None` falls back to
    /// the `NodeChildBridge::spawn` path — fine for desktop
    /// `cargo test`, fails fast on Android.
    js_bridge: Option<std::sync::Arc<dyn JsBridge>>,
    /// Injected indexer client. When `None` (the default), every
    /// chain-op method builds a fresh `HttpIndexerClient` via
    /// `crate::chain::default_indexer(network)` — the original
    /// pre-Task 1.5.B behaviour. When `Some`, the injected client
    /// is reused for every call, which is how Task 1.5.D's
    /// `stub_wallet` factory swaps the live indexer for an
    /// in-memory mock without touching any of the stream bodies.
    /// `Arc<dyn _>` so multiple `Wallet`s (re-built per stream
    /// inside `create_did` etc.) share one trait object.
    indexer: Option<Arc<dyn chain::IndexerClient>>,
    /// Injected node client. When `None` (the default), the
    /// submit-stage of every chain-op stream builds a fresh
    /// `SubxtNodeClient` via `SubxtNodeClient::connect(network)` —
    /// the original pre-Task 1.5.B behaviour. When `Some`, the
    /// injected client is reused so Task 1.5.D's `stub_wallet`
    /// can return canned `SubmitResult`s without binding a WS
    /// connection.
    node: Option<Arc<dyn chain::NodeClient>>,
    /// Injected prover. When `None` (the default), the
    /// prove-stage of every chain-op stream selects between
    /// `LocalProver` and `HttpProver` based on the wallet's
    /// `proof_server_url`. When `Some`, the injected prover is
    /// used unconditionally — Task 1.5.D's `stub_wallet` plugs in
    /// a `StubProver` that returns a canned `ProvenTx` without
    /// running halo2 (which would dwarf the rest of the pipeline
    /// in test wall-clock terms).
    prover: Option<Arc<dyn chain::Prover>>,
    /// Stub-mode shared DID-document map. When `Some`, the four
    /// public awaitable DID methods (`create_did_awaitable`,
    /// `add_verification_method`, `add_verification_method_relation`,
    /// `resolve_did`) short-circuit to this in-memory map instead
    /// of driving the live chain-op pipeline (JS bridge / dust sync /
    /// balance / prove / submit). When `None` (the default real-deps
    /// path), the methods behave exactly as before.
    ///
    /// Set exclusively by `test_support::stub_wallet`; gated behind
    /// `#[cfg(any(test, feature = "test-support"))]` so the field
    /// + accessor + bypass branches do not appear in release builds.
    #[cfg(any(test, feature = "test-support"))]
    stub_did_state: Option<crate::test_support::StubDidMap>,
}

/// Stub-only mirror of the upstream's `absoluteDidUrlReference`
/// (`midnight-did/packages/did/src/ledger-to-domain.ts`): turn a
/// stored fragment / relative reference into the canonical
/// absolute DID URL form (`did:method:net:addr#fragment`).
///
/// - Already absolute (`did:…#fragment`) → returned as-is.
/// - Hash-prefixed bare fragment (`#key-auth`) → prefix with the
///   owning DID (`did:…#key-auth`, single `#`).
/// - Bare fragment (`key-auth`) → same prefix.
///
/// Stays in this module rather than reusing
/// `did::contract::ledger_to_domain`'s inner helper because the
/// stub path bypasses `ledger_to_domain` entirely.
/// Convert a 32-byte big-endian unsigned integer into its
/// base-10 ASCII representation. Used by the
/// `setSchnorrJubjubVerificationMethod` call builder to turn the
/// raw `(x, y)` Jubjub coordinates into the
/// `{ "$bigint": "<decimal>" }` shape the JS harness's
/// `reviveBigints` recognises — the Compact runtime's
/// `CompactTypeJubjubPoint.toValue` consumes the resulting
/// `BigInt` directly. Hand-rolled to avoid pulling in
/// `num-bigint` for a single conversion site.
fn be32_to_decimal_string(be: &[u8; 32]) -> String {
    // Base-10 long division by repeated subtraction of 10⁹ via
    // `u32` chunks. The 256-bit value is held as eight 32-bit
    // limbs (most significant first). On each iteration we take
    // the limb array, divide by 10⁹, emit nine digits (zero-
    // padded except for the leading chunk), continue until the
    // value is zero.
    let mut limbs = [0u32; 8];
    for i in 0..8 {
        let lo = i * 4;
        limbs[i] = u32::from_be_bytes([be[lo], be[lo + 1], be[lo + 2], be[lo + 3]]);
    }
    if limbs.iter().all(|l| *l == 0) {
        return "0".to_string();
    }
    let mut chunks: Vec<u32> = Vec::new();
    while !limbs.iter().all(|l| *l == 0) {
        let mut rem: u64 = 0;
        for limb in limbs.iter_mut() {
            let cur = (rem << 32) | (*limb as u64);
            *limb = (cur / 1_000_000_000) as u32;
            rem = cur % 1_000_000_000;
        }
        chunks.push(rem as u32);
    }
    let mut out = String::new();
    if let Some(top) = chunks.pop() {
        out.push_str(&top.to_string());
    }
    while let Some(c) = chunks.pop() {
        out.push_str(&format!("{c:09}"));
    }
    out
}

/// URL-safe base64 (no padding) of a 32-byte slice. Lives next
/// to `be32_to_decimal_string` so the stub path of
/// `set_schnorr_jubjub_verification_method` can synthesize a JWK
/// envelope matching the shape `ledger_to_domain` emits on the
/// real path. Gated behind the stub feature flags since the
/// real-deps path doesn't need it.
#[cfg(any(test, feature = "test-support"))]
fn be32_to_base64url(be: &[u8; 32]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(be)
}

#[cfg(any(test, feature = "test-support"))]
fn stub_to_absolute_did_url(did: &crate::DidId, stored: &str) -> String {
    let did_str = did.to_did_string();
    if stored.starts_with("did:") {
        stored.to_string()
    } else if let Some(frag) = stored.strip_prefix('#') {
        format!("{did_str}#{frag}")
    } else {
        format!("{did_str}#{stored}")
    }
}

impl Wallet {
    /// Build from a raw 32-byte seed. Mirrors gsd-wallet's seed semantics
    /// (the seed *is* the wallet identity; no BIP39 yet).
    pub fn from_seed(seed: [u8; 32], network: Network) -> Self {
        // Shielded (zswap) keys derive from the HD `Zswap` child
        // (`m/44'/2400'/0'/3/0`), matching the Midnight wallet SDK —
        // NOT the raw seed. Deriving from the raw seed produces a
        // different coin/encryption key, so coins the SDK encrypted
        // on-chain never decrypt (the deposit's `coins=0` bug). Fall
        // back to the raw seed only if HD derivation fails, which never
        // happens for a valid 32-byte seed.
        let zswap_seed =
            crate::hd::derive_child_priv(&seed, 0, crate::hd::Role::Zswap, 0).unwrap_or(seed);
        let keys = SecretKeys::from(Seed::from(zswap_seed));
        Self {
            network,
            keys,
            seed_bytes: seed,
            proof_server_url: None,
            dust_syncer: None,
            shielded_syncer: None,
            js_bridge: None,
            indexer: None,
            node: None,
            prover: None,
            #[cfg(any(test, feature = "test-support"))]
            stub_did_state: None,
        }
    }

    /// Builder: attach a stub DID-document map. Used **only** by
    /// `test_support::stub_wallet` to enable the wallet-level
    /// short-circuit on the four awaitable DID methods. The
    /// `Arc<Mutex<HashMap>>` is shared with the wallet's injected
    /// [`crate::test_support::StubIndexerClient`], so mutations made
    /// here are immediately visible through `IndexerClient` and
    /// vice-versa.
    #[cfg(any(test, feature = "test-support"))]
    pub fn with_stub_did_state(mut self, state: crate::test_support::StubDidMap) -> Self {
        self.stub_did_state = Some(state);
        self
    }

    /// Test-only inspector: is this wallet in stub mode? Used by
    /// the four awaitable methods to branch into the in-memory
    /// short-circuit. Public only inside the crate.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn stub_did_state(&self) -> Option<crate::test_support::StubDidMap> {
        self.stub_did_state.clone()
    }

    /// Builder: attach a WebView-side JS bridge. Subsequent
    /// `call_did_circuit` invocations will route the JS half of the
    /// pipeline (`prepareUnprovenCallTx`) through this handle
    /// instead of spawning a `NodeChildBridge`. The dioxus-wallet
    /// App constructs a `DioxusEvalBridge`, wraps it in `Arc`, and
    /// attaches it to every wallet it builds.
    pub fn with_js_bridge(
        mut self,
        bridge: std::sync::Arc<dyn JsBridge>,
    ) -> Self {
        self.js_bridge = Some(bridge);
        self
    }

    /// Currently-configured JS bridge handle, if any. Cheap
    /// `Arc::clone` — multiple wallets can share the same bridge.
    pub fn js_bridge(&self) -> Option<std::sync::Arc<dyn JsBridge>> {
        self.js_bridge.clone()
    }

    /// Builder: inject an `IndexerClient` trait object. When set,
    /// every chain-op method (`create_did`, `load_did_circuit`,
    /// `call_did_circuit`, `resolve_did`, `resolve_did_full`,
    /// `sync_dust` fallback) consults the injected client instead
    /// of constructing a fresh `HttpIndexerClient`. Used by Task
    /// 1.5.D's `stub_wallet` to drive the pipeline against an
    /// in-memory indexer mock.
    pub fn with_indexer(mut self, indexer: Arc<dyn chain::IndexerClient>) -> Self {
        self.indexer = Some(indexer);
        self
    }

    /// Currently-configured indexer trait object, if any. `None`
    /// means the wallet will build a fresh `HttpIndexerClient` on
    /// each chain-op invocation (the default real-deps path).
    pub fn indexer(&self) -> Option<Arc<dyn chain::IndexerClient>> {
        self.indexer.clone()
    }

    /// Resolve the active indexer: the injected one if any, else
    /// a freshly-built `HttpIndexerClient`. This is the single
    /// seam through which every chain-op method in the file
    /// reaches the indexer.
    fn resolve_indexer(&self) -> Result<Arc<dyn chain::IndexerClient>, crate::indexer::IndexerError> {
        match self.indexer.clone() {
            Some(i) => Ok(i),
            None => chain::default_indexer(self.network),
        }
    }

    /// Builder: inject a `NodeClient` trait object. When set,
    /// the submit-stage of every chain-op stream consults the
    /// injected client instead of opening a fresh
    /// `SubxtNodeClient::connect`. Used by Task 1.5.D's
    /// `stub_wallet` to short-circuit the WS submit with a
    /// canned `SubmitResult`.
    pub fn with_node(mut self, node: Arc<dyn chain::NodeClient>) -> Self {
        self.node = Some(node);
        self
    }

    /// Currently-configured node trait object, if any. `None`
    /// means the wallet will open a fresh `SubxtNodeClient` on
    /// each submit (the default real-deps path).
    pub fn node(&self) -> Option<Arc<dyn chain::NodeClient>> {
        self.node.clone()
    }

    /// Resolve the active node client: the injected one if any,
    /// else a freshly-connected `SubxtNodeClient`. Async because
    /// the default real-deps path opens a WebSocket.
    async fn resolve_node(&self) -> Result<Arc<dyn chain::NodeClient>, crate::node::NodeError> {
        match self.node.clone() {
            Some(n) => Ok(n),
            None => Ok(Arc::new(crate::SubxtNodeClient::connect(self.network).await?)),
        }
    }

    /// Builder: inject a `Prover` trait object. When set, the
    /// prove-stage of every chain-op stream uses the injected
    /// prover unconditionally — `proof_server_url` is ignored.
    /// Used by Task 1.5.D's `stub_wallet` to short-circuit halo2
    /// in tests.
    pub fn with_prover(mut self, prover: Arc<dyn chain::Prover>) -> Self {
        self.prover = Some(prover);
        self
    }

    /// Builder: wrap whichever indexer + prover are currently
    /// installed (or the default real-deps impls if none are set
    /// yet) with the metered decorators
    /// [`crate::MeteredIndexerClient`] / [`crate::MeteredProver`].
    /// Subsequent chain-op invocations record per-call timings
    /// + RSS/CPU deltas through the supplied `metrics` sink.
    ///
    /// `NodeClient` metering isn't wired here yet because the
    /// default `SubxtNodeClient::connect` is async (it opens a
    /// WebSocket). Wrap the node manually via
    /// `.with_node(MeteredNodeClient::new(...))` if the caller
    /// already has an `Arc<dyn NodeClient>` in hand.
    pub fn with_metering(
        self,
        metrics: Arc<dyn crate::Metrics>,
        probe: Arc<dyn crate::ResourceProbe>,
    ) -> Result<Self, crate::indexer::IndexerError> {
        let indexer = match self.indexer.clone() {
            Some(i) => i,
            None => chain::default_indexer(self.network)?,
        };
        let prover = match self.prover.clone() {
            Some(p) => p,
            None => chain::default_prover(self.proof_server_url.as_deref()),
        };
        let metered_indexer: Arc<dyn chain::IndexerClient> = Arc::new(
            crate::MeteredIndexerClient::new(indexer, metrics.clone(), probe.clone()),
        );
        let metered_prover: Arc<dyn chain::Prover> =
            Arc::new(crate::MeteredProver::new(prover, metrics, probe));
        Ok(self.with_indexer(metered_indexer).with_prover(metered_prover))
    }

    /// Currently-configured prover trait object, if any. `None`
    /// means the wallet picks between `LocalProver` and
    /// `HttpProver` based on `proof_server_url`.
    pub fn prover(&self) -> Option<Arc<dyn chain::Prover>> {
        self.prover.clone()
    }

    /// Resolve the active prover: the injected one if any, else
    /// `chain::default_prover(self.proof_server_url.as_deref())`
    /// (which is `LocalProver` when no URL is set and `HttpProver`
    /// when one is).
    fn resolve_prover(&self) -> Arc<dyn chain::Prover> {
        match self.prover.clone() {
            Some(p) => p,
            None => chain::default_prover(self.proof_server_url.as_deref()),
        }
    }

    /// Canonical dependency-injection constructor. Builds a
    /// wallet from raw seed bytes + the three chain-op trait
    /// objects (indexer / node / prover). Equivalent to
    /// `Wallet::from_seed(seed, network)
    ///     .with_indexer(indexer)
    ///     .with_node(node)
    ///     .with_prover(prover)`.
    ///
    /// Task 1.5.D's `stub_wallet` factory composes this with the
    /// three in-memory `Stub*` impls to spin up a wallet that
    /// drives the entire chain-op pipeline deterministically in
    /// a single process, without a live indexer, node, or
    /// proof-server. The existing `from_seed` / `demo` /
    /// `new_random` / `from_chacha_seed` constructors continue to
    /// produce real-deps wallets (Option fields stay `None`,
    /// `resolve_*` builds the live impls on demand).
    pub fn with_deps(
        seed: [u8; 32],
        network: Network,
        indexer: Arc<dyn chain::IndexerClient>,
        node: Arc<dyn chain::NodeClient>,
        prover: Arc<dyn chain::Prover>,
    ) -> Self {
        Self::from_seed(seed, network)
            .with_indexer(indexer)
            .with_node(node)
            .with_prover(prover)
    }

    /// Builder: attach a `midnight-proof-server` base URL. Subsequent
    /// `create_did` / `call_did_circuit` / `load_did_circuit` calls
    /// will route proving through that server instead of running the
    /// in-process zkir prover. See [`Self::proof_server_url`] for the
    /// rationale.
    pub fn with_proof_server_url(mut self, url: impl Into<String>) -> Self {
        self.proof_server_url = Some(url.into());
        self
    }

    /// Builder: attach a persisted DUST syncer. `sync_dust()` will
    /// then consult the cache + delta-sync instead of doing a full
    /// indexer replay every call. The App constructs the syncer
    /// (it has the `WalletStore`) and attaches it here. Tests
    /// without a store leave this `None` and pay the full-replay
    /// cost — fine because the standalone stack has few events.
    pub fn with_dust_syncer(
        mut self,
        syncer: std::sync::Arc<crate::dust::syncer::DustSyncer>,
    ) -> Self {
        self.dust_syncer = Some(syncer);
        self
    }

    /// Currently-configured proof-server URL, if any.
    pub fn proof_server_url(&self) -> Option<&str> {
        self.proof_server_url.as_deref()
    }

    /// Currently-configured DUST syncer, if any.
    pub fn dust_syncer(
        &self,
    ) -> Option<&std::sync::Arc<crate::dust::syncer::DustSyncer>> {
        self.dust_syncer.as_ref()
    }

    /// Builder: attach a persisted SHIELDED (zswap) syncer. The deposit
    /// path calls `sync_shielded()` to get the wallet's spendable coins
    /// + commitment Merkle tree. Same lifecycle as `with_dust_syncer`.
    pub fn with_shielded_syncer(
        mut self,
        syncer: std::sync::Arc<crate::shielded::ShieldedSyncer>,
    ) -> Self {
        self.shielded_syncer = Some(syncer);
        self
    }

    /// Currently-configured SHIELDED syncer, if any.
    pub fn shielded_syncer(
        &self,
    ) -> Option<&std::sync::Arc<crate::shielded::ShieldedSyncer>> {
        self.shielded_syncer.as_ref()
    }

    /// Sync the wallet's SHIELDED (zswap) note state. Requires a
    /// `ShieldedSyncer` to have been attached (the App does this from
    /// the store); without one this errors, since a full from-scratch
    /// shielded replay needs the redb checkpoint to be tractable on
    /// PreProd. Returns the synced `zswap::local::State` — spendable
    /// coins + the commitment Merkle tree the deposit spends against.
    pub async fn sync_shielded(
        &self,
    ) -> Result<zswap::local::State<storage::DefaultDB>, crate::shielded::ShieldedError> {
        match self.shielded_syncer.clone() {
            Some(syncer) => syncer.sync().await,
            None => Err(crate::shielded::ShieldedError::Store(
                "no shielded syncer attached (the App wires one from the wallet store)".into(),
            )),
        }
    }

    /// Demo wallet — uses [`UNDEPLOYED_GENESIS_SEED_HEX`] when the
    /// network targets the standalone chain (either
    /// [`Network::Undeployed`] or [`Network::UndeployedYurii`], both
    /// of which point at the same docker-compose env and therefore
    /// share the funded genesis seed), and the public-knowledge
    /// [`DEMO_SEED_HEX`] for every other network (where there's no
    /// funding implication). Both are stable across launches so the
    /// dev UI shows the same public keys.
    pub fn demo(network: Network) -> Self {
        let seed = if network.is_undeployed() {
            UNDEPLOYED_GENESIS_SEED_HEX
        } else {
            DEMO_SEED_HEX
        };
        Self::from_seed_hex(seed, network)
            .expect("seed constants are 32-byte hex literals")
    }

    /// Generate a fresh wallet from `OsRng`. Seed is also returned via
    /// [`Self::seed_hex`] so the caller can persist it.
    pub fn new_random(network: Network) -> Self {
        let mut seed = [0u8; 32];
        OsRng.fill_bytes(&mut seed);
        Self::from_seed(seed, network)
    }

    /// Deterministic constructor — used in tests and to expose
    /// gsd-wallet's W0–W3 genesis seeds when we add the localnet quick
    /// start in iter-2.
    pub fn from_chacha_seed(rng_seed: u64, network: Network) -> Self {
        let mut rng = ChaCha20Rng::seed_from_u64(rng_seed);
        let mut seed = [0u8; 32];
        rng.fill_bytes(&mut seed);
        Self::from_seed(seed, network)
    }

    pub fn from_seed_hex(seed_hex: &str, network: Network) -> Result<Self, WalletError> {
        let bytes = hex::decode(seed_hex)?;
        let arr: [u8; 32] = bytes
            .try_into()
            .map_err(|v: Vec<u8>| WalletError::InvalidSeedLen(v.len()))?;
        Ok(Self::from_seed(arr, network))
    }

    pub fn network(&self) -> Network {
        self.network
    }

    pub fn seed_hex(&self) -> String {
        hex::encode(self.seed_bytes)
    }

    /// Hex-encoded coin public key. Useful as an "address-ish" display
    /// while we don't yet wire up Midnight's bech32 address format.
    pub fn coin_public_key_hex(&self) -> Result<String, WalletError> {
        let mut buf = Vec::new();
        let pk = self.keys.coin_public_key();
        Serializable::serialize(&pk, &mut buf)?;
        Ok(hex::encode(buf))
    }

    /// Hex-encoded encryption public key (used by ZSwap to encrypt coin
    /// info to this wallet).
    pub fn encryption_public_key_hex(&self) -> Result<String, WalletError> {
        let mut buf = Vec::new();
        let pk = self.keys.enc_public_key();
        Serializable::serialize(&pk, &mut buf)?;
        Ok(hex::encode(buf))
    }

    /// Bech32m-encoded unshielded NIGHT receive address for this
    /// wallet on the chosen network. This is the string the user
    /// pastes into a faucet to top the wallet up.
    pub fn unshielded_address(&self) -> Result<String, WalletError> {
        crate::address::unshielded_bech32m(&self.seed_bytes, self.network)
            .map_err(|e| WalletError::Address(e.to_string()))
    }

    /// The wallet's unshielded `UserAddress` as raw 32-byte payload hex
    /// (SHA-256 of the night verifying key). This is the byte form the
    /// contract's `UserAddress` type expects as an unshielded `sendUnshielded`
    /// recipient — the holder receives the released NIGHT at this address.
    pub fn unshielded_user_address_hex(&self) -> Result<String, WalletError> {
        let sk = self.did_controller_signing_key()?;
        let ua = coin_structure::coin::UserAddress::from(sk.verifying_key());
        Ok(hex::encode(ua.0.0))
    }

    /// Bech32m-encoded shielded NIGHT receive address for this
    /// wallet on the chosen network. Pairs the coin public key
    /// + encryption public key into one HRP-prefixed string —
    /// senders use the coin pk to identify your UTXOs and the
    /// enc pk to encrypt the coin info to you. See
    /// `crate::address::shielded_bech32m` for the byte layout +
    /// upstream-TS caveat.
    pub fn shielded_address(&self) -> Result<String, WalletError> {
        let coin_pk = self.keys.coin_public_key();
        let enc_pk = self.keys.enc_public_key();
        crate::address::shielded_bech32m(&coin_pk, &enc_pk, self.network)
            .map_err(|e| WalletError::Address(e.to_string()))
    }

    /// Snapshot the unshielded UTXO set for this wallet's default
    /// address. See `docs/superpowers/specs/2026-05-14-unshielded-sync-design.md`.
    /// Opens a fresh `graphql-transport-ws` WebSocket to the
    /// indexer, replays UTXO create/spend events, terminates on
    /// the first `Progress` event, and returns the live set.
    pub async fn sync_unshielded(&self) -> Result<crate::UtxoSet, crate::UnshieldedError> {
        let address = self
            .unshielded_address()
            .map_err(|e| crate::UnshieldedError::InvalidAddress(e.to_string()))?;
        let cfg = self.network.config();
        crate::unshielded::snapshot::snapshot(cfg.indexer_ws_url, &address).await
    }

    // BalanceSnapshot lives at module scope right below the
    // impl block so it picks up `pub use` cleanly via lib.rs.

    /// One-shot snapshot of the wallet's DUST + NIGHT
    /// balances. Used to bracket a transaction-pipeline run:
    /// take a snapshot before the flow starts, drive the
    /// `WizardStage` stream, snapshot again on `Done`, and
    /// compute `before − after` to get the per-flow cost.
    ///
    /// Both balances are reported in atomic units (10^-6 NIGHT,
    /// 10^-15 DUST). NIGHT is summed across every UTXO in the
    /// wallet's unshielded set; DUST is the
    /// `wallet_balance(now)` of the freshly-synced
    /// `DustLocalState`. The dust value moves with chain time
    /// (it accrues from the underlying NIGHT generators) so a
    /// no-op snapshot taken seconds later will read slightly
    /// higher; consumers should treat tiny positive deltas as
    /// "no transaction, just accrual" rather than a refund.
    pub async fn balance_snapshot(&self) -> Result<BalanceSnapshot, WalletError> {
        let dust = self
            .sync_dust()
            .await
            .map_err(|e| WalletError::Address(format!("sync_dust: {e}")))?;
        let unshielded = self
            .sync_unshielded()
            .await
            .map_err(|e| WalletError::Address(format!("sync_unshielded: {e}")))?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let now_ts = base_crypto::time::Timestamp::from_secs(now);
        let dust_atomic = dust.wallet_balance(now_ts);
        let night_atomic: u128 = unshielded.iter().fold(0u128, |a, u| a.saturating_add(u.value));
        Ok(BalanceSnapshot { dust_atomic, night_atomic })
    }

    /// Snapshot the wallet's DUST state. When a `DustSyncer` is
    /// attached (App build), drives it to completion — that's
    /// "load persisted cache + sync delta from `last_id + 1` +
    /// persist updated snapshot". When no syncer is attached
    /// (standalone tests), falls back to the original full-replay
    /// path. The returned state is consumed by `tx::balance` to
    /// cover deploy/call fees.
    ///
    /// First call against a fresh cache pays the full-replay
    /// cost once (~10–15 min for PreProd's ~534k events); every
    /// subsequent call is a few-seconds delta sync.
    pub async fn sync_dust(
        &self,
    ) -> Result<crate::DustLocalState<storage::DefaultDB>, crate::DustError> {
        use futures::StreamExt;
        if let Some(syncer) = self.dust_syncer.clone() {
            // Drive the syncer to completion. Drops the progress
            // updates — callers wanting a progress bar drive the
            // syncer directly via `DustSyncer::sync()`.
            let mut stream = std::pin::pin!(syncer.clone().sync());
            while let Some(progress) = stream.next().await {
                progress?;
            }
            // Pull the freshly-persisted state.
            return syncer
                .cached_state()?
                .map(|(state, _)| state)
                .ok_or_else(|| {
                    crate::DustError::Replay(
                        "syncer completed without persisting any state".into(),
                    )
                });
        }
        // Fallback for standalone / no-store builds.
        let sk = self
            .dust_secret_key()
            .map_err(|e| crate::DustError::InvalidPublicKey(e.to_string()))?;
        let cfg = self.network.config();
        let params = ledger::structure::INITIAL_PARAMETERS.dust;
        crate::dust::snapshot::snapshot(cfg.indexer_ws_url, &sk, params).await
    }

    /// Derive the DUST secret key for this wallet.
    ///
    /// The seed feeding `ledger::dust::DustSecretKey::derive_secret_key`
    /// is the BIP44 child at `m/44'/2400'/0'/2/0` (account 0, role
    /// Dust, index 0) — same path the upstream wallet SDKs use.
    pub fn dust_secret_key(&self) -> Result<ledger::dust::DustSecretKey, WalletError> {
        let child = crate::hd::derive_child_priv(&self.seed_bytes, 0, crate::hd::Role::Dust, 0)
            .map_err(|e| WalletError::Address(format!("hd: {e}")))?;
        Ok(ledger::dust::DustSecretKey::derive_secret_key(&child))
    }

    /// Hex-encoded 32-byte DUST public key (little-endian Fr bytes).
    /// Ready to feed as a `HexEncoded` indexer scalar for any
    /// future address-keyed DUST queries — `dustLedgerEvents`
    /// itself is global and doesn't need this, but it's the
    /// natural display form too.
    pub fn dust_public_key_hex(&self) -> Result<String, WalletError> {
        let sk = self.dust_secret_key()?;
        let pk = ledger::dust::DustPublicKey::from(sk);
        Ok(hex::encode(pk.0.as_le_bytes()))
    }

    /// 32-byte raw seed. Returned by-copy to keep callers honest
    /// about it being secret material. `cfg(test)` because it's
    /// only referenced from in-crate tests today; remove the gate
    /// when external callers need it.
    #[cfg(test)]
    pub(crate) fn seed_bytes(&self) -> [u8; 32] {
        self.seed_bytes
    }

    /// Substrate tx-envelope signer. Derives the same secp256k1
    /// secret scalar the DID controller uses (BIP32 path
    /// `m/44'/2400'/0'/0/0`, role `NightExternal`) and exposes it
    /// as an ECDSA-capable signer for `MultiSignature::Ecdsa(_)`.
    /// One key, two signature schemes — see
    /// `wallet_core::node::signer` module-doc.
    pub fn midnight_signer(&self) -> Result<crate::MidnightSigner, WalletError> {
        let secret = crate::hd::derive_child_priv(
            &self.seed_bytes,
            0,
            crate::hd::Role::NightExternal,
            0,
        )
        .map_err(|e| WalletError::Address(e.to_string()))?;
        crate::MidnightSigner::from_secret_bytes(&secret)
            .map_err(|e| WalletError::Address(e.to_string()))
    }

    /// Derive the controller signing key for DID operations.
    ///
    /// Mirrors `midnight-did-api`'s
    /// `HDWallet.fromSeed(seed).selectAccount(0).selectRole(Roles.NightExternal).deriveKeyAt(0)`.
    /// The 32-byte child secret seeds a BIP340 schnorr signing key
    /// whose verifying key (after SHA-256) becomes the
    /// `controllerPublicKey` in the on-chain DID state.
    pub fn did_controller_signing_key(
        &self,
    ) -> Result<base_crypto::signatures::SigningKey, WalletError> {
        let child = crate::hd::derive_child_priv(
            &self.seed_bytes,
            0,
            crate::hd::Role::NightExternal,
            0,
        )
        .map_err(|e| WalletError::Address(e.to_string()))?;
        base_crypto::signatures::SigningKey::from_bytes(&child)
            .map_err(|e| WalletError::Address(format!("signing key: {e}")))
    }

    /// 32-byte commitment the on-chain DID contract stores as
    /// `controllerPublicKey`. The `publicKey` circuit in `did.compact`
    /// is:
    ///
    /// ```compact
    /// publicKey(sk) = persistentHash(["did:controller:pk" + pad32, sk]);
    /// ```
    ///
    /// i.e. `SHA-256("did:controller:pk" + 15 zero bytes || sk)`.
    /// Domain-separated commitment to the secret — the contract
    /// authorises the controller via this pk without learning sk.
    ///
    /// Each DID gets its own random sk (see [`Wallet::create_did`]);
    /// without that sk the wallet cannot update or deactivate the
    /// DID. Mirror in JS: `DIDContract.pureCircuits.publicKey(sk)`.
    pub fn controller_public_key_for(secret: &[u8; 32]) -> [u8; 32] {
        use sha2::{Digest, Sha256};
        let mut domain = [0u8; 32];
        let tag = b"did:controller:pk";
        domain[..tag.len()].copy_from_slice(tag);
        let mut hasher = Sha256::new();
        hasher.update(domain);
        hasher.update(secret);
        hasher.finalize().into()
    }

    /// Compose what the new DID's id *would be* if we deployed
    /// right now, without actually submitting anything. The full
    /// `ContractDeploy` payload is assembled from the wallet's
    /// controller commitment + a current-time `created`/`updated`
    /// stamp + a freshly-sampled 32-byte nonce; the resulting
    /// `deploy.address()` is wrapped as a [`crate::DidId`] on the
    /// wallet's network.
    ///
    /// Useful before submission to (a) verify our state composition
    /// is bit-for-bit what the chain would accept, and (b) show the
    /// would-be DID in the UI so the user knows what address they
    /// would control.
    /// Compose what the new DID's id *would be* given a caller-
    /// supplied pk-commitment, timestamp, and nonce. Each DID gets
    /// a fresh random `controller_sk`, so a wallet-level "what's my
    /// next DID" preview is no longer meaningful — pass the sk in
    /// via `pk_commitment = Wallet::controller_public_key_for(&sk)`
    /// to see the address before submission.
    pub fn create_did_preview_with(
        &self,
        pk_commitment: [u8; 32],
        timestamp_ms: u64,
        nonce: [u8; 32],
    ) -> Result<crate::DidId, crate::DidError> {
        let committee = vec![self
            .did_maintenance_verifying_key()
            .map_err(|e| crate::DidError::Indexer(e.to_string()))?];
        let deploy =
            crate::did::deploy::compose_deploy(pk_commitment, timestamp_ms, nonce, committee);
        let bytes: crate::ContractAddressBytes = deploy.address().0.0;
        Ok(crate::DidId::new(self.network, bytes))
    }

    /// BIP340-Schnorr signing key for the wallet's DID maintenance
    /// authority. Used to:
    ///   - populate the deploy's `ContractMaintenanceAuthority.committee`
    ///     (via [`did_maintenance_verifying_key`])
    ///   - sign `MaintenanceUpdate::data_to_sign()` when loading the
    ///     11 DID circuits' verifier keys post-deploy.
    /// Derived from the BIP-44 child at `m/44'/2400'/0'/0/0` —
    /// the same `NightExternal` path the unshielded address uses, so
    /// `unshielded_address` and the maintenance authority are bound
    /// to the same secret.
    pub fn did_maintenance_signing_key(
        &self,
    ) -> Result<base_crypto::signatures::SigningKey, WalletError> {
        let child = crate::hd::derive_child_priv(
            &self.seed_bytes,
            0,
            crate::hd::Role::NightExternal,
            0,
        )
        .map_err(|e| WalletError::Address(format!("hd: {e}")))?;
        base_crypto::signatures::SigningKey::from_bytes(&child)
            .map_err(|e| WalletError::Address(format!("schnorr sk: {e}")))
    }

    /// BIP340-Schnorr verifying key that goes into the contract's
    /// `ContractMaintenanceAuthority.committee` slot at deploy time.
    pub fn did_maintenance_verifying_key(
        &self,
    ) -> Result<base_crypto::signatures::VerifyingKey, WalletError> {
        Ok(self.did_maintenance_signing_key()?.verifying_key())
    }

    /// Submit a real deploy of the DID contract. Returns a
    /// `Stream<WizardStage>` so the UI renders progress.
    /// See docs/superpowers/specs/2026-05-15-did-deploy-submit-design.md.
    pub fn create_did(
        &self,
    ) -> impl futures::Stream<Item = crate::WizardStage> + Send + 'static {
        let network = self.network;
        let seed_bytes = self.seed_bytes;
        let proof_server_url = self.proof_server_url.clone();
        let dust_syncer = self.dust_syncer.clone();
        let indexer_dep = self.indexer.clone();
        let node_dep = self.node.clone();
        let prover_dep = self.prover.clone();
        async_stream::stream! {
            // 1. SyncingDust
            yield crate::WizardStage::SyncingDust;
            let mut wallet = Wallet::from_seed(seed_bytes, network);
            if let Some(url) = proof_server_url.clone() { wallet = wallet.with_proof_server_url(url); }
            if let Some(s) = dust_syncer.clone() { wallet = wallet.with_dust_syncer(s); }
            if let Some(i) = indexer_dep.clone() { wallet = wallet.with_indexer(i); }
            if let Some(n) = node_dep.clone() { wallet = wallet.with_node(n); }
            if let Some(p) = prover_dep.clone() { wallet = wallet.with_prover(p); }
            let mut dust_state = match wallet.sync_dust().await {
                Ok(s) => s,
                Err(e) => { yield crate::WizardStage::Failed(format!("sync dust: {e}")); return; }
            };
            let dust_key = match wallet.dust_secret_key() {
                Ok(k) => k,
                Err(e) => { yield crate::WizardStage::Failed(format!("dust key: {e}")); return; }
            };

            // 2. Composing
            yield crate::WizardStage::Composing;
            // StdRng (not ThreadRng) so the returned Stream is Send.
            let mut rng = <rand::rngs::StdRng as rand::SeedableRng>::from_entropy();
            // Generate a fresh random controller secret per DID. The
            // wallet must persist this — without it we cannot supply
            // the `localSecretKey()` witness for any subsequent
            // update / deactivate circuit call on this DID.
            let controller_sk: [u8; 32] = rand::Rng::r#gen(&mut rng);
            let pk_commitment = Wallet::controller_public_key_for(&controller_sk);
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            let ttl = base_crypto::time::Timestamp::from_secs(now_ms / 1000 + 3600);
            tracing::debug!(
                now_s = now_ms / 1000,
                ttl_s = now_ms / 1000 + 3600,
                "create_did: intent ttl",
            );
            let nonce: [u8; 32] = rand::Rng::r#gen(&mut rng);
            let net_id = network.config().network_id;
            let maintenance_vk = match wallet.did_maintenance_verifying_key() {
                Ok(vk) => vk,
                Err(e) => {
                    yield crate::WizardStage::Failed(format!("maintenance key: {e}"));
                    return;
                }
            };
            let unproven = match crate::tx::build::build_deploy(
                pk_commitment,
                net_id,
                now_ms,
                nonce,
                ttl,
                &mut rng,
                vec![maintenance_vk],
            ) {
                Ok(t) => t,
                Err(e) => { yield crate::WizardStage::Failed(format!("compose: {e}")); return; }
            };

            // Preview DID id from the exact inputs we just used.
            let preview_id = match wallet.create_did_preview_with(pk_commitment, now_ms, nonce) {
                Ok(id) => id,
                Err(e) => { yield crate::WizardStage::Failed(format!("preview id: {e}")); return; }
            };

            // 3. Balancing
            //
            // Same tip-params dance as `call_did_circuit` (see
            // wallet.rs:1325-1394 for the long-form rationale).
            // This path used to take `INITIAL_PARAMETERS`
            // unconditionally; that broke against any chain whose
            // live `LedgerParameters` had drifted from the wallet
            // crate's hardcoded constant — the balanced tx's
            // `v_fee` no longer matched what the chain rederives
            // during validation, so submit failed with
            // `Invalid Transaction (1010)`. Reproduces every time
            // against a freshly-booted standalone env post 2026-05-29
            // ledger upgrade.
            yield crate::WizardStage::Balancing;
            let tip_params_hex_balance = match wallet.resolve_indexer() {
                Ok(c) => match c.chain_tip().await {
                    Ok(Some(t)) => t.ledger_parameters_hex,
                    _ => None,
                },
                Err(_) => None,
            };
            let parsed_tip_params_balance: Option<ledger::structure::LedgerParameters> =
                tip_params_hex_balance.as_ref().and_then(|h| {
                    let bytes = hex::decode(h.trim_start_matches("0x")).ok()?;
                    serialize::tagged_deserialize(&bytes[..]).ok()
                });
            let initial_params_fallback_balance = ledger::structure::INITIAL_PARAMETERS;
            let used_tip = parsed_tip_params_balance.is_some();
            let params: &ledger::structure::LedgerParameters =
                parsed_tip_params_balance
                    .as_ref()
                    .unwrap_or(&initial_params_fallback_balance);
            tracing::warn!(
                target: "params-debug",
                used_tip_params = used_tip,
                tip_hex_len = tip_params_hex_balance.as_ref().map(|s| s.len()).unwrap_or(0),
                "wallet params source for deploy/maintenance"
            );
            // Pick a `ctime` the verifier will accept and that
            // matches our local commitment_tree.root().
            //
            // The verifier's `commitment_root` is looked up via
            // `root_history.get(ctime)` (exact match else predecessor;
            // entries inserted on every `post_block_update`). The
            // value must equal the root our local tree holds.
            //
            // Two constraints stacked:
            // 1. Validity window: `tblock - 3h ≤ ctime ≤ tblock` (the
            //    chain's `OutOfDustValidityWindow` check).
            // 2. Root match: between the most-recent block whose
            //    `root_history` we agree with and `ctime`, no
            //    dust events advanced the tree.
            //
            // On a wallet whose `sync_time` is recent (every block
            // generates a fresh DUST event for the holder),
            // `sync_time` itself satisfies both. After a chain
            // reset or for a wallet with only genesis allocation,
            // `sync_time` is stuck at block 0's tblock — way
            // outside the 3-hour window. Falling back to the
            // chain tip's tblock keeps us inside the window; the
            // root match holds because no dust events have
            // advanced our local tree (single-tenant standalone).
            let chain_tip_secs: u64 = match wallet.resolve_indexer() {
                Ok(c) => match c.chain_tip().await {
                    Ok(Some(t)) => (t.timestamp_unix as u64) / 1000,
                    _ => 0,
                },
                Err(_) => 0,
            };
            let ctime = if chain_tip_secs > dust_state.sync_time.to_secs() as u64 {
                base_crypto::time::Timestamp::from_secs(chain_tip_secs)
            } else {
                dust_state.sync_time
            };
            let mut ctx = crate::tx::balance::BalanceCtx {
                dust_state: &mut dust_state,
                dust_key: &dust_key,
                params: &params,
                time: ctime,
                ttl,
                network_id: net_id,
            };
            let balanced = match crate::tx::balance::balance(unproven, &mut ctx) {
                Ok(b) => b,
                Err(e) => { yield crate::WizardStage::Failed(format!("balance: {e}")); return; }
            };

            // 4. Proving
            yield crate::WizardStage::Proving;
            // Trait-routed: real-deps wallets resolve to
            // `LocalProver` / `HttpProver` based on
            // `proof_server_url` (the pre-refactor selection
            // logic, now inside `resolve_prover`); test wallets
            // can inject a stub via `Wallet::with_prover`.
            let proven = match wallet.resolve_prover().prove(balanced).await {
                Ok(p) => p,
                Err(e) => { yield crate::WizardStage::Failed(format!("prove: {e}")); return; }
            };

            // 5. Submitting
            yield crate::WizardStage::Submitting;
            let bytes = match crate::tx::scale::scale_encode(&proven) {
                Ok(b) => b,
                Err(e) => { yield crate::WizardStage::Failed(format!("encode: {e}")); return; }
            };
            let signer = match wallet.midnight_signer() {
                Ok(s) => s,
                Err(e) => { yield crate::WizardStage::Failed(format!("signer: {e}")); return; }
            };
            let node = match wallet.resolve_node().await {
                Ok(n) => n,
                Err(e) => { yield crate::WizardStage::Failed(format!("node connect: {e}")); return; }
            };

            // 6. Confirming
            yield crate::WizardStage::Confirming;
            let submit = match node.submit_deploy(bytes, &signer).await {
                Ok(r) => r,
                Err(e) => { yield crate::WizardStage::Failed(format!("submit: {e}")); return; }
            };

            yield crate::WizardStage::Done(crate::DeployOutcome {
                did_id: preview_id,
                tx_hash: submit.tx_hash,
                block_hash: submit.block_hash,
                controller_sk,
            });
        }
    }

    /// Submit a MaintenanceUpdate that loads a single circuit's
    /// verifier key into a freshly-deployed DID contract. Reuses
    /// the same WizardStage pipeline as `create_did` — the Done
    /// outcome carries the maintenance-tx hash + block hash; the
    /// DID id is unchanged.
    ///
    /// `counter` is the maintenance-authority's counter value at
    /// the time of submission. For the first MaintenanceUpdate
    /// against a contract that was just deployed, this is 0;
    /// every successful update bumps it.
    ///
    /// `circuit` selects which bundled artifact to load. Today
    /// only `"setVerificationMethod"` is bundled — the other 10
    /// follow in a subsequent slice once their artifacts land.
    pub fn load_did_circuit(
        &self,
        did_id: crate::DidId,
        circuit_name: String,
        counter: u32,
    ) -> impl futures::Stream<Item = crate::WizardStage> + Send + 'static {
        use coin_structure::contract::ContractAddress;
        use base_crypto::hash::HashOutput;

        let network = self.network;
        let seed_bytes = self.seed_bytes;
        let proof_server_url = self.proof_server_url.clone();
        let dust_syncer = self.dust_syncer.clone();
        let indexer_dep = self.indexer.clone();
        let node_dep = self.node.clone();
        let prover_dep = self.prover.clone();
        async_stream::stream! {
            // 1. SyncingDust
            yield crate::WizardStage::SyncingDust;
            let mut wallet = Wallet::from_seed(seed_bytes, network);
            if let Some(url) = proof_server_url.clone() { wallet = wallet.with_proof_server_url(url); }
            if let Some(s) = dust_syncer.clone() { wallet = wallet.with_dust_syncer(s); }
            if let Some(i) = indexer_dep.clone() { wallet = wallet.with_indexer(i); }
            if let Some(n) = node_dep.clone() { wallet = wallet.with_node(n); }
            if let Some(p) = prover_dep.clone() { wallet = wallet.with_prover(p); }
            let mut dust_state = match wallet.sync_dust().await {
                Ok(s) => s,
                Err(e) => { yield crate::WizardStage::Failed(format!("sync dust: {e}")); return; }
            };
            let dust_key = match wallet.dust_secret_key() {
                Ok(k) => k,
                Err(e) => { yield crate::WizardStage::Failed(format!("dust key: {e}")); return; }
            };

            // 2. Composing
            yield crate::WizardStage::Composing;
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            let ttl = base_crypto::time::Timestamp::from_secs(now_ms / 1000 + 3600);
            let mut rng = <rand::rngs::StdRng as rand::SeedableRng>::from_entropy();
            let net_id = network.config().network_id;

            let sk = match wallet.did_maintenance_signing_key() {
                Ok(k) => k,
                Err(e) => {
                    yield crate::WizardStage::Failed(format!("maintenance key: {e}"));
                    return;
                }
            };

            // Resolve the verifier key for this circuit from the bundled
            // 11-entry registry. Any name outside that set is a config
            // error — surface it before kicking off the network round trip.
            let vk = match crate::did::artifacts::parsed_verifier_key_by_name(&circuit_name) {
                Some(Ok(v)) => v,
                Some(Err(e)) => {
                    yield crate::WizardStage::Failed(format!("parse verifier key: {e}"));
                    return;
                }
                None => {
                    yield crate::WizardStage::Failed(format!(
                        "unknown circuit '{circuit_name}': not in the DID artifact registry",
                    ));
                    return;
                }
            };

            let contract_address = ContractAddress(HashOutput(did_id.contract_address));
            let unproven = match crate::tx::maintain::build_load_verifier_key(
                contract_address,
                &circuit_name,
                vk,
                counter,
                &sk,
                net_id,
                ttl,
                &mut rng,
            ) {
                Ok(t) => t,
                Err(e) => { yield crate::WizardStage::Failed(format!("compose: {e}")); return; }
            };

            // 3. Balancing
            //
            // Same tip-params dance as `call_did_circuit` (see
            // wallet.rs:1325-1394 for the long-form rationale).
            // This path used to take `INITIAL_PARAMETERS`
            // unconditionally; that broke against any chain whose
            // live `LedgerParameters` had drifted from the wallet
            // crate's hardcoded constant — the balanced tx's
            // `v_fee` no longer matched what the chain rederives
            // during validation, so submit failed with
            // `Invalid Transaction (1010)`. Reproduces every time
            // against a freshly-booted standalone env post 2026-05-29
            // ledger upgrade.
            yield crate::WizardStage::Balancing;
            let tip_params_hex_balance = match wallet.resolve_indexer() {
                Ok(c) => match c.chain_tip().await {
                    Ok(Some(t)) => t.ledger_parameters_hex,
                    _ => None,
                },
                Err(_) => None,
            };
            let parsed_tip_params_balance: Option<ledger::structure::LedgerParameters> =
                tip_params_hex_balance.as_ref().and_then(|h| {
                    let bytes = hex::decode(h.trim_start_matches("0x")).ok()?;
                    serialize::tagged_deserialize(&bytes[..]).ok()
                });
            let initial_params_fallback_balance = ledger::structure::INITIAL_PARAMETERS;
            let used_tip = parsed_tip_params_balance.is_some();
            let params: &ledger::structure::LedgerParameters =
                parsed_tip_params_balance
                    .as_ref()
                    .unwrap_or(&initial_params_fallback_balance);
            tracing::warn!(
                target: "params-debug",
                used_tip_params = used_tip,
                tip_hex_len = tip_params_hex_balance.as_ref().map(|s| s.len()).unwrap_or(0),
                "wallet params source for deploy/maintenance"
            );
            // See `create_did` for the full rationale on ctime
            // selection (chain-tip vs sync_time).
            let chain_tip_secs: u64 = match wallet.resolve_indexer() {
                Ok(c) => match c.chain_tip().await {
                    Ok(Some(t)) => (t.timestamp_unix as u64) / 1000,
                    _ => 0,
                },
                Err(_) => 0,
            };
            let ctime = if chain_tip_secs > dust_state.sync_time.to_secs() as u64 {
                base_crypto::time::Timestamp::from_secs(chain_tip_secs)
            } else {
                dust_state.sync_time
            };
            let mut ctx = crate::tx::balance::BalanceCtx {
                dust_state: &mut dust_state,
                dust_key: &dust_key,
                params: &params,
                time: ctime,
                ttl,
                network_id: net_id,
            };
            let balanced = match crate::tx::balance::balance(unproven, &mut ctx) {
                Ok(b) => b,
                Err(e) => { yield crate::WizardStage::Failed(format!("balance: {e}")); return; }
            };

            // 4. Proving
            yield crate::WizardStage::Proving;
            // Trait-routed: real-deps wallets resolve to
            // `LocalProver` / `HttpProver` based on
            // `proof_server_url` (the pre-refactor selection
            // logic, now inside `resolve_prover`); test wallets
            // can inject a stub via `Wallet::with_prover`.
            let proven = match wallet.resolve_prover().prove(balanced).await {
                Ok(p) => p,
                Err(e) => { yield crate::WizardStage::Failed(format!("prove: {e}")); return; }
            };

            // 5. Submitting
            yield crate::WizardStage::Submitting;
            let bytes = match crate::tx::scale::scale_encode(&proven) {
                Ok(b) => b,
                Err(e) => { yield crate::WizardStage::Failed(format!("encode: {e}")); return; }
            };
            let signer = match wallet.midnight_signer() {
                Ok(s) => s,
                Err(e) => { yield crate::WizardStage::Failed(format!("signer: {e}")); return; }
            };
            let node = match wallet.resolve_node().await {
                Ok(n) => n,
                Err(e) => { yield crate::WizardStage::Failed(format!("node connect: {e}")); return; }
            };

            // 6. Confirming
            yield crate::WizardStage::Confirming;
            let submit = match node.submit_deploy(bytes, &signer).await {
                Ok(r) => r,
                Err(e) => { yield crate::WizardStage::Failed(format!("submit: {e}")); return; }
            };

            yield crate::WizardStage::Done(crate::DeployOutcome {
                did_id,
                tx_hash: submit.tx_hash,
                block_hash: submit.block_hash,
                // MaintenanceUpdate does not mint a DID; no fresh sk.
                controller_sk: [0u8; 32],
            });
        }
    }

    /// Invoke a DID circuit on a deployed contract — the *Update*
    /// / *Deactivate* half of CRUD. The Compact circuit body runs
    /// in a Node-driven JS harness (the production WebView path
    /// uses the same code via Dioxus eval); the resulting
    /// `UnprovenTransaction` is balanced, proven, and submitted
    /// natively in Rust through the existing pipeline.
    ///
    /// `controller_sk` is the per-DID random secret the wallet
    /// minted at deploy time (in [`DeployOutcome::controller_sk`]).
    /// Without it, the circuit's `localSecretKey()` witness can't
    /// be supplied and the call fails the contract's controller
    /// assertion.
    ///
    /// `args_json` is a JSON array of circuit arguments. Each is
    /// passed through unchanged to JS — bigints use the
    /// `{ "$bigint": "n" }` tagged form. `[]` for `deactivate`.
    pub fn call_did_circuit(
        &self,
        did_id: crate::DidId,
        circuit: String,
        args_json: serde_json::Value,
        controller_sk: [u8; 32],
    ) -> impl futures::Stream<Item = crate::WizardStage> + Send + 'static {
        use coin_structure::contract::ContractAddress;
        use base_crypto::hash::HashOutput;

        let network = self.network;
        let seed_bytes = self.seed_bytes;
        let proof_server_url = self.proof_server_url.clone();
        let dust_syncer = self.dust_syncer.clone();
        let js_bridge_handle = self.js_bridge.clone();
        let indexer_dep = self.indexer.clone();
        let node_dep = self.node.clone();
        let prover_dep = self.prover.clone();
        async_stream::stream! {
            yield crate::WizardStage::SyncingDust;
            let mut wallet = Wallet::from_seed(seed_bytes, network);
            if let Some(url) = proof_server_url.clone() { wallet = wallet.with_proof_server_url(url); }
            if let Some(s) = dust_syncer.clone() { wallet = wallet.with_dust_syncer(s); }
            if let Some(b) = js_bridge_handle.clone() { wallet = wallet.with_js_bridge(b); }
            if let Some(i) = indexer_dep.clone() { wallet = wallet.with_indexer(i); }
            if let Some(n) = node_dep.clone() { wallet = wallet.with_node(n); }
            if let Some(p) = prover_dep.clone() { wallet = wallet.with_prover(p); }
            let mut dust_state = match wallet.sync_dust().await {
                Ok(s) => s,
                Err(e) => { yield crate::WizardStage::Failed(format!("sync dust: {e}")); return; }
            };
            let dust_key = match wallet.dust_secret_key() {
                Ok(k) => k,
                Err(e) => { yield crate::WizardStage::Failed(format!("dust key: {e}")); return; }
            };

            // 2. Composing — the heavy lift: JS-side Compact runtime
            //    builds the UnprovenTransaction. Rust pulls current
            //    contract state from the indexer first.
            yield crate::WizardStage::Composing;
            let coin_pk_hex = match wallet.coin_public_key_hex() {
                Ok(s) => s,
                Err(e) => { yield crate::WizardStage::Failed(format!("coin pk: {e}")); return; }
            };
            let enc_pk_hex = match wallet.encryption_public_key_hex() {
                Ok(s) => s,
                Err(e) => { yield crate::WizardStage::Failed(format!("encryption pk: {e}")); return; }
            };
            let indexer = match wallet.resolve_indexer() {
                Ok(c) => c,
                Err(e) => { yield crate::WizardStage::Failed(format!("indexer client: {e}")); return; }
            };
            let addr_hex = did_id.contract_address_hex();
            let info = match indexer.contract_state(&addr_hex).await {
                Ok(Some(i)) => i,
                Ok(None) => {
                    yield crate::WizardStage::Failed(format!(
                        "no on-chain state for {addr_hex} — was the DID deployed?",
                    ));
                    return;
                }
                Err(e) => { yield crate::WizardStage::Failed(format!("indexer: {e}")); return; }
            };

            // Prefer the in-process JS bridge (the WebView eval bridge
            // installed by `dioxus-wallet` at app startup). Falls back
            // to spawning a Node child for the harness — fine for
            // desktop `cargo test`, fails fast on Android per the gate
            // in `NodeChildBridge::spawn`. Both implement the same
            // trait so the rest of the flow doesn't branch.
            //
            // `owned_node_bridge` keeps the spawned child alive until
            // the end of the stream — without it, the `&dyn JsBridge`
            // we hand to `call_prepare_unproven` would dangle the
            // moment the `match` block finished.
            let owned_node_bridge: Option<crate::js_bridge::NodeChildBridge> =
                if wallet.js_bridge.is_some() {
                    None
                } else {
                    match crate::js_bridge::NodeChildBridge::spawn(
                        &crate::js_bridge::NodeChildBridge::default_harness_path(),
                    ) {
                        Ok(b) => Some(b),
                        Err(e) => {
                            yield crate::WizardStage::Failed(format!("spawn harness: {e}"));
                            return;
                        }
                    }
                };
            let bridge: &dyn JsBridge = match (wallet.js_bridge.as_ref(), owned_node_bridge.as_ref()) {
                (Some(b), _) => b.as_ref(),
                (None, Some(b)) => b,
                (None, None) => unreachable!(
                    "neither attached JS bridge nor spawned NodeChildBridge — \
                     the spawn-failure branch above bails first",
                ),
            };
            // Plumb the live chain Zswap state + LedgerParameters
            // into the harness AND the Rust balance step. For
            // `LedgerParameters` we want the params at the **chain
            // tip**, not at the contract's last-call block — PreProd
            // retunes parameters over time (cost model, block limits,
            // fee schedule) and:
            //   1. `ledger::construct::partition_transcripts` keys its
            //      guaranteed/fallible split off those params (stale
            //      params land the transcript in `fallible_transcript`,
            //      chain rejects with `Invalid Transaction (1010)`);
            //   2. `tx::balance::balance` computes `v_fee` from the
            //      params' fee schedule (stale params produce a v_fee
            //      that doesn't match what the chain re-derives during
            //      validation, also surfacing as 1010).
            // See `preprod_decode_diff` for the byte-diff that surfaced
            // these and `BACKLOG.md` for Path B's longer-term direction.
            let tip_params_hex = match indexer.chain_tip().await {
                Ok(Some(t)) => t.ledger_parameters_hex,
                _ => None,
            };
            let ledger_params_hex = tip_params_hex.or(info.ledger_parameters_hex);
            let unproven_hex = match call_prepare_unproven(
                bridge,
                did_id.to_did_string(),
                circuit.clone(),
                args_json,
                info.state_hex,
                addr_hex.clone(),
                info.zswap_state_hex,
                ledger_params_hex.clone(),
                controller_sk,
                coin_pk_hex,
                enc_pk_hex,
                network.config().network_id.to_string(),
            )
            .await
            {
                Ok(h) => h,
                Err(e) => { yield crate::WizardStage::Failed(format!("prepare call tx: {e}")); return; }
            };
            let unproven_bytes = match hex::decode(&unproven_hex) {
                Ok(b) => b,
                Err(e) => { yield crate::WizardStage::Failed(format!("hex decode: {e}")); return; }
            };
            let unproven: crate::tx::build::UnprovenTx =
                match serialize::tagged_deserialize(&unproven_bytes[..]) {
                    Ok(t) => t,
                    Err(e) => {
                        yield crate::WizardStage::Failed(format!(
                            "deserialise unproven tx: {e}"
                        ));
                        return;
                    }
                };

            // 3. Balancing — same dust pipeline our deploy uses.
            // Parse the tip params we already fetched above; fall
            // back to `INITIAL_PARAMETERS` only if the hex is absent
            // or fails to decode. The chain re-derives v_fee from
            // these same params during validation; building against
            // stale params produces a v_fee the chain rejects (1010).
            yield crate::WizardStage::Balancing;
            let parsed_tip_params: Option<ledger::structure::LedgerParameters> =
                ledger_params_hex.as_ref().and_then(|h| {
                    let bytes = hex::decode(h.trim_start_matches("0x")).ok()?;
                    serialize::tagged_deserialize(&bytes[..]).ok()
                });
            let initial_params_fallback = ledger::structure::INITIAL_PARAMETERS;
            let params: &ledger::structure::LedgerParameters =
                parsed_tip_params.as_ref().unwrap_or(&initial_params_fallback);
            let now_secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let ttl = base_crypto::time::Timestamp::from_secs(now_secs + 3600);
            // See `create_did` for the full rationale on ctime
            // selection (chain-tip vs sync_time).
            let chain_tip_secs: u64 = match wallet.resolve_indexer() {
                Ok(c) => match c.chain_tip().await {
                    Ok(Some(t)) => (t.timestamp_unix as u64) / 1000,
                    _ => 0,
                },
                Err(_) => 0,
            };
            let ctime = if chain_tip_secs > dust_state.sync_time.to_secs() as u64 {
                base_crypto::time::Timestamp::from_secs(chain_tip_secs)
            } else {
                dust_state.sync_time
            };
            let mut ctx = crate::tx::balance::BalanceCtx {
                dust_state: &mut dust_state,
                dust_key: &dust_key,
                params: &params,
                time: ctime,
                ttl,
                network_id: network.config().network_id,
            };
            let balanced = match crate::tx::balance::balance(unproven, &mut ctx) {
                Ok(b) => b,
                Err(e) => { yield crate::WizardStage::Failed(format!("balance: {e}")); return; }
            };

            // 4. Proving
            yield crate::WizardStage::Proving;
            // Trait-routed: real-deps wallets resolve to
            // `LocalProver` / `HttpProver` based on
            // `proof_server_url` (the pre-refactor selection
            // logic, now inside `resolve_prover`); test wallets
            // can inject a stub via `Wallet::with_prover`.
            let proven = match wallet.resolve_prover().prove(balanced).await {
                Ok(p) => p,
                Err(e) => { yield crate::WizardStage::Failed(format!("prove: {e}")); return; }
            };

            // 5. Submitting
            yield crate::WizardStage::Submitting;
            let scale_bytes = match crate::tx::scale::scale_encode(&proven) {
                Ok(b) => b,
                Err(e) => { yield crate::WizardStage::Failed(format!("encode: {e}")); return; }
            };
            // Diagnostic hook: when `WALLET_CORE_DUMP_TX=<path>` is
            // set, write the encoded tx bytes to disk before submit.
            // Used by the preprod byte-diff investigation to capture
            // both a known-good upstream tx and our own pipeline's
            // output for a side-by-side comparison.
            if let Ok(p) = std::env::var("WALLET_CORE_DUMP_TX") {
                let _ = std::fs::write(&p, hex::encode(&scale_bytes));
                tracing::info!(target: "wallet-core", path = %p, bytes = scale_bytes.len(), "dumped tx bytes pre-submit");
            }
            let signer = match wallet.midnight_signer() {
                Ok(s) => s,
                Err(e) => { yield crate::WizardStage::Failed(format!("signer: {e}")); return; }
            };
            let node = match wallet.resolve_node().await {
                Ok(n) => n,
                Err(e) => { yield crate::WizardStage::Failed(format!("node connect: {e}")); return; }
            };

            // 6. Confirming
            yield crate::WizardStage::Confirming;
            let submit = match node.submit_deploy(scale_bytes, &signer).await {
                Ok(r) => r,
                Err(e) => { yield crate::WizardStage::Failed(format!("submit: {e}")); return; }
            };

            // Resolve placeholder ContractAddress to silence
            // unused-import on the trait if we ever drop the
            // call-site below. Keeps the import live for future
            // intra-call address checks.
            let _: ContractAddress = ContractAddress(HashOutput(did_id.contract_address));

            yield crate::WizardStage::Done(crate::DeployOutcome {
                did_id,
                tx_hash: submit.tx_hash,
                block_hash: submit.block_hash,
                // ContractCall doesn't mint a DID; no fresh sk.
                controller_sk: [0u8; 32],
            });
        }
    }

    /// Invoke a circuit on a deployed **non-DID** Compact contract —
    /// today the passport-vault `depositFunds` (lock) / `claimFunds`
    /// (unlock) / `adminWithdraw` circuits. This is the generic sibling
    /// of [`Wallet::call_did_circuit`]: the Compact circuit body is
    /// composed in JS via `prepareVaultCallTx` (WebView on
    /// mobile/desktop, the Node harness in `cargo test`), and the
    /// resulting `UnprovenTransaction` is balanced, proven, and
    /// submitted natively through the identical Rust pipeline.
    ///
    /// Unlike the DID path there is no per-DID controller secret. The
    /// circuit's private witnesses are supplied via `private_state`:
    /// `claimFunds` needs the holder date-of-birth witness
    /// (`{ holderDateOfBirthDays, holderDateOfBirthOpening }`),
    /// `depositFunds` passes `None` (empty private state).
    ///
    /// `circuit_args` is the JSON arg array (the deposit shielded coin,
    /// or the claim credential + presentation + age proof), encoded
    /// with `{ "$bigint": "n" }` / `{ "$bytes": "0x.." }` markers that
    /// `prepareVaultCallTx`'s `reviveVaultValue` turns back into
    /// `bigint` / `Uint8Array`. Returns the submitted transaction hash
    /// on success.
    ///
    /// NOTE: the caller is responsible for building the args — in
    /// particular the deposit shielded coin (selected from the wallet's
    /// shielded state) and the claim selective-disclosure presentation.
    pub async fn call_contract_circuit(
        &self,
        contract_address_hex: String,
        circuit: String,
        circuit_args: serde_json::Value,
        private_state: Option<serde_json::Value>,
    ) -> Result<String, String> {
        let network = self.network;
        let mut wallet = Wallet::from_seed(self.seed_bytes, network);
        if let Some(url) = self.proof_server_url.clone() {
            wallet = wallet.with_proof_server_url(url);
        }
        if let Some(s) = self.dust_syncer.clone() {
            wallet = wallet.with_dust_syncer(s);
        }
        if let Some(b) = self.js_bridge.clone() {
            wallet = wallet.with_js_bridge(b);
        }
        if let Some(i) = self.indexer.clone() {
            wallet = wallet.with_indexer(i);
        }
        if let Some(n) = self.node.clone() {
            wallet = wallet.with_node(n);
        }
        if let Some(p) = self.prover.clone() {
            wallet = wallet.with_prover(p);
        }

        // 1. Sync DUST (fees are paid in DUST).
        tracing::info!(target: "wallet_core", %circuit, "vault call: syncing dust");
        let mut dust_state = wallet
            .sync_dust()
            .await
            .map_err(|e| format!("sync dust: {e}"))?;
        let dust_key = wallet
            .dust_secret_key()
            .map_err(|e| format!("dust key: {e}"))?;

        // 2. Composing — JS builds the UnprovenTransaction; Rust pulls
        //    current contract + Zswap state from the indexer first.
        tracing::info!(target: "wallet_core", %circuit, "vault call: composing");
        let coin_pk_hex = wallet
            .coin_public_key_hex()
            .map_err(|e| format!("coin pk: {e}"))?;
        let enc_pk_hex = wallet
            .encryption_public_key_hex()
            .map_err(|e| format!("encryption pk: {e}"))?;
        let indexer = wallet
            .resolve_indexer()
            .map_err(|e| format!("indexer client: {e}"))?;
        let info = match indexer.contract_state(&contract_address_hex).await {
            Ok(Some(i)) => i,
            Ok(None) => {
                return Err(format!(
                    "no on-chain state for {contract_address_hex} — is the vault deployed?",
                ));
            }
            Err(e) => return Err(format!("indexer: {e}")),
        };

        // Prefer the in-process WebView bridge; fall back to a spawned
        // Node harness for desktop `cargo test`. `owned_node_bridge`
        // keeps the spawned child alive for the duration of the call.
        let owned_node_bridge: Option<crate::js_bridge::NodeChildBridge> =
            if wallet.js_bridge.is_some() {
                None
            } else {
                Some(
                    crate::js_bridge::NodeChildBridge::spawn(
                        &crate::js_bridge::NodeChildBridge::default_harness_path(),
                    )
                    .map_err(|e| format!("spawn harness: {e}"))?,
                )
            };
        let bridge: &dyn JsBridge =
            match (wallet.js_bridge.as_ref(), owned_node_bridge.as_ref()) {
                (Some(b), _) => b.as_ref(),
                (None, Some(b)) => b,
                (None, None) => return Err("no JS bridge available".to_string()),
            };

        // Use chain-tip LedgerParameters (PreProd retunes them over
        // time; stale params land the transcript in the fallible slot
        // and the chain rejects with 1010). Same rationale as
        // `call_did_circuit`.
        let tip_params_hex = match indexer.chain_tip().await {
            Ok(Some(t)) => t.ledger_parameters_hex,
            _ => None,
        };
        let ledger_params_hex = tip_params_hex.or(info.ledger_parameters_hex);
        let unproven_hex = call_prepare_vault_unproven(
            bridge,
            circuit.clone(),
            circuit_args,
            private_state,
            info.state_hex,
            contract_address_hex.clone(),
            info.zswap_state_hex,
            ledger_params_hex.clone(),
            coin_pk_hex,
            enc_pk_hex,
            network.config().network_id.to_string(),
        )
        .await
        .map_err(|e| format!("prepare vault call tx: {e}"))?;
        let unproven_bytes =
            hex::decode(&unproven_hex).map_err(|e| format!("hex decode: {e}"))?;
        let unproven: crate::tx::build::UnprovenTx =
            serialize::tagged_deserialize(&unproven_bytes[..])
                .map_err(|e| format!("deserialise unproven tx: {e}"))?;

        // 3. Balancing — identical DUST pipeline to deploy / DID calls.
        tracing::info!(target: "wallet_core", %circuit, "vault call: balancing");
        let parsed_tip_params: Option<ledger::structure::LedgerParameters> =
            ledger_params_hex.as_ref().and_then(|h| {
                let bytes = hex::decode(h.trim_start_matches("0x")).ok()?;
                serialize::tagged_deserialize(&bytes[..]).ok()
            });
        let initial_params_fallback = ledger::structure::INITIAL_PARAMETERS;
        let params: &ledger::structure::LedgerParameters =
            parsed_tip_params.as_ref().unwrap_or(&initial_params_fallback);
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let ttl = base_crypto::time::Timestamp::from_secs(now_secs + 3600);
        let chain_tip_secs: u64 = match indexer.chain_tip().await {
            Ok(Some(t)) => (t.timestamp_unix as u64) / 1000,
            _ => 0,
        };
        let ctime = if chain_tip_secs > dust_state.sync_time.to_secs() as u64 {
            base_crypto::time::Timestamp::from_secs(chain_tip_secs)
        } else {
            dust_state.sync_time
        };
        let mut ctx = crate::tx::balance::BalanceCtx {
            dust_state: &mut dust_state,
            dust_key: &dust_key,
            params: &params,
            time: ctime,
            ttl,
            network_id: network.config().network_id,
        };
        let balanced = crate::tx::balance::balance(unproven, &mut ctx)
            .map_err(|e| format!("balance: {e}"))?;

        // 4. Proving — trait-routed LocalProver / HttpProver.
        tracing::info!(target: "wallet_core", %circuit, "vault call: proving");
        let proven = wallet
            .resolve_prover()
            .prove(balanced)
            .await
            .map_err(|e| format!("prove: {e}"))?;

        // 5. Submitting.
        tracing::info!(target: "wallet_core", %circuit, "vault call: submitting");
        let scale_bytes = crate::tx::scale::scale_encode(&proven)
            .map_err(|e| format!("encode: {e}"))?;
        let signer = wallet
            .midnight_signer()
            .map_err(|e| format!("signer: {e}"))?;
        let node = wallet
            .resolve_node()
            .await
            .map_err(|e| format!("node connect: {e}"))?;
        let submit = node
            .submit_deploy(scale_bytes, &signer)
            .await
            .map_err(|e| format!("submit: {e}"))?;

        Ok(hex::encode(submit.tx_hash))
    }

    /// Read the passport-vault contract's currently-locked total.
    ///
    /// Pulls the contract's serialised `ContractState` from the indexer,
    /// then asks the JS bundle's `readVaultLedger` to decode it (the
    /// Compact ledger layout lives in the vendored contract module, not
    /// Rust). Returns the escrow value in base units (1 NIGHT = 1e6):
    /// `escrowVault.value` when the vault `hasDeposit`, else 0. The
    /// embedded dApp surfaces this via the `vaultTotalLocked` connector
    /// verb. This is a read-only call — no seed, dust, proving, or
    /// submission involved.
    pub async fn vault_total_locked(
        &self,
        contract_address_hex: String,
    ) -> Result<u128, String> {
        let indexer = self
            .resolve_indexer()
            .map_err(|e| format!("indexer client: {e}"))?;
        let info = match indexer.contract_state(&contract_address_hex).await {
            Ok(Some(i)) => i,
            Ok(None) => {
                return Err(format!(
                    "no on-chain state for {contract_address_hex} — is the vault deployed on this network?",
                ));
            }
            Err(e) => return Err(format!("indexer: {e}")),
        };

        // Prefer the in-process WebView bridge; fall back to a spawned
        // Node harness for desktop `cargo test`. Mirrors the transport
        // resolution in `call_contract_circuit`.
        let owned_node_bridge: Option<crate::js_bridge::NodeChildBridge> =
            if self.js_bridge.is_some() {
                None
            } else {
                Some(
                    crate::js_bridge::NodeChildBridge::spawn(
                        &crate::js_bridge::NodeChildBridge::default_harness_path(),
                    )
                    .map_err(|e| format!("spawn harness: {e}"))?,
                )
            };
        let bridge: &dyn JsBridge =
            match (self.js_bridge.as_ref(), owned_node_bridge.as_ref()) {
                (Some(b), _) => b.as_ref(),
                (None, Some(b)) => b,
                (None, None) => return Err("no JS bridge available".to_string()),
            };

        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct VaultLedgerRead {
            total_locked_base_units: String,
        }
        let read: VaultLedgerRead = bridge
            .call(
                "readVaultLedger",
                serde_json::json!({ "contractStateHex": info.state_hex }),
            )
            .await
            .map_err(|e| format!("readVaultLedger: {e}"))?;
        read
            .total_locked_base_units
            .parse::<u128>()
            .map_err(|e| format!("parse total locked '{}': {e}", read.total_locked_base_units))
    }

    /// Run an UNSHIELDED-funded passport-vault circuit (`createLock` with an
    /// initial deposit, or `depositToLock`): compose the call in the
    /// WebView/Node bridge (whose `receiveUnshielded(nativeToken(), amount)`
    /// pulls native UNSHIELDED NIGHT into the contract), add the matching
    /// wallet-owned NIGHT spend + change **signed with the night key** + DUST
    /// fees, prove, and submit. Returns the submitted tx hash.
    ///
    /// The deposit spends the wallet's unshielded NIGHT directly (the faucet
    /// output) — no shielding step. Signing the funding spend in Rust is what
    /// avoids the node's `1010 Custom error 192 (InputsSignaturesLengthMismatch)`
    /// that the JS SDK hits on unshielded contract funding. `circuit_args` is
    /// the `{ $bigint } / { $bytes }`-tagged arg array for the circuit.
    async fn run_unshielded_funded_vault_circuit(
        &self,
        contract_address_hex: String,
        circuit: &str,
        circuit_args: serde_json::Value,
    ) -> Result<String, String> {
        let network = self.network;
        let network_id = network.config().network_id.to_string();

        // 1. Sync the wallet's unshielded NIGHT UTXOs (the funding source) and
        //    derive the night key that owns + signs them.
        tracing::info!(target: "wallet_core", "vault deposit: syncing unshielded NIGHT");
        let utxos = self
            .sync_unshielded()
            .await
            .map_err(|e| format!("sync unshielded: {e}"))?;
        let night_sk = self
            .did_controller_signing_key()
            .map_err(|e| format!("night key: {e}"))?;

        // 2. Sync DUST (fees).
        tracing::info!(target: "wallet_core", "vault deposit: syncing dust");
        let mut dust_state = self
            .sync_dust()
            .await
            .map_err(|e| format!("sync dust: {e}"))?;
        let dust_key = self
            .dust_secret_key()
            .map_err(|e| format!("dust key: {e}"))?;

        // 3. Compose depositFunds(coin) via the bridge.
        tracing::info!(target: "wallet_core", "vault deposit: composing");
        let coin_pk_hex = self
            .coin_public_key_hex()
            .map_err(|e| format!("coin pk: {e}"))?;
        let enc_pk_hex = self
            .encryption_public_key_hex()
            .map_err(|e| format!("encryption pk: {e}"))?;
        let indexer = self
            .resolve_indexer()
            .map_err(|e| format!("indexer client: {e}"))?;
        let info = match indexer.contract_state(&contract_address_hex).await {
            Ok(Some(i)) => i,
            Ok(None) => {
                return Err(format!(
                    "no on-chain state for {contract_address_hex} — is the vault deployed?",
                ));
            }
            Err(e) => return Err(format!("indexer: {e}")),
        };
        let owned_node_bridge: Option<crate::js_bridge::NodeChildBridge> =
            if self.js_bridge.is_some() {
                None
            } else {
                Some(
                    crate::js_bridge::NodeChildBridge::spawn(
                        &crate::js_bridge::NodeChildBridge::default_harness_path(),
                    )
                    .map_err(|e| format!("spawn harness: {e}"))?,
                )
            };
        let bridge: &dyn JsBridge =
            match (self.js_bridge.as_ref(), owned_node_bridge.as_ref()) {
                (Some(b), _) => b.as_ref(),
                (None, Some(b)) => b,
                (None, None) => return Err("no JS bridge available".to_string()),
            };
        let tip_params_hex = match indexer.chain_tip().await {
            Ok(Some(t)) => t.ledger_parameters_hex,
            _ => None,
        };
        let ledger_params_hex = tip_params_hex.or(info.ledger_parameters_hex);

        // The circuit's `receiveUnshielded(nativeToken(), amount)` pulls
        // native unshielded NIGHT into the contract; the unshielded balancer
        // below adds the matching wallet spend + change + signature.
        let unproven_hex = call_prepare_vault_unproven(
            bridge,
            circuit.to_string(),
            circuit_args,
            None,
            info.state_hex,
            contract_address_hex.clone(),
            info.zswap_state_hex,
            ledger_params_hex.clone(),
            coin_pk_hex,
            enc_pk_hex,
            network_id.clone(),
        )
        .await
        .map_err(|e| format!("prepare vault {circuit} tx: {e}"))?;
        let unproven_bytes =
            hex::decode(&unproven_hex).map_err(|e| format!("hex decode: {e}"))?;
        let unproven: crate::tx::build::UnprovenTx =
            serialize::tagged_deserialize(&unproven_bytes[..])
                .map_err(|e| format!("deserialise unproven tx: {e}"))?;

        // 4. Parse tip LedgerParameters for balancing (same rationale
        //    as call_contract_circuit / call_did_circuit).
        let parsed_tip_params: Option<ledger::structure::LedgerParameters> =
            ledger_params_hex.as_ref().and_then(|h| {
                let bytes = hex::decode(h.trim_start_matches("0x")).ok()?;
                serialize::tagged_deserialize(&bytes[..]).ok()
            });
        let initial_params_fallback = ledger::structure::INITIAL_PARAMETERS;
        let params: &ledger::structure::LedgerParameters =
            parsed_tip_params.as_ref().unwrap_or(&initial_params_fallback);
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let ttl = base_crypto::time::Timestamp::from_secs(now_secs + 3600);

        // 5. Unshielded balance: add the wallet's NIGHT spend + change to fund
        //    the contract's `receiveUnshielded`, SIGNED with the night key so
        //    input-count == signature-count (avoids 1010 Custom error 192).
        tracing::info!(target: "wallet_core", "vault deposit: unshielded balancing");
        let unshielded_balanced = crate::tx::balance::balance_unshielded(
            unproven,
            &utxos,
            &night_sk,
            ttl,
            &network_id,
        )
        .map_err(|e| format!("unshielded balance: {e}"))?;

        // 6. DUST balance.
        tracing::info!(target: "wallet_core", "vault deposit: dust balancing");
        let chain_tip_secs: u64 = match indexer.chain_tip().await {
            Ok(Some(t)) => (t.timestamp_unix as u64) / 1000,
            _ => 0,
        };
        let ctime = if chain_tip_secs > dust_state.sync_time.to_secs() as u64 {
            base_crypto::time::Timestamp::from_secs(chain_tip_secs)
        } else {
            dust_state.sync_time
        };
        let mut ctx = crate::tx::balance::BalanceCtx {
            dust_state: &mut dust_state,
            dust_key: &dust_key,
            params,
            time: ctime,
            ttl,
            network_id: &network_id,
        };
        let balanced = crate::tx::balance::balance(unshielded_balanced, &mut ctx)
            .map_err(|e| format!("dust balance: {e}"))?;

        // 7. Prove + submit.
        tracing::info!(target: "wallet_core", "vault deposit: proving");
        let proven = self
            .resolve_prover()
            .prove(balanced)
            .await
            .map_err(|e| format!("prove: {e}"))?;
        tracing::info!(target: "wallet_core", "vault deposit: submitting");
        let scale_bytes =
            crate::tx::scale::scale_encode(&proven).map_err(|e| format!("encode: {e}"))?;
        let signer = self.midnight_signer().map_err(|e| format!("signer: {e}"))?;
        let node = self
            .resolve_node()
            .await
            .map_err(|e| format!("node connect: {e}"))?;
        let submit = node
            .submit_deploy(scale_bytes, &signer)
            .await
            .map_err(|e| format!("submit: {e}"))?;
        Ok(hex::encode(submit.tx_hash))
    }

    /// Read-only bridge call against a vault contract's on-chain state.
    /// Fetches the serialised `ContractState` from the indexer and asks
    /// the JS bundle's `verb` (e.g. `readVaultLocks`) to decode it. Owns
    /// any spawned Node harness internally so the returned JSON outlives
    /// the bridge. No seed / dust / proving / submission.
    async fn vault_bridge_read(
        &self,
        contract_address_hex: &str,
        verb: &str,
    ) -> Result<serde_json::Value, String> {
        let indexer = self
            .resolve_indexer()
            .map_err(|e| format!("indexer client: {e}"))?;
        let info = match indexer.contract_state(contract_address_hex).await {
            Ok(Some(i)) => i,
            Ok(None) => {
                return Err(format!(
                    "no on-chain state for {contract_address_hex} — is the vault deployed on this network?",
                ));
            }
            Err(e) => return Err(format!("indexer: {e}")),
        };
        let owned_node_bridge: Option<crate::js_bridge::NodeChildBridge> =
            if self.js_bridge.is_some() {
                None
            } else {
                Some(
                    crate::js_bridge::NodeChildBridge::spawn(
                        &crate::js_bridge::NodeChildBridge::default_harness_path(),
                    )
                    .map_err(|e| format!("spawn harness: {e}"))?,
                )
            };
        let bridge: &dyn JsBridge =
            match (self.js_bridge.as_ref(), owned_node_bridge.as_ref()) {
                (Some(b), _) => b.as_ref(),
                (None, Some(b)) => b,
                (None, None) => return Err("no JS bridge available".to_string()),
            };
        bridge
            .call(verb, serde_json::json!({ "contractStateHex": info.state_hex }))
            .await
            .map_err(|e| format!("{verb}: {e}"))
    }

    /// Enumerate the vault's locks (id, policy, per-lock pool) plus the
    /// global `lockCount`. Returns the raw `readVaultLocks` JSON for the
    /// dApp's lock list + claim selector.
    pub async fn list_locks(
        &self,
        contract_address_hex: String,
    ) -> Result<serde_json::Value, String> {
        self.vault_bridge_read(&contract_address_hex, "readVaultLocks")
            .await
    }

    /// Read the vault's current `lockCount` — the id the *next*
    /// `createLock` will assign. Used to report the new lock id after a
    /// create (the circuit returns the pre-increment counter).
    async fn vault_lock_count(&self, contract_address_hex: &str) -> Result<u64, String> {
        let v = self
            .vault_bridge_read(contract_address_hex, "readVaultLocks")
            .await?;
        v.get("lockCount")
            .and_then(|x| x.as_str().and_then(|s| s.parse::<u64>().ok()).or_else(|| x.as_u64()))
            .ok_or_else(|| "readVaultLocks: missing lockCount".to_string())
    }

    /// Create a new lock with `policy` and an optional initial deposit of
    /// `initial_amount` base units. The caller becomes the lock's creator
    /// (only they may later deposit / withdraw). Returns the submitted tx
    /// hash plus the assigned lock id.
    pub async fn create_lock(
        &self,
        contract_address_hex: String,
        policy: VaultLockPolicy,
        initial_amount: u128,
    ) -> Result<VaultCreateLockOutcome, String> {
        // The circuit returns the pre-increment `lockCount`, which is the
        // id of the lock it creates — read it before submitting.
        let lock_id = self.vault_lock_count(&contract_address_hex).await?;
        let args = serde_json::json!([
            { "$bigint": policy.min_age.to_string() },
            policy.require_issuing_state,
            { "$bytes": hex::encode(policy.required_issuing_state) },
            policy.require_document_number,
            { "$bytes": hex::encode(policy.required_document_number) },
            { "$bigint": policy.max_claim.to_string() },
            { "$bytes": hex::encode(policy.verifier_challenge_hash) },
            { "$bigint": initial_amount.to_string() },
        ]);
        let tx_hash = self
            .run_unshielded_funded_vault_circuit(contract_address_hex, "createLock", args)
            .await?;
        Ok(VaultCreateLockOutcome { tx_hash, lock_id })
    }

    /// Top up an existing lock's pool with `amount_base_units` of native
    /// UNSHIELDED NIGHT. Only the lock's creator may deposit (enforced
    /// on-chain).
    pub async fn deposit_to_lock(
        &self,
        contract_address_hex: String,
        lock_id: u64,
        amount_base_units: u128,
    ) -> Result<String, String> {
        let args = serde_json::json!([
            { "$bigint": lock_id.to_string() },
            { "$bigint": amount_base_units.to_string() },
        ]);
        self.run_unshielded_funded_vault_circuit(contract_address_hex, "depositToLock", args)
            .await
    }

    /// Claim (unlock) funds from a specific lock. Composes
    /// `claimFromLock(lockId, ...)` from the holder's credential `bundle`
    /// via the bridge's `prepareVaultClaim` (which reads the lock's
    /// on-chain policy, builds the selective-disclosure presentation +
    /// age proof, and supplies the date-of-birth witness), then balances
    /// DUST fees, proves, and submits. The released NIGHT goes to this
    /// wallet's unshielded address.
    ///
    /// No shielded sync needed: the payout is drawn from the contract's
    /// own unshielded balance; the holder only covers DUST fees.
    pub async fn claim_from_lock(
        &self,
        contract_address_hex: String,
        lock_id: u64,
        amount_base_units: u128,
        bundle: serde_json::Value,
        current_day: Option<u64>,
    ) -> Result<String, String> {
        let network = self.network;
        let network_id = network.config().network_id.to_string();

        tracing::info!(target: "wallet_core", "vault claim: syncing dust");
        let mut dust_state = self
            .sync_dust()
            .await
            .map_err(|e| format!("sync dust: {e}"))?;
        let dust_key = self
            .dust_secret_key()
            .map_err(|e| format!("dust key: {e}"))?;

        tracing::info!(target: "wallet_core", "vault claim: composing");
        let coin_pk_hex = self
            .coin_public_key_hex()
            .map_err(|e| format!("coin pk: {e}"))?;
        let enc_pk_hex = self
            .encryption_public_key_hex()
            .map_err(|e| format!("encryption pk: {e}"))?;
        // The released NIGHT is UNSHIELDED, so the contract pays out to the
        // holder's unshielded `UserAddress` (not the zswap coin key).
        let recipient_user_address_hex = self
            .unshielded_user_address_hex()
            .map_err(|e| format!("recipient user address: {e}"))?;
        let indexer = self
            .resolve_indexer()
            .map_err(|e| format!("indexer client: {e}"))?;
        let info = match indexer.contract_state(&contract_address_hex).await {
            Ok(Some(i)) => i,
            Ok(None) => {
                return Err(format!(
                    "no on-chain state for {contract_address_hex} — is the vault deployed?",
                ));
            }
            Err(e) => return Err(format!("indexer: {e}")),
        };
        let owned_node_bridge: Option<crate::js_bridge::NodeChildBridge> =
            if self.js_bridge.is_some() {
                None
            } else {
                Some(
                    crate::js_bridge::NodeChildBridge::spawn(
                        &crate::js_bridge::NodeChildBridge::default_harness_path(),
                    )
                    .map_err(|e| format!("spawn harness: {e}"))?,
                )
            };
        let bridge: &dyn JsBridge =
            match (self.js_bridge.as_ref(), owned_node_bridge.as_ref()) {
                (Some(b), _) => b.as_ref(),
                (None, Some(b)) => b,
                (None, None) => return Err("no JS bridge available".to_string()),
            };
        let tip_params_hex = match indexer.chain_tip().await {
            Ok(Some(t)) => t.ledger_parameters_hex,
            _ => None,
        };
        let ledger_params_hex = tip_params_hex.or(info.ledger_parameters_hex);

        let unproven_hex = call_prepare_vault_claim(
            bridge,
            lock_id,
            bundle,
            amount_base_units.to_string(),
            current_day.map(|d| d.to_string()),
            info.state_hex,
            contract_address_hex.clone(),
            info.zswap_state_hex,
            ledger_params_hex.clone(),
            coin_pk_hex,
            enc_pk_hex,
            recipient_user_address_hex,
            network_id.clone(),
        )
        .await
        .map_err(|e| format!("prepare vault claim tx: {e}"))?;
        let unproven_bytes =
            hex::decode(&unproven_hex).map_err(|e| format!("hex decode: {e}"))?;
        let unproven: crate::tx::build::UnprovenTx =
            serialize::tagged_deserialize(&unproven_bytes[..])
                .map_err(|e| format!("deserialise unproven tx: {e}"))?;

        // DUST balance (no shielded pre-pass — contract-owned input).
        tracing::info!(target: "wallet_core", "vault claim: dust balancing");
        let parsed_tip_params: Option<ledger::structure::LedgerParameters> =
            ledger_params_hex.as_ref().and_then(|h| {
                let bytes = hex::decode(h.trim_start_matches("0x")).ok()?;
                serialize::tagged_deserialize(&bytes[..]).ok()
            });
        let initial_params_fallback = ledger::structure::INITIAL_PARAMETERS;
        let params: &ledger::structure::LedgerParameters =
            parsed_tip_params.as_ref().unwrap_or(&initial_params_fallback);
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let ttl = base_crypto::time::Timestamp::from_secs(now_secs + 3600);
        let chain_tip_secs: u64 = match indexer.chain_tip().await {
            Ok(Some(t)) => (t.timestamp_unix as u64) / 1000,
            _ => 0,
        };
        let ctime = if chain_tip_secs > dust_state.sync_time.to_secs() as u64 {
            base_crypto::time::Timestamp::from_secs(chain_tip_secs)
        } else {
            dust_state.sync_time
        };
        let mut ctx = crate::tx::balance::BalanceCtx {
            dust_state: &mut dust_state,
            dust_key: &dust_key,
            params,
            time: ctime,
            ttl,
            network_id: &network_id,
        };
        let balanced = crate::tx::balance::balance(unproven, &mut ctx)
            .map_err(|e| format!("dust balance: {e}"))?;

        tracing::info!(target: "wallet_core", "vault claim: proving");
        let proven = self
            .resolve_prover()
            .prove(balanced)
            .await
            .map_err(|e| format!("prove: {e}"))?;
        tracing::info!(target: "wallet_core", "vault claim: submitting");
        let scale_bytes =
            crate::tx::scale::scale_encode(&proven).map_err(|e| format!("encode: {e}"))?;
        let signer = self.midnight_signer().map_err(|e| format!("signer: {e}"))?;
        let node = self
            .resolve_node()
            .await
            .map_err(|e| format!("node connect: {e}"))?;
        let submit = node
            .submit_deploy(scale_bytes, &signer)
            .await
            .map_err(|e| format!("submit: {e}"))?;
        Ok(hex::encode(submit.tx_hash))
    }

    /// Drain a `WizardStage` stream until it terminates and convert
    /// the outcome into a `Result<DidId, WalletError>`. Internal seam
    /// shared by every awaitable wrapper around a `create_did` /
    /// `call_did_circuit` / `load_did_circuit` stream.
    async fn drain_did_stream(
        stream: impl futures::Stream<Item = crate::WizardStage>,
    ) -> Result<crate::DidId, WalletError> {
        Self::drain_did_stream_outcome(stream).await.map(|o| o.did_id)
    }

    /// Same as [`drain_did_stream`] but surfaces the full
    /// [`crate::DeployOutcome`] (carrying `controller_sk`, `tx_hash`,
    /// etc.). Used by [`Wallet::create_did_awaitable_with_controller`]
    /// so Task 2's `bootstrap_did_with_keys` can chain
    /// `setVerificationMethod` calls without losing the per-DID
    /// controller secret minted at deploy time.
    async fn drain_did_stream_outcome(
        stream: impl futures::Stream<Item = crate::WizardStage>,
    ) -> Result<crate::DeployOutcome, WalletError> {
        use futures::StreamExt;
        let mut stream = std::pin::pin!(stream);
        while let Some(stage) = stream.next().await {
            match stage {
                crate::WizardStage::Done(outcome) => return Ok(outcome),
                crate::WizardStage::Failed(err) => {
                    return Err(WalletError::CreateDidFailed(err))
                }
                _ => continue,
            }
        }
        Err(WalletError::CreateDidFailed(
            "WizardStage stream ended without Done or Failed".into(),
        ))
    }

    /// Awaitable wrapper around [`Wallet::create_did`]. Drains the
    /// `WizardStage` stream the existing wizard UI consumes and
    /// returns the freshly-minted DID id on `Done`, or
    /// [`WalletError::CreateDidFailed`] on `Failed` (or premature
    /// stream end).
    ///
    /// Awaitable callers (Task 2's `bootstrap_did_with_keys`, the
    /// React Native / CLI demo paths) don't need the stage-by-stage
    /// progress signal the Dioxus wizard reads — they only want the
    /// final DID id. This wrapper exists so they don't have to roll
    /// their own `Stream::next` loop.
    ///
    /// **Note**: this method does **not** surface the per-DID
    /// `controller_sk` minted during `create_did`. Task 2 awaits
    /// further plumbing (the `BootstrappedDid` outcome will carry it
    /// once the orchestration lands). For now, callers who need the
    /// sk must continue to drive [`Wallet::create_did`] directly.
    pub async fn create_did_awaitable(&self) -> Result<crate::DidId, WalletError> {
        #[cfg(any(test, feature = "test-support"))]
        if let Some(state) = self.stub_did_state() {
            return Ok(self.stub_create_did(state));
        }
        Self::drain_did_stream(self.create_did()).await
    }

    /// Like [`Wallet::create_did_awaitable`] but additionally surfaces
    /// the 32-byte `controller_sk` the deploy pipeline minted for the
    /// new DID. Required by Task 2's `bootstrap_did_with_keys` so the
    /// subsequent `setVerificationMethod[Relation]` calls can pass the
    /// controller secret that the Compact circuit's
    /// `localSecretKey()` witness consumes.
    ///
    /// In stub mode (see `test_support::stub_wallet`) the chain-op
    /// pipeline is bypassed and no real controller_sk is minted, so
    /// this returns an all-zero secret. The stub
    /// `add_verification_method[_relation]` impls don't inspect the
    /// secret either — both ends of the test path stay consistent.
    pub async fn create_did_awaitable_with_controller(
        &self,
    ) -> Result<(crate::DidId, [u8; 32]), WalletError> {
        #[cfg(any(test, feature = "test-support"))]
        if let Some(state) = self.stub_did_state() {
            return Ok((self.stub_create_did(state), [0u8; 32]));
        }
        let outcome = Self::drain_did_stream_outcome(self.create_did()).await?;
        Ok((outcome.did_id, outcome.controller_sk))
    }

    /// Awaitable wrapper for [`Wallet::load_did_circuit`]. Drains
    /// the `WizardStage` stream and surfaces the `Done` outcome (or
    /// the `Failed` error as [`WalletError::CreateDidFailed`] — the
    /// shared error variant the other awaitables use).
    ///
    /// `counter` MUST match the contract's current
    /// `maintenance_authority.counter`. A fresh deploy starts at 0
    /// and each successful load bumps it; the caller is responsible
    /// for tracking the bump (or calling `resolve_did_full` to read
    /// the on-chain counter first).
    ///
    /// In stub mode this is a no-op that returns `Ok(())` — the
    /// stub `DidLedgerState` doesn't track loaded circuits, so the
    /// subsequent `add_verification_method` short-circuits without
    /// caring whether a VK is "loaded".
    pub async fn load_did_circuit_awaitable(
        &self,
        did: crate::DidId,
        circuit_name: String,
        counter: u32,
    ) -> Result<(), WalletError> {
        #[cfg(any(test, feature = "test-support"))]
        if self.stub_did_state().is_some() {
            return Ok(());
        }
        let stream = self.load_did_circuit(did, circuit_name, counter);
        Self::drain_did_stream_outcome(stream).await.map(|_| ())
    }

    /// Awaitable wrapper for the `setVerificationMethod` Compact
    /// circuit (the post-2026-05-28 successor to
    /// `addVerificationMethod` — the upstream `feat!: Redesign DID
    /// verification method storage` refactor collapsed `add* /
    /// update*` into a single `set*(value, mutation)` entry point).
    /// Builds the JSON arg shape the JS-side harness
    /// (`prepareUnprovenCallTx`) expects, delegates to
    /// [`Wallet::call_did_circuit`], and drains the resulting
    /// `WizardStage` stream.
    ///
    /// `controller_sk` is the 32-byte secret the wallet minted for
    /// `did` at deploy time (see [`crate::DeployOutcome::controller_sk`]).
    /// Without it the circuit's `localSecretKey()` witness fails the
    /// contract's controller assertion.
    ///
    /// `verification_method_json` is the JSON-encoded
    /// `VerificationMethod` object the circuit consumes — `{ id,
    /// typ, publicKeyJwk: { kty, crv, x, y } }` with `x`/`y` as
    /// plain base64url strings (post-2026-05-28 schema). Task 2's
    /// `bootstrap_did_with_keys` assembles this from the
    /// `SecretStorage::get_public_key` result before calling.
    ///
    /// `_key_ref` is reserved for the Task 1.5.D `stub_wallet` audit
    /// (lets the stub record which key was attached) but is unused
    /// in the real-deps path: the JWK already carries the public
    /// material via `verification_method_json`.
    ///
    /// The kept Rust method name (`add_verification_method`) is
    /// stable so Identity Centre + bootstrap callers don't churn;
    /// internally we now call `setVerificationMethod` with the
    /// `MapMutation::Insert` discriminator.
    pub async fn add_verification_method(
        &self,
        did: &crate::DidId,
        _key_ref: &crate::secret_storage::SecretKeyRef,
        verification_method_json: serde_json::Value,
        controller_sk: [u8; 32],
    ) -> Result<(), WalletError> {
        #[cfg(any(test, feature = "test-support"))]
        if let Some(state) = self.stub_did_state() {
            return self.stub_add_verification_method(state, did, &verification_method_json);
        }
        // `MapMutation` in the contract:
        //   0 = Undefined, 1 = Insert, 2 = Update.
        // Encoded as a plain JSON number; the JS harness's
        // `reviveBigints` leaves numeric primitives untouched and
        // the Compact runtime accepts the JS-numeric tag for enum
        // variants — same shape as the existing
        // `VerificationMethodRelation` enum already shipped below.
        const MAP_MUTATION_INSERT: u8 = 1;
        let stream = self.call_did_circuit(
            did.clone(),
            "setVerificationMethod".to_string(),
            serde_json::Value::Array(vec![
                verification_method_json,
                serde_json::Value::Number(MAP_MUTATION_INSERT.into()),
            ]),
            controller_sk,
        );
        Self::drain_did_stream(stream).await.map(|_| ())
    }

    /// Awaitable wrapper for the `setSchnorrJubjubVerificationMethod`
    /// Compact circuit. Added in the 2026-05-28 schema refresh: the
    /// plain `setVerificationMethod` circuit now REJECTS Jubjub JWKs
    /// (the contract asserts `crv != Jubjub` for EC keys), so Jubjub
    /// keys must be pushed through this dedicated map instead.
    ///
    /// `public_key_x_be` / `public_key_y_be` are the Jubjub point's
    /// 32-byte big-endian coordinates — same shape the JWK
    /// `x`/`y` carry after base64url decode. We translate to the
    /// decimal `{$bigint: "<dec>"}` form the JS harness's
    /// `reviveBigints` expects so the Compact runtime's
    /// `CompactTypeJubjubPoint.toValue({ x: BigInt, y: BigInt })`
    /// can consume them.
    ///
    /// `controller_sk` is the 32-byte secret the wallet minted for
    /// `did` at deploy time. `_key_ref` mirrors
    /// `add_verification_method`'s parameter — currently unused by
    /// the real-deps path (the JubjubPoint already carries the
    /// public material) but reserved for the stub path to record
    /// which key was attached.
    ///
    /// Internally routes to `call_did_circuit(
    ///   "setSchnorrJubjubVerificationMethod",
    ///   [ { id, publicKey: { x: {$bigint}, y: {$bigint} } },
    ///     MapMutation::Insert ])`.
    pub async fn set_schnorr_jubjub_verification_method(
        &self,
        did: &crate::DidId,
        _key_ref: &crate::secret_storage::SecretKeyRef,
        method_id: String,
        public_key_x_be: [u8; 32],
        public_key_y_be: [u8; 32],
        controller_sk: [u8; 32],
    ) -> Result<(), WalletError> {
        #[cfg(any(test, feature = "test-support"))]
        if let Some(state) = self.stub_did_state() {
            // Stub path: synthesize a JWK envelope so the
            // resolved-doc surface stays uniform across stub and
            // real paths. Same JWK shape `ledger_to_domain`
            // produces for SchnorrJubjub VMs on the real path.
            let vm_json = serde_json::json!({
                "id": method_id,
                "typ": 1,
                "publicKeyJwk": {
                    "kty": 0, // EC
                    "crv": 2, // Jubjub
                    "x": be32_to_base64url(&public_key_x_be),
                    "y": be32_to_base64url(&public_key_y_be),
                },
            });
            return self.stub_add_verification_method(state, did, &vm_json);
        }
        const MAP_MUTATION_INSERT: u8 = 1;
        let vm_json = serde_json::json!({
            "id": method_id,
            "publicKey": {
                "x": { "$bigint": be32_to_decimal_string(&public_key_x_be) },
                "y": { "$bigint": be32_to_decimal_string(&public_key_y_be) },
            }
        });
        let stream = self.call_did_circuit(
            did.clone(),
            "setSchnorrJubjubVerificationMethod".to_string(),
            serde_json::Value::Array(vec![
                vm_json,
                serde_json::Value::Number(MAP_MUTATION_INSERT.into()),
            ]),
            controller_sk,
        );
        Self::drain_did_stream(stream).await.map(|_| ())
    }

    /// Awaitable wrapper for the `setVerificationMethodRelation`
    /// Compact circuit (the post-2026-05-28 successor to
    /// `addVerificationMethodRelation` /
    /// `removeVerificationMethodRelation` — collapsed into a
    /// single `set*(relation, methodId, mutation)` entry point).
    /// Same delegation pattern as
    /// [`Wallet::add_verification_method`]: build the JSON arg
    /// shape, route through [`Wallet::call_did_circuit`], drain.
    ///
    /// `relation` selects which DID-Core relation array
    /// (`authentication`, `assertionMethod`, etc.) the existing
    /// verification method at `fragment` is appended to. The
    /// circuit then enforces "the VM exists" before mutating the
    /// relation map — so the VM must already have been registered
    /// via `setVerificationMethod` first.
    ///
    /// `fragment` is the `#fragment` of the verification-method
    /// id — e.g. `"key-1"`, not the full `<did>#key-1`. The JS
    /// harness composes the full id internally.
    ///
    /// The kept Rust method name
    /// (`add_verification_method_relation`) is stable so callers
    /// don't churn; internally we now call
    /// `setVerificationMethodRelation` with the
    /// `SetMutation::Insert` discriminator.
    pub async fn add_verification_method_relation(
        &self,
        did: &crate::DidId,
        fragment: &str,
        relation: crate::VerificationMethodRelation,
        controller_sk: [u8; 32],
    ) -> Result<(), WalletError> {
        // The JS-side `VerificationMethodRelation` enum is encoded
        // as the JS numeric value (Undefined=0, Authentication=1,
        // AssertionMethod=2, KeyAgreement=3, CapabilityInvocation=4,
        // CapabilityDelegation=5). The harness re-hydrates the enum
        // from this number before invoking the circuit.
        #[cfg(any(test, feature = "test-support"))]
        if let Some(state) = self.stub_did_state() {
            return self.stub_add_verification_method_relation(state, did, fragment, relation);
        }
        let relation_int: u8 = match relation {
            crate::VerificationMethodRelation::Authentication => 1,
            crate::VerificationMethodRelation::AssertionMethod => 2,
            crate::VerificationMethodRelation::KeyAgreement => 3,
            crate::VerificationMethodRelation::CapabilityInvocation => 4,
            crate::VerificationMethodRelation::CapabilityDelegation => 5,
        };
        // `SetMutation` in the contract:
        //   0 = Undefined, 1 = Insert, 2 = Remove.
        const SET_MUTATION_INSERT: u8 = 1;
        let stream = self.call_did_circuit(
            did.clone(),
            "setVerificationMethodRelation".to_string(),
            serde_json::Value::Array(vec![
                serde_json::Value::Number(relation_int.into()),
                serde_json::Value::String(fragment.to_string()),
                serde_json::Value::Number(SET_MUTATION_INSERT.into()),
            ]),
            controller_sk,
        );
        Self::drain_did_stream(stream).await.map(|_| ())
    }

    /// Resolve a Midnight DID to a [`crate::DidDocument`] by querying
    /// the indexer for the contract's current state and decoding it.
    ///
    /// **Phase 2a**: fetches the latest contract action from the
    /// indexer; full state decoding into a populated `DidDocument`
    /// lands in 2b. For now we return a placeholder document with
    /// the DID's id + the on-chain block height where the contract
    /// was last seen. If the address is unknown to the indexer,
    /// returns [`crate::DidError::Indexer`] with a clear message.
    pub async fn resolve_did(
        &self,
        did: &str,
    ) -> Result<crate::DidDocument, crate::DidError> {
        let id = crate::DidId::parse(did)?;
        if !id.network.same_chain(self.network) {
            return Err(crate::DidError::Indexer(format!(
                "DID network {:?} does not match wallet network {:?}",
                id.network, self.network
            )));
        }

        #[cfg(any(test, feature = "test-support"))]
        if let Some(state) = self.stub_did_state() {
            return self.stub_resolve_did(state, &id);
        }

        let client = self
            .resolve_indexer()
            .map_err(|e| crate::DidError::Indexer(e.to_string()))?;
        let info = client
            .contract_state(&id.contract_address_hex())
            .await
            .map_err(|e| crate::DidError::Indexer(e.to_string()))?
            .ok_or_else(|| {
                crate::DidError::Indexer(format!(
                    "no contract action for address {} on {}",
                    id.contract_address_hex(),
                    self.network.label()
                ))
            })?;

        // Decode the on-chain state into a typed `DidLedgerState`,
        // then map it to a domain `DidDocument`. Phase 2b populates
        // the scalar fields (version, dates, deactivated); Phase 2c
        // walks the maps for VMs / services / relations.
        let ledger_state =
            crate::did::contract::decode_did_ledger_state(&info.state_hex)?;
        Ok(crate::did::contract::ledger_to_domain(&ledger_state, id))
    }

    /// Like [`resolve_did`] but also returns the on-chain housekeeping
    /// (`maintenance_counter`, last tx + block) the LoadCircuitPanel
    /// needs in order to compose the next `MaintenanceUpdate` without
    /// asking the user to track the counter manually.
    pub async fn resolve_did_full(
        &self,
        did: &str,
    ) -> Result<crate::ResolvedDid, crate::DidError> {
        let id = crate::DidId::parse(did)?;
        if !id.network.same_chain(self.network) {
            return Err(crate::DidError::Indexer(format!(
                "DID network {:?} does not match wallet network {:?}",
                id.network, self.network
            )));
        }

        let started = std::time::Instant::now();
        let client = self
            .resolve_indexer()
            .map_err(|e| crate::DidError::Indexer(e.to_string()))?;
        let info = client
            .contract_state(&id.contract_address_hex())
            .await
            .map_err(|e| crate::DidError::Indexer(e.to_string()))?
            .ok_or_else(|| {
                crate::DidError::Indexer(format!(
                    "no contract action for address {} on {}",
                    id.contract_address_hex(),
                    self.network.label()
                ))
            })?;

        let ledger_state =
            crate::did::contract::decode_did_ledger_state(&info.state_hex)?;
        let maintenance_counter =
            crate::did::contract::decode_maintenance_counter(&info.state_hex)?;
        let loaded_circuits =
            crate::did::contract::decode_loaded_circuits(&info.state_hex)?;
        // Snapshot the relation vectors before `ledger_to_domain`
        // consumes them — the UI's relationship matrix needs the
        // raw fragment ids, not the DID-URL-expanded form
        // `ledger_to_domain` produces.
        let authentication_ids = ledger_state.authentication.clone();
        let assertion_method_ids = ledger_state.assertion_method.clone();
        let key_agreement_ids = ledger_state.key_agreement.clone();
        let capability_invocation_ids = ledger_state.capability_invocation.clone();
        let capability_delegation_ids = ledger_state.capability_delegation.clone();
        let document = crate::did::contract::ledger_to_domain(&ledger_state, id);
        let resolve_latency_ms = started.elapsed().as_millis() as u64;
        Ok(crate::ResolvedDid {
            document,
            maintenance_counter,
            last_block_height: info.last_block_height,
            last_tx_hash: info.last_tx_hash,
            raw_state_hex: info.state_hex,
            resolve_latency_ms,
            authentication_ids,
            assertion_method_ids,
            key_agreement_ids,
            capability_invocation_ids,
            capability_delegation_ids,
            loaded_circuits,
        })
    }

    // -- Stub-mode helpers ---------------------------------------------------
    //
    // These four methods implement the in-memory short-circuit used by
    // `test_support::stub_wallet`. They mutate (and read) the shared
    // `Arc<Mutex<HashMap<DidId, DidDocument>>>` rather than driving the
    // chain-op pipeline. Gated behind the same feature as the field so
    // none of this lands in release builds.

    /// Stub-mode `create_did_awaitable`: derive a deterministic
    /// `DidId` from the wallet seed + the current count of stubbed
    /// DIDs (so successive calls produce distinct ids), insert an
    /// empty `DidDocument` into the shared map, and return the id.
    #[cfg(any(test, feature = "test-support"))]
    fn stub_create_did(&self, state: crate::test_support::StubDidMap) -> crate::DidId {
        use sha2::{Digest, Sha256};
        let mut guard = state.lock().expect("stub_did_state poisoned");
        let seq = guard.len() as u64;
        let mut hasher = Sha256::new();
        hasher.update(b"wallet-core/test_support/stub_did");
        hasher.update(self.seed_bytes);
        hasher.update(seq.to_le_bytes());
        let addr: crate::ContractAddressBytes = hasher.finalize().into();
        let did = crate::DidId::new(self.network, addr);
        let now = Some(std::time::SystemTime::now());
        let doc = crate::DidDocument {
            id: did.clone(),
            controller: None,
            also_known_as: vec![],
            verification_method: vec![],
            authentication: vec![],
            assertion_method: vec![],
            key_agreement: vec![],
            capability_invocation: vec![],
            capability_delegation: vec![],
            service: vec![],
            deactivated: false,
            created: now,
            updated: now,
            version: 0,
        };
        guard.insert(did.clone(), doc);
        did
    }

    /// Stub-mode `add_verification_method`: parse the JWK JSON arg
    /// into a `VerificationMethod` (best-effort — the JS-side shape
    /// the real circuit consumes differs from DID-Core; we accept
    /// either) and append it to the DID document's
    /// `verification_method` array.
    #[cfg(any(test, feature = "test-support"))]
    fn stub_add_verification_method(
        &self,
        state: crate::test_support::StubDidMap,
        did: &crate::DidId,
        vm_json: &serde_json::Value,
    ) -> Result<(), WalletError> {
        let mut guard = state.lock().expect("stub_did_state poisoned");
        let doc = guard.get_mut(did).ok_or_else(|| {
            WalletError::CreateDidFailed(format!(
                "stub_add_verification_method: unknown DID {}",
                did.to_did_string()
            ))
        })?;
        // Try DID-Core shape first; if that fails, synthesise a
        // minimal VM from the `id` field alone. The exact JWK
        // payload isn't load-bearing for Task 2's assertions — the
        // test only checks that the relation arrays are populated.
        let mut vm: crate::VerificationMethod = match serde_json::from_value::<
            crate::VerificationMethod,
        >(vm_json.clone())
        {
            Ok(parsed) => parsed,
            Err(_) => {
                let id = vm_json
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                crate::VerificationMethod {
                    id,
                    typ: crate::VerificationMethodType::JsonWebKey,
                    controller: did.clone(),
                    public_key_jwk: crate::PublicKeyJwk {
                        kty: crate::KeyType::OKP,
                        crv: crate::CurveType::Ed25519,
                        x: String::new(),
                        y: None,
                    },
                }
            }
        };
        // Normalize the stored id into the absolute DID URL form
        // the real-deps path emits from `ledger_to_domain`. The wire
        // wallet-side now sends the canonical short form (`#key-auth`)
        // per `normalizeBoundFragmentId`; the stub mirrors the
        // upstream's `absoluteDidUrlReference` re-expansion so
        // resolved docs are interchangeable across stub / real paths.
        vm.id = stub_to_absolute_did_url(did, &vm.id);
        doc.verification_method.push(vm);
        doc.updated = Some(std::time::SystemTime::now());
        Ok(())
    }

    /// Stub-mode `add_verification_method_relation`: look up the VM
    /// by `<did>#<fragment>` in the doc's `verification_method`
    /// array (falling back to inserting a bare-id reference if not
    /// present — the JS circuit enforces presence, but the stub is
    /// lenient so tests can attach a relation directly without
    /// first registering the VM), then push a
    /// `VerificationMethodRef::Id(...)` into the matching relation
    /// vector.
    #[cfg(any(test, feature = "test-support"))]
    fn stub_add_verification_method_relation(
        &self,
        state: crate::test_support::StubDidMap,
        did: &crate::DidId,
        fragment: &str,
        relation: crate::VerificationMethodRelation,
    ) -> Result<(), WalletError> {
        let mut guard = state.lock().expect("stub_did_state poisoned");
        let doc = guard.get_mut(did).ok_or_else(|| {
            WalletError::CreateDidFailed(format!(
                "stub_add_verification_method_relation: unknown DID {}",
                did.to_did_string()
            ))
        })?;
        // `fragment` arrives in the canonical short form
        // (`#key-auth`) per `normalizeBoundFragmentId`; promote to
        // the absolute DID URL so the resolved doc's relation
        // references match what self-verify / off-chain auth flows
        // look up by kid.
        let full_id = stub_to_absolute_did_url(did, fragment);
        let vm_ref = crate::VerificationMethodRef::Id(full_id);
        let target = match relation {
            crate::VerificationMethodRelation::Authentication => &mut doc.authentication,
            crate::VerificationMethodRelation::AssertionMethod => &mut doc.assertion_method,
            crate::VerificationMethodRelation::KeyAgreement => &mut doc.key_agreement,
            crate::VerificationMethodRelation::CapabilityInvocation => {
                &mut doc.capability_invocation
            }
            crate::VerificationMethodRelation::CapabilityDelegation => {
                &mut doc.capability_delegation
            }
        };
        target.push(vm_ref);
        doc.updated = Some(std::time::SystemTime::now());
        Ok(())
    }

    /// Stub-mode `resolve_did`: clone the document out of the shared
    /// map. Returns `DidError::Indexer` if the id is unknown — same
    /// shape the real-deps path uses when the indexer doesn't know
    /// about an address.
    #[cfg(any(test, feature = "test-support"))]
    fn stub_resolve_did(
        &self,
        state: crate::test_support::StubDidMap,
        id: &crate::DidId,
    ) -> Result<crate::DidDocument, crate::DidError> {
        let guard = state.lock().expect("stub_did_state poisoned");
        guard.get(id).cloned().ok_or_else(|| {
            crate::DidError::Indexer(format!(
                "stub: no DID document for {}",
                id.to_did_string()
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn be32_to_decimal_string_handles_zero() {
        assert_eq!(be32_to_decimal_string(&[0u8; 32]), "0");
    }

    #[test]
    fn be32_to_decimal_string_handles_small_values() {
        let mut be = [0u8; 32];
        be[31] = 42;
        assert_eq!(be32_to_decimal_string(&be), "42");
        be[31] = 0;
        be[30] = 1; // 256
        assert_eq!(be32_to_decimal_string(&be), "256");
    }

    #[test]
    fn be32_to_decimal_string_handles_max_value() {
        // 2^256 - 1 = a 78-digit decimal starting with 1157920892…
        let be = [0xffu8; 32];
        let s = be32_to_decimal_string(&be);
        assert_eq!(
            s,
            "115792089237316195423570985008687907853269984665640564039457584007913129639935",
        );
    }

    #[test]
    fn be32_to_decimal_string_known_value_round_trip() {
        // Pattern with a non-trivial number of leading zeros after
        // each 10⁹ subtraction so the format!("{:09}") padding path
        // is exercised.
        let mut be = [0u8; 32];
        be[28..32].copy_from_slice(&1_000_000_001u32.to_be_bytes());
        assert_eq!(be32_to_decimal_string(&be), "1000000001");
    }

    #[test]
    fn deterministic_seed_yields_stable_keys() {
        let w1 = Wallet::from_chacha_seed(0xdeadbeef, Network::PreProd);
        let w2 = Wallet::from_chacha_seed(0xdeadbeef, Network::PreProd);
        assert_eq!(w1.seed_hex(), w2.seed_hex());
        assert_eq!(
            w1.coin_public_key_hex().unwrap(),
            w2.coin_public_key_hex().unwrap()
        );
        assert_eq!(
            w1.encryption_public_key_hex().unwrap(),
            w2.encryption_public_key_hex().unwrap()
        );
    }

    #[test]
    fn seed_hex_roundtrip() {
        let w = Wallet::from_chacha_seed(7, Network::PreProd);
        let hex = w.seed_hex();
        let back = Wallet::from_seed_hex(&hex, Network::PreProd).unwrap();
        assert_eq!(w.coin_public_key_hex().unwrap(), back.coin_public_key_hex().unwrap());
    }

    #[test]
    fn invalid_seed_hex_rejected() {
        let result = Wallet::from_seed_hex("ab", Network::PreProd);
        assert!(matches!(result, Err(WalletError::InvalidSeedLen(_))));
    }

    #[test]
    fn demo_seed_is_well_formed_and_stable() {
        let w1 = Wallet::demo(Network::PreProd);
        let w2 = Wallet::demo(Network::PreProd);
        assert_eq!(w1.seed_hex(), DEMO_SEED_HEX);
        assert_eq!(
            w1.coin_public_key_hex().unwrap(),
            w2.coin_public_key_hex().unwrap()
        );
    }

    #[test]
    fn undeployed_demo_uses_genesis_seed() {
        let w = Wallet::demo(Network::Undeployed);
        assert_eq!(w.seed_hex(), UNDEPLOYED_GENESIS_SEED_HEX);
    }

    #[test]
    fn demo_seed_differs_per_network_class() {
        let pre = Wallet::demo(Network::PreProd);
        let und = Wallet::demo(Network::Undeployed);
        assert_ne!(pre.seed_hex(), und.seed_hex());
    }

    #[test]
    fn dust_public_key_hex_is_deterministic_per_seed() {
        // DustSecretKey doesn't derive PartialEq, so we compare
        // via the public-key hex (which round-trips through the
        // canonical Fr → 32-LE-bytes path).
        let a = Wallet::demo(Network::Undeployed).dust_public_key_hex().unwrap();
        let b = Wallet::demo(Network::Undeployed).dust_public_key_hex().unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn dust_public_key_hex_is_64_chars() {
        let hex = Wallet::demo(Network::Undeployed).dust_public_key_hex().unwrap();
        assert_eq!(hex.len(), 64, "expected 32-byte hex, got {hex}");
        assert!(hex.chars().all(|c| c.is_ascii_hexdigit() && !c.is_uppercase()));
    }

    #[test]
    fn dust_public_key_differs_per_seed() {
        let pre = Wallet::demo(Network::PreProd).dust_public_key_hex().unwrap();
        let und = Wallet::demo(Network::Undeployed).dust_public_key_hex().unwrap();
        assert_ne!(pre, und);
    }

    /// Minimal `chain::*` trait stubs used solely to confirm the
    /// `Wallet::with_deps` constructor accepts trait objects and
    /// hands them back via the public accessors. Real chain-op
    /// stubs live in Task 1.5.D's `stub_wallet` module.
    mod with_deps_stubs {
        use crate::chain;
        use crate::indexer::{ChainTipInfo, ContractStateInfo, IndexerError};
        use crate::node::{NodeError, SubmitResult};
        use crate::tx::TxError;
        use crate::tx::build::UnprovenTx;
        use crate::tx::prove::ProvenTx;
        use async_trait::async_trait;

        pub(super) struct NopIndexer;
        #[async_trait]
        impl chain::IndexerClient for NopIndexer {
            async fn chain_tip(&self) -> Result<Option<ChainTipInfo>, IndexerError> {
                Ok(None)
            }
            async fn contract_state(
                &self,
                _: &str,
            ) -> Result<Option<ContractStateInfo>, IndexerError> {
                Ok(None)
            }
        }

        pub(super) struct NopNode;
        #[async_trait]
        impl chain::NodeClient for NopNode {
            async fn submit_deploy(
                &self,
                _: Vec<u8>,
                _: &crate::MidnightSigner,
            ) -> Result<SubmitResult, NodeError> {
                Ok(SubmitResult { tx_hash: [0u8; 32], block_hash: [0u8; 32] })
            }
        }

        pub(super) struct NopProver;
        #[async_trait]
        impl chain::Prover for NopProver {
            async fn prove(&self, _: UnprovenTx) -> Result<ProvenTx, TxError> {
                Err(TxError::Prove("nop prover; with_deps test only".into()))
            }
        }
    }

    #[test]
    fn with_deps_constructs_and_exposes_injected_clients() {
        use std::sync::Arc;
        let indexer: Arc<dyn crate::chain::IndexerClient> =
            Arc::new(with_deps_stubs::NopIndexer);
        let node: Arc<dyn crate::chain::NodeClient> = Arc::new(with_deps_stubs::NopNode);
        let prover: Arc<dyn crate::chain::Prover> = Arc::new(with_deps_stubs::NopProver);
        let seed = [0u8; 32];
        let w = Wallet::with_deps(
            seed,
            Network::Undeployed,
            Arc::clone(&indexer),
            Arc::clone(&node),
            Arc::clone(&prover),
        );
        // Seed + network round-trip unchanged.
        assert_eq!(w.seed_hex(), hex::encode(seed));
        assert_eq!(w.network(), Network::Undeployed);
        // Each injected dep is exposed via its accessor as the
        // *same* Arc (compare by ptr to avoid trait equality).
        assert!(Arc::ptr_eq(&w.indexer().unwrap(), &indexer));
        assert!(Arc::ptr_eq(&w.node().unwrap(), &node));
        assert!(Arc::ptr_eq(&w.prover().unwrap(), &prover));
    }

    #[test]
    fn from_seed_leaves_chain_deps_unset() {
        let w = Wallet::from_seed([0u8; 32], Network::Undeployed);
        assert!(w.indexer().is_none());
        assert!(w.node().is_none());
        assert!(w.prover().is_none());
    }

    /// Confirms the `create_did_awaitable` wrapper compiles against
    /// the `with_deps` constructor and propagates the underlying
    /// `WizardStage::Failed` as `WalletError::CreateDidFailed`.
    ///
    /// Task 1.5.D's `stub_wallet` factory will plug in a working
    /// dust path so the happy `Ok(DidId)` branch is exercisable;
    /// today the wallet's `sync_dust` always opens its own WS
    /// (regardless of injected `IndexerClient`), so against
    /// `Network::Undeployed` with no live indexer the stream
    /// terminates at `Failed("sync dust: ...")`. Either way the
    /// wrapper must surface a `CreateDidFailed`.
    #[tokio::test]
    async fn create_did_awaitable_surfaces_failed_stage_as_walleterror() {
        use std::sync::Arc;
        let w = Wallet::with_deps(
            [0u8; 32],
            Network::Undeployed,
            Arc::new(with_deps_stubs::NopIndexer),
            Arc::new(with_deps_stubs::NopNode),
            Arc::new(with_deps_stubs::NopProver),
        );
        let result = w.create_did_awaitable().await;
        assert!(
            matches!(result, Err(WalletError::CreateDidFailed(_))),
            "expected CreateDidFailed, got {result:?}",
        );
    }

    /// Same shape assertion for `add_verification_method`. The
    /// wallet's `call_did_circuit` stream fails at the dust-sync
    /// step before it ever touches the JS harness or the JSON args,
    /// so the test exercises the wrapper's stream-draining /
    /// error-mapping path without needing a working chain.
    #[tokio::test]
    async fn add_verification_method_propagates_pipeline_failure() {
        use std::sync::Arc;
        let w = Wallet::with_deps(
            [0u8; 32],
            Network::Undeployed,
            Arc::new(with_deps_stubs::NopIndexer),
            Arc::new(with_deps_stubs::NopNode),
            Arc::new(with_deps_stubs::NopProver),
        );
        let did = crate::DidId::new(Network::Undeployed, [0u8; 32]);
        let key_ref = crate::secret_storage::SecretKeyRef::new(
            "00000000-0000-0000-0000-000000000000",
            "ed25519/authentication",
        );
        let vm_json = serde_json::json!({
            "id": format!("{}#key-1", did.to_did_string()),
            "typ": 1,
            "publicKeyJwk": {
                "kty": 3,
                "crv": 0,
                "x": { "$bigint": "0" },
                "y": { "$bigint": "0" },
            },
        });
        let result = w
            .add_verification_method(&did, &key_ref, vm_json, [0u8; 32])
            .await;
        assert!(
            matches!(result, Err(WalletError::CreateDidFailed(_))),
            "expected CreateDidFailed, got {result:?}",
        );
    }

    /// Same shape assertion for
    /// `add_verification_method_relation`. Confirms the wrapper's
    /// args-encoding (relation enum → JS-side int, fragment passed
    /// through as a string) compiles and the error path lights up.
    #[tokio::test]
    async fn add_verification_method_relation_propagates_pipeline_failure() {
        use std::sync::Arc;
        let w = Wallet::with_deps(
            [0u8; 32],
            Network::Undeployed,
            Arc::new(with_deps_stubs::NopIndexer),
            Arc::new(with_deps_stubs::NopNode),
            Arc::new(with_deps_stubs::NopProver),
        );
        let did = crate::DidId::new(Network::Undeployed, [0u8; 32]);
        let result = w
            .add_verification_method_relation(
                &did,
                "key-1",
                crate::VerificationMethodRelation::Authentication,
                [0u8; 32],
            )
            .await;
        assert!(
            matches!(result, Err(WalletError::CreateDidFailed(_))),
            "expected CreateDidFailed, got {result:?}",
        );
    }
}
