//! Lock-protected in-process aggregator. Reservoir-free histogram
//! per (op-name | http-host+status) keyed bucket. Cheap enough
//! to hold in the bridge state — typical Phase-1 wallets see
//! tens of HTTP calls per session, not thousands.

use std::collections::BTreeMap;
use std::sync::Mutex;

use super::{HttpRecord, Metrics, OpOutcome, OpRecord};

/// Aggregated buckets. Reads return owned values so the UI can
/// drop the mutex before re-rendering.
#[derive(Debug, Default, Clone)]
pub struct MetricsSnapshot {
    /// Free-form counters from [`Metrics::incr`].
    pub counters: BTreeMap<String, u64>,
    /// Per-(host, status) HTTP histograms. Key shape:
    /// `"<METHOD> <host> -> <status>"` (or `"err"` for transport
    /// failures).
    pub http: BTreeMap<String, HistogramSnapshot>,
    /// Per-op histograms keyed by op name. Includes outcome
    /// suffix (`"<name> ok"` / `"<name> err"`) so success and
    /// failure latencies don't blur together.
    pub ops: BTreeMap<String, OpHistogramSnapshot>,
}

/// Frozen histogram view. Sorted on demand inside the aggregator
/// so reads are O(n log n) but writes stay O(1).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct HistogramSnapshot {
    pub count: u64,
    pub min_ms: u64,
    pub max_ms: u64,
    pub mean_ms: u64,
    pub p50_ms: u64,
    pub p95_ms: u64,
    pub total_bytes: u64,
}

/// Same as `HistogramSnapshot` but with RSS / CPU summed across
/// samples so the UI can show "peak RSS during all `issuance`
/// runs" rather than only one.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct OpHistogramSnapshot {
    pub count: u64,
    pub min_ms: u64,
    pub max_ms: u64,
    pub mean_ms: u64,
    pub p50_ms: u64,
    pub p95_ms: u64,
    /// Sum of positive RSS deltas (KiB) across all recorded
    /// samples. Negative deltas (GC / drop) count as zero so the
    /// metric reflects "growth observed", not net change.
    pub total_rss_growth_kb: u64,
    /// Sum of CPU-time deltas in microseconds, signed-but-clamped
    /// to ≥0 (same reasoning as RSS).
    pub total_cpu_us: u64,
}

#[derive(Default)]
struct Inner {
    counters: BTreeMap<String, u64>,
    http: BTreeMap<String, Vec<u64>>, // durations_ms per bucket
    http_bytes: BTreeMap<String, u64>,
    ops: BTreeMap<String, Vec<OpSample>>,
}

#[derive(Clone, Copy)]
struct OpSample {
    duration_ms: u64,
    rss_growth_kb: u64,
    cpu_us: u64,
}

#[derive(Default)]
pub struct InMemoryMetrics {
    inner: Mutex<Inner>,
}

impl InMemoryMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    /// Drop every accumulated sample. Useful for the "Reset
    /// stats" button in a Diagnostics tab.
    pub fn reset(&self) {
        if let Ok(mut g) = self.inner.lock() {
            g.counters.clear();
            g.http.clear();
            g.http_bytes.clear();
            g.ops.clear();
        }
    }

    /// Materialize a sorted snapshot. Cheap-ish: O(n log n) per
    /// bucket where n is the number of samples in that bucket.
    pub fn snapshot(&self) -> MetricsSnapshot {
        let Ok(g) = self.inner.lock() else {
            return MetricsSnapshot::default();
        };
        let counters = g.counters.clone();
        let http = g
            .http
            .iter()
            .map(|(k, samples)| {
                let bytes = g.http_bytes.get(k).copied().unwrap_or(0);
                (k.clone(), summarize_http(samples, bytes))
            })
            .collect();
        let ops = g
            .ops
            .iter()
            .map(|(k, samples)| (k.clone(), summarize_op(samples)))
            .collect();
        MetricsSnapshot {
            counters,
            http,
            ops,
        }
    }
}

