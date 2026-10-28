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

use super::transactions::{generated, recipe};
use crate::structure::*;
use crate::verify::WellFormedStrictness;
use base_crypto::fab::AlignedValue;
use coin_structure::coin::Nullifier;
use onchain_runtime::state::StateValue;
use onchain_runtime::transcript::Transcript;
use onchain_vm::ops::Op;
use proptest::prelude::*;
use serialize::{tagged_deserialize, tagged_serialize};
use storage::db::InMemoryDB;
use storage::storage::{Array, HashMap};
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
    // Mostly unshared, so that some transactions are within budget.
    let level = || prop_oneof![3 => Just(0u32), 1 => 1u32..13];
    (level(), level(), level(), level(), level()).prop_map(|(a, u, r, z, p)| Sharing {
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

fn with_sharing(tx: Tx, s: &Sharing) -> Tx {
    let Transaction::Standard(mut stx) = tx else {
        unreachable!()
    };
    let (segment, mut intent) = stx
        .intents
        .iter()
        .next()
        .map(|e| (*e.0, (*e.1).clone()))
        .unwrap();
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
        dust.registrations = shared(&dust.registrations, s.registrations);
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

/// Decodes, checks and prices `tx`'s encoding, and prices `tx` itself in case
/// decoding rejects it; on a fresh thread, so a regression fails by timeout.
fn measure(tx: &Tx) -> Measured {
    let mut bytes = Vec::new();
    tagged_serialize(tx, &mut bytes).unwrap();
    let len = bytes.len() as u64;
    let tx = tx.clone();
    storage::test::assert_completes_within(std::time::Duration::from_secs(20), move || {
        let start = std::time::Instant::now();
        let before = derefs();
        let mut priced = false;
        if let Ok(decoded) = tagged_deserialize::<Tx>(&mut &bytes[..]) {
            priced = decoded.fees(&INITIAL_PARAMETERS, true).is_ok();
            // An attacker funds any fee it can pass, so those transactions
            // reach the checks after balancing.
            let strictness = WellFormedStrictness {
                enforce_balancing: !priced,
                ..Default::default()
            };
            let state = LedgerState::<D>::new("local-test");
            let _ = decoded.well_formed(&state, strictness, Timestamp::from_secs(0));
        }
        let _ = tx.fees(&INITIAL_PARAMETERS, false);
        Measured {
            len,
            derefs: derefs() - before,
            time: start.elapsed(),
            priced,
        }
    })
}

struct Measured {
    len: u64,
    derefs: u64,
    time: std::time::Duration,
    /// Within its fee budget, so the checks after balancing ran too.
    priced: bool,
}

impl Measured {
    fn check(&self) -> Result<(), TestCaseError> {
        let Measured {
            len, derefs, time, ..
        } = *self;
        prop_assert!(
            derefs <= DEREFS_PER_BYTE * len,
            "{derefs} dereferences for {len} bytes"
        );
        let limit = MIN_TIME.max(TIME_PER_BYTE * len as u32);
        prop_assert!(time <= limit, "{time:?} for {len} bytes");
        Ok(())
    }
}

// Linear work stays within a few dereferences per byte; a walk over shared
// entries exceeds any constant, or the time limit, by orders of magnitude.
const DEREFS_PER_BYTE: u64 = 32;
// Dereferences miss work over large values, such as copying entry points, so
// time is bounded too, loosely: tests run unoptimized, with slow hashing.
const TIME_PER_BYTE: std::time::Duration = std::time::Duration::from_micros(100);
const MIN_TIME: std::time::Duration = std::time::Duration::from_millis(200);

/// Shapes that make a walk treating shared nodes as a tree super-linear: one
/// intent in many segments, distinct actions sharing an array of updates,
/// deploys sharing a state of maps that share a trie, calls with large entry
/// points in a shared intent, and a program of `noop 0`, which adds no public
/// inputs, shared `levels` deep.
#[derive(Debug, Clone)]
enum Shape {
    SharedIntent {
        segments: u16,
        actions: u32,
        updates: u32,
    },
    SharedState {
        deploys: u32,
        maps: u32,
        entries: u32,
    },
    EntryPoints {
        segments: u16,
        calls: u32,
        len: usize,
    },
    Noops {
        levels: u32,
    },
}

impl Shape {
    fn scaled(&self, k: u32) -> Shape {
        match *self {
            Shape::SharedIntent {
                segments,
                actions,
                updates,
            } => Shape::SharedIntent {
                segments: segments * k as u16,
                actions: actions * k,
                updates: updates * k,
            },
            Shape::SharedState {
                deploys,
                maps,
                entries,
            } => Shape::SharedState {
                deploys: deploys * k,
                maps: maps * k,
                entries: entries * k,
            },
            Shape::EntryPoints {
                segments,
                calls,
                len,
            } => Shape::EntryPoints {
                segments: segments * k as u16,
                calls: calls * k,
                len: len * k as usize,
            },
            Shape::Noops { levels } => Shape::Noops { levels: levels * k },
        }
    }
}

fn shape() -> impl Strategy<Value = Shape> {
    prop_oneof![
        (1u16..6, 1u32..6, 1u32..6).prop_map(|(segments, actions, updates)| {
            Shape::SharedIntent {
                segments,
                actions,
                updates,
            }
        }),
        (1u32..4, 1u32..4, 1u32..8).prop_map(|(deploys, maps, entries)| {
            Shape::SharedState {
                deploys,
                maps,
                entries,
            }
        }),
        (1u16..6, 1u32..2, 1usize..256).prop_map(|(segments, calls, len)| {
            Shape::EntryPoints {
                segments,
                calls,
                len,
            }
        }),
        // Scaled 8×, it holds 16^8 ops.
        Just(Shape::Noops { levels: 1 }),
    ]
}

fn in_segments(segments: u16, actions: Vec<ContractAction<(), D>>) -> Tx {
    let intent = Intent::<(), (), Pedersen, D> {
        guaranteed_unshielded_offer: None,
        fallible_unshielded_offer: None,
        actions: actions.into(),
        dust_actions: None,
        ttl: Timestamp::from_secs(0),
        binding_commitment: Default::default(),
    };
    Transaction::Standard(StandardTransaction {
        network_id: "local-test".into(),
        intents: (1..=segments).fold(HashMap::new(), |m, seg| m.insert(seg, intent.clone())),
        guaranteed_coins: None,
        fallible_coins: HashMap::new(),
        binding_randomness: Default::default(),
    })
}

impl Shape {
    fn transaction(&self) -> Tx {
        match *self {
            Shape::SharedIntent {
                segments,
                actions,
                updates,
            } => {
                let updates: Array<SingleUpdate, D> = (0..updates)
                    .map(|i| {
                        SingleUpdate::VerifierKeyRemove(
                            EntryPointBuf(i.to_le_bytes().to_vec()),
                            ContractOperationVersion::V3,
                        )
                    })
                    .collect();
                let actions = (0..actions)
                    .map(|counter| {
                        ContractAction::Maintain(MaintenanceUpdate {
                            address: Default::default(),
                            updates: updates.clone(),
                            counter,
                            signatures: Array::new(),
                        })
                    })
                    .collect();
                in_segments(segments, actions)
            }
            Shape::SharedState {
                deploys,
                maps,
                entries,
            } => {
                type M = HashMap<AlignedValue, StateValue<D>, D>;
                let base: M = (0..entries).fold(HashMap::new(), |m, i| {
                    m.insert(AlignedValue::from(i), StateValue::Null)
                });
                let outer: M = (0..maps).fold(HashMap::new(), |m, j| {
                    let map = base.insert(AlignedValue::from(entries + j), StateValue::Null);
                    m.insert(AlignedValue::from(j), StateValue::Map(map))
                });
                let state = ContractState::new(
                    StateValue::Map(outer),
                    HashMap::new(),
                    ContractMaintenanceAuthority::default(),
                );
                let actions = (0..deploys)
                    .map(|nonce| {
                        ContractAction::Deploy(Sp::new(ContractDeploy {
                            initial_state: state.clone(),
                            nonce: HashOutput([nonce as u8; 32]),
                        }))
                    })
                    .collect();
                in_segments(1, actions)
            }
            Shape::EntryPoints {
                segments,
                calls,
                len,
            } => {
                let transcript = Sp::new(Transcript {
                    gas: RunningCost::ZERO,
                    effects: Default::default(),
                    program: vec![Op::Noop { n: 1 }].into(),
                    version: None,
                });
                let actions = (0..calls)
                    .map(|i| {
                        let mut ep = vec![0xaa; len];
                        ep.extend(i.to_le_bytes());
                        ContractAction::Call(Sp::new(ContractCall {
                            address: Default::default(),
                            entry_point: EntryPointBuf(ep),
                            guaranteed_transcript: None,
                            fallible_transcript: Some(transcript.clone()),
                            communication_commitment: Default::default(),
                            proof: (),
                        }))
                    })
                    .collect();
                in_segments(segments, actions)
            }
            Shape::Noops { levels } => {
                let transcript = Sp::new(Transcript {
                    gas: RunningCost::ZERO,
                    effects: Default::default(),
                    program: Array::shared_dense(levels, Op::Noop { n: 0 }),
                    version: None,
                });
                let call = ContractAction::Call(Sp::new(ContractCall {
                    address: Default::default(),
                    entry_point: EntryPointBuf(b"noops".to_vec()),
                    guaranteed_transcript: None,
                    fallible_transcript: Some(transcript),
                    communication_commitment: Default::default(),
                    proof: (),
                }));
                in_segments(1, vec![call])
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn pre_budget_work_is_linear_in_encoding(r in recipe(), s in sharing(), repeats in any::<bool>()) {
        measure(&with_sharing(generated(&r, repeats), &s)).check()?;
    }

    // Linear work keeps its rate per byte as a shape grows, while a walk
    // repeated over shared structure multiplies it. A quadratic walk across
    // maps sharing a trie is too small to see at this scale next to decoding;
    // `storage` bounds that one directly.
    #[test]
    fn pre_budget_work_per_byte_does_not_grow_with_known_shapes(shape in shape()) {
        let small = measure(&shape.transaction());
        let large = measure(&shape.scaled(8).transaction());
        small.check()?;
        large.check()?;
        prop_assert!(
            large.derefs * small.len <= 3 * small.derefs * large.len,
            "{} dereferences for {} bytes, then {} for {}",
            small.derefs,
            small.len,
            large.derefs,
            large.len
        );
    }
}

// Work after the fee check is paid for, so the property above only binds if
// some generated transactions reach it.
#[test]
fn generated_transactions_include_some_within_budget() {
    use proptest::strategy::ValueTree;
    use proptest::test_runner::TestRunner;
    let mut runner = TestRunner::deterministic();
    let priced = (0..64)
        .filter(|_| {
            let r = recipe().new_tree(&mut runner).unwrap().current();
            let s = sharing().new_tree(&mut runner).unwrap().current();
            measure(&with_sharing(generated(&r, false), &s)).priced
        })
        .count();
    assert!(priced > 0, "no generated transaction was within budget");
}
