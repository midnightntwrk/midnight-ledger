//! Lightweight `/proc/self/{status,stat}` sampler used by the
//! Benchmark tab's live `RSS MiB` / `CPU %` pills.
//!
//! Split into a pure parser (`parse_proc_stats`, exercised by
//! the unit tests below against known fixtures) and the IO
//! wrapper (`proc_self_stats`) so the parsing logic can be
//! tested without a live `/proc` filesystem. macOS / iOS have
//! no `/proc`; the IO wrapper compiles down to a `None`-stub on
//! those targets so the call sites can stay cfg-gate free.

/// Linux / Android scheduler clock tick — `sysconf(_SC_CLK_TCK)`.
/// Hard-coded so we don't pay a libc dep on non-Android hosts;
/// the value is `100` on every Android device and effectively
/// every Linux distro built in the last decade.
pub(crate) const CLK_TCK: u64 = 100;

/// Parse a `/proc/self/status` body and a `/proc/self/stat` body
/// into `(rss_kb, total_cpu_jiffies)`. Returns `None` on any
/// parse failure — caller is expected to skip the sample.
///
/// `/proc/self/status` has a `VmRSS:    NNN kB` line; the
/// numeric field is the resident-set size in KiB.
///
/// `/proc/self/stat` is a single space-separated line whose
/// **`comm`** field (the executable name) is in parens and may
/// itself contain spaces. The fix is to split *after the last
/// closing paren* — every field past that point is well-defined
/// space-separated. After that split, `utime` is field index 11
/// and `stime` is field index 12 (both in clock ticks).
// On non-Linux/Android targets the IO wrapper is a stub that
// returns `None`, so `parse_proc_stats` has no live callers
// outside the test module on those builds. The parser still
// needs to compile (the tests run on macOS dev hosts), so
// silence the dead-code lint rather than cfg-gating the
// function out of the desktop build.
#[allow(dead_code)]
pub(crate) fn parse_proc_stats(status: &str, stat: &str) -> Option<(u64, u64)> {
    let rss_kb: u64 = status
        .lines()
        .find(|l| l.starts_with("VmRSS:"))?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()?;
    let after = &stat[stat.rfind(')')? + 2..];
    let fields: Vec<&str> = after.split_whitespace().collect();
    let utime: u64 = fields.get(11)?.parse().ok()?;
    let stime: u64 = fields.get(12)?.parse().ok()?;
    Some((rss_kb, utime + stime))
}

/// Read live RSS (KiB) and total CPU jiffies from `/proc/self`.
/// Returns `None` on parse failure or on platforms without `/proc`.
/// Cheap enough to poll a few times a second.
#[cfg(any(target_os = "android", target_os = "linux"))]
pub(crate) fn proc_self_stats() -> Option<(u64, u64)> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let stat = std::fs::read_to_string("/proc/self/stat").ok()?;
    parse_proc_stats(&status, &stat)
}

#[cfg(not(any(target_os = "android", target_os = "linux")))]
pub(crate) fn proc_self_stats() -> Option<(u64, u64)> {
    None
}

/// Parse a `/proc/stat` body into per-core `(busy_jiffies,
/// total_jiffies)` cumulative counters. Each entry corresponds to
/// one CPU; index 0 = `cpu0`, etc. The aggregate `cpu` line at the
/// top is skipped — callers compute aggregate from the sum or use
/// `proc_self_stats` for self-only.
///
/// `/proc/stat` cpu lines:
///   `cpuN user nice system idle iowait irq softirq steal guest guest_nice`
///
/// We treat `busy = user + nice + system + irq + softirq` and
/// `total = busy + idle + iowait` (consistent with how `top` and
/// `htop` colour their bars on Android). `steal` and the `guest*`
/// columns are ignored — they're always 0 on bare-metal Android.
///
/// Returns an empty Vec on parse failure rather than `None` so
/// the poller doesn't constantly drop samples on a partial read.
#[allow(dead_code)]
pub(crate) fn parse_proc_stat_per_core(stat_all: &str) -> Vec<(u64, u64)> {
    let mut out = Vec::new();
    for line in stat_all.lines() {
        let mut fields = line.split_whitespace();
        let Some(label) = fields.next() else { continue };
        if !label.starts_with("cpu") {
            // cpu* lines are at the top of /proc/stat. Once we hit
            // `intr` / `ctxt` / `btime` / etc, we're done.
            break;
        }
        if label == "cpu" {
            // Aggregate line — skip.
            continue;
        }
        // Per-core line. Need at least 7 numeric fields to compute busy/total.
        let nums: Vec<u64> = fields
            .filter_map(|s| s.parse::<u64>().ok())
            .collect();
        if nums.len() < 7 {
            continue;
        }
        let user = nums[0];
        let nice = nums[1];
        let system = nums[2];
        let idle = nums[3];
        let iowait = nums[4];
        let irq = nums[5];
        let softirq = nums[6];
        let busy = user + nice + system + irq + softirq;
        let total = busy + idle + iowait;
        out.push((busy, total));
    }
    out
}

/// Read `/proc/stat` and return per-core cumulative
/// `(busy, total)` jiffies. None on platforms without `/proc`.
#[cfg(any(target_os = "android", target_os = "linux"))]
pub(crate) fn proc_per_core_stats() -> Option<Vec<(u64, u64)>> {
    let stat = std::fs::read_to_string("/proc/stat").ok()?;
    Some(parse_proc_stat_per_core(&stat))
}

