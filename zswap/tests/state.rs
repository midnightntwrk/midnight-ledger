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

use std::ops::Deref;

use coin_structure::coin::{Info as CoinInfo, ShieldedTokenType};
use coin_structure::contract::ContractAddress;
use midnight_zswap::keys::SecretKeys;
use midnight_zswap::ledger::State;
use midnight_zswap::local;
use midnight_zswap::{Offer, Output as ZswapOutput};
use rand::rngs::{OsRng, StdRng};
use rand::{Rng, SeedableRng};
use storage::db::{DB, InMemoryDB};
use storage::storage::Map;
use transient_crypto::proofs::ProofPreimage;

#[test]
fn coin_receiving() {
    let mut rng = StdRng::seed_from_u64(0x42);
    let mut state = local::State::<InMemoryDB>::new();
    let keys = SecretKeys::from_rng_seed(&mut rng);
    let coin = CoinInfo {
        nonce: OsRng.r#gen(),
        type_: ShieldedTokenType(OsRng.r#gen()),
        value: OsRng.r#gen(),
    };
    let output = ZswapOutput::new(
        &mut rng,
        &coin,
        None,
        &keys.coin_public_key(),
        Some(keys.enc_public_key()),
    )
    .unwrap();
    let offer = Offer {
        inputs: vec![].into(),
        outputs: vec![output].into(),
        transient: vec![].into(),
        deltas: vec![].into(),
    };
    state = state.apply(&keys, &offer);
    assert_eq!(
        state
            .coins
            .iter()
            .map(|(_, c)| CoinInfo::from(&*c))
            .collect::<std::vec::Vec<_>>(),
        vec![coin]
    );
}

#[test]
fn state_filtering() {
    let mut rng = StdRng::seed_from_u64(0x42);
    let mut state: State<InMemoryDB> = State::new();

    fn apply_random_offer<D: DB>(
        state: &mut State<D>,
        rng: &mut StdRng,
    ) -> ZswapOutput<ProofPreimage, D> {
        let coin = CoinInfo {
            nonce: OsRng.r#gen(),
            type_: ShieldedTokenType(OsRng.r#gen()),
            value: OsRng.r#gen(),
        };
        let address = ContractAddress(OsRng.r#gen());
        let output = ZswapOutput::new_contract_owned(rng, &coin, None, address).unwrap();
        let offer = Offer {
            inputs: vec![].into(),
            outputs: vec![output.clone()].into(),
            transient: vec![].into(),
            deltas: vec![].into(),
        };

        *state = state.try_apply(&offer, None).unwrap().0;
        output
    }

    for _ in 0..2 {
        apply_random_offer(&mut state, &mut rng);
    }
    let output = apply_random_offer(&mut state, &mut rng);

    let reference_tree = state.coin_coms.collapse(0, 1);

    assert_eq!(
        state.filter(&[*(output.contract_address.clone()).unwrap().deref()]),
        reference_tree
    );

    for _ in 0..2 {
        apply_random_offer(&mut state, &mut rng);
    }

    let mut reference_tree = state.coin_coms.collapse(0, 1);
    reference_tree = reference_tree.collapse(3, 4);

    assert_eq!(
        state.filter(&[*(output.contract_address.clone()).unwrap().deref()]),
        reference_tree
    );
}

/// Verifies that `try_apply` with new outputs succeeds on a state whose
/// Merkle tree has been filtered (collapsed) — as the indexer would provide
/// for per-contract tracking — when `whitelist` is `None`.
///
/// This test documents expected behavior: `first_free` always points beyond
/// the collapsed region, so `update_hash(first_free, ...)` reaches a Stub.
#[test]
fn try_apply_output_on_filtered_state_no_whitelist() {
    let mut rng = StdRng::seed_from_u64(0x42);
    let addr_a = ContractAddress(OsRng.r#gen());
    let addr_b = ContractAddress(OsRng.r#gen());
    let mut state: State<InMemoryDB> = State::new();

    // Insert outputs for two different contracts
    for (i, addr) in [(0, addr_a), (1, addr_b), (2, addr_a), (3, addr_b), (4, addr_a)] {
        let coin = CoinInfo {
            nonce: OsRng.r#gen(),
            type_: ShieldedTokenType(OsRng.r#gen()),
            value: (i + 1) as u128 * 100,
        };
        let output = ZswapOutput::new_contract_owned(&mut rng, &coin, None, addr).unwrap();
        let offer = Offer {
            inputs: vec![].into(),
            outputs: vec![output].into(),
            transient: vec![].into(),
            deltas: vec![].into(),
        };
        state = state.try_apply(&offer, None).unwrap().0;
    }
    state = state.post_block_update(Default::default());

    // Filter for contract A only (positions 0, 2, 4 retained; 1, 3 collapsed)
    let filtered_tree = state.filter(&[addr_a]);
    let filtered_state = State {
        coin_coms: filtered_tree,
        coin_coms_set: state.coin_coms_set.clone(),
        first_free: state.first_free,
        nullifiers: state.nullifiers.clone(),
        past_roots: state.past_roots.clone(),
    };

    // Apply a new output to the filtered state with whitelist=None
    let new_coin = CoinInfo {
        nonce: OsRng.r#gen(),
        type_: ShieldedTokenType(OsRng.r#gen()),
        value: 999,
    };
    let new_output = ZswapOutput::new_contract_owned(&mut rng, &new_coin, None, addr_a).unwrap();
    let new_offer = Offer {
        inputs: vec![].into(),
        outputs: vec![new_output].into(),
        transient: vec![].into(),
        deltas: vec![].into(),
    };

    // With whitelist=None, on_whitelist always returns true, so collapse is NOT
    // called in apply_output. update_hash(first_free, ...) should succeed.
    let result = filtered_state.try_apply(&new_offer, None);
    assert!(result.is_ok(), "try_apply on filtered state with whitelist=None should succeed");
}

/// Verifies that `try_apply` with new outputs succeeds on a filtered state
/// when a whitelist is provided containing the contract address.
///
/// When the whitelist contains the output's contract address, `on_whitelist`
/// returns true and collapse is NOT called — the output is retained in the
/// per-contract tree.
#[test]
fn try_apply_output_on_filtered_state_with_matching_whitelist() {
    let mut rng = StdRng::seed_from_u64(0x42);
    let addr_a = ContractAddress(OsRng.r#gen());
    let addr_b = ContractAddress(OsRng.r#gen());
    let mut state: State<InMemoryDB> = State::new();

    // Build up state with interleaved contract outputs
    for addr in [addr_a, addr_b, addr_a, addr_b] {
        let coin = CoinInfo {
            nonce: OsRng.r#gen(),
            type_: ShieldedTokenType(OsRng.r#gen()),
            value: OsRng.r#gen(),
        };
        let output = ZswapOutput::new_contract_owned(&mut rng, &coin, None, addr).unwrap();
        let offer = Offer {
            inputs: vec![].into(),
            outputs: vec![output].into(),
            transient: vec![].into(),
            deltas: vec![].into(),
        };
        state = state.try_apply(&offer, None).unwrap().0;
    }
    state = state.post_block_update(Default::default());

    // Filter for contract A
    let filtered_tree = state.filter(&[addr_a]);
    let filtered_state = State {
        coin_coms: filtered_tree,
        coin_coms_set: state.coin_coms_set.clone(),
        first_free: state.first_free,
        nullifiers: state.nullifiers.clone(),
        past_roots: state.past_roots.clone(),
    };

    // Create whitelist containing contract A
    let whitelist: Map<ContractAddress, ()> = Map::new().insert(addr_a, ());

    // Apply new contract-A output with matching whitelist
    let new_coin = CoinInfo {
        nonce: OsRng.r#gen(),
        type_: ShieldedTokenType(OsRng.r#gen()),
        value: 500,
    };
    let new_output = ZswapOutput::new_contract_owned(&mut rng, &new_coin, None, addr_a).unwrap();
    let new_offer = Offer {
        inputs: vec![].into(),
        outputs: vec![new_output].into(),
        transient: vec![].into(),
        deltas: vec![].into(),
    };

    let result = filtered_state.try_apply(&new_offer, Some(whitelist));
    assert!(
        result.is_ok(),
        "try_apply on filtered state with matching whitelist should succeed"
    );
}

/// Demonstrates that `try_apply` panics when `first_free` incorrectly points
/// into a collapsed region of the Merkle tree. This reproduces the crash
/// mechanism observed in the WASM `ZswapChainState.tryApply()` bug — where
/// the client-side state has a collapsed tree that includes position
/// `first_free`, causing `update_hash` to panic with
/// "Attempted to insert into collapsed portion of Merkle tree!"
///
/// In production, this can happen when the deserialized state from the indexer
/// has the Merkle tree collapsed more aggressively than `filter()` would do
/// (e.g., the entire tree is collapsed including the `first_free` position).
///
/// Related: https://github.com/midnightntwrk/midnight-ledger/issues/179
#[test]
#[should_panic = "Attempted to insert into collapsed portion of Merkle tree!"]
fn try_apply_panics_when_first_free_in_collapsed_tree() {
    let mut rng = StdRng::seed_from_u64(0x42);
    let addr = ContractAddress(OsRng.r#gen());
    let mut state: State<InMemoryDB> = State::new();

    // Insert several outputs
    for _ in 0..5 {
        let coin = CoinInfo {
            nonce: OsRng.r#gen(),
            type_: ShieldedTokenType(OsRng.r#gen()),
            value: OsRng.r#gen(),
        };
        let output = ZswapOutput::new_contract_owned(&mut rng, &coin, None, addr).unwrap();
        let offer = Offer {
            inputs: vec![].into(),
            outputs: vec![output].into(),
            transient: vec![].into(),
            deltas: vec![].into(),
        };
        state = state.try_apply(&offer, None).unwrap().0;
    }
    state = state.post_block_update(Default::default());

    // Simulate a broken deserialized state: collapse the ENTIRE used range
    // INCLUDING first_free's position by collapsing 0..first_free (one past
    // what filter() would do).
    let bad_tree = state.coin_coms.collapse(0, state.first_free);
    let bad_state = State {
        coin_coms: bad_tree,
        coin_coms_set: state.coin_coms_set.clone(),
        first_free: state.first_free,
        nullifiers: state.nullifiers.clone(),
        past_roots: state.past_roots.clone(),
    };

    // Attempt to apply a new output — this crashes because update_hash
    // tries to insert at first_free, which is now in a collapsed region.
    let new_coin = CoinInfo {
        nonce: OsRng.r#gen(),
        type_: ShieldedTokenType(OsRng.r#gen()),
        value: 100,
    };
    let new_output = ZswapOutput::new_contract_owned(&mut rng, &new_coin, None, addr).unwrap();
    let new_offer = Offer {
        inputs: vec![].into(),
        outputs: vec![new_output].into(),
        transient: vec![].into(),
        deltas: vec![].into(),
    };

    // This should panic with "Attempted to insert into collapsed portion of Merkle tree!"
    let _ = bad_state.try_apply(&new_offer, None);
}

/// Verifies that `try_apply` with a whitelist works correctly when an output
/// is NOT on the whitelist. The output gets inserted then immediately collapsed,
/// and a subsequent insert at the next position succeeds.
///
/// This tests the insert-then-collapse pattern in `apply_output` when
/// `!on_whitelist(...)` is true, which is the normal code path for per-contract
/// state tracking.
#[test]
fn try_apply_with_non_matching_whitelist_collapses_outputs() {
    let mut rng = StdRng::seed_from_u64(0x42);
    let addr_a = ContractAddress(OsRng.r#gen());
    let addr_b = ContractAddress(OsRng.r#gen());
    let mut state: State<InMemoryDB> = State::new();
    state = state.post_block_update(Default::default());

    // Create whitelist for contract A
    let whitelist: Map<ContractAddress, ()> = Map::new().insert(addr_a, ());

    // Apply an output for contract B (not on whitelist) — should be collapsed
    let coin_b = CoinInfo {
        nonce: OsRng.r#gen(),
        type_: ShieldedTokenType(OsRng.r#gen()),
        value: 200,
    };
    let output_b = ZswapOutput::new_contract_owned(&mut rng, &coin_b, None, addr_b).unwrap();
    let offer_b = Offer {
        inputs: vec![].into(),
        outputs: vec![output_b].into(),
        transient: vec![].into(),
        deltas: vec![].into(),
    };
    let (state2, _) = state.try_apply(&offer_b, Some(whitelist.clone())).unwrap();

    // Now apply an output for contract A (on whitelist) — should succeed
    let coin_a = CoinInfo {
        nonce: OsRng.r#gen(),
        type_: ShieldedTokenType(OsRng.r#gen()),
        value: 300,
    };
    let output_a = ZswapOutput::new_contract_owned(&mut rng, &coin_a, None, addr_a).unwrap();
    let offer_a = Offer {
        inputs: vec![].into(),
        outputs: vec![output_a].into(),
        transient: vec![].into(),
        deltas: vec![].into(),
    };
    let result = state2.try_apply(&offer_a, Some(whitelist));
    assert!(
        result.is_ok(),
        "try_apply should succeed even after prior outputs were collapsed by whitelist filtering"
    );
}
