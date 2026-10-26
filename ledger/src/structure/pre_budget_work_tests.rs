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

//! Everything a node does with a transaction before its fee is known must
//! take work linear in its encoding, however much structure the encoding
//! shares.

use super::cost_equivalence_tests::{recipe, transaction};
use super::*;
use crate::dust::DustRegistration;
use coin_structure::coin::Nullifier;
use onchain_runtime::state::StateValue;
use onchain_runtime::transcript::Transcript;
use onchain_vm::ops::Op;
use proptest::prelude::*;
use serialize::{tagged_deserialize, tagged_serialize};
use storage::db::InMemoryDB;
use storage::storage::Array;
use storage_core::test::derefs;

type D = InMemoryDB;
type Tx = Transaction<(), (), Pedersen, D>;

/// How many levels of 16-fold sharing to apply to each array, 0 for none.
#[derive(Debug, Clone)]
struct Sharing {
    actions: u32,
    unshielded_inputs: u32,
    registrations: u32,
    zswap_inputs: u32,
    pushed_depth: u32,
}

fn sharing() -> impl Strategy<Value = Sharing> {
    (0u32..13, 0u32..13, 0u32..13, 0u32..13, 0u32..13).prop_map(|(a, u, r, z, p)| Sharing {
        actions: a,
        unshielded_inputs: u,
        registrations: r,
        zswap_inputs: z,
        pushed_depth: p,
    })
}

fn shared<V: Storable<D>>(arr: &Array<V, D>, k: u32) -> Array<V, D> {
    match (k, arr.get(0)) {
        (0, _) | (_, None) => arr.clone(),
        (k, Some(v)) => Array::shared_dense(k, v.clone()),
    }
}

fn apply(tx: Tx, s: &Sharing) -> Tx {
    let Transaction::Standard(mut stx) = tx else {
        unreachable!()
    };
    let (segment, intent) = stx
        .intents
        .iter()
        .next()
        .map(|e| (*e.0, (*e.1).clone()))
        .unwrap();
    let mut intent = intent;
    let mut value = StateValue::<D, _>::Null;
    for _ in 0..s.pushed_depth {
        value = StateValue::Array(vec![value; 16].into());
    }
    let push_call = ContractAction::Call(Sp::new(ContractCall {
        address: Default::default(),
        entry_point: onchain_runtime::state::EntryPointBuf(b"push".to_vec()),
        guaranteed_transcript: None,
        fallible_transcript: Some(Sp::new(Transcript {
            gas: RunningCost::ZERO,
            effects: Default::default(),
            program: vec![Op::Push {
                storage: false,
                value,
            }]
            .into(),
            version: None,
        })),
        communication_commitment: Default::default(),
        proof: (),
    }));
    let actions: Array<_, D> = std::iter::once(push_call)
        .chain(intent.actions.iter_deref().cloned())
        .collect();
    intent.actions = shared(&actions, s.actions);
    if s.actions > 0 {
        // Keep the pushed value reachable alongside the shared actions.
        intent.actions = intent.actions.push(actions.get(0).unwrap().clone());
    }
    if let Some(offer) = &intent.guaranteed_unshielded_offer {
        let mut offer = (**offer).clone();
        offer.inputs = shared(&offer.inputs, s.unshielded_inputs);
        intent.guaranteed_unshielded_offer = Some(Sp::new(offer));
    }
    if let Some(dust) = &intent.dust_actions {
        let mut dust = (**dust).clone();
        let regs: Array<DustRegistration<(), D>, D> = shared(&dust.registrations, s.registrations);
        dust.registrations = regs;
        intent.dust_actions = Some(Sp::new(dust));
    }
    stx.intents = stx.intents.insert(segment, intent);
    if s.zswap_inputs > 0 {
        let input = zswap::Input::<(), D> {
            nullifier: Nullifier(HashOutput([1; 32])),
            value_commitment: Default::default(),
            contract_address: None,
            merkle_tree_root: Default::default(),
            proof: std::sync::Arc::new(()),
        };
        stx.guaranteed_coins = Some(Sp::new(zswap::Offer {
            inputs: Array::shared_dense(s.zswap_inputs, input),
            outputs: Array::new(),
            transient: Array::new(),
            deltas: Array::new(),
        }));
    }
    Transaction::Standard(stx)
}

/// Bytes, and `Sp` dereferences to decode then price the decoded transaction,
/// plus to price `tx` itself, so pricing is measured even where decoding
/// rejects. Measured on a fresh thread so a regression fails by timeout
/// instead of hanging.
fn measure(tx: &Tx) -> (u64, u64) {
    let mut bytes = Vec::new();
    tagged_serialize(tx, &mut bytes).unwrap();
    let len = bytes.len() as u64;
    let tx = tx.clone();
    let work = storage::test_utilities::assert_completes_within(
        std::time::Duration::from_secs(20),
        move || {
            let before = derefs();
            if let Ok(decoded) = tagged_deserialize::<Tx>(&mut &bytes[..]) {
                let _ = decoded.fees(&INITIAL_PARAMETERS, false);
            }
            let _ = tx.fees(&INITIAL_PARAMETERS, false);
            derefs() - before
        },
    );
    (len, work)
}

// Linear work stays within a few dereferences per byte; a walk over shared
// entries exceeds any constant, or the time limit, by orders of magnitude.
const DEREFS_PER_BYTE: u64 = 32;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn pre_budget_work_is_linear_in_encoding(r in recipe(), s in sharing()) {
        let (len, work) = measure(&apply(transaction(&r), &s));
        prop_assert!(work <= DEREFS_PER_BYTE * len, "{work} dereferences for {len} bytes");
    }
}
