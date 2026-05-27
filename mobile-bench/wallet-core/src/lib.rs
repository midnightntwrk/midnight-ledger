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
mod crypto;
mod did;
mod dust;
mod hd;
mod indexer;
pub mod js_bridge;
mod network;
mod node;
mod probe;
pub mod secret_storage;
pub mod store;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
mod tx;
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
    DidIdError, KeyType, PublicKeyJwk, ResolvedDid, Service, ServiceEndpoint, VerificationMethod,
    VerificationMethodRef, VerificationMethodRelation, VerificationMethodType,
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
/// `SHA-256(addVerificationMethod.prover_key_bytes)`. Because
/// the prover key is deterministic per circuit version, every
/// DID created by the manager shares this one constant.
///
/// Use this when the wallet wants to drive write circuits
/// against DIDs minted elsewhere — e.g. the PreProd live demo
/// where the manager created the DIDs, not the prototype.
pub fn upstream_demo_controller_secret() -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(did::artifacts::ADD_VERIFICATION_METHOD.prover_key);
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
pub use probe::{ProbeError, ProbeResult, ProbeStatus, probe_connectivity};
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
pub use vc_store::{StoredVc, VcMetadata, VcOpening, VcStore};
pub use did_auth::{sign_for_authentication, DidAuthError};
pub use oid4vp_client::{run_authentication as oid4vp_run_authentication, AuthFlowError};
pub use oid4vci_client::{run_issuance as oid4vci_run_issuance, IssuanceFlowError};
pub use vc_self_verify::{self_verify, self_verify_and_cache, InvalidReason, SelfVerifyResult};
pub use qr_scanner::{QrScanner, QrScanError, PasteUrlScanner};
