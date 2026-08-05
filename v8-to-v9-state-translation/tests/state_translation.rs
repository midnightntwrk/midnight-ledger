// This file is part of midnight-ledger.
// Copyright (C) 2026 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");

//! Tests mirror the v6→v7 reference suite (PR #160), adapted for v8→v9.
//! The verifier-key-wipe behavior from v6→v7 is intentionally *not* repeated:
//! the v8→v9 translation passes contract operations through unchanged.

use base_crypto::cost_model::CostDuration;
use coin_structure::coin::{TokenType, UserAddress};
use coin_structure::contract::ContractAddress;
use onchain_state_v8::state::{
    ContractMaintenanceAuthority, ContractOperation, ContractState, StateValue,
};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::ops::Deref;
use storage::arena::Sp;
use storage::db::InMemoryDB;
use storage::state_translation::{TranslationTable, TypedTranslationState};
use storage::storage::HashMap;
use transient_crypto::proofs::VerifierKey;
use v8_to_v9_state_translation::StateTranslationTable;

// v9 uses a new-major coin-structure, distinct from the `coin_structure` (v8)
// crate above. Addresses generated for the v8 input must be re-wrapped into
// these types before keying into the translated v9 state.
use coin_structure_v9 as cs9;

const TEST_NETWORK_ID: &str = "test-network";
const ENTRY_OP_A: &[u8] = b"operationA";
const ENTRY_OP_B: &[u8] = b"operationB";
const ENTRY_OP_C: &[u8] = b"operationC";

#[test]
fn test_ledger_state_preserved() {
    let v8 = ledger_v8::structure::LedgerState::<InMemoryDB>::new(TEST_NETWORK_ID);
    let v9 = translate_to_completion(v8.clone());

    assert_eq!(v9.network_id, v8.network_id);
    assert_eq!(v9.reserve_pool, v8.reserve_pool);
    assert_eq!(v9.locked_pool, v8.locked_pool);
    assert_eq!(v9.block_reward_pool, v8.block_reward_pool);
    assert_eq!(v9.zswap.first_free, v8.zswap.first_free);
    // The newly-added v9 parameter gets the v9 INITIAL_PARAMETERS default.
    assert_eq!(
        v9.parameters.min_block_price,
        ledger_v9::structure::INITIAL_PARAMETERS.min_block_price,
    );
}

#[test]
fn test_contract_preserved_after_translation() {
    let mut rng = StdRng::seed_from_u64(0x42);
    let mut v8 = ledger_v8::structure::LedgerState::<InMemoryDB>::new(TEST_NETWORK_ID);

    let contract = create_test_contract(&mut rng);
    let addr: ContractAddress = rng.r#gen();
    let entry_count = contract.operations.iter().count();
    v8.contract = v8.contract.insert(addr.clone(), contract);

    let v9 = translate_to_completion(v8);

    let translated = v9
        .contract
        .get(&v9_contract_addr(&addr))
        .expect("contract should exist after translation");
    assert_eq!(translated.operations.iter().count(), entry_count);
    for ep in [ENTRY_OP_A, ENTRY_OP_B, ENTRY_OP_C] {
        assert!(
            translated.operations.get(&ep.into()).is_some(),
            "entry point {:?} should exist",
            std::str::from_utf8(ep).unwrap(),
        );
    }
    // v8→v9 KEEPS verifier keys (unlike the v6→v7 reference, which wiped them).
    // The v8 key is a zk-stdlib-v1 key, so it lands in v9's `v2` slot; the new
    // zk-stdlib-v2 `v3` (`latest`) slot stays empty.
    for entry in translated.operations.iter() {
        assert!(
            entry.1.v2_vk().is_some(),
            "verifier key should survive v8→v9 translation",
        );
        assert!(
            entry.1.latest().is_none(),
            "v8 key must not be promoted to the v9 (v3) slot",
        );
    }
}