impl Metrics for InMemoryMetrics {
    fn record_http(&self, record: &HttpRecord<'_>) {
        let key = if let Some(err) = record.error {
            format!("{} {} -> err({err})", record.method, record.host)
        } else {
            format!("{} {} -> {}", record.method, record.host, record.status)
        };
        let Ok(mut g) = self.inner.lock() else {
            return;
        };
        g.http.entry(key.clone()).or_default().push(record.duration_ms);
        if let Some(bytes) = record.body_bytes {
            *g.http_bytes.entry(key).or_default() += bytes as u64;
        }
    }

    fn record_op(&self, record: &OpRecord<'_>) {
        let outcome_tag = match record.outcome {
            OpOutcome::Ok => "ok",
            OpOutcome::Err(_) => "err",
        };
        let key = format!("{} {}", record.name, outcome_tag);
        let sample = OpSample {
            duration_ms: record.duration_ms,
            rss_growth_kb: record
                .rss_kb_delta
                .map(|d| d.max(0) as u64)
                .unwrap_or(0),
            cpu_us: record.cpu_us_delta.map(|d| d.max(0) as u64).unwrap_or(0),
        };
        let Ok(mut g) = self.inner.lock() else {
            return;
        };
        g.ops.entry(key).or_default().push(sample);
    }

    fn incr(&self, counter: &str, by: u64) {
        let Ok(mut g) = self.inner.lock() else {
            return;
        };
        *g.counters.entry(counter.to_string()).or_default() += by;
    }
}

fn summarize_http(samples: &[u64], total_bytes: u64) -> HistogramSnapshot {
    if samples.is_empty() {
        return HistogramSnapshot::default();
    }
    let mut sorted: Vec<u64> = samples.to_vec();
    sorted.sort_unstable();
    let count = sorted.len() as u64;
    let min_ms = *sorted.first().unwrap();
    let max_ms = *sorted.last().unwrap();
    let sum: u64 = sorted.iter().sum();
    let mean_ms = sum / count;
    let p50_ms = percentile(&sorted, 50);
    let p95_ms = percentile(&sorted, 95);
    HistogramSnapshot {
        count,
        min_ms,
        max_ms,
        mean_ms,
        p50_ms,
        p95_ms,
        total_bytes,
    }
}

fn summarize_op(samples: &[OpSample]) -> OpHistogramSnapshot {
    if samples.is_empty() {
        return OpHistogramSnapshot::default();
    }
    let mut durations: Vec<u64> = samples.iter().map(|s| s.duration_ms).collect();
    durations.sort_unstable();
    let count = durations.len() as u64;
    let min_ms = *durations.first().unwrap();
    let max_ms = *durations.last().unwrap();
    let sum: u64 = durations.iter().sum();
    let mean_ms = sum / count;
    let p50_ms = percentile(&durations, 50);
    let p95_ms = percentile(&durations, 95);
    let total_rss_growth_kb = samples.iter().map(|s| s.rss_growth_kb).sum();
    let total_cpu_us = samples.iter().map(|s| s.cpu_us).sum();
    OpHistogramSnapshot {
        count,
        min_ms,
        max_ms,
        mean_ms,
        p50_ms,
        p95_ms,
        total_rss_growth_kb,
        total_cpu_us,
    }
}

