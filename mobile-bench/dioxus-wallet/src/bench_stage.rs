//! Tracks the latest "bench stage" tracing event from
//! `contract-benchmark` for display in the Benchmark tab's UI.
//!
//! The contract-benchmark crate emits `tracing::info!` events at
//! every phase boundary inside `run_proof_with_params` — `build_ir`,
//! `keygen`, `prove`, `verify`, etc. Each event carries a
//! `stage` field. This module installs a `tracing_subscriber::Layer`
//! that pulls those events out, stores the latest stage in a
//! process-wide `Mutex<Option<String>>`, and exposes a
//! [`current_stage`] reader the UI polls every render.
//!
//! Why a separate layer rather than reading the `WalletLogLayer`'s
//! redb log? The redb persist drainer is async — by the time a
//! stage event lands in the table the prove has often moved on,
//! making the UI label lag. Capturing in-process gives us
//! sub-millisecond freshness without coupling to the persistence
//! pipeline. Both layers run side-by-side, so the stage events
//! ALSO appear in the Logs tab as ordinary log entries.

use std::sync::Mutex;

use tracing::{Event, Subscriber};
use tracing::field::{Field, Visit};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;

/// Tracing target that the stage events are emitted with. Picked
/// to start with `midnight_` so it also matches `WalletLogLayer`'s
/// `should_capture` filter — stage transitions show up in the
/// Logs tab alongside the in-memory stage label.
pub const STAGE_TARGET: &str = "midnight_bench";

/// Latest stage observed, plus the `k` value if the event carried
/// one. The UI reads this every render via [`current_stage`].
static CURRENT_STAGE: Mutex<Option<String>> = Mutex::new(None);

/// Read the most recent stage. Returns `None` until the first
/// stage event arrives. The string is `"<stage>"` or
/// `"<stage> (k=<n>)"` depending on whether the event carried a
/// `k` field.
pub fn current_stage() -> Option<String> {
    CURRENT_STAGE.lock().ok()?.clone()
}

/// Clear the stage — invoked when the bench loop completes so the
/// UI pill goes away.
#[allow(dead_code)]
pub fn clear() {
    if let Ok(mut g) = CURRENT_STAGE.lock() {
        *g = None;
    }
}

/// The `tracing_subscriber::Layer` itself. Stateless — the only
/// shared state is the `CURRENT_STAGE` static above.
pub struct BenchStageLayer;

impl BenchStageLayer {
    pub const fn new() -> Self {
        Self
    }
}

impl Default for BenchStageLayer {
    fn default() -> Self {
        Self::new()
    }
}

impl<S> Layer<S> for BenchStageLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        if event.metadata().target() != STAGE_TARGET {
            return;
        }
        let mut visitor = StageVisitor::default();
        event.record(&mut visitor);
        let Some(stage) = visitor.stage else { return };
        let label = match visitor.k {
            Some(k) => format!("{stage} (k={k})"),
            None => stage,
        };
        if let Ok(mut g) = CURRENT_STAGE.lock() {
            *g = Some(label);
        }
    }
}

/// Extracts the `stage` and optional `k` fields from a stage event.
#[derive(Default)]
struct StageVisitor {
    stage: Option<String>,
    k: Option<u64>,
}

impl Visit for StageVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "stage" {
            self.stage = Some(value.to_string());
        }
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        if field.name() == "k" {
            self.k = Some(value);
        }
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        if field.name() == "k" && value >= 0 {
            self.k = Some(value as u64);
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        // Fallback: `tracing::info!(stage = "foo")` records via
        // `record_str` on most field types, but if the macro
        // dispatches via Debug for some reason we still want to
        // capture the value.
        if field.name() == "stage" && self.stage.is_none() {
            let s = format!("{value:?}");
            // Strip surrounding quotes that `Debug` for &str adds.
            let trimmed = s.trim_matches('"').to_string();
            self.stage = Some(trimmed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_stage_round_trip() {
        // The static slot is process-wide; isolate by setting
        // and clearing within the test.
        clear();
        assert!(current_stage().is_none());

        // Direct mutation simulates a successful Layer event.
        {
            let mut g = CURRENT_STAGE.lock().unwrap();
            *g = Some("prove (k=14)".to_string());
        }
        assert_eq!(current_stage().as_deref(), Some("prove (k=14)"));

        clear();
        assert!(current_stage().is_none());
    }
}
