//! `WalletService` — unlock / lock / balance / UTXO snapshot.
//!
//! Wave A1: struct + constructor only. Method bodies migrated in
//! wave C3 (see refactor plan §3, Wave C step C3). The current
//! UI continues to call `Wallet::sync_unshielded` etc. directly
//! via `app_wallet_for(network)`; that path stays alive until C3.

use std::sync::Arc;

use crate::chain::{IndexerClient, NodeClient, Prover};
use crate::clock::Clock;
use crate::http::HttpClient;
use crate::secret_storage::SecretStorage;
use crate::telemetry::Metrics;

pub struct WalletService {
    pub(crate) http: Arc<dyn HttpClient>,
    pub(crate) indexer: Arc<dyn IndexerClient>,
    pub(crate) node: Arc<dyn NodeClient>,
    pub(crate) prover: Arc<dyn Prover>,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) metrics: Arc<dyn Metrics>,
    pub(crate) secrets: Arc<dyn SecretStorage>,
}

impl WalletService {
    pub fn new(
        http: Arc<dyn HttpClient>,
        indexer: Arc<dyn IndexerClient>,
        node: Arc<dyn NodeClient>,
        prover: Arc<dyn Prover>,
        clock: Arc<dyn Clock>,
        metrics: Arc<dyn Metrics>,
        secrets: Arc<dyn SecretStorage>,
    ) -> Self {
        Self {
            http,
            indexer,
            node,
            prover,
            clock,
            metrics,
            secrets,
        }
    }
}
