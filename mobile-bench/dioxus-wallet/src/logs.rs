//! In-process log capture for the dioxus-wallet UI.
//!
//! Two halves to the design:
//!
//! 1. **`WalletLogLayer`** — a `tracing_subscriber::Layer` that
//!    sits next to the usual `fmt` layer. Every event it sees
//!    becomes a `CapturedLog`, pushed into two channels at once:
//!    - an in-memory ring buffer (last 1000 events) the Logs
//!      tab renders live
//!    - an `unbounded_channel` the persistence drainer pulls
//!      from in batches
//!
//! 2. **`spawn_persist_drainer`** — a tokio task that owns the
//!    receiver side of the channel. Pulls events in chunks
//!    (size + time bounded), folds them into one `redb`
//!    write txn per flush window, and pushes them into
//!    `WalletStore::append_logs`. Backpressure is "best
//!    effort": if the receiver lags the ring buffer never
//!    drops anything, but persisted rows may trail the live
//!    view by up to one flush interval.
//!
//! The pair is independent: the ring keeps working even when
//! the store isn't attached yet (e.g. early boot), and the
//! drainer just sits idle until events show up.
//!
//! ## Filtering
//!
//! By default we capture every event targeted at this crate
//! or `wallet_core::*`. Anything else — redb internals, the
//! tracing crates themselves — is dropped at the layer
//! boundary. Without this filter every redb write txn would
//! generate events that would then be persisted via another
//! redb write txn → infinite loop.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

/// Process-global capture state. Built in `lib.rs::run()`
/// before any tracing event fires, picked up by the App
/// during component construction. `OnceLock` because the
/// installation happens exactly once and the App never
/// rebuilds it; cloning the inner `LogCapture` is cheap.
pub(crate) static LOG_CAPTURE: OnceLock<LogCapture> = OnceLock::new();
/// Receiver half of the persist channel, parked here until
/// the App takes ownership and hands it to the drainer
/// task. Wrapped in `Mutex<Option<…>>` so the App can
/// `take()` it exactly once.
pub(crate) static LOG_RX: OnceLock<Mutex<Option<UnboundedReceiver<CapturedLog>>>> =
    OnceLock::new();
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;

use wallet_core::store::{LogLevel, LogRow, WalletStore};

/// How many recent events the in-memory ring buffer keeps.
/// Tradeoff: bigger → richer "Logs" tab; smaller → less
/// memory. 1k is plenty for a session's worth of clicks; the
/// full history lives in redb anyway.
const RING_CAPACITY: usize = 1_000;

/// One captured tracing event. Plain owned strings so the
/// channel + ring don't borrow from `tracing` internals.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapturedLog {
    pub timestamp_ns: i64,
    pub timestamp_ms: i64,
    pub level: LogLevel,
    pub target: String,
    pub message: String,
}

impl CapturedLog {
    fn into_store_row(self) -> LogRow {
        LogRow {
            timestamp_ns: self.timestamp_ns,
            timestamp_ms: self.timestamp_ms,
            level: self.level,
            target: self.target,
            message: self.message,
        }
    }
}

/// Handle the App threads through `BridgeState`. Cheap to
/// clone — both internals are `Arc`-shared.
#[derive(Clone)]
pub struct LogCapture {
    ring: Arc<Mutex<VecDeque<CapturedLog>>>,
    persist_tx: UnboundedSender<CapturedLog>,
}

impl LogCapture {
    /// Build the capture state + the matching receiver. The
    /// receiver is consumed by `spawn_persist_drainer` once
    /// the wallet store is attached; before then it just
    /// accumulates events in the channel buffer.
    pub fn new() -> (Self, UnboundedReceiver<CapturedLog>) {
        let (tx, rx) = unbounded_channel();
        let capture = LogCapture {
            ring: Arc::new(Mutex::new(VecDeque::with_capacity(RING_CAPACITY))),
            persist_tx: tx,
        };
        (capture, rx)
    }

