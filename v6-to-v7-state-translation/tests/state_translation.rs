// This file is part of midnight-ledger.
// Copyright (C) 2025 Midnight Foundation
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

use base_crypto::cost_model::CostDuration;
use coin_structure_v6::coin::TokenType;
use coin_structure_v6::contract::ContractAddress;
use onchain_state_v6::state::{
    ContractMaintenanceAuthority, ContractOperation, ContractState, StateValue,
};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::ops::Deref;
use storage::arena::Sp;
use storage::db::InMemoryDB;
use storage::state_translation::TypedTranslationState;
use storage::storage::HashMap;
use transient_crypto_v6::proofs::VerifierKey;
use v6_to_v7_state_translation::StateTranslationTable;

const TEST_NETWORK_ID: &str = "test-network";
const ENTRY_OP_A: &[u8] = b"operationA";
const ENTRY_OP_B: &[u8] = b"operationB";
const ENTRY_OP_C: &[u8] = b"operationC";

#[test]
fn test_ledger_state_preserved() {
    let v6_state = ledger_v6::structure::LedgerState::<InMemoryDB>::new(TEST_NETWORK_ID);

    let v6_network_id = v6_state.network_id.clone();
    let v6_reserve_pool = v6_state.reserve_pool;
    let v6_locked_pool = v6_state.locked_pool;
    let v6_block_reward_pool = v6_state.block_reward_pool;
    let v6_zswap_first_free = v6_state.zswap.first_free;

    let v7_state = translate_to_completion(v6_state);

    assert_eq!(
        v7_state.network_id, v6_network_id,
        "network_id should be preserved"
    );
    assert_eq!(
        v7_state.reserve_pool, v6_reserve_pool,
        "reserve_pool should be preserved"
    );
    assert_eq!(
        v7_state.locked_pool, v6_locked_pool,
        "locked_pool should be preserved"
    );
    assert_eq!(
        v7_state.block_reward_pool, v6_block_reward_pool,
        "block_reward_pool should be preserved"
    );
    assert_eq!(
        v7_state.zswap.first_free, v6_zswap_first_free,
        "zswap.first_free should be preserved"
    );
}

#[test]
fn test_contract_preserved_after_translation() {
    let mut rng = StdRng::seed_from_u64(0x42);
    let mut v6_state = ledger_v6::structure::LedgerState::<InMemoryDB>::new(TEST_NETWORK_ID);

    let contract_state = create_test_contract(&mut rng);
    let contract_address: ContractAddress = rng.r#gen();

    let v6_entry_point_count = contract_state.operations.iter().count();

    v6_state.contract = v6_state
        .contract
        .insert(contract_address.clone(), contract_state);

    let v7_state = translate_to_completion(v6_state);

    let translated_contract = v7_state
        .contract
        .get(&to_v7_address(&contract_address))
        .expect("Contract should exist after translation");

    assert_eq!(
        translated_contract.operations.iter().count(),
        v6_entry_point_count,
        "Entry point count should be preserved"
    );

    for entry_point in [ENTRY_OP_A, ENTRY_OP_B, ENTRY_OP_C] {
        assert!(
            translated_contract
                .operations
                .get(&entry_point.into())
                .is_some(),
            "Entry point {:?} should exist",
            std::str::from_utf8(entry_point).unwrap()
        );
    }

    for entry in translated_contract.operations.iter() {
        assert!(
            entry.1.latest().is_none(),
            "Verifier key should be wiped after translation"
        );
    }
}

#[test]
fn test_contract_entry_points_preserved_verifier_keys_wiped() {
    let mut rng = StdRng::seed_from_u64(0x42);
    let mut v6_state = ledger_v6::structure::LedgerState::<InMemoryDB>::new(TEST_NETWORK_ID);

    let contract_state = create_test_contract(&mut rng);
    let contract_address: ContractAddress = rng.r#gen();

    for entry in contract_state.operations.iter() {
        assert!(
            entry.1.latest().is_some(),
            "v6 contract should have verifier keys before translation"
        );
    }

    v6_state.contract = v6_state
        .contract
        .insert(contract_address.clone(), contract_state);

    let v7_state = translate_to_completion(v6_state);

    let translated_contract = v7_state
        .contract
        .get(&to_v7_address(&contract_address))
        .expect("Contract should exist after translation");

    let entry_points = [ENTRY_OP_A, ENTRY_OP_B, ENTRY_OP_C];
    for entry_point in entry_points {
        assert!(
            translated_contract
                .operations
                .get(&entry_point.into())
                .is_some(),
            "Entry point {:?} should exist",
            std::str::from_utf8(entry_point).unwrap()
        );
    }

    for entry in translated_contract.operations.iter() {
        let operation = &*entry.1;
        assert!(
            operation.latest().is_none(),
            "Verifier key should be None after translation"
        );
    }
}

