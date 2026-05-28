//! `Notifications` port — fire-and-forget toasts / banners /
//! progress hints.  Sits next to `UserInterface` (wave B6):
//! `Notifications::notify` never blocks, never prompts.  When a
//! use-case needs the user to actually answer something, it
//! reaches for `UserInterface::prompt_*`; when it only needs to
//! tell the user "this happened", it uses `Notifications`.
//!
//! See design doc §2.3 (`Notifications` port).
//!
//! The Dioxus adapter (signal-pushing `DioxusNotifier`) lands in
//! wave E alongside the rest of the UI-adapter wiring.  This
//! file ships the headless + test adapters.

use std::sync::Mutex;

/// Severity of a notification.  Used by the adapter to choose
/// visual treatment (colour, icon) or log level (stderr line
/// prefix).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NotifyLevel {
    Info,
    Warn,
    Error,
}

impl NotifyLevel {
    /// Lowercase tag suitable for log lines or CLI tags.
    pub fn as_tag(self) -> &'static str {
        match self {
            NotifyLevel::Info => "info",
            NotifyLevel::Warn => "warn",
            NotifyLevel::Error => "error",
        }
    }
}

/// Object-safe port.  No async needed — `notify` always returns
/// immediately; if the adapter wants to defer (e.g. coalesce
/// before flushing), that's an adapter concern, not the trait
/// contract.
pub trait Notifications: Send + Sync + 'static {
    fn notify(&self, level: NotifyLevel, msg: &str);
}

/// Headless / CLI adapter — writes one line per notification to
/// stderr.  Format: `[LEVEL] <msg>` — terse enough to grep, no
/// JSON wrapping (the headless binary's JSON protocol travels
/// on stdout, not stderr).
#[derive(Debug, Default, Clone, Copy)]
pub struct StderrNotifier;

impl Notifications for StderrNotifier {
    fn notify(&self, level: NotifyLevel, msg: &str) {
        eprintln!("[{}] {}", level.as_tag(), msg);
    }
}

/// Null-object adapter — drops every notification.  Useful
/// when the caller doesn't care about user-facing events at
/// all (long-running batch tests).
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopNotifier;

impl Notifications for NoopNotifier {
    fn notify(&self, _: NotifyLevel, _: &str) {}
}

/// Test adapter — collects every notification into a `Vec<NotifyRecord>`
/// the test can assert on.  Drainable via `take()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotifyRecord {
    pub level: NotifyLevel,
    pub msg: String,
}

#[derive(Default)]
pub struct CollectingNotifier {
    inner: Mutex<Vec<NotifyRecord>>,
}

impl CollectingNotifier {
    pub fn new() -> Self {
        Self::default()
    }

    /// Snapshot the current set without draining.  Cheap — just
    /// clones owned `String`s out of a bounded `Vec`.
    pub fn snapshot(&self) -> Vec<NotifyRecord> {
        self.inner.lock().map(|g| g.clone()).unwrap_or_default()
    }

    /// Drain the recorded notifications.  Subsequent
    /// `snapshot()` / `take()` calls see an empty buffer until
    /// new `notify()` calls land.
    pub fn take(&self) -> Vec<NotifyRecord> {
        self.inner
            .lock()
            .map(|mut g| std::mem::take(&mut *g))
            .unwrap_or_default()
    }

    /// Convenience for tests: count how many records match a
    /// level.  Equivalent to `snapshot().iter().filter(...).count()`
    /// but reads cleaner inline.
    pub fn count_at(&self, level: NotifyLevel) -> usize {
        self.inner
            .lock()
            .map(|g| g.iter().filter(|r| r.level == level).count())
            .unwrap_or(0)
    }
}

impl Notifications for CollectingNotifier {
    fn notify(&self, level: NotifyLevel, msg: &str) {
        if let Ok(mut g) = self.inner.lock() {
            g.push(NotifyRecord {
                level,
                msg: msg.to_string(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collecting_notifier_records_in_order() {
        let n = CollectingNotifier::new();
        n.notify(NotifyLevel::Info, "boot");
        n.notify(NotifyLevel::Warn, "stale cache");
        n.notify(NotifyLevel::Error, "deploy failed");
        let snap = n.snapshot();
        assert_eq!(snap.len(), 3);
        assert_eq!(snap[0].level, NotifyLevel::Info);
        assert_eq!(snap[0].msg, "boot");
        assert_eq!(snap[2].level, NotifyLevel::Error);
    }

    #[test]
    fn take_drains_buffer() {
        let n = CollectingNotifier::new();
        n.notify(NotifyLevel::Info, "x");
        n.notify(NotifyLevel::Info, "y");
        let drained = n.take();
        assert_eq!(drained.len(), 2);
        assert_eq!(n.snapshot().len(), 0);
    }

    #[test]
    fn count_at_filters_by_level() {
        let n = CollectingNotifier::new();
        n.notify(NotifyLevel::Info, "1");
        n.notify(NotifyLevel::Warn, "2");
        n.notify(NotifyLevel::Info, "3");
        assert_eq!(n.count_at(NotifyLevel::Info), 2);
        assert_eq!(n.count_at(NotifyLevel::Warn), 1);
        assert_eq!(n.count_at(NotifyLevel::Error), 0);
    }

    #[test]
    fn noop_notifier_drops_silently() {
        let n: Box<dyn Notifications> = Box::new(NoopNotifier);
        // No panic, no assertion to make — null object.
        n.notify(NotifyLevel::Error, "ignored");
    }

    #[test]
    fn notify_level_tag_strings() {
        assert_eq!(NotifyLevel::Info.as_tag(), "info");
        assert_eq!(NotifyLevel::Warn.as_tag(), "warn");
        assert_eq!(NotifyLevel::Error.as_tag(), "error");
    }
}