    /// Snapshot the in-memory ring buffer, newest-first. The
    /// `LogsTab` component calls this on every render — it's
    /// cheap because we clone owned strings out of an
    /// already-bounded `VecDeque`.
    pub fn snapshot(&self) -> Vec<CapturedLog> {
        let Ok(guard) = self.ring.lock() else {
            return Vec::new();
        };
        guard.iter().rev().cloned().collect()
    }

    /// Drop every entry from the ring (the on-disk archive is
    /// untouched — call `WalletStore::clear_logs` separately
    /// to wipe that). Used by the "Clear" button in the UI.
    pub fn clear_ring(&self) {
        if let Ok(mut g) = self.ring.lock() {
            g.clear();
        }
    }

    /// Internal: push one event. Called by the layer. Never
    /// fails — a full channel (impossible with `unbounded`
    /// today, but defensive) just drops the persist copy and
    /// keeps the ring entry.
    fn push(&self, event: CapturedLog) {
        if let Ok(mut g) = self.ring.lock() {
            if g.len() == RING_CAPACITY {
                g.pop_front();
            }
            g.push_back(event.clone());
        }
        let _ = self.persist_tx.send(event);
    }
}

/// The actual `tracing_subscriber::Layer`. Stateless — all
/// state lives in the `LogCapture` it carries.
pub struct WalletLogLayer {
    capture: LogCapture,
}

impl WalletLogLayer {
    pub fn new(capture: LogCapture) -> Self {
        Self { capture }
    }
}

impl<S> tracing_subscriber::Layer<S> for WalletLogLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let meta = event.metadata();
        let target = meta.target();
        if !should_capture(target) {
            return;
        }
        let level = level_from_tracing(*meta.level());
        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        // ns first so the redb key is monotonic at sub-ms
        // resolution. ms is what the UI renders.
        let timestamp_ns = now.as_nanos() as i64;
        let timestamp_ms = now.as_millis() as i64;
        self.capture.push(CapturedLog {
            timestamp_ns,
            timestamp_ms,
            level,
            target: target.to_string(),
            message: visitor.into_message(),
        });
    }
}

/// Decide which event targets we capture. Anything from the
/// app or wallet-core is in; anything else (tracing
/// internals, redb writes, third-party crates) is dropped at
/// the boundary so we don't end up recursively persisting
/// our own persistence work.
fn should_capture(target: &str) -> bool {
    target.starts_with("dioxuswalletmain")
        || target.starts_with("dioxus_wallet")
        || target.starts_with("wallet_core")
        || target.starts_with("bundle")
        || target.starts_with("mn-pkg")
        || target.starts_with("midnight")
}

fn level_from_tracing(level: Level) -> LogLevel {
    if level == Level::ERROR {
        LogLevel::Error
    } else if level == Level::WARN {
        LogLevel::Warn
    } else if level == Level::INFO {
        LogLevel::Info
    } else if level == Level::DEBUG {
        LogLevel::Debug
    } else {
        LogLevel::Trace
    }
}

/// Folds a `tracing::Event`'s fields into a single message
/// string. The convention across the codebase is to put the
/// human-readable payload in the `message` field (what
/// `tracing::info!("…")` populates by default). Other
/// structured fields land after a `·` separator so debug
/// detail isn't lost — but JSON-shaped output is future
/// work.
#[derive(Default)]
struct MessageVisitor {
    message: Option<String>,
    extra: Vec<(String, String)>,
}

impl MessageVisitor {
    fn into_message(self) -> String {
        let mut out = self.message.unwrap_or_default();
        if !self.extra.is_empty() {
            if !out.is_empty() {
                out.push_str(" · ");
            }
            for (i, (k, v)) in self.extra.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(k);
                out.push('=');
                out.push_str(v);
            }
        }
        out
    }
}