#[cfg(not(any(target_os = "android", target_os = "linux")))]
pub(crate) fn proc_per_core_stats() -> Option<Vec<(u64, u64)>> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // Realistic `/proc/self/status` excerpt — only the lines our
    // parser reads matter, but the fixture keeps surrounding
    // noise to prove the `find(|l| l.starts_with("VmRSS:"))`
    // selector works against the real shape.
    const STATUS_FIXTURE: &str = "\
Name:\tdioxus-wallet
Umask:\t0027
State:\tS (sleeping)
Tgid:\t14801
VmPeak:\t  4731384 kB
VmSize:\t  4731384 kB
VmLck:\t        0 kB
VmPin:\t        0 kB
VmHWM:\t   712504 kB
VmRSS:\t   712504 kB
RssAnon:\t  408192 kB
RssFile:\t  304312 kB
";

    // Realistic `/proc/self/stat` line. The `comm` field in
    // parens contains a single token here; the parens-handling
    // is exercised separately by `stat_handles_paren_comm`.
    const STAT_FIXTURE: &str =
        "14801 (dioxus-wallet) S 14790 14801 14801 0 -1 4194304 \
         5839 0 0 0 12345 67890 0 0 20 0 12 0 87654321 4844736000 \
         178126 18446744073709551615 1 1 0 0 0 0 0 0 0 0 0 0 17 \
         3 0 0 0 0 0 0 0 0 0 0 0 0 0";

    #[test]
    fn parses_typical_fixture() {
        let (rss_kb, jiffies) = parse_proc_stats(STATUS_FIXTURE, STAT_FIXTURE)
            .expect("fixture parses");
        // VmRSS reported as `712504 kB`.
        assert_eq!(rss_kb, 712_504);
        // utime=12345, stime=67890 → 80235 jiffies total.
        assert_eq!(jiffies, 12_345 + 67_890);
    }

    #[test]
    fn stat_handles_paren_comm() {
        // `comm` containing spaces and a literal `)`. Split after
        // the LAST `)` so the rest of the fields stay aligned.
        let stat = "14801 (dio xus-wal:let)) S 1 2 3 4 -1 6 7 8 9 10 \
                    111 222 13 14 15 16 17 18 19 20 21 22 23 24 25 \
                    26 27 28 29 30 31 32 33 34 35 36 37 38 39 40 41";
        let (_rss, jiffies) =
            parse_proc_stats(STATUS_FIXTURE, stat).expect("parses");
        // utime=111, stime=222 → 333 jiffies.
        assert_eq!(jiffies, 333);
    }

    #[test]
    fn rejects_missing_vmrss() {
        // Status without the VmRSS line → None, no panic.
        let status = "Name:\tx\nState:\tS\n";
        assert!(parse_proc_stats(status, STAT_FIXTURE).is_none());
    }

    #[test]
    fn rejects_short_stat() {
        // /proc/self/stat truncated before utime → None.
        let stat = "1 (proc) S 0 0 0 0";
        assert!(parse_proc_stats(STATUS_FIXTURE, stat).is_none());
    }

    #[test]
    fn rejects_garbage_rss() {
        // VmRSS line present but the numeric field is non-numeric.
        let status = "VmRSS:\tABC kB\n";
        assert!(parse_proc_stats(status, STAT_FIXTURE).is_none());
    }

    #[test]
    fn rejects_stat_without_paren() {
        // No `)` in stat at all → rfind returns None → parser
        // bails before unwrap.
        let stat = "1 noparens 2 3 4 5 6 7 8 9 10 11 12 13 14";
        assert!(parse_proc_stats(STATUS_FIXTURE, stat).is_none());
    }

    // Realistic `/proc/stat` excerpt with 4 cores. The header
    // `cpu` line is the aggregate; we want to skip it. After the
    // cpu lines come intr / ctxt / btime / etc, which we also
    // ignore.
    const STAT_PER_CORE_FIXTURE: &str = "\
cpu  1000 0 500 8000 10 20 5 0 0 0
cpu0 250 0 125 2000 2 5 1 0 0 0
cpu1 250 0 125 2000 3 5 2 0 0 0
cpu2 250 0 125 2000 2 5 1 0 0 0
cpu3 250 0 125 2000 3 5 1 0 0 0
intr 12345 ...
ctxt 67890
btime 1700000000
processes 4242
procs_running 1
procs_blocked 0
";

    #[test]
    fn parses_per_core_stat() {
        let cores = parse_proc_stat_per_core(STAT_PER_CORE_FIXTURE);
        assert_eq!(cores.len(), 4, "should skip the aggregate cpu line");
        // cpu0: busy = 250 + 0 + 125 + 5 + 1 = 381
        //       total = 381 + 2000 + 2 = 2383
        assert_eq!(cores[0], (381, 2383));
        // cpu1 has slightly different iowait/softirq.
        assert_eq!(cores[1], (382, 2385));
    }

    #[test]
    fn per_core_rejects_short_line() {
        // Line with fewer than 7 numeric fields — silently skipped.
        let stat = "cpu0 100 0 50\nintr 99\n";
        assert!(parse_proc_stat_per_core(stat).is_empty());
    }

    #[test]
    fn per_core_stops_at_intr() {
        // Ensures we don't mistake `intr ...` as a CPU line just because
        // it doesn't start with `cpu`. Defensive: the `break` triggers.
        let stat = "cpu0 1 2 3 4 5 6 7\nintr fake fake fake\n";
        let cores = parse_proc_stat_per_core(stat);
        assert_eq!(cores.len(), 1);
    }

    #[test]
    fn clk_tck_is_standard_linux_value() {
        // Belt-and-braces sanity: every Linux distro / Android
        // device this binary ships on uses 100 HZ. If this ever
        // needs to be variable we'd plumb `sysconf` from libc;
        // until then the constant is a load-bearing assumption.
        assert_eq!(CLK_TCK, 100);
    }
}
