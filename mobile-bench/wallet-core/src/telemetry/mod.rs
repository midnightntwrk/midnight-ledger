//! Telemetry port + adapters. Non-invasive diagnostics layered on
//! top of the existing hexagonal ports — no flow signatures change
//! to opt in.
//!
//! Three things this module gives you:
//!
//! 1. A [`Metrics`] trait — the port. Three recording verbs
//!    ([`record_http`], [`record_op`], [`incr`]) plus a
//!    [`snapshot`] read side. Default impl is [`NoopMetrics`].
//!
//! 2. Plug-in adapters:
//!    - [`InMemoryMetrics`] — locked counters + per-key
//!      histograms (count / min / max / mean / p50 / p95).
//!      Cheap; suitable for a per-process aggregator readable
//!      from a Diagnostics tab.
//!    - [`TracingMetrics`] — re-emits every record as a
//!      `tracing::info!(target = "metrics", ...)` event so the
//!      existing Logs-tab `WalletLogLayer` captures it too.
//!    - [`CompositeMetrics`] — fan-out wrapper; install both
//!      InMemory + Tracing simultaneously.
//!
//! 3. A [`MeteredHttpClient`] decorator that wraps any
//!    [`HttpClient`] and records `http.duration_ms`,
//!    `http.status`, and response-body bytes per call. No
//!    callsite changes — drop it in at construction.
//!
//! Plus a [`time_op`] helper that wraps a future with a
//! wall-clock + RSS-delta snapshot via [`ResourceProbe`]. The
//! POSIX [`RusageProbe`] adapter works on macOS / iOS / Linux /
//! Android (every platform the wallet ships on).

mod chain_metered;
mod composite;
mod http_metered;
mod in_memory;
mod resource;
mod time_op;
mod tracing_sink;

pub use chain_metered::{MeteredIndexerClient, MeteredNodeClient, MeteredProver};
pub use composite::CompositeMetrics;
pub use http_metered::MeteredHttpClient;
pub use in_memory::{HistogramSnapshot, InMemoryMetrics, MetricsSnapshot, OpHistogramSnapshot};
pub use resource::{NoopResourceProbe, ResourceProbe, ResourceSample, RusageProbe};
pub use time_op::{time_op, time_op_simple};
pub use tracing_sink::TracingMetrics;

use std::sync::Arc;

/// Per-call HTTP record. The decorator passes one of these to
/// [`Metrics::record_http`] after every GET / POST.
#[derive(Debug, Clone)]
pub struct HttpRecord<'a> {
    pub method: &'static str,
    /// Host part of the URL (no scheme, no path) — keeps the
    /// metric label cardinality bounded.
    pub host: &'a str,
    /// Full URL — only used by `TracingMetrics` for the
    /// human-readable log line; `InMemoryMetrics` bucket keys
    /// drop it to keep the aggregator small.
    pub url: &'a str,
    pub status: u16,
    pub duration_ms: u64,
    /// Response body length. `None` for transport errors.
    pub body_bytes: Option<usize>,
    pub error: Option<&'a str>,
}

/// One bracketed operation timing — what [`time_op`] emits when
/// the wrapped future completes.
#[derive(Debug, Clone)]
pub struct OpRecord<'a> {
    pub name: &'a str,
    pub duration_ms: u64,
    /// Resident-set delta in KiB, signed. `None` when the
    /// [`ResourceProbe`] couldn't sample (e.g. `NoopResourceProbe`).
    pub rss_kb_delta: Option<i64>,
    /// CPU-time delta in microseconds (user + system), signed.
    /// `None` when unavailable.
    pub cpu_us_delta: Option<i64>,
    /// `Ok` / `Err` tag so the caller can break out
    /// success / failure latency.
    pub outcome: OpOutcome<'a>,
}

#[derive(Debug, Clone, Copy)]
pub enum OpOutcome<'a> {
    Ok,
    Err(&'a str),
}

/// The port. Implementors hold their own state; consumers take
/// `&dyn Metrics` (or `Arc<dyn Metrics>`).
pub trait Metrics: Send + Sync + 'static {
    /// Called once per HTTP call by [`MeteredHttpClient`].
    fn record_http(&self, record: &HttpRecord<'_>);
    /// Called once per [`time_op`] completion.
    fn record_op(&self, record: &OpRecord<'_>);
    /// Bump a free-form counter. Names should be stable across
    /// calls — they become metric keys.
    fn incr(&self, counter: &str, by: u64);
}

/// Default null-object adapter. Stateless; share an `Arc`.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopMetrics;

impl Metrics for NoopMetrics {
    fn record_http(&self, _: &HttpRecord<'_>) {}
    fn record_op(&self, _: &OpRecord<'_>) {}
    fn incr(&self, _: &str, _: u64) {}
}

/// Convenience: an `Arc<NoopMetrics>` you can hand to anything
/// expecting `Arc<dyn Metrics>` without constructing it inline.
pub fn noop_metrics() -> Arc<dyn Metrics> {
    Arc::new(NoopMetrics)
}

/// Pull the host out of a URL for low-cardinality metric labels.
/// Falls back to the full URL if parsing fails — never panics.
pub(crate) fn host_of(url: &str) -> &str {
    // Strip scheme:// once.
    let after_scheme = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
    // Stop at the first '/', '?', or '#'.
    let end = after_scheme
        .find(|c: char| c == '/' || c == '?' || c == '#')
        .unwrap_or(after_scheme.len());
    &after_scheme[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_of_strips_scheme_and_path() {
        assert_eq!(host_of("https://issuer.local/credential"), "issuer.local");
        assert_eq!(host_of("http://192.168.1.5:8088/x?y=1"), "192.168.1.5:8088");
        assert_eq!(host_of("ws://node.example.com:9944"), "node.example.com:9944");
        // No scheme — return the raw string.
        assert_eq!(host_of("no-scheme-here"), "no-scheme-here");
    }

    #[test]
    fn noop_metrics_swallows_everything() {
        let m: Arc<dyn Metrics> = noop_metrics();
        m.record_http(&HttpRecord {
            method: "GET",
            host: "x",
            url: "x",
            status: 200,
            duration_ms: 1,
            body_bytes: Some(0),
            error: None,
        });
        m.record_op(&OpRecord {
            name: "x",
            duration_ms: 1,
            rss_kb_delta: None,
            cpu_us_delta: None,
            outcome: OpOutcome::Ok,
        });
        m.incr("x", 1);
        // Doesn't panic, doesn't blow up. That's the contract.
    }
}
