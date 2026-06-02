//! wallet-core: pure-Rust Midnight wallet primitives consumed by the
//! `dioxus-wallet` UI and (eventually) by other front-ends.
//!
//! ## Architecture: ports + adapters
//!
//! Inner hexagon: pure domain (DIDs, wallets, OID4VP/OID4VCI
//! flows, VC storage semantics) expressed against traits. Every
//! side effect — clock, RNG, HTTP, chain RPC, persistent store,
//! UI events, signing — happens through a port (trait). Adapters
//! live either in this crate (anything every caller can reuse —
//! reqwest, redb, the system clock) or in `dioxus-wallet` (the
//! Wry WebView JS bridge, the Android secret store, the
//! Tailscale-aware network picker).
//!
//! See `docs/superpowers/specs/2026-06-03-hex-architecture-audit.md`
//! for the full inventory + improvement roadmap.

#![deny(unreachable_pub)]
#![deny(warnings)]

mod address;
mod artifacts;
pub mod chain;
pub mod chain_publisher;
pub mod clock;
mod crypto;
mod did;
mod dust;
mod hd;
pub mod http;
mod indexer;
pub mod js_bridge;
mod network;
mod node;
pub mod notifications;
mod probe;
pub mod randomness;
pub mod secret_storage;
pub mod service;
pub mod store;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
pub mod telemetry;
mod tx;
pub mod ui_port;
pub mod unlock;
mod unshielded;
mod wallet;
pub mod vc_store;
pub mod oid4vp_client;
pub mod oid4vci_client;
pub mod vc_self_verify;
pub mod qr_scanner;
pub mod prelude;
#[cfg(any(test, feature = "test-support"))]
pub mod headless;

// ─── Domain types ──────────────────────────────────────────────
// The "inner hexagon" content: types every layer of the stack
// reasons about. No I/O, no scheduling, no port dependencies.

pub use address::{
    AddressError, shielded_bech32m, shielded_hrp, truncate_middle, unshielded_bech32m,
    unshielded_hrp,
};
pub use did::{
    CONTRACT_ADDRESS_LEN, ContractAddressBytes, CurveType, DidDocument, DidError, DidId,
    DidIdError, KeyType, PublicKeyJwk, ResolvedDid, SchnorrJubjubVerificationMethod, Service,
    ServiceEndpoint, VerificationMethod, VerificationMethodRef, VerificationMethodRelation,
    VerificationMethodType,
};
pub use hd::{HdError, Role};
pub use network::{Network, NetworkConfig};
pub use ledger::dust::{DustLocalState, DustPublicKey, DustSecretKey};
pub use dust::DustError;
pub use unshielded::{TokenType, UnshieldedError, UnshieldedUtxo, UtxoId, UtxoSet};
pub use tx::{DeployOutcome, TxError, WizardStage};
pub use wallet::{
    BalanceSnapshot, DEMO_SEED_HEX, UNDEPLOYED_GENESIS_SEED_HEX, Wallet, WalletError,
};
pub use vc_store::{StoredVc, VcMetadata, VcOpening};

// ─── Ports ─────────────────────────────────────────────────────
// Trait surfaces every side effect routes through. Adapters
// (next section) provide the implementations callers actually
// wire up.

// Time + randomness
pub use clock::Clock;
pub use randomness::Randomness;

// Network — outbound HTTP and chain RPC
pub use http::{HttpClient, HttpError, HttpResponse};
pub use chain::{IndexerClient, NodeClient, Prover};
pub use chain_publisher::{ChainError, ChainPublisher};

// Storage
pub use store::api::WalletStorage;
pub use vc_store::{VcStorage, VcStoreError};
// `SecretStorage` lives under `pub mod secret_storage` — read it
// at the fully-qualified path. Re-exporting trips the
// `unreachable_pub` lint because the trait is sealed behind a
// `pub(crate)` module reorganisation pending.

// DID-protocol convenience ports — composed on top of the
// chain + storage ports.
// `oid4vp_client::{DidAuthnDiscovery, DidSigner, ResponseBuilder}`
// live in the `oid4vp_client` module; callers reach them via the
// fully-qualified path so the OID4VP namespace stays cohesive.

// Side effects — UI, notifications, observability.
pub use notifications::Notifications;
pub use ui_port::{UiEvent, UiOutcome, UserInterface};
pub use telemetry::{Metrics, ResourceProbe};

// App-level
pub use unlock::{UnlockGate, UnlockOutcome};
pub use qr_scanner::{QrScanError, QrScanner};

// ─── Adapters — production ─────────────────────────────────────
// Concrete implementations callers wire to ports in production.
// Test-only adapters live in the "test-support" section.

// Time + randomness
pub use clock::SystemClock;
pub use randomness::OsRandomness;

// Network — outbound HTTP
pub use http::ReqwestHttpClient;

