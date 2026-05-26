//! Chain-op trait seams.
//!
//! These three traits — [`IndexerClient`], [`NodeClient`], and
//! [`Prover`] — extract the *small* surface that [`crate::Wallet`]
//! actually consumes from each chain dependency. They exist so
//! Task 1.5.D can wire a `stub_wallet` factory that drives the
//! same write-pipeline code paths with deterministic in-process
//! mocks (no live indexer / node / proof-server required).
//!
//! Design notes:
//! - The bare names `IndexerClient` / `NodeClient` are now the
//!   *trait* names. The concrete impls live in
//!   [`crate::indexer::HttpIndexerClient`] and
//!   [`crate::node::SubxtNodeClient`]. Old `IndexerClient` and
//!   `NodeClient` exports are kept as type aliases for backward
//!   compatibility with downstream callers (tests, dioxus-wallet).
//! - The trait surface is the minimal set [`crate::Wallet`]
//!   actually invokes — `chain_tip` + `contract_state` for the
//!   indexer, `submit_deploy` for the node, `prove` (with optional
//!   proof-server URL) for the prover.
//! - All methods are `async` via [`async_trait::async_trait`] so
//!   the traits are dyn-compatible — `Wallet` holds them as
//!   `Arc<dyn Trait>` so multiple clones share one client.

use std::sync::Arc;

use async_trait::async_trait;
use rand::SeedableRng;
use rand::rngs::StdRng;

use crate::indexer::{ChainTipInfo, ContractStateInfo, IndexerError};
use crate::node::{NodeError, SubmitResult};
use crate::tx::TxError;
use crate::tx::build::UnprovenTx;
use crate::tx::prove::ProvenTx;
use crate::MidnightSigner;
use crate::Network;

/// The subset of indexer GraphQL operations [`crate::Wallet`]
/// invokes from its chain-op methods. See module doc for the
/// rationale on why the trait surface is intentionally tiny.
#[async_trait]
pub trait IndexerClient: Send + Sync + 'static {
    /// Latest block known to the indexer. `None` if the indexer
    /// is reachable but has no blocks yet (cold-start).
    async fn chain_tip(&self) -> Result<Option<ChainTipInfo>, IndexerError>;

    /// Latest contract action (deploy / call / update) for the
    /// given hex-encoded contract address. `None` if the indexer
    /// doesn't know about the address.
    async fn contract_state(
        &self,
        address_hex: &str,
    ) -> Result<Option<ContractStateInfo>, IndexerError>;
}

/// The subset of substrate-node RPC [`crate::Wallet`] invokes
/// from its chain-op streams. Today this is just `submit_deploy`
/// — the existing `health` / `finalized_head` / `status` methods
/// on [`crate::node::SubxtNodeClient`] are only used by the
/// connectivity probe and stay outside the trait.
#[async_trait]
pub trait NodeClient: Send + Sync + 'static {
    /// Submit a SCALE-encoded Midnight transaction via the
    /// `Midnight.send_mn_transaction(bytes)` runtime call, then
    /// wait for it to be included in a block. See
    /// [`crate::node::SubxtNodeClient::submit_deploy`] for the
    /// rationale on the unsigned envelope.
    async fn submit_deploy(
        &self,
        bytes: Vec<u8>,
        signer: &MidnightSigner,
    ) -> Result<SubmitResult, NodeError>;
}

/// ZK proof generation for an `UnprovenTx`. Two impls today:
/// [`LocalProver`] runs `zkir_v2::LocalProvingProvider` in-process
/// (fine for tests, slow for debug-built mobile), and
/// [`HttpProver`] routes per-preimage `prove` calls to a
/// `midnight-proof-server` `/prove` endpoint.
#[async_trait]
pub trait Prover: Send + Sync + 'static {
    /// Prove every `ProofPreimage` carried by the balanced `tx`
    /// and seal the resulting transaction so the chain accepts
    /// the on-wire header (`pedersen-schnorr[v1]`).
    async fn prove(&self, tx: UnprovenTx) -> Result<ProvenTx, TxError>;
}

/// In-process zkir prover. Mirrors `crate::tx::prove::prove`.
/// Holds no state — each call seeds a fresh `StdRng` from the OS
/// entropy pool, same as the inline call sites used before this
/// refactor.
#[derive(Default, Debug, Clone, Copy)]
pub struct LocalProver;

#[async_trait]
impl Prover for LocalProver {
    async fn prove(&self, tx: UnprovenTx) -> Result<ProvenTx, TxError> {
        let rng = StdRng::from_entropy();
        crate::tx::prove::prove(tx, rng).await
    }
}

/// Proof-server-backed prover. Carries the base URL only —
/// `crate::tx::prove::prove_via_http` builds its own
/// `reqwest::Client` per call (same per-call shape as the inline
/// call sites used before this refactor).
#[derive(Debug, Clone)]
pub struct HttpProver {
    base_url: String,
}

impl HttpProver {
    /// Build a proof-server prover. `base_url` is e.g.
    /// `http://127.0.0.1:57610` — no trailing `/prove`, the
    /// provider appends that itself.
    pub fn new(base_url: impl Into<String>) -> Self {
        Self { base_url: base_url.into() }
    }
}

#[async_trait]
impl Prover for HttpProver {
    async fn prove(&self, tx: UnprovenTx) -> Result<ProvenTx, TxError> {
        let rng = StdRng::from_entropy();
        crate::tx::prove::prove_via_http(tx, rng, self.base_url.clone()).await
    }
}

/// Convenience: build the default real-deps prover from an
/// optional proof-server URL. `None` → `LocalProver`; `Some(url)`
/// → `HttpProver`. Used by [`crate::Wallet::with_deps`] and the
/// in-stream fallback paths so the selection logic is in one
/// place.
#[allow(dead_code)] // Wired by Wallet::with_deps in Task 1.5.B B.5+
pub(crate) fn default_prover(proof_server_url: Option<&str>) -> Arc<dyn Prover> {
    match proof_server_url {
        Some(url) => Arc::new(HttpProver::new(url.to_owned())),
        None => Arc::new(LocalProver),
    }
}

/// Convenience: build the default real-deps indexer client for
/// a network. Wraps [`crate::indexer::HttpIndexerClient::new`]
/// and returns it behind the trait object so the call site
/// doesn't care about the concrete type.
pub(crate) fn default_indexer(
    network: Network,
) -> Result<Arc<dyn IndexerClient>, IndexerError> {
    Ok(Arc::new(crate::indexer::HttpIndexerClient::new(network)?))
}
