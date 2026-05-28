//! `IdentityCentreService` — bootstrap the Identity-Centre DID with
//! Ed25519 + Jubjub VC keys.
//!
//! Wave A1: struct + constructor only. Bodies in wave C6.

use std::sync::Arc;

use crate::chain::{IndexerClient, NodeClient, Prover};
use crate::clock::Clock;
use crate::secret_storage::SecretStorage;
use crate::telemetry::Metrics;

pub struct IdentityCentreService {
    pub(crate) indexer: Arc<dyn IndexerClient>,
    pub(crate) node: Arc<dyn NodeClient>,
    pub(crate) prover: Arc<dyn Prover>,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) metrics: Arc<dyn Metrics>,
    pub(crate) secrets: Arc<dyn SecretStorage>,
}

impl IdentityCentreService {
    pub fn new(
        indexer: Arc<dyn IndexerClient>,
        node: Arc<dyn NodeClient>,
        prover: Arc<dyn Prover>,
        clock: Arc<dyn Clock>,
        metrics: Arc<dyn Metrics>,
        secrets: Arc<dyn SecretStorage>,
    ) -> Self {
        Self { indexer, node, prover, clock, metrics, secrets }
    }
}