#[test]
fn test_multiple_contracts_translated() {
    let mut rng = StdRng::seed_from_u64(0x42);
    let mut v8 = ledger_v8::structure::LedgerState::<InMemoryDB>::new(TEST_NETWORK_ID);

    let mut addresses = Vec::new();
    for _ in 0..5 {
        let contract = create_test_contract(&mut rng);
        let addr: ContractAddress = rng.r#gen();
        addresses.push(addr.clone());
        v8.contract = v8.contract.insert(addr, contract);
    }

    let v9 = translate_to_completion(v8);

    for (i, addr) in addresses.iter().enumerate() {
        let c = v9
            .contract
            .get(&v9_contract_addr(addr))
            .unwrap_or_else(|| panic!("contract {i} should exist after translation"));
        let op = c
            .operations
            .get(&ENTRY_OP_A.into())
            .expect("operationA should exist");
        assert!(op.v2_vk().is_some(), "verifier key {i} should survive");
    }
}

#[test]
fn test_contract_with_empty_operations() {
    let mut rng = StdRng::seed_from_u64(0x42);
    let mut v8 = ledger_v8::structure::LedgerState::<InMemoryDB>::new(TEST_NETWORK_ID);

    let contract = ContractState::new(StateValue::Null, HashMap::new(), Default::default());
    let addr: ContractAddress = rng.r#gen();
    v8.contract = v8.contract.insert(addr.clone(), contract);

    let v9 = translate_to_completion(v8);

    let translated = v9
        .contract
        .get(&v9_contract_addr(&addr))
        .expect("contract should exist");
    assert!(
        translated.operations.iter().next().is_none(),
        "contract should have no operations",
    );
}

#[test]
fn test_contract_balance_preserved() {
    let mut rng = StdRng::seed_from_u64(0x42);
    let mut v8 = ledger_v8::structure::LedgerState::<InMemoryDB>::new(TEST_NETWORK_ID);

    let mut contract = create_test_contract(&mut rng);
    let amount: u128 = 1_000_000;
    contract.balance = contract.balance.insert(TokenType::Dust, amount);

    let addr: ContractAddress = rng.r#gen();
    v8.contract = v8.contract.insert(addr.clone(), contract);

    let v9 = translate_to_completion(v8);

    let translated = v9
        .contract
        .get(&v9_contract_addr(&addr))
        .expect("contract should exist");
    let translated_balance = translated
        .balance
        .get(&cs9::coin::TokenType::Dust)
        .expect("balance entry should exist");
    assert_eq!(*translated_balance, amount);
}

