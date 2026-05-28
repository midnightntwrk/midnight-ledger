//! `TelemetryService` — snapshot / reset the metrics aggregator,
//! plus the resource probe for op-level RSS / CPU sampling.
//!
//! Wave A1: struct + constructor only. Bodies in wave C2.

use std::sync::Arc;

use crate::telemetry::{InMemoryMetrics, ResourceProbe};

pub struct TelemetryService {
    pub(crate) metrics: Arc<InMemoryMetrics>,
    pub(crate) probe: Arc<dyn ResourceProbe>,
}

impl TelemetryService {
    pub fn new(metrics: Arc<InMemoryMetrics>, probe: Arc<dyn ResourceProbe>) -> Self {
        Self { metrics, probe }
    }
}
