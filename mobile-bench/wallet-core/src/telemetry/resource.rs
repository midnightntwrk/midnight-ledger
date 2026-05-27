//! Cross-platform RSS + CPU-time sampler via POSIX `getrusage`.
//!
//! Works on every target the wallet ships on: macOS, iOS, Linux,
//! Android. Returns `None` on anything else (Windows desktop dev
//! hosts) so consumers gracefully degrade to "no resource delta
//! recorded".
//!
//! Why `getrusage` and not `/proc/self/status`?
//! - `/proc` is Linux-only; macOS / iOS have no equivalent.
//! - `getrusage(RUSAGE_SELF)` is POSIX-standard and a single
//!   syscall — cheap enough to bracket any operation.
//!
//! `ru_maxrss` units differ across platforms:
//! - Linux / Android: kilobytes
//! - macOS / iOS: **bytes** (Apple's man page documents this
//!   inconsistency)
//!
//! The sampler normalises to **kilobytes** so callers see one
//! unit regardless of host.

use super::{HttpRecord, Metrics, OpRecord};

/// One point-in-time resource sample.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ResourceSample {
    /// Resident-set size in KiB. Linux reports current,
    /// macOS / iOS report peak — both are useful for spotting
    /// memory-heavy ops.
    pub rss_kb: u64,
    /// User + system CPU time consumed by the process so far,
    /// in microseconds. Monotonic; subtract two samples to get
    /// the CPU time spent inside a bracketed region.
    pub cpu_us: u64,
}

/// Take an RSS + CPU snapshot. `None` if the host doesn't
/// expose one cheaply.
pub trait ResourceProbe: Send + Sync + 'static {
    fn sample(&self) -> Option<ResourceSample>;
}

/// Always-`None` adapter. Use this when you want the
/// `time_op` machinery for wall-clock timing without paying for
/// resource accounting (or on a host that doesn't support it).
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopResourceProbe;

impl ResourceProbe for NoopResourceProbe {
    fn sample(&self) -> Option<ResourceSample> {
        None
    }
}

/// POSIX `getrusage`-backed probe. Stateless — share an `Arc`.
#[derive(Debug, Default, Clone, Copy)]
pub struct RusageProbe;

impl ResourceProbe for RusageProbe {
    fn sample(&self) -> Option<ResourceSample> {
        rusage_sample()
    }
}

#[cfg(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
))]
fn rusage_sample() -> Option<ResourceSample> {
    // SAFETY: `getrusage` is a POSIX syscall taking a flag + a
    // valid pointer to a zero-initialised rusage struct. We
    // zero-init via `std::mem::zeroed` (libc::rusage is
    // POD-equivalent), pass `RUSAGE_SELF`, and only read fields
    // documented to be populated on every POSIX target.
    unsafe {
        let mut ru: libc::rusage = std::mem::zeroed();
        if libc::getrusage(libc::RUSAGE_SELF, &mut ru) != 0 {
            return None;
        }
        let ru_maxrss = ru.ru_maxrss as i64;
        // Normalise to KiB. macOS / iOS report bytes; Linux /
        // Android report KiB.
        let rss_kb = if cfg!(any(target_os = "macos", target_os = "ios")) {
            (ru_maxrss / 1024) as u64
        } else {
            ru_maxrss as u64
        };
        let user_us =
            (ru.ru_utime.tv_sec as u64) * 1_000_000 + (ru.ru_utime.tv_usec as u64);
        let sys_us =
            (ru.ru_stime.tv_sec as u64) * 1_000_000 + (ru.ru_stime.tv_usec as u64);
        Some(ResourceSample {
            rss_kb,
            cpu_us: user_us + sys_us,
        })
    }
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
)))]
fn rusage_sample() -> Option<ResourceSample> {
    None
}

// Forwarding `Metrics` impls so an `Arc<RusageProbe>` etc.
// doesn't get accidentally used where `Metrics` was wanted —
// the compiler error is clearer than the trait-bound mismatch.
// These helpers don't carry an opinion; they exist to make
// `ResourceProbe` adapters easy to mix into composite stacks
// when a future op also wants to forward metrics events.
impl Metrics for NoopResourceProbe {
    fn record_http(&self, _: &HttpRecord<'_>) {}
    fn record_op(&self, _: &OpRecord<'_>) {}
    fn incr(&self, _: &str, _: u64) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noop_probe_returns_none() {
        assert!(NoopResourceProbe.sample().is_none());
    }

    #[cfg(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios",
    ))]
    #[test]
    fn rusage_probe_returns_some_on_posix() {
        let s = RusageProbe.sample().expect("posix host should return a sample");
        // The test process has a non-zero RSS by definition (we're
        // running). A 0-byte RSS would mean we mis-parsed.
        assert!(s.rss_kb > 0, "rss_kb should be > 0, got {}", s.rss_kb);
        // CPU may legitimately be 0 if scheduling hasn't ticked
        // yet, but on real CI it almost always isn't.
    }

    #[cfg(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios",
    ))]
    #[test]
    fn rusage_probe_cpu_is_monotonic_under_load() {
        let a = RusageProbe.sample().unwrap();
        // Burn a small amount of CPU.
        let mut acc: u64 = 0;
        for i in 0..200_000 {
            acc = acc.wrapping_add(i);
        }
        std::hint::black_box(acc);
        let b = RusageProbe.sample().unwrap();
        // Allow equality — on a very fast machine the loop may
        // not bump the rusage struct between samples — but never
        // a regression.
        assert!(b.cpu_us >= a.cpu_us, "{} < {}", b.cpu_us, a.cpu_us);
    }
}
