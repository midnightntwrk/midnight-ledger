//! Fan-out `Metrics` adapter — forwards every record to every
//! contained sink in order. Lets the bridge install both
//! `InMemoryMetrics` (for the Diagnostics tab) and
//! `TracingMetrics` (for the Logs tab) simultaneously.

use std::sync::Arc;

use super::{HttpRecord, Metrics, OpRecord};

#[derive(Clone, Default)]
pub struct CompositeMetrics {
    sinks: Vec<Arc<dyn Metrics>>,
}

impl CompositeMetrics {
    pub fn new(sinks: Vec<Arc<dyn Metrics>>) -> Self {
        Self { sinks }
    }
    pub fn push(&mut self, sink: Arc<dyn Metrics>) {
        self.sinks.push(sink);
    }
}

impl Metrics for CompositeMetrics {
    fn record_http(&self, record: &HttpRecord<'_>) {
        for s in &self.sinks {
            s.record_http(record);
        }
    }
    fn record_op(&self, record: &OpRecord<'_>) {
        for s in &self.sinks {
            s.record_op(record);
        }
    }
    fn incr(&self, counter: &str, by: u64) {
        for s in &self.sinks {
            s.incr(counter, by);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::{InMemoryMetrics, OpOutcome, OpRecord};

    #[test]
    fn fan_out_two_in_memory_sinks() {
        let a: Arc<InMemoryMetrics> = Arc::new(InMemoryMetrics::new());
        let b: Arc<InMemoryMetrics> = Arc::new(InMemoryMetrics::new());
        let c = CompositeMetrics::new(vec![a.clone(), b.clone()]);
        c.record_op(&OpRecord {
            name: "x",
            duration_ms: 10,
            rss_kb_delta: None,
            cpu_us_delta: None,
            outcome: OpOutcome::Ok,
        });
        c.incr("k", 3);
        assert_eq!(a.snapshot().counters.get("k"), Some(&3));
        assert_eq!(b.snapshot().counters.get("k"), Some(&3));
        assert_eq!(a.snapshot().ops.get("x ok").map(|h| h.count), Some(1));
        assert_eq!(b.snapshot().ops.get("x ok").map(|h| h.count), Some(1));
    }
}
