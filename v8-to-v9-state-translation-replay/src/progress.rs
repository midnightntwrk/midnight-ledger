use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle};
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::task::JoinHandle;

/// `(blocks_replayed, blocks_total)` parsed from `replay_blocks_8` debug logs.
type ReplayState = Option<(u64, u64)>;

#[derive(Clone)]
pub struct Phases {
    multi: MultiProgress,
    /// Verified-batch progress (0..100%) from `fetch progress: X.X%` info lines.
    /// Lags the fetched count — this is the most-downstream signal.
    fetch_pct: Arc<Mutex<Option<f64>>>,
    /// Count of batches whose RPC fetch has completed (one increment per
    /// `worker N: completed job.` debug line). Leads `fetch_pct`.
    batches_fetched: Arc<AtomicU64>,
    /// Total batch count parsed from `spawning … N jobs`. 0 until the toolkit logs it.
    batches_total: Arc<AtomicU64>,
    /// Latest replay progress (blocks_done, blocks_total) from `[perf] replay_blocks_8`.
    replay: Arc<Mutex<ReplayState>>,
}

impl Phases {
    pub fn install() -> Self {
        let multi = MultiProgress::with_draw_target(ProgressDrawTarget::stderr_with_hz(10));
        let fetch_pct = Arc::new(Mutex::new(None));
        let batches_fetched = Arc::new(AtomicU64::new(0));
        let batches_total = Arc::new(AtomicU64::new(0));
        let replay = Arc::new(Mutex::new(None));

        // env_logger filters info+ globally. We raise the toolkit's fetcher
        // and builder modules to debug to capture per-batch progress lines,
        // and lower the chatty ledger family to warn — those info logs add
        // no signal during a long replay and the stderr volume competes with
        // indicatif for the terminal (so suppressing them is also a small
        // throughput win on top of the visual cleanup).
        let inner = env_logger::Builder::from_env(
            env_logger::Env::default().default_filter_or("info"),
        )
        .filter_module("midnight_node_toolkit::fetcher", log::LevelFilter::Debug)
        .filter_module(
            "midnight_node_toolkit::tx_generator::builder",
            log::LevelFilter::Debug,
        )
        .filter_module("midnight_ledger", log::LevelFilter::Warn)
        .filter_module("midnight_onchain_runtime", log::LevelFilter::Warn)
        .filter_module("midnight_onchain_state", log::LevelFilter::Warn)
        .filter_module("midnight_onchain_vm", log::LevelFilter::Warn)
        .filter_module("midnight_zswap", log::LevelFilter::Warn)
        .filter_module("midnight_transient_crypto", log::LevelFilter::Warn)
        .filter_module("midnight_storage_core", log::LevelFilter::Warn)
        .filter_module("midnight_serialize", log::LevelFilter::Warn)
        .filter_module("midnight_coin_structure", log::LevelFilter::Warn)
        .filter_module("midnight_base_crypto", log::LevelFilter::Warn)
        // The `tracing-log` bridge emits a record per span entry/exit with
        // target "tracing::span" — the ledger has a `try_apply` span per
        // transaction, which is millions of records during replay.
        .filter_module("tracing", log::LevelFilter::Warn)
        .build();

        let wrapped = ProgressLogger {
            inner,
            fetch_pct: fetch_pct.clone(),
            batches_fetched: batches_fetched.clone(),
            batches_total: batches_total.clone(),
            replay: replay.clone(),
        };
        indicatif_log_bridge::LogWrapper::new(multi.clone(), wrapped)
            .try_init()
            .expect("logger already installed");
        // Let the framework deliver debug records to us; the inner logger
        // re-filters them per-module.
        log::set_max_level(log::LevelFilter::Debug);

        Self { multi, fetch_pct, batches_fetched, batches_total, replay }
    }

