//! Sync to the Midnight mainnet tip via the toolkit, then run test fixtures
//! against the resulting in-memory ledger context.

pub mod progress;

use anyhow::Result;
use async_trait::async_trait;
use midnight_node_ledger_helpers::{DefaultDB, WalletSeed};
use midnight_node_ledger_helpers::ledger_8::context::LedgerContext;
use midnight_node_toolkit::tx_generator::{
    builder::build_fork_aware_context_cached,
    source::{FetchCacheConfig, GetTxs, GetTxsFromUrl, create_file_wallet_cache},
};
use std::path::PathBuf;

use crate::progress::Phases;

/// Deterministic dummy seed. The toolkit's `build_fork_aware_context_cached`
/// only writes ledger snapshots when at least one wallet seed is provided
/// (mod.rs:936-944 short-circuits to the non-caching path otherwise). We pass a
/// fixed throwaway seed so subsequent runs can warm-start from the saved
/// snapshot instead of replaying from genesis every time.
const CACHE_SEED: [u8; 32] = [0x01; 32];

#[derive(Clone, Debug)]
pub struct SyncConfig {
    /// e.g. `wss://rpc.mainnet.midnight.network` — confirm with node team.
    pub rpc_url: String,
    /// Persistent block-data cache (redb / postgres / in-memory).
    pub fetch_cache: FetchCacheConfig,
    /// Directory for the file-based ledger-snapshot cache.
    pub ledger_state_db: String,
    pub fetch_concurrency: usize,
    pub compute_concurrency: usize,
}

pub struct SyncedTip {
    pub block_height: u64,
    pub ctx: LedgerContext<DefaultDB>,
}

pub async fn sync_to_tip(cfg: &SyncConfig, phases: &Phases) -> Result<SyncedTip> {
    let cache_path = match &cfg.fetch_cache {
        FetchCacheConfig::Redb { filename } => Some(PathBuf::from(filename)),
        _ => None,
    };
    let fetch = phases.fetch_progress(cache_path);
    let blocks = GetTxsFromUrl::new(
        &cfg.rpc_url,
        cfg.fetch_concurrency,
        cfg.compute_concurrency,
        /* dust_warp        */ false,
        /* fetch_only_cache */ false,
        cfg.fetch_cache.clone(),
    )
    .get_txs()
    .await
    .map_err(|e| anyhow::anyhow!("fetch failed: {e}"))?;
    fetch.finish(format!("Fetched {} blocks", blocks.blocks.len()));

    let total_blocks = blocks.blocks.len() as u64;
    let replay = phases.replay_progress(total_blocks);
    let wallet_cache = create_file_wallet_cache(&cfg.ledger_state_db, &cfg.fetch_cache);
    let cache_seed = WalletSeed::from(CACHE_SEED);
    let fork_ctx = build_fork_aware_context_cached(
        std::slice::from_ref(&cache_seed),
        &blocks,
        wallet_cache.as_deref(),
    )
    .await;
    let ctx = fork_ctx
        .into_ledger8()
        .ok_or_else(|| anyhow::anyhow!("expected Ledger8 at tip"))?;
    let height = blocks.blocks.last().map(|b| b.number).unwrap_or(0);
    replay.finish(format!("Ledger ready at block {height}"));

    Ok(SyncedTip { block_height: height, ctx })
}

/// Implement this for each test fixture you want to run against the tip.
/// `phases` gives the test access to the shared `MultiProgress` so it can
/// register its own bars/spinners and have them rendered alongside the outer
/// "Tests" bar.
#[async_trait]
pub trait LedgerTest: Send + Sync {
    fn name(&self) -> &str;
    async fn run(&self, tip: &SyncedTip, phases: &Phases) -> Result<()>;
}

pub async fn run_tests(
    tip: &SyncedTip,
    tests: &[Box<dyn LedgerTest>],
    phases: &Phases,
) -> Vec<(String, Result<()>)> {
    let bar = phases.bar(tests.len() as u64, "Tests");
    let mut out = Vec::with_capacity(tests.len());
    for t in tests {
        let name = t.name().to_string();
        bar.set_current(&name);
        let res = t.run(tip, phases).await;
        bar.tick();
        out.push((name, res));
    }
    let passes = out.iter().filter(|(_, r)| r.is_ok()).count();
    bar.finish(format!("{passes}/{} passed", out.len()));
    out
}
