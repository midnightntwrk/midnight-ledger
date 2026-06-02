//! Wall-clock + resource bracket around any async operation.
//!
//! ```ignore
//! let uri = time_op(
//!     &*metrics,
//!     &probe,
//!     "issuance",
//!     async {
//!         oid4vci_run_issuance(&http, &clock, js_bridge, &url, &wallet, &store, &did, &vc_store).await
//!     },
//! ).await?;
//! ```
//!
//! The helper records exactly one [`OpRecord`] per call,
//! tagged with success / failure via [`OpOutcome`], and infers
//! the error label by formatting the `Err` with `Display`. If
//! the wrapped future is infallible (returns plain `T`), wrap
//! it in `async { Ok::<T, std::convert::Infallible>(...) }` or
//! call the lower-level [`time_op_simple`].

use std::fmt::Display;
use std::time::Instant;

use super::{Metrics, OpOutcome, OpRecord, ResourceProbe};

/// Bracket a `Result`-returning future. Records an `Ok` op on
/// success, an `Err(<display>)` op on failure. Always emits
/// exactly one record. `T` and `E` are passed through unchanged.
pub async fn time_op<T, E, F>(
    metrics: &dyn Metrics,
    probe: &dyn ResourceProbe,
    name: &str,
    fut: F,
) -> Result<T, E>
where
    F: std::future::Future<Output = Result<T, E>>,
    E: Display,
{
    let start = Instant::now();
    let before = probe.sample();
    let result = fut.await;
    let elapsed = start.elapsed();
    let after = probe.sample();
    let duration_ms = elapsed.as_millis() as u64;
    let (rss_delta, cpu_delta) = match (before, after) {
        (Some(b), Some(a)) => (
            Some((a.rss_kb as i64) - (b.rss_kb as i64)),
            Some((a.cpu_us as i64) - (b.cpu_us as i64)),
        ),
        _ => (None, None),
    };
    let err_label = result.as_ref().err().map(|e| e.to_string());
    let outcome = match err_label.as_deref() {
        None => OpOutcome::Ok,
        Some(s) => OpOutcome::Err(s),
    };
    metrics.record_op(&OpRecord {
        name,
        duration_ms,
        rss_kb_delta: rss_delta,
        cpu_us_delta: cpu_delta,
        outcome,
    });
    result
}

/// Variant for infallible futures returning a plain `T`. Always
/// records `OpOutcome::Ok`.
pub async fn time_op_simple<T, F>(
    metrics: &dyn Metrics,
    probe: &dyn ResourceProbe,
    name: &str,
    fut: F,
) -> T
where
    F: std::future::Future<Output = T>,
{
    let start = Instant::now();
    let before = probe.sample();
    let result = fut.await;
    let elapsed = start.elapsed();
    let after = probe.sample();
    let (rss_delta, cpu_delta) = match (before, after) {
        (Some(b), Some(a)) => (
            Some((a.rss_kb as i64) - (b.rss_kb as i64)),
            Some((a.cpu_us as i64) - (b.cpu_us as i64)),
        ),
        _ => (None, None),
    };
    metrics.record_op(&OpRecord {
        name,
        duration_ms: elapsed.as_millis() as u64,
        rss_kb_delta: rss_delta,
        cpu_us_delta: cpu_delta,
        outcome: OpOutcome::Ok,
    });
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::{InMemoryMetrics, NoopResourceProbe};

    #[tokio::test]
    async fn time_op_records_ok_on_success() {
        let metrics = InMemoryMetrics::new();
        let probe = NoopResourceProbe;
        let out = time_op::<_, &str, _>(&metrics, &probe, "demo", async {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            Ok::<_, &str>(42)
        })
        .await
        .unwrap();
        assert_eq!(out, 42);
        let snap = metrics.snapshot();
        let h = snap.ops.get("demo ok").expect("recorded");
        assert_eq!(h.count, 1);
        assert!(h.max_ms >= 5, "elapsed should be at least 5ms, got {}", h.max_ms);
    }

    #[tokio::test]
    async fn time_op_records_err_with_display_label() {
        let metrics = InMemoryMetrics::new();
        let probe = NoopResourceProbe;
        let res: Result<i32, _> =
            time_op(&metrics, &probe, "issuance", async { Err::<i32, _>("boom") })
                .await;
        assert!(res.is_err());
        let snap = metrics.snapshot();
        assert!(snap.ops.contains_key("issuance err"));
        assert!(!snap.ops.contains_key("issuance ok"));
    }

    #[tokio::test]
    async fn time_op_simple_always_ok() {
        let metrics = InMemoryMetrics::new();
        let probe = NoopResourceProbe;
        let out = time_op_simple(&metrics, &probe, "verify", async { 7u32 }).await;
        assert_eq!(out, 7);
        assert!(metrics.snapshot().ops.contains_key("verify ok"));
    }
}
