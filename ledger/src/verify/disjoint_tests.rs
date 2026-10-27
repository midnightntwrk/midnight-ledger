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

//! `disjoint_check` rejects exactly what the per-entry checks it replaces
//! rejected, plus registrations and committee signatures reached twice,
//! however entries are shared.

use super::*;
use crate::structure::cost_equivalence_tests::{Entries, Recipe, build, recipe, transaction};
use base_crypto::hash::HashOutput;
use proptest::prelude::*;
use std::sync::Arc;
use storage::db::InMemoryDB;
use storage::storage::{Array, HashMap};
use zswap::{Input, Offer, Output, Transient};

type D = InMemoryDB;
type Stx = StandardTransaction<(), (), Pedersen, D>;

/// Per offer, its inputs, outputs and transients, and how it shares the
/// previous offer's inputs: 0 not at all, 1 the same array, 2 that array plus
/// one more entry.
type ShieldedRecipe = Vec<(Entries, Entries, Entries, u8)>;

fn shielded() -> impl Strategy<Value = ShieldedRecipe> {
    let entries = || prop::collection::vec((0u8..6, 0u8..4), 0..4);
    prop::collection::vec((entries(), entries(), entries(), 0u8..3), 0..4)
}

fn input(kind: u8) -> Input<(), D> {
    Input {
        nullifier: Nullifier(HashOutput([kind; 32])),
        value_commitment: Default::default(),
        contract_address: None,
        merkle_tree_root: Default::default(),
        proof: Arc::new(()),
    }
}

fn offers(r: &[(Entries, Entries, Entries, u8)]) -> Vec<Offer<(), D>> {
    let mut out: Vec<Offer<(), D>> = Vec::new();
    for (inputs, outputs, transients, sharing) in r {
        let mut inputs: Array<_, D> = build(inputs, input).into();
        if let (Some(prev), 1 | 2) = (out.last(), sharing) {
            inputs = prev.inputs.clone();
            if *sharing == 2 {
                inputs = inputs.push(input(7));
            }
        }
        let outputs = build(outputs, |kind| Output {
            coin_com: Commitment(HashOutput([kind; 32])),
            value_commitment: Default::default(),
            contract_address: None,
            ciphertext: None,
            proof: Arc::new(()),
        });
        // Transients of kinds 2j and 2j + 1 share their input but not their
        // output, so input collisions are not always output collisions.
        let transient = build(transients, |kind| Transient {
            nullifier: Nullifier(HashOutput([kind / 2; 32])),
            coin_com: Commitment(HashOutput([kind / 2; 32])),
            value_commitment_input: Default::default(),
            value_commitment_output: match kind % 2 {
                0 => Default::default(),
                _ => Pedersen(EmbeddedGroupAffine::generator()),
            },
            contract_address: None,
            ciphertext: None,
            proof_input: Arc::new(()),
            proof_output: Arc::new(()),
        });
        out.push(Offer {
            inputs,
            outputs: outputs.into(),
            transient: transient.into(),
            deltas: Array::new(),
        });
    }
    out
}

#[derive(Debug, Clone, Copy)]
enum Intents {
    None,
    Distinct,
    FirstRepeated,
}

/// With `actions_only`, intents keep only their actions, whose committee
/// signatures are otherwise mostly masked by repeated inputs or
/// registrations.
fn standard(r: &Recipe, shielded: &ShieldedRecipe, intents: Intents, actions_only: bool) -> Stx {
    let Transaction::Standard(mut stx) = transaction(r) else {
        unreachable!()
    };
    if let Intents::None = intents {
        stx.intents = HashMap::new();
    }
    if actions_only {
        for (segment, mut intent) in stx.intents().collect::<Vec<_>>() {
            intent.guaranteed_unshielded_offer = None;
            intent.fallible_unshielded_offer = None;
            intent.dust_actions = None;
            stx.intents = stx.intents.insert(segment, intent);
        }
    }
    let shielded = if actions_only { &[][..] } else { &shielded[..] };
    let mut offers = offers(shielded).into_iter();
    stx.guaranteed_coins = offers.next().map(Sp::new);
    for (segment, offer) in (1u16..).zip(offers) {
        stx.fallible_coins = stx.fallible_coins.insert(segment, offer);
    }
    if let Intents::FirstRepeated = intents {
        let first = stx.intents.get(&1).map(|i| (*i).clone()).unwrap();
        stx.intents = stx.intents.insert(100, first);
    }
    stx
}

