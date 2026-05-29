//! `ChainPublisher` port — the chain-write surface that services
//! reach for to deploy / mutate / deactivate DIDs.
//!
//! Why a port instead of letting services hold a `Wallet`
//! directly: `Wallet` is 1500+ lines of carefully sequenced
//! indexer-settle / VK-load / counter-bump logic baked from
//! real PreProd + standalone bug fixes.  Pulling it apart in
//! the same pass as the hex completion would multiply risk for
//! no test-coverage gain.  This port treats `Wallet` as the
//! "real adapter" behind a narrow trait surface; services
//! consume `Arc<dyn ChainPublisher>` and tests swap in a stub
//! that returns canned answers.
//!
//! See design doc §2.3 (`ChainPublisher` port) and §3 Wave B4.
//!
//! Wave B4 (this commit): trait + `StubChainPublisher` test
//! adapter.  The `WalletChainPublisher` real adapter (which
//! wraps `Wallet`) lives in `wallet.rs` itself and lands in a
//! follow-up sub-commit once we agree on the exact `Wallet`
//! method shape that wraps cleanly (the existing
//! `Wallet::call_did_circuit` etc. surfaces a Stream of
//! `WizardStage`s; the trait below returns a `Result`, so the
//! adapter has to either await the terminal stage or expose
//! a streaming variant — to be decided at wave C5 when
//! `DidService` consumes the port).

use std::sync::Mutex;

use async_trait::async_trait;
use serde_json::Value;

use crate::DidId;

/// Outcome of a chain write — opaque to the service layer, but
/// carries the fields the UI / metrics surface today.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallReceipt {
    /// Hex-encoded tx hash assigned by the node at submit.
    pub tx_hash: String,
    /// Hex-encoded block hash where the tx was included.
    pub block_hash: String,
}

/// Errors a service can react to.  Mirrors the variants
/// `Wallet::call_did_circuit` produces today, normalised so
/// service tests can assert on shape rather than free-form
/// strings.
#[derive(Debug, thiserror::Error)]
pub enum ChainError {
    #[error("indexer: {0}")]
    Indexer(String),
    #[error("node: {0}")]
    Node(String),
    #[error("prove: {0}")]
    Prove(String),
    #[error("contract assertion: {0}")]
    ContractAssertion(String),
    #[error("js bridge: {0}")]
    JsBridge(String),
    #[error("other: {0}")]
    Other(String),
}

/// Narrow trait — only the verbs services actually call.  Each
/// is the *terminal* outcome of a flow that today returns a
/// `Stream<WizardStage>`; the adapter consumes the stream and
/// folds it into one `Result`.  Streaming services that want
/// per-stage callbacks reach for `UserInterface::report_stage`
/// from the same call path.
#[async_trait]
pub trait ChainPublisher: Send + Sync + 'static {
    /// Deploy a fresh DID contract.  Returns the assigned DID
    /// + the random controller secret committed at deploy time.
    async fn create_did_with_controller(
        &self,
    ) -> Result<(DidId, [u8; 32]), ChainError>;

    /// Deactivate an existing DID.  Idempotent against an
    /// already-deactivated DID is NOT guaranteed by this trait
    /// — adapters may surface `ContractAssertion` or `Ok`
    /// depending on how the underlying flow models the
    /// second-call case.
    async fn deactivate_did(
        &self,
        did: &DidId,
        sk: &[u8; 32],
    ) -> Result<CallReceipt, ChainError>;

    /// Invoke a write circuit.  `arg` is the
    /// serde-JSON-shaped argument array the circuit expects.
    ///
    /// JSON leaks deliberately at the trait boundary — typed
    /// circuit args require typed Rust bindings from the
    /// Compact compiler which is still in flux upstream.  When
    /// the toolchain stabilises around typed Rust output the
    /// trait can grow per-circuit methods or take a typed
    /// `DidUpdateOp` enum; today the JSON path matches what
    /// `Wallet::call_did_circuit` carries on the wire.
    async fn call_did_circuit(
        &self,
        did: &DidId,
        circuit: &str,
        arg: Value,
        sk: &[u8; 32],
    ) -> Result<CallReceipt, ChainError>;

    /// Load a circuit's verifier key onto a freshly-deployed
    /// DID via MaintenanceUpdate.  `counter` is the
    /// maintenance-authority counter the wallet currently
    /// expects (read from the resolved doc + bumped per
    /// successful MaintenanceUpdate).
    async fn load_did_circuit(
        &self,
        did: &DidId,
        circuit: &str,
        counter: u64,
    ) -> Result<(), ChainError>;
}

