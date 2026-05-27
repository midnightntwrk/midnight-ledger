//! Clock port. Same DI shape as `HttpClient` / `VcStorage`:
//! consumers take an `&dyn Clock` and never read the system clock
//! directly. The real-deps adapter (`SystemClock`) lives next to
//! the trait; a `FixedClock` lives behind `#[cfg(any(test,
//! feature = "test-support"))]` so unit tests can make
//! `last_verified_ms` / `issued_at_ms` deterministic.
//!
//! Phase 1 footprint: epoch-millis only. If a flow ever needs
//! sub-millisecond resolution or a monotonic instant, add a second
//! method here rather than scattering `SystemTime::now()` calls
//! across the codebase.

#[cfg(any(test, feature = "test-support"))]
use std::sync::atomic::{AtomicU64, Ordering};

/// Returns the current Unix-epoch time in milliseconds.
pub trait Clock: Send + Sync + 'static {
    /// Milliseconds since the Unix epoch.
    ///
    /// Implementations that can't read a wall clock (e.g. a
    /// pre-epoch test clock) should return `0` rather than panic
    /// — the existing `SystemTime::now().duration_since(UNIX_EPOCH)`
    /// callers all collapsed the error to `0` with `unwrap_or(0)`.
    fn now_ms(&self) -> u64;
}

/// Real-deps adapter reading `std::time::SystemTime`.
/// Stateless — share an `Arc` or pass `&SystemClock` directly.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }
}

/// Test adapter: returns a fixed epoch-ms value. `set()` /
/// `advance()` let a test drive the clock forward without
/// constructing a new instance.
#[cfg(any(test, feature = "test-support"))]
#[derive(Debug, Default)]
pub struct FixedClock(AtomicU64);

#[cfg(any(test, feature = "test-support"))]
impl FixedClock {
    /// Build a fixed clock pinned at `now_ms`.
    pub fn new(now_ms: u64) -> Self {
        Self(AtomicU64::new(now_ms))
    }
    /// Overwrite the reported time.
    pub fn set(&self, now_ms: u64) {
        self.0.store(now_ms, Ordering::Relaxed);
    }
    /// Advance the reported time by `delta_ms`.
    pub fn advance(&self, delta_ms: u64) {
        self.0.fetch_add(delta_ms, Ordering::Relaxed);
    }
}

#[cfg(any(test, feature = "test-support"))]
impl Clock for FixedClock {
    fn now_ms(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_clock_returns_nonzero() {
        // Loose smoke check — Phase 1 doesn't need a closer assertion
        // than "we got a plausible recent epoch".
        let t = SystemClock.now_ms();
        // 2020-01-01 in epoch-ms, comfortably below "now"
        assert!(t > 1_577_836_800_000, "got {t}");
    }

    #[test]
    fn fixed_clock_returns_pinned_value() {
        let c = FixedClock::new(123_456);
        assert_eq!(c.now_ms(), 123_456);
        c.set(999);
        assert_eq!(c.now_ms(), 999);
        c.advance(1);
        assert_eq!(c.now_ms(), 1000);
    }

    #[test]
    fn fixed_clock_default_starts_at_zero() {
        let c = FixedClock::default();
        assert_eq!(c.now_ms(), 0);
    }
}
