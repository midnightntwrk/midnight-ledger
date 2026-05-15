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
mod crypto;
mod did;
mod dust;
mod hd;
mod indexer;
mod network;
mod node;
mod probe;
mod tx;
mod unshielded;
mod wallet;

pub use did::{
    CONTRACT_ADDRESS_LEN, ContractAddressBytes, CurveType, DidDocument, DidError, DidId,
    DidIdError, KeyType, PublicKeyJwk, ResolvedDid, Service, ServiceEndpoint, VerificationMethod,
    VerificationMethodRef, VerificationMethodRelation, VerificationMethodType,
};

/// Names of every DID circuit whose verifier key is bundled and
/// loadable via [`Wallet::load_did_circuit`].
pub fn did_circuit_names() -> &'static [&'static str] {
    did::artifacts::CIRCUIT_NAMES
}

pub use address::{AddressError, truncate_middle, unshielded_bech32m, unshielded_hrp};
pub use hd::{HdError, Role};
pub use indexer::{ChainTipInfo, ContractStateInfo, IndexerClient, IndexerError};
pub use network::{Network, NetworkConfig};
pub use node::{
    MidnightSigner, NodeClient, NodeError, NodeHealth, NodeStatus, SignerError, SubmitResult,
};
pub use probe::{ProbeError, ProbeResult, ProbeStatus, probe_connectivity};
pub use wallet::{DEMO_SEED_HEX, UNDEPLOYED_GENESIS_SEED_HEX, Wallet, WalletError};
pub use unshielded::{
    TokenType, UnshieldedError, UnshieldedUtxo, UtxoId, UtxoSet,
};
pub use dust::DustError;
pub use ledger::dust::{DustLocalState, DustPublicKey, DustSecretKey};
pub use tx::{DeployOutcome, TxError, WizardStage};