#[test]
fn test_multiple_contracts_translated() {
    let mut rng = StdRng::seed_from_u64(0x42);
    let mut v6_state = ledger_v6::structure::LedgerState::<InMemoryDB>::new(TEST_NETWORK_ID);

    let mut addresses = Vec::new();
    for _ in 0..5 {
        let contract_state = create_test_contract(&mut rng);
        let contract_address: ContractAddress = rng.r#gen();
        addresses.push(contract_address.clone());
        v6_state.contract = v6_state.contract.insert(contract_address, contract_state);
    }

    let v7_state = translate_to_completion(v6_state);

    for (i, v6_addr) in addresses.iter().enumerate() {
        let contract = v7_state
            .contract
            .get(&to_v7_address(v6_addr))
            .expect(&format!("Contract {} should exist after translation", i));

        let op = contract
            .operations
            .get(&ENTRY_OP_A.into())
            .expect("operationA should exist");
        assert!(op.latest().is_none(), "Verifier key should be None");
    }
}

#[test]
fn test_contract_with_empty_operations() {
    let mut rng = StdRng::seed_from_u64(0x42);
    let mut v6_state = ledger_v6::structure::LedgerState::<InMemoryDB>::new(TEST_NETWORK_ID);

    let contract = ContractState::new(StateValue::Null, HashMap::new(), Default::default());
    let contract_address: ContractAddress = rng.r#gen();
    v6_state.contract = v6_state.contract.insert(contract_address.clone(), contract);

    let v7_state = translate_to_completion(v6_state);

    let translated_contract = v7_state
        .contract
        .get(&to_v7_address(&contract_address))
        .expect("Contract should exist");

    assert!(
        translated_contract.operations.iter().next().is_none(),
        "Contract should have no operations"
    );
}

#[test]
fn test_contract_balance_preserved() {
    let mut rng = StdRng::seed_from_u64(0x42);
    let mut v6_state = ledger_v6::structure::LedgerState::<InMemoryDB>::new(TEST_NETWORK_ID);

    let mut contract_state = create_test_contract(&mut rng);
    let token_type = TokenType::Dust;
    let balance_amount: u128 = 1_000_000;
    contract_state.balance = contract_state.balance.insert(token_type, balance_amount);

    let contract_address: ContractAddress = rng.r#gen();
    v6_state.contract = v6_state
        .contract
        .insert(contract_address.clone(), contract_state);

    let v7_state = translate_to_completion(v6_state);

    let translated_contract = v7_state
        .contract
        .get(&to_v7_address(&contract_address))
        .expect("Contract should exist after translation");

    let translated_balance = translated_contract
        .balance
        .get(&coin_structure_v7::coin::TokenType::Dust)
        .expect("Balance should exist after translation");
    assert_eq!(
        *translated_balance, balance_amount,
        "Balance amount should be preserved"
    );
}

