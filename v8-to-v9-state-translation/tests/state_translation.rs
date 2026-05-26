// This file is part of midnight-ledger.
// Copyright (C) 2026 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");

//! Sketch test suite. Mirrors the v6→v7 reference (PR #160) — fill in once
//! the crate compiles.

use base_crypto::cost_model::CostDuration;
use std::ops::Deref;
use storage::arena::Sp;
use storage::db::InMemoryDB;
use storage::state_translation::TypedTranslationState;
use v8_to_v9_state_translation::StateTranslationTable;

const TEST_NETWORK_ID: &str = "test-network";

#[test]
fn empty_state_round_trips() {
    let v8 = ledger_v8::structure::LedgerState::<InMemoryDB>::new(TEST_NETWORK_ID);
    let v9 = translate_to_completion(v8.clone());
    assert_eq!(v9.network_id, v8.network_id);
    assert_eq!(v9.reserve_pool, v8.reserve_pool);
    assert_eq!(v9.locked_pool, v8.locked_pool);
    assert_eq!(v9.block_reward_pool, v8.block_reward_pool);
    assert_eq!(v9.zswap.first_free, v8.zswap.first_free);
    // New parameter field should be populated to a sensible default.
    assert_eq!(
        v9.parameters.min_block_price,
        ledger_v9::structure::INITIAL_PARAMETERS.min_block_price,
    );
}

// TODO mirror the v6→v7 test set:
// - contract_preserved_after_translation
// - contract_entry_points_preserved_verifier_keys_wiped (NOTE: v8→v9 does NOT
//   wipe verifier keys, unlike v6→v7. Check whether that's intentional.)
// - multiple_contracts_translated
// - contract_with_empty_operations
// - contract_balance_preserved
// - maintenance_authority_preserved (also check committee becomes Schnorr-wrapped)
// - incremental_translation_requires_multiple_iterations
// - translation_is_deterministic
// - bridge_receiving_preserved (NEW — exercises the SizeAnn → NightAnn path)

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
