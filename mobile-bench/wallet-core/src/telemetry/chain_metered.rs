//! Metering decorators over the chain-op ports
//! ([`IndexerClient`], [`NodeClient`], [`Prover`]).
//!
//! Same shape as [`crate::telemetry::MeteredHttpClient`] but for
//! the higher-level abstractions: indexer GraphQL queries, node
//! RPC submits, and ZK proof generation. The decorators record
//! per-call wall-time + RSS / CPU deltas through [`time_op`] so
//! the aggregator surfaces them as `ops:` entries — letting the
//! Diagnostics tab show "prover.prove p95=1840ms total_cpu=23s"
//! next to the OID4VCI top-level timings.
//!
//! Wiring stays drop-in via the `Wallet`'s existing
//! `with_indexer` / `with_node` / `with_prover` builders:
//! ```ignore
//! let inner: Arc<dyn IndexerClient> = ...;
//! let metered = Arc::new(MeteredIndexerClient::new(inner, metrics.clone(), probe.clone()));
//! let wallet = Wallet::from_seed(seed, network).with_indexer(metered);
//! ```

use std::sync::Arc;

use async_trait::async_trait;

use crate::chain::{IndexerClient, NodeClient, Prover};
use crate::indexer::{ChainTipInfo, ContractStateInfo, IndexerError};
use crate::node::{NodeError, SubmitResult};
use crate::tx::TxError;
use crate::tx::build::UnprovenTx;
use crate::tx::prove::ProvenTx;
use crate::MidnightSigner;

use super::{time_op, Metrics, ResourceProbe};

/// Decorator over any `IndexerClient`. Times `chain_tip` and
/// `contract_state`; emits `OpRecord`s named
/// `indexer.chain_tip` / `indexer.contract_state`.
pub struct MeteredIndexerClient {
    inner: Arc<dyn IndexerClient>,
    metrics: Arc<dyn Metrics>,
    probe: Arc<dyn ResourceProbe>,
}

impl MeteredIndexerClient {
    pub fn new(
        inner: Arc<dyn IndexerClient>,
        metrics: Arc<dyn Metrics>,
        probe: Arc<dyn ResourceProbe>,
    ) -> Self {
        Self { inner, metrics, probe }
    }
}

#[async_trait]
impl IndexerClient for MeteredIndexerClient {
    async fn chain_tip(&self) -> Result<Option<ChainTipInfo>, IndexerError> {
        time_op(
            &*self.metrics,
            &*self.probe,
            "indexer.chain_tip",
            self.inner.chain_tip(),
        )
        .await
    }

    async fn contract_state(
        &self,
        address_hex: &str,
    ) -> Result<Option<ContractStateInfo>, IndexerError> {
        time_op(
            &*self.metrics,
            &*self.probe,
            "indexer.contract_state",
            self.inner.contract_state(address_hex),
        )
        .await
    }
}

/// Decorator over any `NodeClient`. Times `submit_deploy`; emits
/// `node.submit_deploy`.
pub struct MeteredNodeClient {
    inner: Arc<dyn NodeClient>,
    metrics: Arc<dyn Metrics>,
    probe: Arc<dyn ResourceProbe>,
}

impl MeteredNodeClient {
    pub fn new(
        inner: Arc<dyn NodeClient>,
        metrics: Arc<dyn Metrics>,
        probe: Arc<dyn ResourceProbe>,
    ) -> Self {
        Self { inner, metrics, probe }
    }
}

#[async_trait]
impl NodeClient for MeteredNodeClient {
    async fn submit_deploy(
        &self,
        bytes: Vec<u8>,
        signer: &MidnightSigner,
    ) -> Result<SubmitResult, NodeError> {
        time_op(
            &*self.metrics,
            &*self.probe,
            "node.submit_deploy",
            self.inner.submit_deploy(bytes, signer),
        )
        .await
    }
}

/// Decorator over any `Prover`. Times `prove`; emits
/// `prover.prove`. This is typically the heaviest op in the
/// chain-op pipeline — halo2 KZG proof generation runs for
/// hundreds of milliseconds on desktop and several seconds on
/// debug-built mobile. The RSS delta also reveals proving-key
/// page-ins on first call.
pub struct MeteredProver {
    inner: Arc<dyn Prover>,
    metrics: Arc<dyn Metrics>,
    probe: Arc<dyn ResourceProbe>,
}