#[test]
fn test_contract_maintenance_authority_preserved() {
    let mut rng = StdRng::seed_from_u64(0x42);
    let mut v6_state = ledger_v6::structure::LedgerState::<InMemoryDB>::new(TEST_NETWORK_ID);

    let maintenance_authority = ContractMaintenanceAuthority {
        committee: vec![],
        threshold: 2,
        counter: 3,
    };
    let mut operations = HashMap::new();
    operations = operations.insert(
        ENTRY_OP_A.into(),
        ContractOperation::new(Some(rng.r#gen::<VerifierKey>())),
    );
    let contract_state = ContractState::new(StateValue::Null, operations, maintenance_authority);

    let contract_address: ContractAddress = rng.r#gen();
    v6_state.contract = v6_state
        .contract
        .insert(contract_address.clone(), contract_state);

    let v7_state = translate_to_completion(v6_state);

    let translated_contract = v7_state
        .contract
        .get(&to_v7_address(&contract_address))
        .expect("Contract should exist after translation");

    assert_eq!(
        translated_contract.maintenance_authority.threshold, 2,
        "Maintenance authority threshold should be preserved"
    );
    assert_eq!(
        translated_contract.maintenance_authority.counter, 3,
        "Maintenance authority counter should be preserved"
    );
}

#[test]
fn test_incremental_translation_requires_multiple_iterations() {
    // State with 50 contracts (each with 3 operations) - many nodes to translate
    let (v6_state, addresses) = create_large_state(50);

    let cost_per_run = CostDuration::from_picoseconds(1_000_000_000); // small budget, just 1ms

    let (v7_state, iterations) = translate_incrementally(v6_state, cost_per_run, 100);

    assert!(
        iterations > 1,
        "Expected multiple iterations, got {}",
        iterations
    );

    for (i, v6_addr) in addresses.iter().enumerate() {
        let contract_state = v7_state
            .contract
            .get(&to_v7_address(v6_addr))
            .expect(&format!("Contract {} should exist after translation", i));

        for entry_point in [ENTRY_OP_A, ENTRY_OP_B, ENTRY_OP_C] {
            assert!(
                contract_state.operations.get(&entry_point.into()).is_some(),
                "Contract {} should have entry point {:?}",
                i,
                std::str::from_utf8(entry_point).unwrap()
            );
        }

        for entry in contract_state.operations.iter() {
            assert!(
                entry.1.latest().is_none(),
                "Contract {} verifier key should be wiped",
                i
            );
        }
    }
}

#[test]
fn test_translation_is_deterministic() {
    let create_state = || {
        let mut rng = StdRng::seed_from_u64(0x42);
        let mut v6_state = ledger_v6::structure::LedgerState::<InMemoryDB>::new(TEST_NETWORK_ID);
        let contract_state = create_test_contract(&mut rng);
        let contract_address: ContractAddress = rng.r#gen();
        v6_state.contract = v6_state.contract.insert(contract_address, contract_state);
        v6_state
    };

    let v7_state_1 = translate_to_completion(create_state());
    let v7_state_2 = translate_to_completion(create_state());

    assert_eq!(v7_state_1.network_id, v7_state_2.network_id);
    assert_eq!(v7_state_1.reserve_pool, v7_state_2.reserve_pool);
    assert_eq!(v7_state_1.locked_pool, v7_state_2.locked_pool);
    assert_eq!(v7_state_1.block_reward_pool, v7_state_2.block_reward_pool);
}

fn translate_to_completion(
    v6_state: ledger_v6::structure::LedgerState<InMemoryDB>,
) -> ledger_v7::structure::LedgerState<InMemoryDB> {
    let tl_state = TypedTranslationState::<
        ledger_v6::structure::LedgerState<InMemoryDB>,
        ledger_v7::structure::LedgerState<InMemoryDB>,
        StateTranslationTable,
        InMemoryDB,
    >::start(Sp::new(v6_state))
    .expect("Failed to start translation");

    let cost = CostDuration::from_picoseconds(1_000_000_000_000);
    let finished_state = tl_state.run(cost).expect("Translation failed");

    finished_state
        .result()
        .expect("Failed to get result")
        .expect("Translation did not complete")
        .deref()
        .clone()
}

fn create_test_contract(rng: &mut StdRng) -> ContractState<InMemoryDB> {
    let mut operations = HashMap::new();
    operations = operations.insert(
        ENTRY_OP_A.into(),
        ContractOperation::new(Some(rng.r#gen::<VerifierKey>())),
    );
    operations = operations.insert(
        ENTRY_OP_B.into(),
        ContractOperation::new(Some(rng.r#gen::<VerifierKey>())),
    );
    operations = operations.insert(
        ENTRY_OP_C.into(),
        ContractOperation::new(Some(rng.r#gen::<VerifierKey>())),
    );

    ContractState::new(StateValue::Null, operations, Default::default())
}

fn to_v7_address(v6_addr: &ContractAddress) -> coin_structure_v7::contract::ContractAddress {
    coin_structure_v7::contract::ContractAddress(v6_addr.0.clone())
}

fn translate_incrementally(
    v6_state: ledger_v6::structure::LedgerState<InMemoryDB>,
    cost_per_run: CostDuration,
    max_iterations: usize,
) -> (ledger_v7::structure::LedgerState<InMemoryDB>, usize) {
    let mut tl_state = TypedTranslationState::<
        ledger_v6::structure::LedgerState<InMemoryDB>,
        ledger_v7::structure::LedgerState<InMemoryDB>,
        StateTranslationTable,
        InMemoryDB,
    >::start(Sp::new(v6_state))
    .expect("Failed to start translation");

    let mut iterations = 0;
    loop {
        iterations += 1;
        if iterations > max_iterations {
            panic!(
                "Translation did not complete within {} iterations",
                max_iterations
            );
        }

        tl_state = tl_state.run(cost_per_run).expect("Translation failed");
        if let Some(result) = tl_state.result().expect("Failed to get result") {
            return (result.deref().clone(), iterations);
        }
    }
}

fn create_large_state(
    num_contracts: usize,
) -> (
    ledger_v6::structure::LedgerState<InMemoryDB>,
    Vec<ContractAddress>,
) {
    let mut rng = StdRng::seed_from_u64(0x42);
    let mut v6_state = ledger_v6::structure::LedgerState::<InMemoryDB>::new(TEST_NETWORK_ID);
    let mut addresses = Vec::new();

    for _ in 0..num_contracts {
        let contract_state = create_test_contract(&mut rng);
        let contract_address: ContractAddress = rng.r#gen();
        addresses.push(contract_address.clone());
        v6_state.contract = v6_state.contract.insert(contract_address, contract_state);
    }

    (v6_state, addresses)
}
