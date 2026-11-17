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

//! Entry counts come from size annotations, which a shared encoding can push
//! towards `usize::MAX`. Costs must grow with every count, or a wrapped sum
//! would price a huge transaction as a small one.

use crate::structure::*;
use coin_structure::coin::Nullifier;
use proptest::prelude::*;
use std::sync::Arc;
use storage::db::InMemoryDB;
use storage::storage::{Array, HashMap};

type D = InMemoryDB;
type Tx = Transaction<(), (), Pedersen, D>;

/// The length of a generated array: none, one entry, or `c * 16^(k-1)`
/// entries shared from few nodes, up to just below `2^64`. Two arrays of
/// `2^63` sum to exactly `2^64`, so that length is common.
type Len = Option<(u32, u32)>;

fn len() -> impl Strategy<Value = Len> {
    prop_oneof![
        1 => Just(None),
        1 => Just(Some((1, 1))),
        1 => (2u32..16, 2u32..16).prop_map(Some),
        1 => (2u32..16).prop_map(|c| Some((16, c))),
        2 => Just(Some((16, 8))),
    ]
}

fn array<V: Storable<D>>(len: Len, value: V) -> Array<V, D> {
    match len {
        None => Array::new(),
        Some((1, _)) => vec![value].into(),
        Some((k, c)) => Array::shared_dense_scaled(k, c, value),
    }
}

/// Every count pricing sums: per zswap offer (guaranteed, then one fallible)
/// its inputs, outputs, transients and deltas, and per unshielded offer of an
/// intent (guaranteed, then fallible) its signatures.
const SITES: usize = 10;

fn transaction(lens: &[Len; SITES]) -> Tx {
    let offer = |l: &[Len]| zswap::Offer::<(), D> {
        inputs: array(
            l[0],
            zswap::Input {
                nullifier: Nullifier(HashOutput([1; 32])),
                value_commitment: Default::default(),
                contract_address: None,
                merkle_tree_root: Default::default(),
                proof: Arc::new(()),
            },
        ),
        outputs: array(
            l[1],
            zswap::Output {
                coin_com: Commitment(HashOutput([2; 32])),
                value_commitment: Default::default(),
                contract_address: None,
                ciphertext: None,
                proof: Arc::new(()),
            },
        ),
        transient: array(
            l[2],
            zswap::Transient {
                nullifier: Nullifier(HashOutput([3; 32])),
                coin_com: Commitment(HashOutput([3; 32])),
                value_commitment_input: Default::default(),
                value_commitment_output: Default::default(),
                contract_address: None,
                ciphertext: None,
                proof_input: Arc::new(()),
                proof_output: Arc::new(()),
            },
        ),
        deltas: array(
            l[3],
            zswap::Delta {
                token_type: Default::default(),
                value: 1,
            },
        ),
    };
    let unshielded = |l: Len| {
        Some(Sp::new(UnshieldedOffer::<(), D> {
            inputs: Array::new(),
            outputs: Array::new(),
            signatures: array(l, ()),
        }))
    };
    let intent = Intent::<(), (), Pedersen, D> {
        guaranteed_unshielded_offer: unshielded(lens[8]),
        fallible_unshielded_offer: unshielded(lens[9]),
        actions: Array::new(),
        dust_actions: None,
        ttl: Timestamp::from_secs(0),
        binding_commitment: Default::default(),
    };
    Transaction::Standard(StandardTransaction {
        network_id: "local-test".into(),
        intents: HashMap::new().insert(1, intent),
        guaranteed_coins: Some(Sp::new(offer(&lens[0..4]))),
        fallible_coins: HashMap::new().insert(1, offer(&lens[4..8])),
        binding_randomness: Default::default(),
    })
}

/// Each site with its counterpart in the other offer.
const PAIRS: [(usize, usize); 5] = [(0, 4), (1, 5), (2, 6), (3, 7), (8, 9)];

proptest! {
    // A count summed across offers must never price lower for the second
    // offer's entries; a wrapped sum does.
    #[test]
    fn entries_in_a_second_offer_never_lower_the_cost(
        pair in 0..PAIRS.len(),
        first in len(),
        second in len(),
    ) {
        let (a, b) = PAIRS[pair];
        let mut lens = [None; SITES];
        lens[a] = first;
        let one = transaction(&lens);
        lens[b] = second;
        let both = transaction(&lens);
        let model = &INITIAL_PARAMETERS.cost_model;
        let validation = |tx: &Tx| {
            let cost = tx.validation_cost(model);
            (cost.compute_time, cost.read_time)
        };
        // Application costs sum the same counts, but only once
        // `disjoint_check` has ruled out repeated entries, which bounds them.
        prop_assert!(validation(&both) >= validation(&one));
    }
}
