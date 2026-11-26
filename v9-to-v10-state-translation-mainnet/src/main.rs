// This file is part of midnight-ledger.
// Copyright (C) 2026 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");
// You may not use this file except in compliance with the License.
// You may obtain a copy of the License at
// http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;
use std::time::Duration;
use v9_to_v10_state_translation_mainnet::chain::{STATE_KEY_STORAGE_KEY, read_finalized};
use v9_to_v10_state_translation_mainnet::{
    extract_state, open_state, parse_state_key, root_hash, rss_mib,
};

mod translation;

#[derive(Parser)]
#[command(about = "Translate mainnet's ledger state from v9 to v10, read from a node's database")]
struct Cli {
    /// A stopped mainnet node's data directory, e.g. an unpacked snapshot's
    /// `node/`. Only read, but parity-db locks it even for readers.
    #[arg(long, default_value = "./mainnet_cache/node-snapshot/node")]
    node_dir: PathBuf,
    /// Where to copy the state out of the node's database. Reused across runs.
    #[arg(long, default_value = "./mainnet_cache/v9_state_db")]
    state_db: PathBuf,
    /// Storage nodes cached in memory in front of the state database.
    #[arg(long, default_value_t = 1_000_000)]
    cache_nodes: usize,
    /// Nodes the canonicity walk decodes between GC runs.
    #[arg(long, default_value_t = 10_000)]
    check_window: usize,
    /// Time budget per GC run, in milliseconds.
    #[arg(long, default_value_t = 200)]
    gc_budget_ms: u64,
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let cli = Cli::parse();

    let finalized = read_finalized(
        &cli.node_dir.join("chains/midnight/paritydb/full"),
        &STATE_KEY_STORAGE_KEY,
    )?;
    let key = parse_state_key(&finalized.value)?;
    let root = root_hash(&key)?;
    log::info!(
        "block {} ({}): state root {}",
        finalized.number,
        hex::encode(finalized.hash),
        hex::encode(root.0)
    );
    extract_state(&cli.node_dir.join("ledger_storage"), &root, &cli.state_db)?;
    let v9 = open_state(&cli.state_db, &key, cli.cache_nodes)?;
    log::info!("opened the state; rss {} MiB", rss_mib());

    translation::run(
        v9,
        cli.check_window,
        Duration::from_millis(cli.gc_budget_ms),
    )?;
    log::info!("PASS");
    Ok(())
}
