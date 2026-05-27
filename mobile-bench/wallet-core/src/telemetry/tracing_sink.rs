//! `tracing::info!`-emitting `Metrics` adapter. Lets HTTP / op
//! events flow into the existing `WalletLogLayer` (Logs tab) so
//! operators can scrub through latency timeline live, without
//! needing the Diagnostics-tab read path. Uses the
//! `target = "wallet_core::metrics"` namespace so the existing
//! `should_capture` filter picks it up (it allows `wallet_core::*`).

use tracing::info;

use super::{HttpRecord, Metrics, OpOutcome, OpRecord};

/// Stateless emitter — share an `Arc`.
#[derive(Debug, Default, Clone, Copy)]
pub struct TracingMetrics;

impl Metrics for TracingMetrics {
    fn record_http(&self, r: &HttpRecord<'_>) {
        match (r.error, r.body_bytes) {
            (Some(err), _) => {
                info!(
                    target: "wallet_core::metrics",
                    kind = "http",
                    method = r.method,
                    host = r.host,
                    duration_ms = r.duration_ms,
                    err = err,
                    "http {} {} -> err({}) in {}ms",
                    r.method, r.host, err, r.duration_ms,
                );
            }
            (None, Some(bytes)) => {
                info!(
                    target: "wallet_core::metrics",
                    kind = "http",
                    method = r.method,
                    host = r.host,
                    status = r.status,
                    duration_ms = r.duration_ms,
                    body_bytes = bytes as u64,
                    "http {} {} -> {} in {}ms ({}B)",
                    r.method, r.host, r.status, r.duration_ms, bytes,
                );
            }
            (None, None) => {
                info!(
                    target: "wallet_core::metrics",
                    kind = "http",
                    method = r.method,
                    host = r.host,
                    status = r.status,
                    duration_ms = r.duration_ms,
                    "http {} {} -> {} in {}ms",
                    r.method, r.host, r.status, r.duration_ms,
                );
            }
        }
    }

    fn record_op(&self, r: &OpRecord<'_>) {
        let outcome = match r.outcome {
            OpOutcome::Ok => "ok",
            OpOutcome::Err(e) => e,
        };
        // Use a structured-fields-only emission. The Logs tab
        // composes them via the `MessageVisitor` into a single
        // line so this still looks like "op=issuance dur=120
        // rss_kb_delta=400" in the UI.
        info!(
            target: "wallet_core::metrics",
            kind = "op",
            op = r.name,
            outcome = outcome,
            duration_ms = r.duration_ms,
            rss_kb_delta = r.rss_kb_delta.unwrap_or(0),
            cpu_us_delta = r.cpu_us_delta.unwrap_or(0),
            "op {} {} in {}ms (rss Δ{}kB, cpu Δ{}μs)",
            r.name,
            outcome,
            r.duration_ms,
            r.rss_kb_delta.unwrap_or(0),
            r.cpu_us_delta.unwrap_or(0),
        );
    }

    fn incr(&self, counter: &str, by: u64) {
        info!(
            target: "wallet_core::metrics",
            kind = "counter",
            counter = counter,
            by = by,
            "counter {} += {}",
            counter, by,
        );
    }
}
