//! `TelemetryService` — snapshot / reset the metrics aggregator,
//! plus the resource probe for op-level RSS / CPU sampling.
//!
//! Today these accessors live on `BridgeState`
//! (`metrics`, `metrics_dyn`, `resource_probe`). Wave C2 lifts
//! the wrap/compose logic here so:
//!
//! - the headless binary can mount a JSON-line metrics
//!   exporter without depending on Dioxus, and
//! - the Diagnostics tab gets a single object to call
//!   `.snapshot()` / `.reset()` against instead of grabbing
//!   the bare `Arc<InMemoryMetrics>` and reaching for trait
//!   methods.
//!
//! Wave C2 (this commit): bodies + use-case tests. The bridge
//! keeps its inline accessors until wave D wires the service
//! through Dioxus context — both share the same
//! `Arc<InMemoryMetrics>` instance, so behaviour is unchanged.

use std::sync::Arc;

use crate::telemetry::{
    CompositeMetrics, InMemoryMetrics, Metrics, MetricsSnapshot, ResourceProbe, TracingMetrics,
};

pub struct TelemetryService {
    pub(crate) metrics: Arc<InMemoryMetrics>,
    pub(crate) probe: Arc<dyn ResourceProbe>,
}

impl TelemetryService {
    pub fn new(metrics: Arc<InMemoryMetrics>, probe: Arc<dyn ResourceProbe>) -> Self {
        Self { metrics, probe }
    }

    /// Materialise the current aggregate. Cheap O(n log n) per
    /// bucket — fine to call per Diagnostics-tab render.
    pub fn snapshot(&self) -> MetricsSnapshot {
        self.metrics.snapshot()
    }

    /// Drop every accumulated sample. The "Reset stats" button
    /// in the Diagnostics tab.
    pub fn reset(&self) {
        self.metrics.reset();
    }

    /// Composite sink for instrumenting an adapter: forwards
    /// every record to both the in-memory aggregator (read by
    /// the Diagnostics tab) and `TracingMetrics` (so events
    /// flow into the Logs tab via `WalletLogLayer`). Cheap to
    /// construct — the `Arc`s are reference-counted clones.
    pub fn composite_sink(&self) -> Arc<dyn Metrics> {
        Arc::new(CompositeMetrics::new(vec![
            self.metrics.clone() as Arc<dyn Metrics>,
            Arc::new(TracingMetrics),
        ]))
    }

    /// Borrow the underlying in-memory aggregator. Use this
    /// when an adapter needs the concrete type (e.g.
    /// `MeteredHttpClient::new(http, telemetry.metrics_concrete())`).
    pub fn metrics_concrete(&self) -> Arc<InMemoryMetrics> {
        self.metrics.clone()
    }

    /// Borrow the resource probe. Cheap clone of the `Arc`.
    pub fn probe(&self) -> Arc<dyn ResourceProbe> {
        self.probe.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::{HttpRecord, NoopResourceProbe};

    fn svc() -> TelemetryService {
        TelemetryService::new(
            Arc::new(InMemoryMetrics::new()),
            Arc::new(NoopResourceProbe),
        )
    }

    fn http_rec(host: &str, status: u16, dur: u64) -> HttpRecord<'_> {
        HttpRecord {
            method: "GET",
            host,
            url: host,
            status,
            duration_ms: dur,
            body_bytes: Some(0),
            error: None,
        }
    }

    #[test]
    fn snapshot_empty_when_no_records() {
        let s = svc();
        let snap = s.snapshot();
        assert!(snap.http.is_empty(), "fresh service has no http buckets");
        assert!(snap.ops.is_empty(), "fresh service has no op buckets");
        assert!(snap.counters.is_empty(), "fresh service has no counters");
    }

    #[test]
    fn snapshot_after_record_returns_aggregate() {
        let s = svc();
        // Drive a record through the concrete handle — this
        // is what an adapter does when telemetry is wired.
        let m: Arc<dyn Metrics> = s.metrics_concrete();
        m.record_http(&http_rec("issuer.local", 200, 12));
        let snap = s.snapshot();
        let bucket = snap
            .http
            .get("GET issuer.local -> 200")
            .expect("http bucket present");
        assert_eq!(bucket.count, 1);
        assert_eq!(bucket.mean_ms, 12);
    }

    #[test]
    fn reset_drops_accumulated_samples() {
        let s = svc();
        let m: Arc<dyn Metrics> = s.metrics_concrete();
        m.record_http(&http_rec("issuer.local", 200, 12));
        assert_eq!(s.snapshot().http.len(), 1);
        s.reset();
        assert!(s.snapshot().http.is_empty());
    }

    #[test]
    fn composite_sink_writes_into_in_memory_aggregator() {
        // The composite forwards to both InMemoryMetrics and
        // TracingMetrics. We can't easily observe Tracing in a
        // unit test, but we can confirm the InMemory side sees
        // the write — which is the user-visible contract.
        let s = svc();
        let composite = s.composite_sink();
        composite.record_http(&http_rec("composite.local", 201, 7));
        let snap = s.snapshot();
        assert_eq!(
            snap.http.get("GET composite.local -> 201").unwrap().count,
            1
        );
    }

    #[test]
    fn snapshot_after_reset_after_record_is_empty() {
        let s = svc();
        let m: Arc<dyn Metrics> = s.metrics_concrete();
        m.record_http(&http_rec("a.local", 200, 1));
        m.record_http(&http_rec("b.local", 500, 2));
        assert!(!s.snapshot().http.is_empty());
        s.reset();
        // Repeated snapshot stays empty — no residue.
        assert!(s.snapshot().http.is_empty());
        assert!(s.snapshot().http.is_empty());
    }
}
