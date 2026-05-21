//! Pure formatting helpers used across the UI.
//!
//! Everything in this module is intentionally free of Dioxus
//! and wallet-core dependencies so it can be unit-tested in
//! isolation. The functions are all pure (input → string,
//! deterministic) and the tests at the bottom exercise the
//! boundary conditions the UI cares about — K/M/B/T thresholds,
//! comma grouping at every multiple of three digits, padding
//! of zero-fractional values, etc.

/// NIGHT subunit precision: 1 NIGHT = 10^6 atomic units.
pub(crate) const NIGHT_DECIMALS: u32 = 6;

/// DUST subunit precision: 1 DUST = 10^15 atomic units.
pub(crate) const DUST_DECIMALS: u32 = 15;

/// Render a u128 subunit count as a comma-grouped decimal string —
/// e.g. `250000000000000` → `"250,000,000,000,000"`. Matches
/// example-counter's `formatBalance` (`BigInt.toLocaleString()`)
/// so the displayed values agree between wallets.
pub(crate) fn format_subunits(n: u128) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out.chars().rev().collect()
}

/// Convert a u128 subunit count to whole-unit representations.
///
/// Returns `(compact, exact)` where:
/// - `compact` collapses large whole-unit values to K/M/B/T notation
///   (e.g. `1,000` → `"1K"`, `5,234,000` → `"5.23M"`); whole values
///   under 1,000 render as a comma-grouped integer with up to two
///   significant fractional digits when meaningful.
/// - `exact` is the full whole.fractional value with comma-grouped
///   thousands and trailing zeros trimmed from the fraction.
///
/// Both strings are unit-less; callers append " NIGHT" / " DUST".
pub(crate) fn format_balance(subunits: u128, decimals: u32) -> (String, String) {
    let scale = 10u128.pow(decimals);
    let whole = subunits / scale;
    let frac = subunits % scale;

    let frac_padded = format!("{:0>width$}", frac, width = decimals as usize);
    let frac_trimmed = frac_padded.trim_end_matches('0');
    let whole_str = format_subunits(whole);
    let exact = if frac_trimmed.is_empty() {
        whole_str.clone()
    } else {
        format!("{}.{}", whole_str, frac_trimmed)
    };

    let compact = if whole >= 1_000 {
        let (divisor, suffix) = if whole >= 1_000_000_000_000 {
            (1_000_000_000_000u128, "T")
        } else if whole >= 1_000_000_000 {
            (1_000_000_000u128, "B")
        } else if whole >= 1_000_000 {
            (1_000_000u128, "M")
        } else {
            (1_000u128, "K")
        };
        // Two decimal digits of precision (e.g. 1234 → "1.23K").
        let scaled = whole * 100 / divisor;
        let int_part = scaled / 100;
        let frac_part = scaled % 100;
        if frac_part == 0 {
            format!("{}{}", int_part, suffix)
        } else if frac_part % 10 == 0 {
            format!("{}.{}{}", int_part, frac_part / 10, suffix)
        } else {
            format!("{}.{:02}{}", int_part, frac_part, suffix)
        }
    } else if frac_trimmed.is_empty() {
        whole_str
    } else {
        // Sub-unit balance: show up to four significant fractional
        // digits so tiny accruals are visible without dumping all 15.
        let frac_short: String = frac_trimmed.chars().take(4).collect();
        format!("{}.{}", whole_str, frac_short)
    };

    (compact, exact)
}

/// Render a DUST atomic-unit count with comma grouping and the
/// "atomic" suffix. Dust is `10^-15 DUST` per atomic so even
/// small transactions show 11+ digits; we leave the unit
/// decimal-grouped for readability rather than converting.
pub(crate) fn format_atomic_dust(n: u128) -> String {
    if n == 0 {
        "0 atomic".to_string()
    } else {
        format!("{} atomic", group_thousands(n))
    }
}

/// Render a NIGHT atomic-unit count. NIGHT is `10^-6 NIGHT` so
/// 1_000_000 atomic = 1 NIGHT. For values ≥ 10^6 we render a
/// short suffix; smaller values stay as raw atomic units.
pub(crate) fn format_atomic_night(n: u128) -> String {
    if n == 0 {
        "0 atomic".to_string()
    } else if n >= 1_000_000 {
        let whole = n / 1_000_000;
        let frac = n % 1_000_000;
        format!("{}.{:06} NIGHT", group_thousands(whole), frac)
    } else {
        format!("{} atomic", group_thousands(n))
    }
}

/// Comma-group an integer for readability — `12345678 → "12,345,678"`.
pub(crate) fn group_thousands(n: u128) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out.chars().rev().collect()
}

/// Render a millisecond count as `Xms` / `X.YZs` / `X.Ys` /
/// `Xm YYs` depending on magnitude. Buckets:
/// - `< 1_000`        → `Xms`
/// - `< 10_000`       → two-decimal seconds (`1.23s`)
/// - `< 60_000`       → one-decimal seconds (`12.3s`)
/// - `≥ 60_000`       → minutes + zero-padded seconds (`1m 03s`)
pub(crate) fn format_ms(ms: u64) -> String {
    if ms < 1_000 {
        format!("{ms}ms")
    } else if ms < 10_000 {
        let s = ms as f64 / 1000.0;
        format!("{s:.2}s")
    } else if ms < 60_000 {
        let s = ms as f64 / 1000.0;
        format!("{s:.1}s")
    } else {
        let m = ms / 60_000;
        let s = (ms % 60_000) / 1_000;
        format!("{m}m {s:02}s")
    }
}

