//! `DustSyncService` — sync DUST events + cached balance.
//!
//! Wave A1: struct + constructor only. Bodies in wave C4.

use std::sync::Arc;

use crate::chain::IndexerClient;
use crate::clock::Clock;
use crate::telemetry::Metrics;

pub struct DustSyncService {
    pub(crate) indexer: Arc<dyn IndexerClient>,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) metrics: Arc<dyn Metrics>,
}

impl DustSyncService {
    pub fn new(
        indexer: Arc<dyn IndexerClient>,
        clock: Arc<dyn Clock>,
        metrics: Arc<dyn Metrics>,
    ) -> Self {
        Self { indexer, clock, metrics }
    }
}
