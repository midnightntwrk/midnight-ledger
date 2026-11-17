// This file is part of midnight-ledger.
// Copyright (C) Midnight Foundation
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

//! Pricing stops copying deployed states once the bytes charged exceed what a
//! block may write and churn; these tests compare it against an unlimited
//! copy budget.

use crate::structure::*;
use base_crypto::fab::AlignedValue;
use onchain_runtime::context::Effects;
use onchain_runtime::state::{
    ContractMaintenanceAuthority, ContractState, EntryPointBuf, StateValue,
};
use onchain_runtime::transcript::Transcript;
use onchain_vm::ops::Op;
use proptest::prelude::*;
use storage::db::InMemoryDB;
use storage::storage::HashMap;
use transient_crypto::curve::Fr;

type D = InMemoryDB;
type Tx = Transaction<(), (), Pedersen, D>;

/// Deploys of `states` distinct contract states that share most of their
/// nodes, each deployed `repeats` times, plus a call declaring `deleted`
/// bytes deleted.
fn deploys(states: u32, entries: u32, repeats: u32, deleted: u64) -> Tx {
    type M = HashMap<AlignedValue, StateValue<D>, D>;
    let base: M = (0..entries).fold(HashMap::new(), |m, i| {
        m.insert(AlignedValue::from(i), StateValue::Null)
    });
    let mut actions: Vec<ContractAction<(), D>> = (0..states)
        .flat_map(|i| {
            let deploy = ContractAction::Deploy(Sp::new(ContractDeploy {
                initial_state: ContractState::new(
                    StateValue::Map(base.insert(AlignedValue::from(entries + i), StateValue::Null)),
                    HashMap::new(),
                    ContractMaintenanceAuthority::default(),
                ),
                nonce: HashOutput([i as u8; 32]),
            }));
            std::iter::repeat_n(deploy, repeats as usize)
        })
        .collect();
    actions.push(ContractAction::Call(Sp::new(ContractCall {
        address: Default::default(),
        entry_point: EntryPointBuf(vec![]),
        guaranteed_transcript: None,
        fallible_transcript: Some(Sp::new(Transcript {
            gas: RunningCost {
                bytes_deleted: deleted,
                ..RunningCost::ZERO
            },
            effects: Effects::default(),
            program: vec![Op::Noop { n: 1 }].into(),
            version: None,
        })),
        communication_commitment: Fr::from(0u64),
        proof: (),
    })));
    let intent = Intent {
        guaranteed_unshielded_offer: None,
        fallible_unshielded_offer: None,
        actions: actions.into(),
        dust_actions: None,
        ttl: Timestamp::from_secs(0),
        binding_commitment: Default::default(),
    };
    Transaction::Standard(StandardTransaction {
        network_id: "local-test".into(),
        intents: HashMap::new().insert(1, intent),
        guaranteed_coins: None,
        fallible_coins: HashMap::new(),
        binding_randomness: Default::default(),
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    // Pricing stops copying deployed states once the bytes charged are
    // certain to fail the block limits; it must accept exactly the
    // transactions an unlimited copy budget accepts, at the same fee.
    #[test]
    fn capped_copies_price_as_exact_copies_do(
        states in 1u32..5,
        entries in 1u32..200,
        repeats in 1u32..4,
        deleted in prop_oneof![1 => Just(0u64), 1 => 0u64..20_000, 2 => Just(u64::MAX)],
        limits in prop_oneof![
            1 => (-2_000i64..2_000, -2_000i64..2_000).prop_map(|(w, c)| Limits::Near(w, c)),
            2 => (0u64..=120, 0u64..=120).prop_map(|(w, c)| Limits::Share(w, c)),
        ],
    ) {
        let tx = deploys(states, entries, repeats, deleted);
        let model = &INITIAL_PARAMETERS.cost_model;
        let mut unlimited = INITIAL_PARAMETERS.clone();
        unlimited.limits.block_limits.bytes_written = u64::MAX;
        unlimited.limits.block_limits.bytes_churned = u64::MAX;
        let exact = tx.validation_cost(model) + tx.application_cost(&unlimited).unwrap().1;
        let mut params = INITIAL_PARAMETERS.clone();
        let (written, churned) = limits.around(exact.bytes_written, exact.bytes_churned);
        params.limits.block_limits.bytes_written = written;
        params.limits.block_limits.bytes_churned = churned;
        let exact_fee = exact
            .normalize(params.limits.block_limits)
            .map(|cost| params.fee_prices.overall_cost(&cost).into_atomic_units(SPECKS_PER_DUST));
        prop_assert_eq!(tx.fees(&params, false).ok(), exact_fee);
    }
}

/// Block limits for bytes written and churned, relative to a transaction's
/// own: within a few KB of them, where the block limits decide, or a share of
/// them, where pricing's copy budget runs out first.
#[derive(Debug, Clone)]
enum Limits {
    Near(i64, i64),
    Share(u64, u64),
}

impl Limits {
    fn around(&self, written: u64, churned: u64) -> (u64, u64) {
        match *self {
            Limits::Near(w, c) => (
                written.saturating_add_signed(w),
                churned.saturating_add_signed(c),
            ),
            Limits::Share(w, c) => (
                written.saturating_mul(w) / 100,
                churned.saturating_mul(c) / 100,
            ),
        }
    }
}
