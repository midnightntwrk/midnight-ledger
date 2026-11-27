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

// Runs the v9→v10 state translation incrementally, with a 500 ms budget per
// increment, then validates the result: every contract keeps its state, its
// operations and its `rc_0` entries, and the whole translated state is
// canonical.

use anyhow::Result;
use base_crypto::cost_model::CostDuration;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use storage_v10::arena::Sp;
use storage_v10::delta_tracking::ChildRef;
use storage_v10::merkle_patricia_trie::MerklePatriciaTrie;
use storage_v10::state_translation::TypedTranslationState;
use storage_v10::storage::{Map, default_storage};
use v9_to_v10_state_translation::StateTranslationTable;
use v9_to_v10_state_translation_mainnet::{Db, LedgerV9, rss_mib};

type LedgerV10 = ledger_v10::structure::LedgerState<Db>;
type Rc0V10 = MerklePatriciaTrie<ChildRef<Db>, Db>;

const PER_RUN_BUDGET: Duration = Duration::from_millis(500);

/// `check_window` nodes of the canonicity walk are decoded between GC runs of
/// `gc_budget` each.
pub fn run(v9: Sp<LedgerV9, Db>, check_window: usize, gc_budget: Duration) -> Result<()> {
    // What each contract must come out with: its state hash, its operations,
    // and its `rc_0` hash. An `rc_0` that is canonical read as a v10 trie must
    // keep its hash; one holding a short extension chain must hash as a fresh
    // ledger 10 map of the same entries.
    let mut v9_contracts = HashMap::new();
    let mut short_chained = 0usize;
    for (addr, contract) in v9.contract.iter() {
        let v9_rc_0 = &contract.data.charged_keys.rc_0;
        let as_v10: Sp<Rc0V10, Db> =
            default_storage::<Db>().get_lazy(&v9_rc_0.mpt.as_child().into())?;
        let rc_0 = if as_v10.canonicity().is_ok() {
            v9_rc_0.mpt.hash()
        } else {
            short_chained += 1;
            let rebuilt = v9_rc_0.iter().fold(Map::<_, _, Db>::new(), |m, (k, v)| {
                m.insert(k, ChildRef::new(v.child.clone()))
            });
            rebuilt.mpt.hash()
        };
        let mut operations = Vec::new();
        for entry in contract.operations.iter() {
            let (key, op) = &*entry;
            let mut vks = Vec::new();
            serialize::Serializable::serialize(&op.v2, &mut vks)?;
            serialize::Serializable::serialize(&op.v3, &mut vks)?;
            operations.push((key[..].to_vec(), vks, op.ir.as_ref().map(|ir| ir.hash())));
        }
        operations.sort_by(|a, b| a.0.cmp(&b.0));
        v9_contracts.insert(addr.0, (contract.data.state.hash(), rc_0, operations));
    }
    let v9_bridge = v9.bridge_receiving.size();
    let v9_unclaimed = v9.unclaimed_block_rewards.size();
    log::info!(
        "v9 source state: network_id={:?}, contracts={} ({short_chained} with short-chained rc_0), bridge_receiving={v9_bridge}, unclaimed_block_rewards={v9_unclaimed}; rss {} MiB",
        v9.network_id,
        v9_contracts.len(),
        rss_mib(),
    );

    let mut state =
        TypedTranslationState::<LedgerV9, LedgerV10, StateTranslationTable, Db>::start(v9)
            .map_err(|e| anyhow::anyhow!("start translation: {e}"))?;

    let started = Instant::now();
    let mut iter: u64 = 0;
    // `run`'s `CostDuration` is a heuristic with little relation to real
    // time, so each increment drives `run` in small chunks until the
    // wall-clock budget is spent.
    let inner_cost = CostDuration::from_picoseconds(50_000_000_000); // 50 ms of cost

    let v10 = loop {
        iter += 1;
        let step_start = Instant::now();
        let mut done = None;
        while step_start.elapsed() < PER_RUN_BUDGET {
            state = state
                .run(inner_cost)
                .map_err(|e| anyhow::anyhow!("translation step {iter}: {e}"))?;
            if let Some(result) = state
                .result()
                .map_err(|e| anyhow::anyhow!("result poll: {e}"))?
            {
                done = Some(result);
                break;
            }
        }
        if let Some(result) = done {
            break result;
        }
    };
    log::info!(
        "v9→v10 done: {iter} iterations in {:.1}s; rss {} MiB",
        started.elapsed().as_secs_f64(),
        rss_mib(),
    );

    anyhow::ensure!(
        v10.contract.size() == v9_contracts.len(),
        "contract count drift: v9={} v10={}",
        v9_contracts.len(),
        v10.contract.size(),
    );
    anyhow::ensure!(
        v10.bridge_receiving.size() == v9_bridge,
        "bridge_receiving count drift"
    );
    anyhow::ensure!(
        v10.unclaimed_block_rewards.size() == v9_unclaimed,
        "unclaimed_block_rewards count drift"
    );
    for (addr, contract) in v10.contract.iter() {
        let (state_hash, rc_0_hash, v9_operations) = v9_contracts
            .get(&addr.0)
            .ok_or_else(|| anyhow::anyhow!("contract {addr:?} appeared in translation"))?;
        anyhow::ensure!(
            contract.data.state.hash() == *state_hash,
            "contract {addr:?} changed its state"
        );
        let rc_0 = &contract.data.charged_keys.rc_0.mpt;
        rc_0.canonicity()
            .map_err(|e| anyhow::anyhow!("contract {addr:?} rc_0: {e}"))?;
        anyhow::ensure!(
            rc_0.hash() == *rc_0_hash,
            "contract {addr:?} changed its rc_0 entries"
        );
        let mut operations = Vec::new();
        for entry in contract.operations.iter() {
            let (key, op) = &*entry;
            anyhow::ensure!(op.v4.is_none(), "contract {addr:?} gained a v4 key");
            let mut vks = Vec::new();
            serialize::Serializable::serialize(&op.v2, &mut vks)?;
            serialize::Serializable::serialize(&op.v3, &mut vks)?;
            operations.push((key[..].to_vec(), vks, op.ir.as_ref().map(|ir| ir.hash())));
        }
        operations.sort_by(|a, b| a.0.cmp(&b.0));
        anyhow::ensure!(
            operations == *v9_operations,
            "contract {addr:?} changed its operations"
        );
    }

    // Final validation: every node of the translated state passes the checks
    // the wire decoder makes, which for tries means canonical form. Walked
    // from the stored state, so only a window of it is decoded at a time.
    let mut v10 = v10;
    v10.persist();
    let root = v10.as_child();
    let root_hash = root.hash().clone();
    drop((v10, state));
    let storage = default_storage::<Db>();
    storage.arena.with_backend(|b| b.flush_all_changes_to_db());
    let checked = v9_to_v10_state_translation_mainnet::canonicity::check_stored::<LedgerV10>(
        &storage.arena,
        root,
        check_window,
        gc_budget,
    )
    .map_err(|e| anyhow::anyhow!("translated state is not canonical: {e}"))?;
    log::info!(
        "translated state canonical: {checked} nodes; rss {} MiB",
        rss_mib()
    );
    // Leave the reused state database rooted at the v9 state only.
    storage.arena.with_backend(|b| {
        b.unpersist(&root_hash);
        b.flush_all_changes_to_db();
    });
    Ok(())
}