    /// Phase 1/2 bar: blocks fetched-and-verified by the toolkit pipeline. The
    /// bar advances per 100-block batch (the toolkit's worker granularity).
    /// Verified % and cache size live in the 30 s heartbeat log only — kept
    /// off the bar to avoid noise.
    pub fn fetch_progress(&self, cache_path: Option<PathBuf>) -> FetchProgress {
        // Bar length is replaced once we parse the toolkit's spawn line.
        let bar = self.multi.add(ProgressBar::new(1));
        bar.set_style(
            ProgressStyle::with_template(
                "Phase 1/2 fetch  {elapsed_precise} [{bar:30.cyan/blue}] {pos}/{len} ({percent:>3}%) ETA {eta:>8} ({per_sec})",
            )
            .unwrap()
            .progress_chars("█▉▊▋▌▍▎▏ "),
        );
        bar.enable_steady_tick(Duration::from_millis(250));

        let bar_clone = bar.clone();
        let fetch_pct = self.fetch_pct.clone();
        let batches_fetched = self.batches_fetched.clone();
        let batches_total = self.batches_total.clone();
        let start = Instant::now();
        let path = cache_path.clone();

        let handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(2));
            let mut window_start = Instant::now();
            let mut window_start_fetched: u64 = 0;
            let mut last_logged = Instant::now();
            let mut last_total: u64 = 0;
            loop {
                interval.tick().await;

                let total = batches_total.load(Ordering::Relaxed);
                if total != last_total && total > 0 {
                    bar_clone.set_length(total);
                    last_total = total;
                }

                let fetched = batches_fetched.load(Ordering::Relaxed);
                bar_clone.set_position(fetched);

                if last_logged.elapsed() >= Duration::from_secs(30) {
                    let total_elapsed = start.elapsed().as_secs_f64().max(0.001);
                    let avg_bps = fetched as f64 / total_elapsed;
                    let window_elapsed = window_start.elapsed().as_secs_f64().max(0.001);
                    let window_bps =
                        (fetched.saturating_sub(window_start_fetched)) as f64 / window_elapsed;
                    let size_mb = path
                        .as_ref()
                        .and_then(|p| std::fs::metadata(p).ok())
                        .map(|m| m.len() as f64 / (1024.0 * 1024.0))
                        .unwrap_or(0.0);
                    let verified_pct = *fetch_pct.lock().unwrap();
                    let fetched_pct = if total > 0 {
                        (fetched as f64 / total as f64) * 100.0
                    } else {
                        0.0
                    };
                    let total_str = if total > 0 { total.to_string() } else { "?".into() };
                    let verified_str = verified_pct
                        .map(|p| format!("{p:.2}%"))
                        .unwrap_or_else(|| "—".into());

                    log::info!(
                        "fetch heartbeat: elapsed {:.0}s, fetched {fetched}/{total_str} \
                         ({fetched_pct:.2}%), verified {verified_str}, cache {size_mb:.1} MB \
                         ({avg_bps:.2} batch/s avg, {window_bps:.2} batch/s last 30s)",
                        total_elapsed
                    );
                    window_start = Instant::now();
                    window_start_fetched = fetched;
                    last_logged = Instant::now();
                }
            }
        });

        FetchProgress { bar, heartbeat: Some(handle) }
    }

    /// Phase 2/2 bar: ledger replay, fed by `[perf] replay_blocks_8 progress: X/Y`
    /// debug lines. Granularity is 1000 blocks (the toolkit's `DUST_BATCH_SIZE`).
    pub fn replay_progress(&self, total_blocks: u64) -> ReplayProgress {
        let bar = self.multi.add(ProgressBar::new(total_blocks));
        bar.set_style(
            ProgressStyle::with_template(
                "Phase 2/2 replay {elapsed_precise} [{bar:30.magenta/blue}] {pos}/{len} ({percent:>3}%) ETA {eta:>8} ({per_sec})",
            )
            .unwrap()
            .progress_chars("█▉▊▋▌▍▎▏ "),
        );
        bar.enable_steady_tick(Duration::from_millis(250));

        let bar_clone = bar.clone();
        let replay = self.replay.clone();
        let start = Instant::now();

        let handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(2));
            let mut last_logged = Instant::now();
            let mut window_start = Instant::now();
            let mut window_start_done: u64 = 0;
            loop {
                interval.tick().await;
                let snapshot = *replay.lock().unwrap();
                let (done, total) = match snapshot {
                    Some(s) => s,
                    None => (0, total_blocks),
                };
                bar_clone.set_position(done);

                if last_logged.elapsed() >= Duration::from_secs(30) {
                    let total_elapsed = start.elapsed().as_secs_f64().max(0.001);
                    let avg_bps = done as f64 / total_elapsed;
                    let window_elapsed = window_start.elapsed().as_secs_f64().max(0.001);
                    let window_bps =
                        (done.saturating_sub(window_start_done)) as f64 / window_elapsed;
                    let pct = if total == 0 { 0.0 } else { done as f64 / total as f64 * 100.0 };
                    log::info!(
                        "replay heartbeat: elapsed {:.0}s, {done}/{total} blocks ({:.1}%, \
                         {avg_bps:.0} blk/s avg, {window_bps:.0} blk/s last 30s)",
                        total_elapsed,
                        pct
                    );
                    window_start = Instant::now();
                    window_start_done = done;
                    last_logged = Instant::now();
                }
            }
        });

        ReplayProgress { bar, heartbeat: Some(handle) }
    }

    pub fn spinner(&self, label: &'static str) -> Spinner {
        let pb = self.multi.add(ProgressBar::new_spinner());
        pb.set_style(
            ProgressStyle::with_template("{spinner} {elapsed:>8} {msg}")
                .unwrap()
                .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏", "✓"]),
        );
        pb.enable_steady_tick(Duration::from_millis(100));
        pb.set_message(label);
        Spinner { pb, label }
    }

    pub fn bar(&self, total: u64, label: &'static str) -> Bar {
        let pb = self.multi.add(ProgressBar::new(total));
        pb.set_style(
            ProgressStyle::with_template(
                "{elapsed:>8} [{bar:30.cyan/blue}] {pos}/{len} {msg}",
            )
            .unwrap()
            .progress_chars("█▉▊▋▌▍▎▏ "),
        );
        pb.set_message(label);
        Bar { pb }
    }
}