/// Cheap nearest-rank percentile. Returns 0 for an empty slice.
fn percentile(sorted: &[u64], pct: u8) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let n = sorted.len();
    // Nearest-rank: index = ceil(pct/100 * n) - 1, clamped.
    let idx = ((pct as usize) * n + 99) / 100;
    let idx = idx.saturating_sub(1).min(n - 1);
    sorted[idx]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::OpOutcome;

    fn http<'a>(
        method: &'static str,
        host: &'a str,
        status: u16,
        dur: u64,
        body: usize,
    ) -> HttpRecord<'a> {
        HttpRecord {
            method,
            host,
            url: host,
            status,
            duration_ms: dur,
            body_bytes: Some(body),
            error: None,
        }
    }

    fn op<'a>(name: &'a str, dur: u64, rss: Option<i64>) -> OpRecord<'a> {
        OpRecord {
            name,
            duration_ms: dur,
            rss_kb_delta: rss,
            cpu_us_delta: None,
            outcome: OpOutcome::Ok,
        }
    }

    #[test]
    fn http_bucketed_by_host_status_method() {
        let m = InMemoryMetrics::new();
        m.record_http(&http("GET", "issuer.local", 200, 10, 100));
        m.record_http(&http("GET", "issuer.local", 200, 30, 100));
        m.record_http(&http("GET", "issuer.local", 200, 20, 100));
        m.record_http(&http("POST", "issuer.local", 500, 5, 0));
        let snap = m.snapshot();
        let ok = snap.http.get("GET issuer.local -> 200").unwrap();
        assert_eq!(ok.count, 3);
        assert_eq!(ok.min_ms, 10);
        assert_eq!(ok.max_ms, 30);
        assert_eq!(ok.mean_ms, 20);
        assert_eq!(ok.p50_ms, 20);
        assert_eq!(ok.p95_ms, 30);
        assert_eq!(ok.total_bytes, 300);
        let err = snap.http.get("POST issuer.local -> 500").unwrap();
        assert_eq!(err.count, 1);
    }

    #[test]
    fn http_transport_error_lands_in_err_bucket() {
        let m = InMemoryMetrics::new();
        let rec = HttpRecord {
            method: "GET",
            host: "issuer.local",
            url: "x",
            status: 0,
            duration_ms: 1234,
            body_bytes: None,
            error: Some("conn refused"),
        };
        m.record_http(&rec);
        let snap = m.snapshot();
        assert!(snap.http.contains_key("GET issuer.local -> err(conn refused)"));
    }

    #[test]
    fn op_buckets_split_ok_and_err() {
        let m = InMemoryMetrics::new();
        m.record_op(&op("issuance", 100, Some(500)));
        m.record_op(&op("issuance", 200, Some(1500)));
        m.record_op(&OpRecord {
            name: "issuance",
            duration_ms: 50,
            rss_kb_delta: None,
            cpu_us_delta: None,
            outcome: OpOutcome::Err("token decode"),
        });
        let snap = m.snapshot();
        let ok = snap.ops.get("issuance ok").unwrap();
        assert_eq!(ok.count, 2);
        assert_eq!(ok.mean_ms, 150);
        assert_eq!(ok.total_rss_growth_kb, 2000);
        let err = snap.ops.get("issuance err").unwrap();
        assert_eq!(err.count, 1);
        assert_eq!(err.total_rss_growth_kb, 0);
    }

    #[test]
    fn op_negative_rss_delta_clamps_to_zero() {
        let m = InMemoryMetrics::new();
        m.record_op(&op("verify", 5, Some(-200)));
        m.record_op(&op("verify", 5, Some(300)));
        let snap = m.snapshot();
        let v = snap.ops.get("verify ok").unwrap();
        assert_eq!(v.total_rss_growth_kb, 300);
    }

    #[test]
    fn counters_increment_idempotently() {
        let m = InMemoryMetrics::new();
        m.incr("vcs.issued", 1);
        m.incr("vcs.issued", 2);
        m.incr("verifies.failed", 5);
        let snap = m.snapshot();
        assert_eq!(snap.counters.get("vcs.issued"), Some(&3));
        assert_eq!(snap.counters.get("verifies.failed"), Some(&5));
    }

    #[test]
    fn reset_clears_all_state() {
        let m = InMemoryMetrics::new();
        m.incr("x", 1);
        m.record_http(&http("GET", "h", 200, 1, 1));
        m.record_op(&op("o", 1, None));
        assert!(!m.snapshot().counters.is_empty());
        m.reset();
        let s = m.snapshot();
        assert!(s.counters.is_empty());
        assert!(s.http.is_empty());
        assert!(s.ops.is_empty());
    }

    #[test]
    fn percentile_nearest_rank_well_behaved() {
        let v = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        assert_eq!(percentile(&v, 50), 5);
        assert_eq!(percentile(&v, 95), 10);
        assert_eq!(percentile(&v, 100), 10);
        assert_eq!(percentile(&[], 50), 0);
        assert_eq!(percentile(&[42], 95), 42);
    }
}