impl Visit for MessageVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message = Some(value.to_string());
        } else {
            self.extra.push((field.name().to_string(), value.to_string()));
        }
    }
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        let formatted = format!("{value:?}");
        if field.name() == "message" {
            self.message = Some(formatted);
        } else {
            self.extra.push((field.name().to_string(), formatted));
        }
    }
    fn record_i64(&mut self, field: &Field, value: i64) {
        self.extra.push((field.name().to_string(), value.to_string()));
    }
    fn record_u64(&mut self, field: &Field, value: u64) {
        self.extra.push((field.name().to_string(), value.to_string()));
    }
    fn record_bool(&mut self, field: &Field, value: bool) {
        self.extra.push((field.name().to_string(), value.to_string()));
    }
}

/// Drainer task. Pulls captured events from the channel,
/// batches them up to `MAX_BATCH` or until the flush
/// interval elapses (whichever is sooner), and writes the
/// batch in a single redb txn. Survives forever — exits
/// only when the channel sender closes (process shutdown).
///
/// `MAX_BATCH = 64` keeps each txn small; `FLUSH_INTERVAL =
/// 1s` keeps the on-disk archive at most one second behind
/// the live ring.
const MAX_BATCH: usize = 64;
const FLUSH_INTERVAL: Duration = Duration::from_secs(1);

pub async fn run_persist_drainer(
    store: WalletStore,
    mut rx: UnboundedReceiver<CapturedLog>,
) {
    let mut buffer: Vec<LogRow> = Vec::with_capacity(MAX_BATCH);
    let mut deadline = tokio::time::Instant::now() + FLUSH_INTERVAL;
    loop {
        let sleep = tokio::time::sleep_until(deadline);
        tokio::pin!(sleep);
        tokio::select! {
            maybe_event = rx.recv() => {
                match maybe_event {
                    Some(ev) => {
                        buffer.push(ev.into_store_row());
                        if buffer.len() >= MAX_BATCH {
                            flush(&store, &mut buffer);
                            deadline = tokio::time::Instant::now() + FLUSH_INTERVAL;
                        }
                    }
                    None => {
                        // Channel closed — final flush + exit.
                        flush(&store, &mut buffer);
                        return;
                    }
                }
            }
            _ = &mut sleep => {
                flush(&store, &mut buffer);
                deadline = tokio::time::Instant::now() + FLUSH_INTERVAL;
            }
        }
    }
}