struct ProgressLogger {
    inner: env_logger::Logger,
    fetch_pct: Arc<Mutex<Option<f64>>>,
    batches_fetched: Arc<AtomicU64>,
    batches_total: Arc<AtomicU64>,
    replay: Arc<Mutex<ReplayState>>,
}

impl log::Log for ProgressLogger {
    fn enabled(&self, m: &log::Metadata) -> bool {
        self.inner.enabled(m)
    }
    fn log(&self, r: &log::Record) {
        let line = r.args().to_string();

        if let Some(total) = parse_total_batches(&line) {
            self.batches_total.store(total, Ordering::Relaxed);
            // The spawn line is useful at startup, keep forwarding it.
        }
        if let Some(p) = parse_fetch_pct(&line) {
            *self.fetch_pct.lock().unwrap() = Some(p);
            return; // absorb — the bar shows it
        }
        if parse_completed_job(&line) {
            self.batches_fetched.fetch_add(1, Ordering::Relaxed);
            return; // absorb — bar counts it
        }
        if let Some((done, total)) = parse_replay_progress(&line) {
            *self.replay.lock().unwrap() = Some((done, total));
            return; // absorb
        }

        // Suppress the fetcher and builder modules' chatty debug logs (job
        // pushes, work pickups, [perf] timings) — they'd flood the terminal
        // now that we've raised those modules to debug for progress capture.
        if r.level() == log::Level::Debug
            && (r.target().starts_with("midnight_node_toolkit::fetcher")
                || r.target().starts_with("midnight_node_toolkit::tx_generator::builder"))
        {
            return;
        }

        if self.inner.enabled(r.metadata()) {
            self.inner.log(r);
        }
    }
    fn flush(&self) {
        self.inner.flush()
    }
}

/// Parses lines of the form: "fetch progress: 12.3% of 1234567 blocks complete".
fn parse_fetch_pct(line: &str) -> Option<f64> {
    let after = line.split_once("fetch progress: ")?.1;
    let pct_str = after.split('%').next()?;
    pct_str.trim().parse().ok()
}

