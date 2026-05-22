//! Headless bench runner — same code path as the dioxus-wallet
//! Benchmark tab (Path B / web wasm), just driven from a CLI so
//! you can capture per-k timings + RSS without an interactive UI.
//!
//! Use:
//!     cargo run --release --bin bench_cli -- --max-k=14
//!     cargo run --release --bin bench_cli -- --max-k=12 --repeat=3
//!
//! Flags:
//!     --max-k=<N>      stop after k=N (default 14, the embedded-verifier ceiling)
//!     --min-k=<N>      start at k=N (default 1)
//!     --repeat=<N>     prove each k N times back-to-back; second+ runs
//!                      benefit from IR_CACHE + KEY_CACHE warm-up (default 1)
//!     --no-cache-keys  disable the process-wide KEY_CACHE — every prove runs
//!                      keygen from scratch. Use with --repeat to time
//!                      the cold-path floor.
//!     --json           emit one JSON line per row instead of a table
//!     --skip-verify    pass through to RunOpts (default: verify_after = true for k ≤ 14)
//!     --build-mmap     for each k in --min-k..=--max-k, build the
//!                      `bls_midnight_2pN.mmap` companion file from the
//!                      original SRS and exit. Skips proving entirely.
//!                      Use on a roomy desktop, then push the resulting
//!                      `.mmap` files to the device's params cache dir
//!                      next to the originals — the mmap path activates
//!                      automatically when the companion is found.
//!
//! Reads SRS files from `$MIDNIGHT_PP` / `$XDG_CACHE_HOME` /
//! `$HOME/.cache/midnight/zk-params/` — matches the wallet's
//! resolution order. First call at a given k stalls on
//! srs.midnight.network fetch; subsequent calls hit the cache.
//!
//! The bin compiles only on native targets — `run_proof_with_opts`
//! constructs a filesystem-backed `MidnightDataProvider`, which is
//! `cfg(not(target_arch = "wasm32"))` in the lib. A wasm stub `main`
//! is provided so `cargo check --target wasm32-unknown-unknown`
//! (which compiles every `[[bin]]` regardless) succeeds for the
//! workspace check.

#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;

#[cfg(not(target_arch = "wasm32"))]
use contract_benchmark::{MAX_K, MIN_K, RunOpts, run_proof_with_opts};

#[cfg(target_arch = "wasm32")]
fn main() {
    // bench_cli is native-only; wasm targets use the
    // `contract-benchmark-wasm` crate's wasm-bindgen wrapper.
}

// Mimalloc as global allocator on native targets. Returns freed
// pages to the OS more aggressively than macOS libmalloc; on
// Linux/Android it tracks libc-malloc closely. Drop-in test for
// whether allocator pooling is masking real heap drops between
// phases. wasm target keeps the default allocator.
#[cfg(not(target_arch = "wasm32"))]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[cfg(not(target_arch = "wasm32"))]

