// Runs the v8→v9 state translation incrementally against the synced mainnet
// tip, with a 500 ms budget per `.run()` call. Each iteration updates the
// spinner with:
//   - cumulative wall-clock elapsed
//   - the iteration's actual wall-clock duration vs the 500 ms budget
//   - a rolling average of wall-clock per iteration
//
// "Real vs intended" tracking surfaces budget overshoot or undershoot per
// step — if the translation system can't honor the budget cleanly, this is
// where we'd see it.

use anyhow::Result;
use async_trait::async_trait;
use base_crypto::cost_model::CostDuration;
use midnight_node_ledger_helpers::DefaultDB;
use midnight_node_ledger_helpers::ledger_8::mn_ledger;
use std::ops::Deref;
use std::time::{Duration, Instant};
use storage::arena::Sp;
use storage::state_translation::TypedTranslationState;
use v8_to_v9_state_translation::StateTranslationTable;

use v8_to_v9_state_translation_replay::progress::Phases;
use v8_to_v9_state_translation_replay::{LedgerTest, SyncedTip};

const PER_RUN_BUDGET: Duration = Duration::from_millis(500);

pub struct IncrementalTranslation;

#[async_trait]
impl LedgerTest for IncrementalTranslation {
    fn name(&self) -> &str {
        "translation::incremental_v8_to_v9"
    }

    async fn run(&self, tip: &SyncedTip, phases: &Phases) -> Result<()> {
        // Take a snapshot of the synced v8 state. `helpers::ledger_8::mn_ledger`
        // is the same Cargo crate instance as the translation crate's
        // `ledger-v8`, so this `Sp<LedgerState<...>, ...>` plugs straight into
        // `TypedTranslationState::start`.
        let v8_state: Sp<mn_ledger::structure::LedgerState<DefaultDB>, DefaultDB> = {
            let guard = tip
                .ctx
                .ledger_state
                .lock()
                .map_err(|_| anyhow::anyhow!("ledger_state mutex poisoned"))?;
            (*guard).clone()
        };

        let v8_contracts = v8_state.contract.size();
        let v8_bridge = v8_state.bridge_receiving.size();
        let v8_unclaimed = v8_state.unclaimed_block_rewards.size();
        log::info!(
            "v8 source state: network_id={:?}, contracts={}, bridge_receiving={}, unclaimed_block_rewards={}",
            v8_state.network_id,
            v8_contracts,
            v8_bridge,
            v8_unclaimed,
        );

        let mut state = TypedTranslationState::<
            mn_ledger::structure::LedgerState<DefaultDB>,
            ledger_v9::structure::LedgerState<DefaultDB>,
            StateTranslationTable,
            DefaultDB,
        >::start(v8_state)
        .map_err(|e| anyhow::anyhow!("start translation: {e}"))?;

        let spinner = phases.spinner("v8→v9");
        let started = Instant::now();
        let mut iter: u64 = 0;
        let mut total_step_time = Duration::ZERO;

        // `CostDuration` charged inside `state.run()` is a heuristic (20µs per
        // queue item) and bears little relation to real time. To honor the
        // user-facing "500 ms per increment" guarantee, we drive `run()` in
        // an inner loop with a small cost chunk and exit when wall-clock
        // exceeds the budget. The cost chunk is sized so each `run()` returns
        // in tens of ms of wall-clock at most, keeping our overshoot bounded.
        let inner_cost = CostDuration::from_picoseconds(50_000_000_000); // 50 ms of cost

        let final_state = loop {
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
            let step_real = step_start.elapsed();
            total_step_time += step_real;

            let avg = total_step_time / u32::try_from(iter).unwrap_or(u32::MAX);
            let drift = step_real
                .checked_sub(PER_RUN_BUDGET)
                .map(|d| format!("+{}ms over", d.as_millis()))
                .unwrap_or_else(|| {
                    let under = PER_RUN_BUDGET - step_real;
                    format!("-{}ms under", under.as_millis())
                });

            spinner.set(format!(
                "iter {iter}, last {}ms ({drift}, avg {}ms, total {:.1}s)",
                step_real.as_millis(),
                avg.as_millis(),
                started.elapsed().as_secs_f64(),
            ));

            if let Some(result) = done {
                break result;
            }
        };

        let v9 = final_state.deref().clone();
        let v9_contracts = v9.contract.size();
        let v9_bridge = v9.bridge_receiving.size();
        let v9_unclaimed = v9.unclaimed_block_rewards.size();
        log::info!(
            "v9 result state: network_id={:?}, contracts={}, bridge_receiving={}, unclaimed_block_rewards={}",
            v9.network_id,
            v9_contracts,
            v9_bridge,
            v9_unclaimed,
        );

        spinner.finish(format!(
            "v8→v9 done: {iter} iter / {:.2}s (avg {}ms/iter)",
            started.elapsed().as_secs_f64(),
            (total_step_time / u32::try_from(iter).unwrap_or(u32::MAX)).as_millis(),
        ));

        // Sanity: counts must match — the contract & bridge maps round-trip
        // entry-by-entry, even though the underlying MPTs change shape.
        anyhow::ensure!(v9_contracts == v8_contracts, "contract count drift: v8={v8_contracts} v9={v9_contracts}");
        anyhow::ensure!(v9_bridge == v8_bridge, "bridge_receiving count drift: v8={v8_bridge} v9={v9_bridge}");
        anyhow::ensure!(v9_unclaimed == v8_unclaimed, "unclaimed_block_rewards count drift: v8={v8_unclaimed} v9={v9_unclaimed}");

        Ok(())
    }
}