fn flush(store: &WalletStore, buffer: &mut Vec<LogRow>) {
    if buffer.is_empty() {
        return;
    }
    if let Err(e) = store.append_logs(buffer) {
        // Can't log this through `tracing` — that would
        // re-enter the layer. Use stderr directly.
        eprintln!("[logs] persist drain failed: {e}");
    }
    buffer.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_visitor_concatenates_extras() {
        let mut v = MessageVisitor::default();
        // Simulate a tracing event with message + a couple of
        // structured fields. Field names are 'static so we
        // synthesise them via the trace macros' machinery in
        // the integration paths; here we exercise the visitor's
        // formatting directly via owned strings.
        v.message = Some("opened store".into());
        v.extra.push(("path".into(), "/tmp/wallet.redb".into()));
        v.extra.push(("hydrated".into(), "5".into()));
        assert_eq!(
            v.into_message(),
            "opened store · path=/tmp/wallet.redb, hydrated=5",
        );
    }

    #[test]
    fn message_visitor_handles_message_only() {
        let mut v = MessageVisitor::default();
        v.message = Some("simple".into());
        assert_eq!(v.into_message(), "simple");
    }

    #[test]
    fn message_visitor_handles_extras_only() {
        let mut v = MessageVisitor::default();
        v.extra.push(("k".into(), "v".into()));
        assert_eq!(v.into_message(), "k=v");
    }

    #[test]
    fn should_capture_filters_third_party() {
        assert!(should_capture("dioxuswalletmain::app"));
        assert!(should_capture("wallet_core::store::mod"));
        assert!(should_capture("bundle"));
        assert!(!should_capture("redb::transaction"));
        assert!(!should_capture("tokio::runtime"));
        assert!(!should_capture("hyper::client"));
    }

    /// End-to-end drainer happy path: build an in-memory
    /// store, hand the receiver to `run_persist_drainer`,
    /// push three events, close the sender, await the
    /// drainer's final flush, verify rows landed in redb.
    ///
    /// The "close the sender" pattern is how the drainer
    /// terminates in production too (process shutdown drops
    /// the App). Here we trigger it by dropping the
    /// `LogCapture` once we're done sending.
    #[tokio::test]
    async fn drainer_persists_events_to_store_end_to_end() {
        let store = WalletStore::open_in_memory("pw").expect("open store");
        let (capture, rx) = LogCapture::new();
        // Spawn the drainer; it runs until the channel closes.
        let store_for_drain = store.clone();
        let drainer_handle = tokio::spawn(async move {
            run_persist_drainer(store_for_drain, rx).await;
        });

        for i in 0..3 {
            capture.push(CapturedLog {
                timestamp_ns: 10_000 + i,
                timestamp_ms: i,
                level: LogLevel::Info,
                target: "tests".into(),
                message: format!("e{i}"),
            });
        }
        // Close the channel by dropping the capture (and the
        // sender it owns). The drainer should observe `None`
        // from `recv()`, do a final flush, and return.
        drop(capture);
        // Wait for the drainer to actually terminate so we know
        // the flush completed before we read.
        drainer_handle.await.expect("drainer task panicked");

        let rows = store.list_logs_recent(10).expect("list logs");
        assert_eq!(rows.len(), 3);
        // Newest-first: row 0 is the latest insertion (i==2).
        assert_eq!(rows[0].message, "e2");
        assert_eq!(rows[2].message, "e0");
    }

    /// Confirms the level + target round-trip cleanly through
    /// the drainer (LogLevel enum and target string both
    /// survive bincode + redb encoding).
    #[tokio::test]
    async fn drainer_preserves_level_and_target() {
        let store = WalletStore::open_in_memory("pw").expect("open store");
        let (capture, rx) = LogCapture::new();
        let store_for_drain = store.clone();
        let drainer = tokio::spawn(async move {
            run_persist_drainer(store_for_drain, rx).await;
        });
        capture.push(CapturedLog {
            timestamp_ns: 1,
            timestamp_ms: 0,
            level: LogLevel::Error,
            target: "wallet_core::store".into(),
            message: "boom".into(),
        });
        capture.push(CapturedLog {
            timestamp_ns: 2,
            timestamp_ms: 1,
            level: LogLevel::Debug,
            target: "dioxuswalletmain::app".into(),
            message: "rendered tab".into(),
        });
        drop(capture);
        drainer.await.expect("drainer task panicked");
        let rows = store.list_logs_recent(10).unwrap();
        assert_eq!(rows.len(), 2);
        // Newest first.
        assert_eq!(rows[0].level, LogLevel::Debug);
        assert_eq!(rows[0].target, "dioxuswalletmain::app");
        assert_eq!(rows[1].level, LogLevel::Error);
        assert_eq!(rows[1].target, "wallet_core::store");
    }

    #[test]
    fn ring_buffer_caps_at_capacity() {
        let (capture, _rx) = LogCapture::new();
        for i in 0..(RING_CAPACITY + 50) {
            capture.push(CapturedLog {
                timestamp_ns: i as i64,
                timestamp_ms: i as i64,
                level: LogLevel::Info,
                target: "t".into(),
                message: format!("e{i}"),
            });
        }
        let snap = capture.snapshot();
        assert_eq!(snap.len(), RING_CAPACITY);
        // Newest first: the latest event written should be at
        // index 0.
        let newest = snap.first().unwrap();
        assert_eq!(newest.message, format!("e{}", RING_CAPACITY + 49));
    }
}
