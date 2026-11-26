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

//! Runs the v9→v10 state translation against mainnet's ledger state, read from
//! a Midnight node's database.

pub mod canonicity;
pub mod chain;

use anyhow::{Context, Result, anyhow};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;
use storage_core::DefaultHasher;
use storage_core::arena::{ArenaHash, ArenaKey, Sp, TypedArenaKey};
use storage_core::db::paritydb::{NUM_COLUMNS, OwnedDb, set_init_options};
use storage_core::db::{DB, ParityDb, Update};
use storage_core::storage::{Storage, default_storage, set_default_storage};

pub type Db = ParityDb;
pub type LedgerV9 = ledger_v9::structure::LedgerState<Db>;
pub type StateKey = TypedArenaKey<LedgerV9, DefaultHasher>;

/// Decodes the `Midnight::StateKey` storage value: a SCALE byte vector
/// holding the tagged root key.
pub fn parse_state_key(value: &[u8]) -> Result<StateKey> {
    let mut body = value;
    let len = chain::compact(&mut body)?;
    anyhow::ensure!(
        body.len() as u64 == len,
        "state key length prefix {len}, body {}",
        body.len()
    );
    serialize::tagged_deserialize(body).context("decoding the state key")
}

pub fn root_hash(key: &StateKey) -> Result<ArenaHash<DefaultHasher>> {
    match &key.key {
        ArenaKey::Ref(hash) => Ok(hash.clone()),
        ArenaKey::Direct(_) => Err(anyhow!("state root is inlined, not stored")),
    }
}

/// Copies the nodes reachable from `root` out of the node's ledger database
/// into a fresh database at `dest`, and marks `root` as its GC root. The node's
/// database is opened read-only, which still needs the node stopped.
///
/// A copy whose root isn't marked is incomplete and is redone from scratch:
/// parents are written before their subtrees.
pub fn extract_state(node_db: &Path, root: &ArenaHash<DefaultHasher>, dest: &Path) -> Result<()> {
    if dest.exists() {
        if Db::open(dest).get_root_count(root) > 0 {
            log::info!("{} already holds the state", dest.display());
            return Ok(());
        }
        log::warn!("{} holds an incomplete copy; starting over", dest.display());
        std::fs::remove_dir_all(dest)?;
    }

    let mut options = parity_db::Options::with_columns(node_db, NUM_COLUMNS);
    set_init_options(&mut options, 0, false);
    let src_db = parity_db::Db::open_read_only(&options)
        .map_err(|e| anyhow!("opening {} read-only: {e}", node_db.display()))?;
    let src = Db::from_existing_db(OwnedDb(Arc::new(src_db)));
    let mut dst = Db::open(dest);

    let started = Instant::now();
    let mut nodes = 0u64;
    let mut batch = HashMap::new();
    let mut stack = vec![root.clone()];
    while let Some(key) = stack.pop() {
        if batch.contains_key(&key) || dst.get_node(&key).is_some() {
            continue;
        }
        let obj = src
            .get_node(&key)
            .with_context(|| format!("node {key:?} missing from the node database"))?;
        stack.extend(obj.children.iter().flat_map(ArenaKey::refs).cloned());
        nodes += 1;
        batch.insert(key, Update::InsertNode(obj));
        if batch.len() >= 100_000 {
            dst.batch_update(batch.drain());
            log::info!(
                "copied {nodes} nodes in {:.0}s; {} pending",
                started.elapsed().as_secs_f64(),
                stack.len(),
            );
        }
    }
    dst.batch_update(batch.drain());
    dst.batch_update(std::iter::once((root.clone(), Update::SetRootCount(1))));
    log::info!(
        "copied the state: {nodes} nodes in {:.0}s",
        started.elapsed().as_secs_f64(),
    );
    Ok(())
}

/// Makes the copy at `db` the default storage and loads the state lazily.
pub fn open_state(db: &Path, key: &StateKey, cache_nodes: usize) -> Result<Sp<LedgerV9, Db>> {
    set_default_storage(|| Storage::new(cache_nodes, Db::open(db)))
        .map_err(|_| anyhow!("default storage already set"))?;
    Ok(default_storage::<Db>().get_lazy(key)?)
}

pub fn rss_mib() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find_map(|l| l.strip_prefix("VmRSS:"))
                .and_then(|v| v.trim().trim_end_matches(" kB").parse::<u64>().ok())
        })
        .map_or(0, |kb| kb >> 10)
}
