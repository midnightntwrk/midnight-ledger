//! Test-support scaffolding for Identity Centre Task 2 and beyond.
//!
//! The `stub_wallet` factory produces a [`crate::Wallet`] that
//! short-circuits the four DID write/read methods Task 2's
//! `bootstrap_did_with_keys` orchestration drives:
//!
//! - [`crate::Wallet::create_did_awaitable`]
//! - [`crate::Wallet::add_verification_method`]
//! - [`crate::Wallet::add_verification_method_relation`]
//! - [`crate::Wallet::resolve_did`]
//!
//! The stub keeps an in-process `HashMap<DidId, DidDocument>` shared
//! between the wallet and its injected [`StubIndexerClient`]. Each
//! awaitable method mutates the map directly; `resolve_did` reads
//! from it. The full chain-op pipeline (JS bridge ->
//! prepareUnprovenCallTx -> balance -> prove -> submit) is NOT
//! exercised — that pipeline requires a JS Compact runtime, a real
//! dust state, live `LedgerParameters`, and halo2 proving, none of
//! which fits the <300-LOC budget for Task 1.5.D.
//!
//! Pipeline-level fidelity (Path 1 in the Task 1.5.D brief) is a
//! Phase 2 follow-up; for Task 2's "does the orchestration body
//! call the four wallet methods in the right order with the right
//! args?" question, the wallet-level bypass (Path 2) is sufficient.
//!
//! Gated behind `#[cfg(any(test, feature = "test-support"))]` so
//! none of this lands in release builds unless a downstream crate
//! opts in via the `test-support` feature.

#![cfg(any(test, feature = "test-support"))]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use crate::chain::{IndexerClient, NodeClient, Prover};
use crate::indexer::{ChainTipInfo, ContractStateInfo, IndexerError};
use crate::node::{NodeError, SubmitResult};
use crate::secret_storage::InMemorySecretStore;
use crate::tx::TxError;
use crate::tx::build::UnprovenTx;
use crate::tx::prove::ProvenTx;
use crate::{DidDocument, DidId, MidnightSigner, Network, Wallet};

/// Shared in-memory DID-document map. The stub wallet and its
/// injected [`StubIndexerClient`] hold the same `Arc<Mutex<_>>` so
/// mutations made via `Wallet::add_verification_method` /
/// `Wallet::add_verification_method_relation` (which go through the
/// stub-mode bypass on `Wallet`) are immediately visible to
/// `resolve_did` and to direct `IndexerClient::contract_state`
/// queries.
pub(crate) type StubDidMap = Arc<Mutex<HashMap<DidId, DidDocument>>>;

/// In-memory indexer stub.
///
/// `chain_tip` returns `None` (no chain — the stub doesn't simulate
/// blocks); `contract_state` returns `None` for every address (DID
/// state is served via the wallet-level bypass and `resolve_did`,
/// not through the indexer trait, because `ContractStateInfo`
/// requires a SCALE-encoded `state_hex` we don't synthesise).
#[derive(Clone, Debug)]
pub struct StubIndexerClient {
    state: StubDidMap,
}

impl StubIndexerClient {
    /// Build a stub indexer sharing `state` with the parent wallet.
    pub fn new(state: StubDidMap) -> Self {
        Self { state }
    }

    /// Direct accessor for the shared map. Tests use this to
    /// pre-seed a DID document or to assert on the final state
    /// without round-tripping through `Wallet::resolve_did`.
    pub fn state(&self) -> StubDidMap {
        Arc::clone(&self.state)
    }
}

#[async_trait]
impl IndexerClient for StubIndexerClient {
    async fn chain_tip(&self) -> Result<Option<ChainTipInfo>, IndexerError> {
        Ok(None)
    }

    async fn contract_state(
        &self,
        _address_hex: &str,
    ) -> Result<Option<ContractStateInfo>, IndexerError> {
        // The wallet-level stub bypass serves DID state directly
        // (see Wallet::resolve_did's stub-mode branch); the trait
        // method returns None so that any code that reaches here
        // bails cleanly rather than fabricating bogus state_hex.
        Ok(None)
    }
}

/// In-memory node stub. `submit_deploy` records each call's byte
/// length in `submitted_lens` and returns a canned `SubmitResult`
/// with zero hashes. The wallet-level bypass means this is never
/// reached during Task 2's unit-test flow, but the impl exists so
/// `Wallet::with_deps` accepts a valid trait object.
#[derive(Clone, Debug, Default)]
pub struct StubNodeClient {
    submitted_lens: Arc<Mutex<Vec<usize>>>,
}