/// Parses lines of the form:
/// `[perf] replay_blocks_8 progress: 1000/985000 blocks`.
fn parse_replay_progress(line: &str) -> Option<(u64, u64)> {
    let after = line.split_once("replay_blocks_8 progress: ")?.1;
    let pair = after.split(' ').next()?;
    let (done, total) = pair.split_once('/')?;
    Some((done.parse().ok()?, total.parse().ok()?))
}

/// True when the line announces that a fetch worker finished a batch.
/// Matches: `worker 3: completed job.`
fn parse_completed_job(line: &str) -> bool {
    line.contains(": completed job.")
}

/// Parses the total batch count from:
/// `spawning 20 fetch workers (capped from requested, 9855 jobs)`
fn parse_total_batches(line: &str) -> Option<u64> {
    let after = line.split_once(", ")?.1;
    let num_str = after.split(' ').next()?;
    num_str.parse().ok()
}

pub struct FetchProgress {
    bar: ProgressBar,
    heartbeat: Option<JoinHandle<()>>,
}

impl FetchProgress {
    pub fn finish(mut self, msg: impl Into<String>) {
        if let Some(h) = self.heartbeat.take() {
            h.abort();
        }
        self.bar.finish_with_message(msg.into());
    }
}

impl Drop for FetchProgress {
    fn drop(&mut self) {
        if let Some(h) = self.heartbeat.take() {
            h.abort();
        }
    }
}

pub struct ReplayProgress {
    bar: ProgressBar,
    heartbeat: Option<JoinHandle<()>>,
}

impl ReplayProgress {
    pub fn finish(mut self, msg: impl Into<String>) {
        if let Some(h) = self.heartbeat.take() {
            h.abort();
        }
        self.bar.finish_with_message(msg.into());
    }
}

impl Drop for ReplayProgress {
    fn drop(&mut self) {
        if let Some(h) = self.heartbeat.take() {
            h.abort();
        }
    }
}

pub struct Spinner {
    pb: ProgressBar,
    label: &'static str,
}

impl Spinner {
    pub fn set(&self, extra: impl Into<String>) {
        self.pb.set_message(format!("{} — {}", self.label, extra.into()));
    }
    pub fn finish(self, msg: impl Into<String>) {
        self.pb.finish_with_message(msg.into());
    }
}

pub struct Bar {
    pub pb: ProgressBar,
}

impl Bar {
    pub fn set_current(&self, name: &str) {
        self.pb.set_message(name.to_string());
    }
    pub fn tick(&self) {
        self.pb.inc(1);
    }
    pub fn finish(self, msg: impl Into<String>) {
        self.pb.finish_with_message(msg.into());
    }
}

#[cfg(test)]
mod tests {
    use super::{
        parse_completed_job, parse_fetch_pct, parse_replay_progress, parse_total_batches,
    };

    #[test]
    fn parses_toolkit_fetch_progress_line() {
        assert_eq!(
            parse_fetch_pct("fetch progress: 12.3% of 1234567 blocks complete"),
            Some(12.3)
        );
        assert_eq!(
            parse_fetch_pct(
                "[2026-05-26T17:12:16Z INFO  midnight_node_toolkit::fetcher] fetch progress: 0.1% of 9852 blocks complete"
            ),
            Some(0.1)
        );
        assert_eq!(parse_fetch_pct("nothing to see here"), None);
    }

    #[test]
    fn parses_replay_progress_line() {
        assert_eq!(
            parse_replay_progress("[perf] replay_blocks_8 progress: 1000/985000 blocks"),
            Some((1000, 985_000))
        );
        assert_eq!(parse_replay_progress("unrelated"), None);
    }

    #[test]
    fn parses_completed_job_line() {
        assert!(parse_completed_job("worker 3: completed job."));
        assert!(parse_completed_job(
            "[2026-05-26T18:00:00Z DEBUG midnight_node_toolkit::fetcher] worker 17: completed job."
        ));
        assert!(!parse_completed_job("worker 3: received new job..."));
    }

    #[test]
    fn parses_total_batches_line() {
        assert_eq!(
            parse_total_batches("spawning 20 fetch workers (capped from requested, 9855 jobs)"),
            Some(9855)
        );
        assert_eq!(parse_total_batches("no comma here"), None);
    }
}