// Chain — indexer / node / prover. Each pair is the live
// adapter + a metering decorator.
pub use indexer::{ChainTipInfo, ContractStateInfo, HttpIndexerClient, IndexerError};
pub use node::{
    MidnightSigner, NodeError, NodeHealth, NodeStatus, SignerError, SubmitResult, SubxtNodeClient,
};
pub use chain::{HttpProver, LocalProver};
pub use chain_publisher::{CallReceipt, RecordedCall};

// Side effects
pub use notifications::{NotifyLevel, NotifyRecord, StderrNotifier};
pub use ui_port::{NoopUiAdapter, UiError};
pub use telemetry::{
    noop_metrics, time_op, time_op_simple, CompositeMetrics, HistogramSnapshot, HttpRecord,
    InMemoryMetrics, MeteredHttpClient, MeteredIndexerClient, MeteredNodeClient, MeteredProver,
    MetricsSnapshot, NoopMetrics, NoopResourceProbe, OpHistogramSnapshot, OpOutcome,
    OpRecord, ResourceSample, RusageProbe, TracingMetrics,
};

// Storage
pub use vc_store::RedbVcStore;

// App-level
pub use unlock::ScryptUnlockGate;
pub use qr_scanner::PasteUrlScanner;

// Misc utility (crypto bootstrap)
pub use crypto::ensure_default_crypto_provider;

// ─── Adapters — test-support ───────────────────────────────────
// Gated behind `#[cfg(any(test, feature = "test-support"))]`.
// Downstream crates that want the doubles in their own tests opt
// in via `wallet-core = { features = ["test-support"] }`.

#[cfg(any(test, feature = "test-support"))]
pub use clock::FixedClock;
#[cfg(any(test, feature = "test-support"))]
pub use randomness::DeterministicRng;
#[cfg(any(test, feature = "test-support"))]
pub use store::api::InMemoryWalletStorage;
#[cfg(any(test, feature = "test-support"))]
pub use vc_store::InMemoryVcStore;
#[cfg(any(test, feature = "test-support"))]
pub use chain_publisher::StubChainPublisher;
pub use notifications::{CollectingNotifier, NoopNotifier};
pub use ui_port::TestUiAdapter;
pub use unlock::{AlwaysOkUnlockGate, NeverOkUnlockGate};

// ─── Use cases / orchestrators ─────────────────────────────────
// The application core's business logic — each composes multiple
// ports to drive a single user-meaningful operation.

pub use crate::did::{bootstrap_did_with_keys, derive_keys, BootstrapError, BootstrappedDid};
pub use oid4vci_client::{run_issuance as oid4vci_run_issuance, IssuanceFlowError};
pub use vc_self_verify::{self_verify, self_verify_and_cache, InvalidReason, SelfVerifyResult};
pub use probe::{ProbeError, ProbeResult, ProbeStatus, probe_connectivity};
pub use dust::syncer::{DustSyncer, SyncProgress};
// OID4VP entry point — dioxus-wallet calls
// `oid4vp_client::run_authentication` directly through the
// fully-qualified path (commit 758a5fa3); no lib-level alias.
// The shared signing helper
// `oid4vp_client::id_token::sign_id_token_with_ports` drives both
// OID4VP id_tokens and OID4VCI proof-of-possession JWSs — see
// the audit doc §3 for the symmetry argument.

/// Names of every DID circuit whose verifier key is bundled and
/// loadable via [`Wallet::load_did_circuit`].
pub fn did_circuit_names() -> &'static [&'static str] {
    did::artifacts::CIRCUIT_NAMES
}

/// The 32-byte controller secret upstream
/// `midnight-did-manager-service` uses for every DID it
/// mints. Derivation matches `midnight-did/api/src/lib.ts::initPrivateState`:
/// `SHA-256(setVerificationMethod.prover_key_bytes)`. Because
/// the prover key is deterministic per circuit version, every
/// DID created by the manager shares this one constant.
///
/// Use this when the wallet wants to drive write circuits
/// against DIDs minted elsewhere — e.g. the PreProd live demo
/// where the manager created the DIDs, not the prototype.
///
/// 2026-05-28 schema refresh: the `addVerificationMethod` circuit
/// was renamed `setVerificationMethod`; the derivation now hashes
/// the new circuit's prover bytes. If the upstream manager-service
/// still uses the OLD prover key for its derivation, the two
/// secrets will no longer agree — re-vendor & re-run a
/// preprod-fetched DID's controller probe to confirm parity before
/// shipping against the new contract on a shared-DID network.
pub fn upstream_demo_controller_secret() -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(did::artifacts::SET_VERIFICATION_METHOD.prover_key);
    h.finalize().into()
}

// ─── Internal-only re-exports ──────────────────────────────────
// Surface that exists because cargo can't render `pub(crate)`
// across an inner module boundary, not because callers should
// reach for it.

#[doc(hidden)]
pub use did::deploy::{testing_deploy_state_with_circuits_hex, testing_initial_deploy_state_hex};