impl MeteredProver {
    pub fn new(
        inner: Arc<dyn Prover>,
        metrics: Arc<dyn Metrics>,
        probe: Arc<dyn ResourceProbe>,
    ) -> Self {
        Self { inner, metrics, probe }
    }
}

#[async_trait]
impl Prover for MeteredProver {
    async fn prove(&self, tx: UnprovenTx) -> Result<ProvenTx, TxError> {
        time_op(
            &*self.metrics,
            &*self.probe,
            "prover.prove",
            self.inner.prove(tx),
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indexer::{ChainTipInfo, ContractStateInfo, IndexerError};
    use crate::telemetry::{InMemoryMetrics, NoopResourceProbe};
    use std::sync::atomic::{AtomicU32, Ordering};

    // Minimal indexer stub that returns canned responses + counts
    // how many times each method was called. Lets the test assert
    // both the decorator's pass-through behaviour and the metric
    // recording.
    struct StubIndexer {
        tip_calls: AtomicU32,
        state_calls: AtomicU32,
    }
    #[async_trait]
    impl IndexerClient for StubIndexer {
        async fn chain_tip(&self) -> Result<Option<ChainTipInfo>, IndexerError> {
            self.tip_calls.fetch_add(1, Ordering::SeqCst);
            Ok(None)
        }
        async fn contract_state(
            &self,
            _: &str,
        ) -> Result<Option<ContractStateInfo>, IndexerError> {
            self.state_calls.fetch_add(1, Ordering::SeqCst);
            Ok(None)
        }
    }

    #[tokio::test]
    async fn metered_indexer_records_each_method_distinctly() {
        let stub = Arc::new(StubIndexer {
            tip_calls: AtomicU32::new(0),
            state_calls: AtomicU32::new(0),
        });
        let metrics: Arc<InMemoryMetrics> = Arc::new(InMemoryMetrics::new());
        let probe: Arc<dyn ResourceProbe> = Arc::new(NoopResourceProbe);
        let metered = MeteredIndexerClient::new(stub.clone(), metrics.clone(), probe);

        let _ = metered.chain_tip().await.unwrap();
        let _ = metered.chain_tip().await.unwrap();
        let _ = metered.contract_state("addr").await.unwrap();
        // Inner saw three calls.
        assert_eq!(stub.tip_calls.load(Ordering::SeqCst), 2);
        assert_eq!(stub.state_calls.load(Ordering::SeqCst), 1);

        let snap = metrics.snapshot();
        assert_eq!(snap.ops.get("indexer.chain_tip ok").unwrap().count, 2);
        assert_eq!(snap.ops.get("indexer.contract_state ok").unwrap().count, 1);
    }

    #[tokio::test]
    async fn metered_indexer_records_err_branch() {
        struct ErrIndexer;
        #[async_trait]
        impl IndexerClient for ErrIndexer {
            async fn chain_tip(&self) -> Result<Option<ChainTipInfo>, IndexerError> {
                // `IndexerError::Http` wraps a `reqwest::Error`
                // which can't be constructed directly; the
                // `GraphQl(String)` variant is the easiest
                // dynamic-error path for tests.
                Err(IndexerError::GraphQl("boom".into()))
            }
            async fn contract_state(
                &self,
                _: &str,
            ) -> Result<Option<ContractStateInfo>, IndexerError> {
                unreachable!()
            }
        }
        let metrics: Arc<InMemoryMetrics> = Arc::new(InMemoryMetrics::new());
        let probe: Arc<dyn ResourceProbe> = Arc::new(NoopResourceProbe);
        let metered = MeteredIndexerClient::new(
            Arc::new(ErrIndexer),
            metrics.clone(),
            probe,
        );
        let r = metered.chain_tip().await;
        assert!(r.is_err());
        let snap = metrics.snapshot();
        assert_eq!(snap.ops.get("indexer.chain_tip err").unwrap().count, 1);
        assert!(snap.ops.get("indexer.chain_tip ok").is_none());
    }
}