fn main() {
    // Wire a `tracing-subscriber::fmt` layer so the
    // `midnight_bench` per-phase events emitted by the patched
    // midnight-proofs + contract-benchmark print to stderr.
    // The dioxus-wallet has its own subscribers; here we want
    // the events visible in shell output for the autonomous
    // profile-and-optimise loop. `BENCH_LOG` env var lets the
    // caller override (default: only `midnight_bench=info`).
    use tracing_subscriber::EnvFilter;
    let filter = std::env::var("BENCH_LOG")
        .unwrap_or_else(|_| "midnight_bench=info".to_string());
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new(filter))
        .with_target(false)
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .compact()
        .try_init();

    let args: Vec<String> = std::env::args().collect();
    let mut min_k = MIN_K;
    let mut max_k = 14u32; // default to verifiable range so verify counts something
    let mut emit_json = false;
    let mut skip_verify = false;
    let mut repeat: u32 = 1;
    let mut cache_keys = true;
    let mut build_mmap = false;
    for a in &args[1..] {
        if let Some(v) = a.strip_prefix("--min-k=") {
            min_k = v.parse().expect("--min-k expects an integer");
        } else if let Some(v) = a.strip_prefix("--max-k=") {
            max_k = v.parse().expect("--max-k expects an integer");
        } else if let Some(v) = a.strip_prefix("--repeat=") {
            repeat = v.parse().expect("--repeat expects an integer");
            if repeat == 0 {
                eprintln!("--repeat must be ≥ 1");
                std::process::exit(2);
            }
        } else if a == "--no-cache-keys" {
            cache_keys = false;
        } else if a == "--build-mmap" {
            build_mmap = true;
        } else if a == "--json" {
            emit_json = true;
        } else if a == "--skip-verify" {
            skip_verify = true;
        } else if a == "--help" || a == "-h" {
            eprintln!(
                "usage: bench_cli [--min-k=N] [--max-k=N] [--repeat=N] [--no-cache-keys] [--json] [--skip-verify]"
            );
            std::process::exit(0);
        } else {
            eprintln!("unknown arg: {a}\nuse --help for options");
            std::process::exit(2);
        }
    }
    if !(MIN_K..=MAX_K).contains(&max_k) || !(MIN_K..=MAX_K).contains(&min_k) {
        eprintln!("k out of range: must be in {MIN_K}..={MAX_K}");
        std::process::exit(2);
    }

    if !emit_json {
        // Header row when we're rendering a table.
        let iter_col = if repeat > 1 { "iter" } else { " " };
        println!(
            "{:>3}  {:>4}  {:>8}  {:>9}  {:>9}  {:>9}  {:>10}  {:>10}  {:>10}",
            "k", iter_col, "hashes", "keygen", "prove", "verify", "proof_b", "rss_mb",
            "peak_mb",
        );
    }

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");

    // --build-mmap: precompute `.mmap` companion files for each k
    // in the range, then exit without proving. We set
    // `MIDNIGHT_MMAP_BUILD=1` so the very first `get_params(k)`
    // call inside `run_proof_with_opts` writes the companion as a
    // side effect of the eager load. A skip-verify prove with
    // chain_len=0 keeps the actual work negligible — the per-k
    // wall time is dominated by the file read + serialise.
    if build_mmap {
        // SAFETY: single-threaded, set before any other code touches env.
        unsafe {
            std::env::set_var("MIDNIGHT_MMAP_BUILD", "1");
        }
        for k in min_k..=max_k {
            let started = Instant::now();
            let result = rt.block_on(run_proof_with_opts(
                k,
                RunOpts {
                    verify_after: false,
                    cache_keys: false,
                    ..RunOpts::default()
                },
            ));
            let elapsed = started.elapsed().as_secs_f32();
            match result {
                Ok(_) => println!("k={k:>2}  built  {:.1}s", elapsed),
                Err(e) => eprintln!("k={k:>2}  ERROR: {e}"),
            }
        }
        return;
    }

    for k in min_k..=max_k {
      for iter in 0..repeat {
        let opts = RunOpts {
            verify_after: !skip_verify,
            cache_keys,
            ..RunOpts::default()
        };
        let wall_start = Instant::now();
        let result = rt.block_on(run_proof_with_opts(k, opts));
        let wall_ms = wall_start.elapsed().as_millis() as u64;
        match result {
            Ok(s) => {
                let rss_mb = proc_rss_mb().unwrap_or(0);
                let peak_mb = proc_peak_rss_mb().unwrap_or(0);
                if emit_json {
                    println!(
                        "{{\"k\":{},\"iter\":{},\"hashes\":{},\"keygen_ms\":{},\"prove_ms\":{},\"verify_ms\":{},\"verified\":{},\"proof_bytes\":{},\"wall_ms\":{},\"rss_mb\":{},\"peak_mb\":{}}}",
                        k,
                        iter,
                        s.hash_chain_len,
                        s.keygen.as_millis(),
                        s.prove.as_millis(),
                        s.verify.map(|d| d.as_millis() as i64).unwrap_or(-1),
                        s.verified.map(|b| b.to_string()).unwrap_or_else(|| "null".into()),
                        s.proof_bytes,
                        wall_ms,
                        rss_mb,
                        peak_mb,
                    );
                } else {
                    println!(
                        "{:>3}  {:>4}  {:>8}  {:>7}ms  {:>7}ms  {:>7}  {:>10}  {:>9}  {:>9}",
                        k,
                        iter,
                        s.hash_chain_len,
                        s.keygen.as_millis(),
                        s.prove.as_millis(),
                        s.verify
                            .map(|d| format!("{}ms", d.as_millis()))
                            .unwrap_or_else(|| "—".into()),
                        s.proof_bytes,
                        format!("{rss_mb} MiB"),
                        format!("{peak_mb} MiB"),
                    );
                }
            }
            Err(e) => {
                if emit_json {
                    println!(
                        "{{\"k\":{},\"iter\":{},\"error\":\"{}\",\"wall_ms\":{}}}",
                        k,
                        iter,
                        e.to_string().replace('"', "\\\""),
                        wall_ms,
                    );
                } else {
                    println!("{:>3}  {:>4}  ERROR  {}", k, iter, e);
                }
            }
        }
      }
    }
}