#[test]
fn test_maintenance_authority_preserved() {
    let mut rng = StdRng::seed_from_u64(0x42);
    let mut v8 = ledger_v8::structure::LedgerState::<InMemoryDB>::new(TEST_NETWORK_ID);

    let vk1: base_crypto::schnorr::VerifyingKey = rng.r#gen();
    let vk2: base_crypto::schnorr::VerifyingKey = rng.r#gen();
    let authority = ContractMaintenanceAuthority {
        committee: vec![vk1.clone(), vk2.clone()],
        threshold: 2,
        counter: 3,
    };
    let mut ops = HashMap::new();
    ops = ops.insert(ENTRY_OP_A.into(), ContractOperation::new(Some(rng.r#gen())));
    let contract = ContractState::new(StateValue::Null, ops, authority);
    let addr: ContractAddress = rng.r#gen();
    v8.contract = v8.contract.insert(addr.clone(), contract);

    let v9 = translate_to_completion(v8);

    let translated = v9
        .contract
        .get(&v9_contract_addr(&addr))
        .expect("contract should exist");
    assert_eq!(translated.maintenance_authority.threshold, 2);
    assert_eq!(translated.maintenance_authority.counter, 3);
    // The v8 schnorr-only committee gets wrapped as the Schnorr variant of
    // v9's ContractMaintenanceVerifyingKey sum.
    use onchain_state_v9::state::ContractMaintenanceVerifyingKey as CMVK;
    let committee = &translated.maintenance_authority.committee;
    assert_eq!(committee.len(), 2);
    match (&committee[0], &committee[1]) {
        (CMVK::Schnorr(a), CMVK::Schnorr(b)) => {
            assert_eq!(*a, vk1);
            assert_eq!(*b, vk2);
        }
        _ => panic!("committee entries should be Schnorr-wrapped"),
    }
}

#[test]
fn test_incremental_translation_requires_multiple_iterations() {
    let (v8, addresses) = create_large_state(50);
    let cost_per_run = CostDuration::from_picoseconds(1_000_000_000); // 1 ms

    let (v9, iterations) = translate_incrementally(v8, cost_per_run, 100);

    assert!(iterations > 1, "expected multiple iterations, got {iterations}");

    for (i, addr) in addresses.iter().enumerate() {
        let c = v9
            .contract
            .get(&v9_contract_addr(addr))
            .unwrap_or_else(|| panic!("contract {i} should exist after translation"));
        for ep in [ENTRY_OP_A, ENTRY_OP_B, ENTRY_OP_C] {
            assert!(c.operations.get(&ep.into()).is_some(),);
        }
    }
}

#[test]
fn test_translation_is_deterministic() {
    let build = || {
        let mut rng = StdRng::seed_from_u64(0x42);
        let mut v8 = ledger_v8::structure::LedgerState::<InMemoryDB>::new(TEST_NETWORK_ID);
        let contract = create_test_contract(&mut rng);
        let addr: ContractAddress = rng.r#gen();
        v8.contract = v8.contract.insert(addr, contract);
        v8
    };

    let a = translate_to_completion(build());
    let b = translate_to_completion(build());

    assert_eq!(a.network_id, b.network_id);
    assert_eq!(a.reserve_pool, b.reserve_pool);
    assert_eq!(a.locked_pool, b.locked_pool);
    assert_eq!(a.block_reward_pool, b.block_reward_pool);
}

#[test]
fn test_bridge_receiving_preserved() {
    let mut rng = StdRng::seed_from_u64(0x42);
    let mut v8 = ledger_v8::structure::LedgerState::<InMemoryDB>::new(TEST_NETWORK_ID);

    let vk: base_crypto::schnorr::VerifyingKey = rng.r#gen();
    let addr = UserAddress::from(vk);
    let amount: u128 = 12_345;
    v8.bridge_receiving = v8.bridge_receiving.insert(addr.clone(), amount);

    let v9 = translate_to_completion(v8);

    let translated = v9
        .bridge_receiving
        .get(&v9_user_addr(&addr))
        .expect("bridge_receiving entry should survive translation");
    assert_eq!(*translated, amount);
}

// ---------- Helpers ----------

/// Re-wrap a v8 contract address as the (distinct) v9 coin-structure type.
/// `HashOutput` comes from the version-unified `base-crypto`, so the inner
/// value carries across the coin-structure major-version boundary unchanged.
fn v9_contract_addr(a: &ContractAddress) -> cs9::contract::ContractAddress {
    cs9::contract::ContractAddress(a.0)
}

/// Re-wrap a v8 user address as the v9 coin-structure type (see above).
fn v9_user_addr(a: &UserAddress) -> cs9::coin::UserAddress {
    cs9::coin::UserAddress(a.0)
}

fn translate_to_completion(
    v8: ledger_v8::structure::LedgerState<InMemoryDB>,
) -> ledger_v9::structure::LedgerState<InMemoryDB> {
    let tl_state = TypedTranslationState::<
        ledger_v8::structure::LedgerState<InMemoryDB>,
        ledger_v9::structure::LedgerState<InMemoryDB>,
        StateTranslationTable,
        InMemoryDB,
    >::start(Sp::new(v8))
    .expect("Failed to start translation");

    let cost = CostDuration::from_picoseconds(1_000_000_000_000);
    let finished = tl_state.run(cost).expect("Translation failed");

    finished
        .result()
        .expect("Failed to get result")
        .expect("Translation did not complete")
        .deref()
        .clone()
}

fn translate_incrementally(
    v8: ledger_v8::structure::LedgerState<InMemoryDB>,
    cost_per_run: CostDuration,
    max_iterations: usize,
) -> (ledger_v9::structure::LedgerState<InMemoryDB>, usize) {
    let mut tl_state = TypedTranslationState::<
        ledger_v8::structure::LedgerState<InMemoryDB>,
        ledger_v9::structure::LedgerState<InMemoryDB>,
        StateTranslationTable,
        InMemoryDB,
    >::start(Sp::new(v8))
    .expect("Failed to start translation");

    let mut iterations = 0;
    loop {
        iterations += 1;
        assert!(
            iterations <= max_iterations,
            "Translation did not complete within {max_iterations} iterations",
        );
        tl_state = tl_state.run(cost_per_run).expect("Translation failed");
        if let Some(result) = tl_state.result().expect("Failed to get result") {
            return (result.deref().clone(), iterations);
        }
    }
}

fn create_test_contract(rng: &mut StdRng) -> ContractState<InMemoryDB> {
    let mut ops = HashMap::new();
    ops = ops.insert(ENTRY_OP_A.into(), ContractOperation::new(Some(rng.r#gen::<VerifierKey>())));
    ops = ops.insert(ENTRY_OP_B.into(), ContractOperation::new(Some(rng.r#gen::<VerifierKey>())));
    ops = ops.insert(ENTRY_OP_C.into(), ContractOperation::new(Some(rng.r#gen::<VerifierKey>())));
    ContractState::new(StateValue::Null, ops, Default::default())
}

fn create_large_state(
    num_contracts: usize,
) -> (
    ledger_v8::structure::LedgerState<InMemoryDB>,
    Vec<ContractAddress>,
) {
    let mut rng = StdRng::seed_from_u64(0x42);
    let mut v8 = ledger_v8::structure::LedgerState::<InMemoryDB>::new(TEST_NETWORK_ID);
    let mut addresses = Vec::new();
    for _ in 0..num_contracts {
        let contract = create_test_contract(&mut rng);
        let addr: ContractAddress = rng.r#gen();
        addresses.push(addr.clone());
        v8.contract = v8.contract.insert(addr, contract);
    }
    (v8, addresses)
}

// ---------- Table consistency ----------
//
// These two tests guard the translation table itself rather than its data
// behavior. They are cheap to run and catch type/tag drift early — far
// before any state actually exercises a missing or mismatched entry.

#[test]
fn table_is_closed() {
    // Every TranslationId returned by a table entry's `required_translations`
    // must itself have an entry in the table. If not, translation would error
    // at runtime the first time the missing translation was needed.
    <StateTranslationTable as TranslationTable<InMemoryDB>>::assert_closure();
}

#[test]
fn translates_serialized_non_trivial_state_preserving_invariants() {
    // End-to-end: build a non-trivial v8 state, serialize/deserialize the
    // v8 side, translate to v9, then assert the v9 state passes its own
    // NIGHT-balance invariant and survives a v9 serialize round-trip.
    //
    // This is the integration-style counterpart to the v6→v7 reference's
    // `micro_dao.rs` — we skip the whole DAO scenario (which exercised the
    // ledger, not the translation) and just confirm the translated state
    // is well-formed enough to plug back into the v9 machinery.

    let mut rng = StdRng::seed_from_u64(0xabc);

    // Mildly non-default LedgerParameters: bump global_ttl.
    let mut params = ledger_v8::structure::INITIAL_PARAMETERS;
    params.global_ttl = base_crypto::time::Duration::from_secs(3600);

    // Genesis split: keep almost everything in reserve_pool, leave a sliver
    // in treasury. The constructor checks the NIGHT balance invariant.
    let bridge_amount = 500_000u128;
    let reward_amount = 200_000u128;
    let treasury_amount = 1_000_000u128;
    let reserve_init = ledger_v8::structure::MAX_SUPPLY - treasury_amount;

    let mut v8 = ledger_v8::structure::LedgerState::<InMemoryDB>::with_genesis_settings(
        TEST_NETWORK_ID,
        params,
        0,
        reserve_init,
        treasury_amount,
    )
    .expect("v8 genesis state should be valid");

    // Move some NIGHT into bridge_receiving and unclaimed_block_rewards.
    // Each move keeps total NIGHT == MAX_SUPPLY.
    v8.reserve_pool -= bridge_amount;
    let bridge_addr = UserAddress::from(rng.r#gen::<base_crypto::schnorr::VerifyingKey>());
    v8.bridge_receiving = v8.bridge_receiving.insert(bridge_addr.clone(), bridge_amount);

    v8.reserve_pool -= reward_amount;
    let reward_addr = UserAddress::from(rng.r#gen::<base_crypto::schnorr::VerifyingKey>());
    v8.unclaimed_block_rewards = v8
        .unclaimed_block_rewards
        .insert(reward_addr.clone(), reward_amount);

    // Three contracts. They carry no NIGHT (their balance maps are empty),
    // so they don't perturb the invariant.
    let mut contract_addrs = Vec::new();
    for _ in 0..3 {
        let contract = create_test_contract(&mut rng);
        let addr: ContractAddress = rng.r#gen();
        contract_addrs.push(addr.clone());
        v8.contract = v8.contract.insert(addr, contract);
    }

    // Round-trip v8 through bytes — exercises the serialized-form path.
    let mut buf = Vec::new();
    serialize::tagged_serialize(&v8, &mut buf).expect("v8 serialize");
    let v8_round_tripped: ledger_v8::structure::LedgerState<InMemoryDB> =
        serialize::tagged_deserialize(&mut &buf[..]).expect("v8 deserialize");

    // Translate.
    let v9 = translate_to_completion(v8_round_tripped);

    // (a) The translated state preserves the NIGHT-balance invariant.
    v9.check_night_balance_invariant()
        .expect("translated v9 state must preserve NIGHT-balance invariant");

    // (b) Non-trivial pieces survive the translation.
    assert_eq!(
        *v9.bridge_receiving
            .get(&v9_user_addr(&bridge_addr))
            .expect("bridge entry"),
        bridge_amount,
    );
    assert_eq!(
        *v9.unclaimed_block_rewards
            .get(&v9_user_addr(&reward_addr))
            .expect("reward entry"),
        reward_amount,
    );
    for addr in &contract_addrs {
        assert!(
            v9.contract.get(&v9_contract_addr(addr)).is_some(),
            "contract should survive",
        );
    }
    assert_eq!(v9.parameters.global_ttl, base_crypto::time::Duration::from_secs(3600));
    assert_eq!(
        v9.parameters.min_block_price,
        ledger_v9::structure::INITIAL_PARAMETERS.min_block_price,
    );

    // (c) Round-trip the v9 state through bytes. Catches any structural
    // inconsistency the translation might have introduced.
    let mut buf = Vec::new();
    serialize::tagged_serialize(&v9, &mut buf).expect("v9 serialize");
    let v9_round_tripped: ledger_v9::structure::LedgerState<InMemoryDB> =
        serialize::tagged_deserialize(&mut &buf[..]).expect("v9 deserialize");
    v9_round_tripped
        .check_night_balance_invariant()
        .expect("v9 state must still be valid after a serialize round-trip");
    assert_eq!(v9_round_tripped.network_id, v9.network_id);
}

#[test]
fn table_tags_match_types() {
    // The TABLE hardcodes string literals for each TranslationId. If a tag on
    // either side gets bumped without updating the table, the literal would
    // drift from what `T::tag()` actually produces. Rebuild every expected ID
    // from types and compare.
    use serialize::Tagged;
    use storage::merkle_patricia_trie::{MerklePatriciaTrie, Node};
    use storage::storable::SizeAnn;

    type V8Ann = ledger_v8::annotation::NightAnn;
    type V9Ann = ledger_v9::annotation::NightAnn;
    type V8Contract = onchain_state_v8::state::ContractState<InMemoryDB>;
    type V9Contract = onchain_state_v9::state::ContractState<InMemoryDB>;

    let expected: Vec<(std::borrow::Cow<'static, str>, std::borrow::Cow<'static, str>)> = vec![
        (
            ledger_v8::structure::LedgerState::<InMemoryDB>::tag(),
            ledger_v9::structure::LedgerState::<InMemoryDB>::tag(),
        ),
        (
            ledger_v8::structure::LedgerParameters::tag(),
            ledger_v9::structure::LedgerParameters::tag(),
        ),
        (V8Contract::tag(), V9Contract::tag()),
        (
            MerklePatriciaTrie::<V8Contract, InMemoryDB, V8Ann>::tag(),
            MerklePatriciaTrie::<V9Contract, InMemoryDB, V9Ann>::tag(),
        ),
        (
            Node::<V8Contract, InMemoryDB, V8Ann>::tag(),
            Node::<V9Contract, InMemoryDB, V9Ann>::tag(),
        ),
        (u128::tag(), u128::tag()),
        (
            MerklePatriciaTrie::<u128, InMemoryDB, SizeAnn>::tag(),
            MerklePatriciaTrie::<u128, InMemoryDB, V9Ann>::tag(),
        ),
        (
            Node::<u128, InMemoryDB, SizeAnn>::tag(),
            Node::<u128, InMemoryDB, V9Ann>::tag(),
        ),
    ];

    let actual: Vec<_> =
        <StateTranslationTable as TranslationTable<InMemoryDB>>::TABLE
            .iter()
            .map(|(id, _)| (id.0.clone(), id.1.clone()))
            .collect();

    assert_eq!(actual, expected);
}