/// Render an `i64` with **space**-grouped thousands. Used by the
/// Metrics tab where the slightly looser visual matches the
/// "engineering" aesthetic of that view; balance rows use
/// [`format_subunits`] (comma) for parity with example-counter.
pub(crate) fn format_int(n: i64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(' ');
        }
        out.push(c);
    }
    out.chars().rev().collect()
}

/// Render a unix-millis timestamp as a 24h `HH:MM:SS.mmm` string
/// in **UTC**. Used by the Logs tab — short enough to fit in the
/// stamp column without overlapping the level column.
///
/// Returns the same string for any `ts_ms` modulo 86,400,000 (one
/// day), so consecutive logs across a day boundary look identical
/// in the time component but distinct in absolute terms. The Logs
/// tab orders rows by `timestamp_ns` separately so the visual
/// repeat doesn't confuse chronology.
pub(crate) fn format_log_timestamp(ts_ms: i64) -> String {
    let total_secs = ts_ms.div_euclid(1000);
    let millis = ts_ms.rem_euclid(1000) as u32;
    let secs = total_secs.rem_euclid(86_400);
    let h = (secs / 3600) as u32;
    let m = ((secs % 3600) / 60) as u32;
    let s = (secs % 60) as u32;
    format!("{h:02}:{m:02}:{s:02}.{millis:03}")
}