// ───────────────────────────────────────────────────────────────
// `StubChainPublisher` — test adapter
// ───────────────────────────────────────────────────────────────

/// Scripted chain publisher for service tests.  Every method
/// has a queue of canned answers; methods pop from the queue
/// in FIFO order and `RecordedCall` is pushed for assertion.
///
/// Empty queue + a call → `ChainError::Other("stub: queue
/// empty")` so the test fails loudly instead of hanging.
#[derive(Default)]
pub struct StubChainPublisher {
    inner: Mutex<StubState>,
}

#[derive(Debug, Default)]
struct StubState {
    create_did_queue: std::collections::VecDeque<Result<(DidId, [u8; 32]), ChainError>>,
    deactivate_queue: std::collections::VecDeque<Result<CallReceipt, ChainError>>,
    call_circuit_queue: std::collections::VecDeque<Result<CallReceipt, ChainError>>,
    load_circuit_queue: std::collections::VecDeque<Result<(), ChainError>>,
    recorded: Vec<RecordedCall>,
}

/// One captured invocation against the stub.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordedCall {
    CreateDid,
    Deactivate {
        did: String,
    },
    CallCircuit {
        did: String,
        circuit: String,
        arg: Value,
    },
    LoadCircuit {
        did: String,
        circuit: String,
        counter: u64,
    },
}

impl StubChainPublisher {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_create_did(&self, result: Result<(DidId, [u8; 32]), ChainError>) {
        if let Ok(mut g) = self.inner.lock() {
            g.create_did_queue.push_back(result);
        }
    }

    pub fn push_deactivate(&self, result: Result<CallReceipt, ChainError>) {
        if let Ok(mut g) = self.inner.lock() {
            g.deactivate_queue.push_back(result);
        }
    }

    pub fn push_call_circuit(&self, result: Result<CallReceipt, ChainError>) {
        if let Ok(mut g) = self.inner.lock() {
            g.call_circuit_queue.push_back(result);
        }
    }

    pub fn push_load_circuit(&self, result: Result<(), ChainError>) {
        if let Ok(mut g) = self.inner.lock() {
            g.load_circuit_queue.push_back(result);
        }
    }

    /// Snapshot the recorded calls without draining.
    pub fn recorded(&self) -> Vec<RecordedCall> {
        self.inner.lock().map(|g| g.recorded.clone()).unwrap_or_default()
    }

    /// Test convenience: assert every scripted queue is empty.
    pub fn all_queues_drained(&self) -> bool {
        self.inner
            .lock()
            .map(|g| {
                g.create_did_queue.is_empty()
                    && g.deactivate_queue.is_empty()
                    && g.call_circuit_queue.is_empty()
                    && g.load_circuit_queue.is_empty()
            })
            .unwrap_or(false)
    }
}

#[async_trait]
impl ChainPublisher for StubChainPublisher {
    async fn create_did_with_controller(
        &self,
    ) -> Result<(DidId, [u8; 32]), ChainError> {
        let mut g = self
            .inner
            .lock()
            .map_err(|_| ChainError::Other("stub: lock poisoned".into()))?;
        g.recorded.push(RecordedCall::CreateDid);
        g.create_did_queue
            .pop_front()
            .unwrap_or_else(|| Err(ChainError::Other("stub: queue empty".into())))
    }

    async fn deactivate_did(
        &self,
        did: &DidId,
        _sk: &[u8; 32],
    ) -> Result<CallReceipt, ChainError> {
        let mut g = self
            .inner
            .lock()
            .map_err(|_| ChainError::Other("stub: lock poisoned".into()))?;
        g.recorded.push(RecordedCall::Deactivate {
            did: did.to_did_string(),
        });
        g.deactivate_queue
            .pop_front()
            .unwrap_or_else(|| Err(ChainError::Other("stub: queue empty".into())))
    }

