use anyhow::Result;
use clap::Parser;
use midnight_node_toolkit::tx_generator::source::FetchCacheConfig;
use std::time::Duration;
use v8_to_v9_state_translation_replay::{
    LedgerTest, SyncConfig, SyncedTip, progress::Phases, run_tests, sync_to_tip,
};

mod tests;

#[derive(Parser)]
#[command(about = "Sync to the Midnight mainnet tip and run a set of ledger tests against it")]
struct Cli {
    #[arg(long, env = "MN_SRC_URL", default_value = "wss://rpc.mainnet.midnight.network")]
    rpc_url: String,
    #[arg(long, default_value = "./mainnet_cache/fetch_cache.db")]
    fetch_cache_file: String,
    #[arg(long, default_value = "./mainnet_cache/ledger_cache_db")]
    ledger_state_db: String,
    #[arg(long, default_value_t = 20)]
    fetch_concurrency: usize,
    /// Maximum sync attempts. The redb cache persists across attempts, so each
    /// restart picks up roughly where the previous one died.
    #[arg(long, default_value_t = 20)]
    max_sync_attempts: u32,
    /// Seconds to wait between sync restart attempts.
    #[arg(long, default_value_t = 5)]
    restart_backoff_secs: u64,
    /// Substring filter; only tests whose name contains this run.
    #[arg(long)]
    only: Option<String>,
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    let phases = Phases::install();
    let cli = Cli::parse();

    let cfg = SyncConfig {
        rpc_url: cli.rpc_url,
        fetch_cache: FetchCacheConfig::Redb { filename: cli.fetch_cache_file },
        ledger_state_db: cli.ledger_state_db,
        fetch_concurrency: cli.fetch_concurrency,
        compute_concurrency: std::thread::available_parallelism()?.get(),
    };

    let tip = sync_with_restart(&cfg, &phases, cli.max_sync_attempts, cli.restart_backoff_secs)
        .await?;
    log::info!("synced to block {}", tip.block_height);

    let all: Vec<Box<dyn LedgerTest>> = tests::all();
    let selected: Vec<_> = match &cli.only {
        Some(s) => all.into_iter().filter(|t| t.name().contains(s)).collect(),
        None => all,
    };

    let results = run_tests(&tip, &selected, &phases).await;

    let failed = results.iter().filter(|(_, r)| r.is_err()).count();
    for (name, res) in &results {
        match res {
            Ok(()) => log::info!("PASS {name}"),
            Err(e) => log::error!("FAIL {name}: {e:?}"),
        }
    }
    if failed > 0 {
        anyhow::bail!("{failed} test(s) failed");
    }
    Ok(())
}

/// Drive `sync_to_tip` with bounded restarts. The toolkit panics on transient
/// RPC drops (compute_task.rs:193 unwraps the client); we treat that as a
/// retryable error since the redb cache persists work done so far.
async fn sync_with_restart(
    cfg: &SyncConfig,
    phases: &Phases,
    max_attempts: u32,
    backoff_secs: u64,
) -> Result<SyncedTip> {
    let mut last_err: Option<anyhow::Error> = None;
    for attempt in 1..=max_attempts {
        if attempt > 1 {
            log::warn!(
                "sync attempt {attempt}/{max_attempts} after {backoff_secs}s backoff…"
            );
            tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
        }
        match sync_to_tip(cfg, phases).await {
            Ok(tip) => return Ok(tip),
            Err(e) => {
                log::error!("sync attempt {attempt} failed: {e:#}");
                last_err = Some(e);
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("sync failed without an error")))
        .map_err(|e| e.context(format!("gave up after {max_attempts} sync attempts")))
}