/// Shorten a secret-key-ref UUID to `01234567…cdef` for table
/// display. Keys ≤ 12 chars are returned as-is so the function
/// is idempotent on already-short identifiers.
pub(crate) fn short_keyref(k: &str) -> String {
    if k.len() <= 12 {
        k.to_string()
    } else {
        format!("{}…{}", &k[..8], &k[k.len() - 4..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── format_subunits ────────────────────────────────────────

    #[test]
    fn subunits_zero() {
        assert_eq!(format_subunits(0), "0");
    }

    #[test]
    fn subunits_under_thousand() {
        assert_eq!(format_subunits(1), "1");
        assert_eq!(format_subunits(999), "999");
    }

    #[test]
    fn subunits_groups_thousands() {
        assert_eq!(format_subunits(1_000), "1,000");
        assert_eq!(format_subunits(12_345), "12,345");
        assert_eq!(format_subunits(123_456), "123,456");
        assert_eq!(format_subunits(1_234_567), "1,234,567");
    }

    #[test]
    fn subunits_u128_max_fits() {
        // u128::MAX = 340_282_366_920_938_463_463_374_607_431_768_211_455
        let s = format_subunits(u128::MAX);
        assert!(s.starts_with("340,282"));
        assert!(s.ends_with("455"));
        // Every 3 digits gets a comma → for u128::MAX that's 13
        // commas in the 39-digit decimal expansion.
        assert_eq!(s.matches(',').count(), 12);
    }

    // ─── format_balance ────────────────────────────────────────

    #[test]
    fn balance_zero() {
        let (c, e) = format_balance(0, NIGHT_DECIMALS);
        assert_eq!(c, "0");
        assert_eq!(e, "0");
    }

    #[test]
    fn balance_exact_one_unit() {
        let (c, e) = format_balance(1_000_000, NIGHT_DECIMALS);
        assert_eq!(c, "1");
        assert_eq!(e, "1");
    }

    #[test]
    fn balance_under_one_unit_truncates_fraction_to_4() {
        // 0.123456789 NIGHT — only 4 frac digits surface in the
        // compact form, but the exact form keeps all 6 (then
        // trims trailing zeros, which there aren't any of here).
        let (c, e) = format_balance(123_456, NIGHT_DECIMALS);
        assert_eq!(c, "0.1234");
        assert_eq!(e, "0.123456");
    }

    #[test]
    fn balance_thousand_boundary_k() {
        let (c, e) = format_balance(1_000 * 1_000_000, NIGHT_DECIMALS);
        assert_eq!(c, "1K");
        assert_eq!(e, "1,000");
    }

    #[test]
    fn balance_million_boundary_m() {
        let (c, e) = format_balance(1_000_000 * 1_000_000, NIGHT_DECIMALS);
        assert_eq!(c, "1M");
        assert_eq!(e, "1,000,000");
    }

    #[test]
    fn balance_billion_boundary_b() {
        let (c, _e) = format_balance(1_000_000_000u128 * 1_000_000, NIGHT_DECIMALS);
        assert_eq!(c, "1B");
    }

    #[test]
    fn balance_trillion_boundary_t() {
        let (c, _e) = format_balance(1_000_000_000_000u128 * 1_000_000, NIGHT_DECIMALS);
        assert_eq!(c, "1T");
    }

    #[test]
    fn balance_compact_two_decimal_digits() {
        // 5,234 NIGHT → "5.23K", and the third digit (4) is dropped
        // via integer division — *not* rounded.
        let (c, e) = format_balance(5_234 * 1_000_000, NIGHT_DECIMALS);
        assert_eq!(c, "5.23K");
        assert_eq!(e, "5,234");
    }

    #[test]
    fn balance_compact_trims_trailing_zero_in_fraction() {
        // 5,200 NIGHT → scaled = 520 → "5.2K", not "5.20K".
        let (c, _e) = format_balance(5_200 * 1_000_000, NIGHT_DECIMALS);
        assert_eq!(c, "5.2K");
    }

    #[test]
    fn balance_exact_strips_trailing_zero_fraction() {
        // 1.500000 NIGHT — exact form trims trailing zeros to
        // "1.5". When whole < 1,000 the compact branch falls
        // through to the sub-unit case (compact = "1.5"), not the
        // K/M/B/T abbreviation. Both forms agree here because the
        // value is small.
        let (c, e) = format_balance(1_500_000, NIGHT_DECIMALS);
        assert_eq!(c, "1.5");
        assert_eq!(e, "1.5");
    }

    #[test]
    fn balance_dust_decimals() {
        // 1 DUST = 10^15 atomic. 2.5 DUST → whole=2, frac trims
        // to "5" → both compact and exact read "2.5".
        let (c, e) = format_balance(2_500_000_000_000_000u128, DUST_DECIMALS);
        assert_eq!(c, "2.5");
        assert_eq!(e, "2.5");
    }

    // ─── format_ms ──────────────────────────────────────────────

    #[test]
    fn ms_buckets() {
        assert_eq!(format_ms(0), "0ms");
        assert_eq!(format_ms(999), "999ms");
        assert_eq!(format_ms(1_000), "1.00s");
        assert_eq!(format_ms(9_999), "10.00s");
        assert_eq!(format_ms(10_000), "10.0s");
        assert_eq!(format_ms(59_999), "60.0s");
        assert_eq!(format_ms(60_000), "1m 00s");
        assert_eq!(format_ms(125_000), "2m 05s");
    }

    // ─── group_thousands ────────────────────────────────────────

    #[test]
    fn group_thousands_basic() {
        assert_eq!(group_thousands(0), "0");
        assert_eq!(group_thousands(999), "999");
        assert_eq!(group_thousands(1_000), "1,000");
        assert_eq!(group_thousands(12_345_678), "12,345,678");
    }

    // ─── format_atomic_night ────────────────────────────────────

    #[test]
    fn atomic_night_zero() {
        assert_eq!(format_atomic_night(0), "0 atomic");
    }

    #[test]
    fn atomic_night_sub_million() {
        assert_eq!(format_atomic_night(123_456), "123,456 atomic");
    }

    #[test]
    fn atomic_night_with_whole_part() {
        // 1.5 NIGHT = 1_500_000 atomic.
        assert_eq!(format_atomic_night(1_500_000), "1.500000 NIGHT");
        // 12.34 NIGHT, fractional padded to 6.
        assert_eq!(format_atomic_night(12_340_000), "12.340000 NIGHT");
    }

    // ─── format_atomic_dust ─────────────────────────────────────

    #[test]
    fn atomic_dust_zero_and_large() {
        assert_eq!(format_atomic_dust(0), "0 atomic");
        assert_eq!(
            format_atomic_dust(250_000_000_000_000u128),
            "250,000,000,000,000 atomic",
        );
    }

    // ─── format_int ─────────────────────────────────────────────

    #[test]
    fn int_uses_space_grouping() {
        assert_eq!(format_int(1_234_567), "1 234 567");
        assert_eq!(format_int(0), "0");
        // Negative: minus sign passes through; spacing is over the
        // digit sequence so the minus sits flush with the first
        // group ("-12 345").
        assert_eq!(format_int(-12_345), "-12 345");
    }

    // ─── format_log_timestamp ──────────────────────────────────

    #[test]
    fn log_timestamp_zero() {
        // Unix epoch — midnight UTC.
        assert_eq!(format_log_timestamp(0), "00:00:00.000");
    }

    #[test]
    fn log_timestamp_handles_millis_under_1k() {
        // 12:34:56.789 UTC on the very first day of the epoch.
        let one_day = 86_400i64 * 1_000;
        let t = 0 + (12 * 3600 + 34 * 60 + 56) * 1000 + 789;
        assert_eq!(format_log_timestamp(t), "12:34:56.789");
        // Same time of day on the next day — output identical.
        assert_eq!(format_log_timestamp(t + one_day), "12:34:56.789");
    }

    // ─── short_keyref ──────────────────────────────────────────

    #[test]
    fn keyref_short_passes_through() {
        assert_eq!(short_keyref("abc"), "abc");
        assert_eq!(short_keyref("0123456789ab"), "0123456789ab");
    }

    #[test]
    fn keyref_long_gets_ellipsis() {
        let k = "06323caf-c779-4827-a0d4-8636a8ab6bca";
        assert_eq!(short_keyref(k), "06323caf…6bca");
    }
}
