//! wallet-core: pure-Rust Midnight wallet primitives consumed by the
//! `dioxus-wallet` UI and (eventually) by other front-ends.
//!
//! Iter-1 step-1 scope: seed → keys, network catalog, and a
//! connectivity probe that confirms the indexer + node URLs for the
//! selected network are reachable from this host. No transaction or
//! sync logic yet.

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
pub mod did_auth;
pub mod oid4vp_client;
pub mod oid4vci_client;
pub mod vc_self_verify;
pub mod qr_scanner;

pub use did::{
    CONTRACT_ADDRESS_LEN, ContractAddressBytes, CurveType, DidDocument, DidError, DidId,
    DidIdError, KeyType, PublicKeyJwk, ResolvedDid, SchnorrJubjubVerificationMethod, Service,
    ServiceEndpoint, VerificationMethod, VerificationMethodRef, VerificationMethodRelation,
    VerificationMethodType,
};
pub use crate::did::{bootstrap_did_with_keys, derive_keys, BootstrapError, BootstrappedDid};

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

pub use address::{AddressError, truncate_middle, unshielded_bech32m, unshielded_hrp};
pub use hd::{HdError, Role};
pub use indexer::{ChainTipInfo, ContractStateInfo, HttpIndexerClient, IndexerError};
pub use network::{Network, NetworkConfig};
pub use node::{
    MidnightSigner, NodeError, NodeHealth, NodeStatus, SignerError, SubmitResult, SubxtNodeClient,
};
pub use chain::{HttpProver, IndexerClient, LocalProver, NodeClient, Prover};
pub use chain_publisher::{CallReceipt, ChainError, ChainPublisher, RecordedCall, StubChainPublisher};
pub use clock::{Clock, SystemClock};
#[cfg(any(test, feature = "test-support"))]
pub use clock::FixedClock;
pub use http::{HttpClient, HttpError, HttpResponse, ReqwestHttpClient};
pub use telemetry::{
    noop_metrics, time_op, time_op_simple, CompositeMetrics, HistogramSnapshot, HttpRecord,
    InMemoryMetrics, MeteredHttpClient, MeteredIndexerClient, MeteredNodeClient, MeteredProver,
    Metrics, MetricsSnapshot, NoopMetrics, NoopResourceProbe, OpHistogramSnapshot, OpOutcome,
    OpRecord, ResourceProbe, ResourceSample, RusageProbe, TracingMetrics,
};
pub use probe::{ProbeError, ProbeResult, ProbeStatus, probe_connectivity};
pub use notifications::{
    CollectingNotifier, NoopNotifier, NotifyLevel, NotifyRecord, Notifications, StderrNotifier,
};
pub use randomness::{DeterministicRng, OsRandomness, Randomness};
pub use ui_port::{
    NoopUiAdapter, TestUiAdapter, UiError, UiEvent, UiOutcome, UserInterface,
};
pub use unlock::{AlwaysOkUnlockGate, NeverOkUnlockGate, ScryptUnlockGate, UnlockGate, UnlockOutcome};
pub use wallet::{
    BalanceSnapshot, DEMO_SEED_HEX, UNDEPLOYED_GENESIS_SEED_HEX, Wallet, WalletError,
};
pub use unshielded::{
    TokenType, UnshieldedError, UnshieldedUtxo, UtxoId, UtxoSet,
};
pub use crypto::ensure_default_crypto_provider;
pub use dust::syncer::{DustSyncer, SyncProgress};
#[doc(hidden)]
pub use did::deploy::{testing_deploy_state_with_circuits_hex, testing_initial_deploy_state_hex};
pub use dust::DustError;
pub use ledger::dust::{DustLocalState, DustPublicKey, DustSecretKey};
pub use tx::{DeployOutcome, TxError, WizardStage};
pub use vc_store::{RedbVcStore, StoredVc, VcMetadata, VcOpening, VcStorage, VcStoreError};
#[cfg(any(test, feature = "test-support"))]
pub use vc_store::InMemoryVcStore;
pub use did_auth::{sign_for_authentication, DidAuthError};
pub use oid4vp_client::{run_authentication as oid4vp_run_authentication, AuthFlowError};
pub use oid4vci_client::{run_issuance as oid4vci_run_issuance, IssuanceFlowError};
pub use vc_self_verify::{self_verify, self_verify_and_cache, InvalidReason, SelfVerifyResult};
pub use qr_scanner::{QrScanner, QrScanError, PasteUrlScanner};