/// Read RSS in MiB from `/proc/self/status` (Linux/Android) or
/// `mach_task_basic_info` (macOS). On platforms we can't probe,
/// returns `None` — caller prints 0.
#[cfg(any(target_os = "linux", target_os = "android"))]
fn proc_rss_mb() -> Option<u64> {
    let s = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = s.lines().find(|l| l.starts_with("VmRSS:"))?;
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb / 1024)
}

/// macOS RSS via `ps -o rss=` on our own pid. Slow-ish but no
/// libc bindings required. Good enough for a benchmark loop that
/// runs once per k.
#[cfg(target_os = "macos")]
fn proc_rss_mb() -> Option<u64> {
    let pid = std::process::id();
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    let s = String::from_utf8(out.stdout).ok()?;
    let kb: u64 = s.trim().parse().ok()?;
    Some(kb / 1024)
}

#[cfg(not(any(target_os = "linux", target_os = "android", target_os = "macos")))]
#[allow(dead_code)] // wasm stub main never calls this
fn proc_rss_mb() -> Option<u64> {
    None
}

/// Peak RSS over the process lifetime in MiB. Uses
/// `getrusage(RUSAGE_SELF).ru_maxrss` on Unix — the high-water
/// mark of resident pages.
///
/// On Linux/Android `ru_maxrss` is **kilobytes**. On macOS / iOS
/// it's **bytes** (deviation from POSIX; documented behaviour).
/// We normalise to MiB.
///
/// Why this matters: `proc_rss_mb()` is sampled *after* each
/// prove returns, by which point the heap has typically settled
/// to its steady state. The transient parse-window spike that
/// `MIDNIGHT_LAZY_PARAMS=1` is designed to reduce
/// (`g_lagrange` briefly resident alongside `g`) is invisible
/// to that sample. `proc_peak_rss_mb()` captures the high-water
/// mark across the whole run, so the optimisation's effect is
/// observable as a smaller terminal-row peak.
#[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
fn proc_peak_rss_mb() -> Option<u64> {
    // SAFETY: getrusage with RUSAGE_SELF + zero-init struct is
    // defined POSIX behaviour; we ignore the return value's
    // semantic checks and only read ru_maxrss on success.
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    let rc = unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) };
    if rc != 0 {
        return None;
    }
    // ru_maxrss is bytes on macOS, kilobytes on Linux.
    #[cfg(target_os = "macos")]
    let bytes = usage.ru_maxrss as u64;
    #[cfg(any(target_os = "linux", target_os = "android"))]
    let bytes = (usage.ru_maxrss as u64) * 1024;
    Some(bytes / (1024 * 1024))
}

#[cfg(not(any(target_os = "linux", target_os = "android", target_os = "macos")))]
#[allow(dead_code)]
fn proc_peak_rss_mb() -> Option<u64> {
    None
}