/// The checks `disjoint_check` replaces, over every entry as reached, plus
/// registration uniqueness by node and committee signature uniqueness.
fn accepts_per_entry(stx: &Stx) -> bool {
    let mut shielded_inputs = BTreeSet::new();
    let mut shielded_outputs = BTreeSet::new();
    let offers = stx
        .guaranteed_coins
        .iter()
        .map(|o| (**o).clone())
        .chain(stx.fallible_coins.values());
    for offer in offers {
        let mut inputs: BTreeSet<_> = offer.inputs.iter_deref().cloned().collect();
        inputs.extend(offer.transient.iter_deref().map(Transient::as_input));
        let mut outputs: BTreeSet<_> = offer.outputs.iter_deref().cloned().collect();
        outputs.extend(offer.transient.iter_deref().map(Transient::as_output));
        if !shielded_inputs.is_disjoint(&inputs) || !shielded_outputs.is_disjoint(&outputs) {
            return false;
        }
        shielded_inputs.extend(inputs);
        shielded_outputs.extend(outputs);
    }
    let mut unshielded_inputs = BTreeSet::new();
    let mut registrations = Vec::new();
    let mut committee_signatures = Vec::new();
    for (_, intent) in stx.intents() {
        for action in intent.actions.iter_deref() {
            if let ContractAction::Maintain(upd) = action {
                committee_signatures.extend(upd.signatures.iter_deref().map(|sig| sig.1.clone()));
            }
        }
        let offers = [
            intent.guaranteed_unshielded_offer.clone(),
            intent.fallible_unshielded_offer.clone(),
        ];
        for offer in offers.into_iter().flatten() {
            let ins = Vec::from(&offer.inputs);
            if !no_duplicates(&ins) {
                return false;
            }
            let ins: BTreeSet<_> = ins.into_iter().collect();
            if !unshielded_inputs.is_disjoint(&ins) {
                return false;
            }
            unshielded_inputs.extend(ins);
        }
        for dust_actions in intent.dust_actions.iter() {
            registrations.extend(dust_actions.registrations.iter().map(|reg| reg.hash()));
        }
    }
    no_duplicates(&registrations) && no_duplicates(&committee_signatures)
}

/// Actions shared by many segments are counted once per distinct node, not
/// walked once per segment.
#[test]
fn actions_shared_by_many_segments_are_checked_quickly() {
    let actions: Array<ContractAction<(), D>, D> = (0..4000)
        .map(|counter| {
            ContractAction::Maintain(MaintenanceUpdate {
                address: Default::default(),
                updates: Array::new(),
                counter,
                signatures: Array::new(),
            })
        })
        .collect();
    let intent = Intent::<(), (), Pedersen, D> {
        guaranteed_unshielded_offer: None,
        fallible_unshielded_offer: None,
        actions,
        dust_actions: None,
        ttl: Timestamp::from_secs(0),
        binding_commitment: Default::default(),
    };
    let stx: Stx = StandardTransaction {
        network_id: "local-test".into(),
        intents: (1..=4000).fold(HashMap::new(), |m, seg| m.insert(seg, intent.clone())),
        guaranteed_coins: None,
        fallible_coins: HashMap::new(),
        binding_randomness: Default::default(),
    };
    storage::test_utilities::assert_completes_within(
        std::time::Duration::from_secs(5),
        move || assert!(stx.disjoint_check().is_ok()),
    );
}

proptest! {
    #[test]
    fn disjoint_check_matches_per_entry_checks(
        r in recipe(),
        s in shielded(),
        // Intents are usually rejected for their own entries; drop them half
        // the time to test the shielded checks alone.
        intents in prop_oneof![
            2 => Just(Intents::None),
            1 => Just(Intents::Distinct),
            1 => Just(Intents::FirstRepeated),
        ],
        actions_only in any::<bool>(),
    ) {
        let stx = standard(&r, &s, intents, actions_only);
        prop_assert_eq!(stx.disjoint_check().is_ok(), accepts_per_entry(&stx));
    }
}