impl StubNodeClient {
    pub fn new() -> Self {
        Self::default()
    }

    /// Byte lengths of every `submit_deploy` call. Test-only
    /// observability hook.
    pub fn submitted_lens(&self) -> Vec<usize> {
        self.submitted_lens.lock().expect("poisoned").clone()
    }
}

#[async_trait]
impl NodeClient for StubNodeClient {
    async fn submit_deploy(
        &self,
        bytes: Vec<u8>,
        _signer: &MidnightSigner,
    ) -> Result<SubmitResult, NodeError> {
        self.submitted_lens
            .lock()
            .expect("poisoned")
            .push(bytes.len());
        Ok(SubmitResult { tx_hash: [0u8; 32], block_hash: [0u8; 32] })
    }
}

/// Pass-through prover stub. Returns `TxError::Prove` because we
/// can't synthesise a `ProvenTx` out of thin air — every halo2
/// proof is curve-specific data — and the wallet-level bypass means
/// no test reaches the prove stage anyway. Exists so
/// `Wallet::with_deps` accepts a valid trait object.
#[derive(Clone, Debug, Default)]
pub struct StubProver;

#[async_trait]
impl Prover for StubProver {
    async fn prove(&self, _tx: UnprovenTx) -> Result<ProvenTx, TxError> {
        Err(TxError::Prove(
            "stub prover; Task 2 unit tests use the wallet-level bypass and \
             do not reach the prove stage".into(),
        ))
    }
}

/// Default test secret store — re-exports
/// [`InMemorySecretStore::default`] under the name the brief and
/// Task 2 reference. Trivial wrapper kept so the factory namespace
/// is self-contained.
pub fn stub_secret_store() -> InMemorySecretStore {
    InMemorySecretStore::default()
}

/// Deterministic 32-byte seed used by [`stub_wallet`]. The exact
/// value doesn't matter; we pick a non-zero pattern so any
/// downstream code that checks "did the test forget to set a seed?"
/// can spot it.
const STUB_SEED: [u8; 32] = [0x5Au8; 32];

/// Factory: a [`Wallet`] wired with in-memory indexer + node +
/// prover stubs **and** stub-mode flag enabled. The four DID
/// write/read methods short-circuit to the shared
/// `Arc<Mutex<HashMap<DidId, DidDocument>>>` rather than driving
/// the live chain-op pipeline.
///
/// Used by Task 2's `bootstrap_did_with_keys` unit tests; can also
/// be used by any future test that needs a "wallet I can call the
/// four awaitable DID methods on, deterministically, without a
/// live indexer / node / proof-server".
pub fn stub_wallet() -> Wallet {
    let state: StubDidMap = Arc::new(Mutex::new(HashMap::new()));
    let indexer = Arc::new(StubIndexerClient::new(Arc::clone(&state)));
    let node = Arc::new(StubNodeClient::new());
    let prover = Arc::new(StubProver);
    Wallet::with_deps(STUB_SEED, Network::Undeployed, indexer, node, prover)
        .with_stub_did_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secret_storage::SecretKeyRef;
    use crate::VerificationMethodRelation;

    #[tokio::test]
    async fn stub_wallet_create_did_awaitable_returns_did() {
        let wallet = stub_wallet();
        let did = wallet.create_did_awaitable().await.expect("ok");
        assert!(did.to_did_string().starts_with("did:midnight:"));
    }

    #[tokio::test]
    async fn stub_wallet_add_vm_then_resolve_shows_it() {
        let wallet = stub_wallet();
        let did = wallet.create_did_awaitable().await.expect("did");

        let jwk_json = serde_json::json!({
            "id": format!("{}#key-auth", did.to_did_string()),
            "type": "Ed25519VerificationKey2020",
            "controller": did.to_did_string(),
            "publicKeyJwk": { "kty": "OKP", "crv": "Ed25519", "x": "ABCD" }
        });
        let key_ref = SecretKeyRef::new("stub-uuid", "ed25519/authentication");
        wallet
            .add_verification_method(&did, &key_ref, jwk_json, [0u8; 32])
            .await
            .expect("ok");
        wallet
            .add_verification_method_relation(
                &did,
                "key-auth",
                VerificationMethodRelation::Authentication,
                [0u8; 32],
            )
            .await
            .expect("ok");

        let doc = wallet
            .resolve_did(&did.to_did_string())
            .await
            .expect("resolve");
        assert!(!doc.authentication.is_empty());
        assert!(!doc.verification_method.is_empty());
    }
}