    async fn call_did_circuit(
        &self,
        did: &DidId,
        circuit: &str,
        arg: Value,
        _sk: &[u8; 32],
    ) -> Result<CallReceipt, ChainError> {
        let mut g = self
            .inner
            .lock()
            .map_err(|_| ChainError::Other("stub: lock poisoned".into()))?;
        g.recorded.push(RecordedCall::CallCircuit {
            did: did.to_did_string(),
            circuit: circuit.to_string(),
            arg,
        });
        g.call_circuit_queue
            .pop_front()
            .unwrap_or_else(|| Err(ChainError::Other("stub: queue empty".into())))
    }

    async fn load_did_circuit(
        &self,
        did: &DidId,
        circuit: &str,
        counter: u64,
    ) -> Result<(), ChainError> {
        let mut g = self
            .inner
            .lock()
            .map_err(|_| ChainError::Other("stub: lock poisoned".into()))?;
        g.recorded.push(RecordedCall::LoadCircuit {
            did: did.to_did_string(),
            circuit: circuit.to_string(),
            counter,
        });
        g.load_circuit_queue
            .pop_front()
            .unwrap_or_else(|| Err(ChainError::Other("stub: queue empty".into())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Network;
    use serde_json::json;

    fn test_did() -> DidId {
        // Construct via the public `new` constructor — no need
        // to round-trip through string parsing for the stub
        // tests; the byte payload is opaque to the trait.
        DidId::new(Network::Undeployed, [0x42u8; 32])
    }

    #[tokio::test]
    async fn stub_create_did_returns_scripted_answer() {
        let stub = StubChainPublisher::new();
        let did = test_did();
        stub.push_create_did(Ok((did.clone(), [7u8; 32])));
        let (got, sk) = stub.create_did_with_controller().await.unwrap();
        assert_eq!(got.to_did_string(), did.to_did_string());
        assert_eq!(sk, [7u8; 32]);
        assert_eq!(stub.recorded(), vec![RecordedCall::CreateDid]);
    }

    #[tokio::test]
    async fn stub_create_did_empty_queue_errors() {
        let stub = StubChainPublisher::new();
        let err = stub.create_did_with_controller().await.unwrap_err();
        assert!(matches!(err, ChainError::Other(ref s) if s.contains("queue empty")));
    }

    #[tokio::test]
    async fn stub_records_call_circuit_args() {
        let stub = StubChainPublisher::new();
        let did = test_did();
        stub.push_call_circuit(Ok(CallReceipt {
            tx_hash: "0xa".into(),
            block_hash: "0xb".into(),
        }));
        let _ = stub
            .call_did_circuit(
                &did,
                "setVerificationMethod",
                json!([{"id": "key-auth"}, 1]),
                &[0u8; 32],
            )
            .await
            .unwrap();
        match &stub.recorded()[0] {
            RecordedCall::CallCircuit { did, circuit, arg } => {
                assert_eq!(circuit, "setVerificationMethod");
                assert!(arg.is_array());
                assert!(did.starts_with("did:midnight:"));
            }
            other => panic!("expected CallCircuit, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn stub_drained_detects_residue() {
        let stub = StubChainPublisher::new();
        stub.push_load_circuit(Ok(()));
        assert!(!stub.all_queues_drained());
        let did = test_did();
        let _ = stub
            .load_did_circuit(&did, "setVM", 0)
            .await
            .unwrap();
        assert!(stub.all_queues_drained());
    }

    #[tokio::test]
    async fn stub_per_method_queues_are_independent() {
        let stub = StubChainPublisher::new();
        let did = test_did();
        // Queue create + deactivate separately; call create
        // once, deactivate once — both succeed even though the
        // other's queue stays empty.
        stub.push_create_did(Ok((did.clone(), [1u8; 32])));
        stub.push_deactivate(Ok(CallReceipt {
            tx_hash: "0xtx".into(),
            block_hash: "0xbl".into(),
        }));
        let _ = stub.create_did_with_controller().await.unwrap();
        let _ = stub.deactivate_did(&did, &[1u8; 32]).await.unwrap();
        assert!(stub.all_queues_drained());
    }

    #[tokio::test]
    async fn stub_propagates_scripted_errors() {
        let stub = StubChainPublisher::new();
        stub.push_create_did(Err(ChainError::Indexer("no contract action".into())));
        let err = stub.create_did_with_controller().await.unwrap_err();
        assert!(matches!(err, ChainError::Indexer(_)));
    }
}
