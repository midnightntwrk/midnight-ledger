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
mod crypto;
mod did;
mod hd;
mod indexer;
mod network;
mod node;
mod probe;
mod wallet;

pub use did::{
    CONTRACT_ADDRESS_LEN, ContractAddressBytes, CurveType, DidDocument, DidError, DidId,
    DidIdError, KeyType, PublicKeyJwk, Service, ServiceEndpoint, VerificationMethod,
    VerificationMethodRef, VerificationMethodRelation, VerificationMethodType,
};

pub use address::{AddressError, truncate_middle, unshielded_bech32m, unshielded_hrp};
pub use hd::{HdError, Role};
pub use indexer::{ChainTipInfo, ContractStateInfo, IndexerClient, IndexerError};
pub use network::{Network, NetworkConfig};
pub use node::{NodeClient, NodeError, NodeHealth, NodeStatus};
pub use probe::{ProbeError, ProbeResult, ProbeStatus, probe_connectivity};
pub use wallet::{DEMO_SEED_HEX, Wallet, WalletError};
