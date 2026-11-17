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

//! Transactions generated with shared and repeated entries, for the pricing
//! and validation tests.

#![allow(deprecated)]

use crate::dust::{DustActions, DustPublicKey, DustRegistration};
use crate::structure::*;
use onchain_runtime::context::Effects;
use onchain_runtime::state::{
    ContractMaintenanceAuthority, ContractState, EntryPointBuf, StateValue,
};
use onchain_runtime::transcript::Transcript;
use onchain_vm::ops::Op;
use proptest::prelude::*;
use rand::{Rng, SeedableRng, rngs::StdRng};
use storage::db::InMemoryDB;
use storage::storage::{Array, HashMap};
use transient_crypto::curve::Fr;

type D = InMemoryDB;
type Tx = Transaction<(), (), Pedersen, D>;

pub(crate) type Entries = Vec<(u8, u8)>;

#[derive(Debug, Clone)]
pub(crate) struct Recipe {
    seed: u64,
    // Per intent: actions, unshielded inputs, outputs and registrations, as
    // (kind, pick) pairs; an even kind repeats the entry at `pick`, if any.
    intents: Vec<(Entries, Entries, Entries, Entries)>,
    // Replaces the first intent's actions with 16^k copies of one action.
    shared: Option<u32>,
}

fn entries() -> impl Strategy<Value = Entries> {
    prop::collection::vec((0u8..6, 0u8..4), 0..6)
}

pub(crate) fn recipe() -> impl Strategy<Value = Recipe> {
    (
        any::<u64>(),
        prop::collection::vec((entries(), entries(), entries(), entries()), 1..3),
        prop::option::weighted(0.2, 1u32..3),
    )
        .prop_map(|(seed, intents, shared)| Recipe {
            seed,
            intents,
            shared,
        })
}

pub(crate) fn build<T: Clone>(kinds: &[(u8, u8)], mut make: impl FnMut(u8) -> T) -> Vec<T> {
    let mut out: Vec<T> = Vec::new();
    for (kind, pick) in kinds {
        let next = match out.get(*pick as usize) {
            Some(prev) if *kind % 2 == 0 => prev.clone(),
            _ => make(*kind),
        };
        out.push(next);
    }
    out
}

pub(crate) fn transaction(r: &Recipe) -> Tx {
    let mut rng = StdRng::seed_from_u64(r.seed);
    let keys: Vec<SignatureVerifyingKey> = (0..3)
        .map(|_| SignatureVerifyingKey::Schnorr(rng.r#gen()))
        .collect();
    let addresses: Vec<ContractAddress> = (0..2).map(|_| rng.r#gen()).collect();
    // Few enough that committee signatures sometimes repeat.
    let signatures: Vec<Signature> = (0..8).map(|_| Signature::Schnorr(rng.r#gen())).collect();
    let mut intents = HashMap::new();
    for (segment, (actions, inputs, outputs, regs)) in r.intents.iter().enumerate() {
        let mut action = |kind: u8| -> ContractAction<(), D> {
            let address = addresses[kind as usize % 2];
            let ep = EntryPointBuf(vec![kind % 3]);
            match kind % 3 {
                0 => {
                    let transcript = Sp::new(Transcript {
                        gas: RunningCost {
                            read_time: CostDuration::from_picoseconds(rng.gen_range(0..1000)),
                            compute_time: CostDuration::from_picoseconds(rng.gen_range(0..1000)),
                            bytes_written: rng.gen_range(0..100),
                            bytes_deleted: rng.gen_range(0..100),
                        },
                        effects: Effects {
                            shielded_mints: (0..rng.gen_range(0..4u8))
                                .fold(HashMap::new(), |m, i| m.insert(HashOutput([i; 32]), 1)),
                            unshielded_outputs: (0..rng.gen_range(0..3u8)).fold(
                                HashMap::new(),
                                |m, i| {
                                    m.insert(
                                        TokenType::Unshielded(UnshieldedTokenType(HashOutput(
                                            [i; 32],
                                        ))),
                                        1,
                                    )
                                },
                            ),
                            ..Effects::default()
                        },
                        program: vec![
                            Op::Noop {
                                n: rng.gen_range(1..5),
                            },
                            Op::Push {
                                storage: false,
                                value: StateValue::Array(
                                    vec![StateValue::Null; kind as usize].into(),
                                ),
                            },
                        ]
                        .into(),
                        version: None,
                    });
                    let guaranteed = rng.r#gen::<bool>();
                    ContractAction::Call(Sp::new(ContractCall {
                        address,
                        entry_point: ep,
                        guaranteed_transcript: guaranteed.then(|| transcript.clone()),
                        fallible_transcript: (!guaranteed || rng.r#gen()).then_some(transcript),
                        communication_commitment: Fr::from(0u64),
                        proof: (),
                    }))
                }
                1 => ContractAction::Deploy(Sp::new(ContractDeploy {
                    initial_state: ContractState::new(
                        StateValue::Array(vec![StateValue::Null; kind as usize].into()),
                        HashMap::new(),
                        ContractMaintenanceAuthority::default(),
                    ),
                    nonce: rng.r#gen(),
                })),
                _ => {
                    let updates = build(&[(kind, 0), (1, 0), (kind + 1, 1)], |k| match k % 3 {
                        0 => {
                            SingleUpdate::ReplaceAuthority(ContractMaintenanceAuthority::default())
                        }
                        1 => SingleUpdate::VerifierKeyRemove(
                            EntryPointBuf(vec![k]),
                            ContractOperationVersion::V3,
                        ),
                        _ => SingleUpdate::IrRemove(EntryPointBuf(vec![k])),
                    });
                    ContractAction::Maintain(MaintenanceUpdate {
                        address,
                        updates: updates.into(),
                        counter: 0,
                        signatures: (0..kind as u32)
                            .map(|i| SignaturesValue(i, signatures[rng.gen_range(0..8)].clone()))
                            .collect(),
                    })
                }
            }
        };
        let mut actions: Array<ContractAction<(), D>, D> = build(actions, &mut action).into();
        if let (0, Some(k)) = (segment, r.shared) {
            actions = Array::shared_dense(k, action(0));
        }
        let utxo_type = |kind: u8| {
            if kind.is_multiple_of(3) {
                NIGHT
            } else {
                UnshieldedTokenType(HashOutput([kind; 32]))
            }
        };
        let inputs = build(inputs, |kind| UtxoSpend {
            value: kind as u128,
            owner: keys[kind as usize % 3].clone(),
            type_: utxo_type(kind),
            intent_hash: IntentHash(HashOutput([kind; 32])),
            output_no: kind as u32,
        });
        let outputs = build(outputs, |kind| UtxoOutput {
            value: kind as u128,
            owner: UserAddress(HashOutput([kind; 32])),
            type_: utxo_type(kind),
        });
        let registrations = build(regs, |kind| DustRegistration::<(), D> {
            night_key: keys[kind as usize % 3].clone(),
            dust_address: (kind % 2 == 0).then(|| Sp::new(DustPublicKey(Fr::from(kind as u64)))),
            allow_fee_payment: 0,
            signature: None,
        });
        let offer = Sp::new(UnshieldedOffer {
            inputs: inputs.into(),
            outputs: outputs.into(),
            signatures: Array::new(),
        });
        intents = intents.insert(
            segment as u16 + 1,
            Intent {
                guaranteed_unshielded_offer: Some(offer.clone()),
                fallible_unshielded_offer: (segment % 2 == 1).then_some(offer),
                actions,
                dust_actions: Some(Sp::new(DustActions {
                    spends: Array::new(),
                    registrations: registrations.into(),
                    ctime: Timestamp::from_secs(0),
                })),
                ttl: Timestamp::from_secs(0),
                binding_commitment: Default::default(),
            },
        );
    }
    Transaction::Standard(StandardTransaction {
        network_id: "local-test".into(),
        intents,
        guaranteed_coins: None,
        fallible_coins: HashMap::new(),
        binding_randomness: Default::default(),
    })
}

/// The transaction `r` describes, with repeats if `repeats`. Repeated entries
/// are rejected before fees, so without them more transactions reach the work
/// after the fee check.
pub(crate) fn generated(r: &Recipe, repeats: bool) -> Tx {
    let tx = transaction(r);
    if repeats { tx } else { without_repeats(&tx) }
}

/// `tx` without the entries `disjoint_check` rejects repeating: each
/// intent's inputs and registrations are made distinct and unique to it, the
/// fallible offer stops sharing the guaranteed offer's inputs, and committee
/// signatures, which application ignores, are dropped. Actions, outputs and
/// their sharing are kept.
fn without_repeats(tx: &Tx) -> Tx {
    let Transaction::Standard(mut stx) = tx.clone() else {
        unreachable!()
    };
    for (segment, mut intent) in tx.intents() {
        if let Some(offer) = &intent.guaranteed_unshielded_offer {
            let mut offer = (**offer).clone();
            offer.inputs = offer
                .inputs
                .distinct_values()
                .into_iter()
                .map(|(input, _)| UtxoSpend {
                    output_no: input.output_no + 1000 * u32::from(segment),
                    ..(*input).clone()
                })
                .collect();
            intent.guaranteed_unshielded_offer = Some(Sp::new(offer));
        }
        if let Some(offer) = &intent.fallible_unshielded_offer {
            let mut offer = (**offer).clone();
            offer.inputs = Array::new();
            intent.fallible_unshielded_offer = Some(Sp::new(offer));
        }
        if intent
            .actions
            .iter_deref()
            .any(|action| matches!(action, ContractAction::Maintain(_)))
        {
            intent.actions = intent
                .actions
                .iter_deref()
                .map(|action| match action {
                    ContractAction::Maintain(upd) => ContractAction::Maintain(MaintenanceUpdate {
                        signatures: Array::new(),
                        ..upd.clone()
                    }),
                    action => action.clone(),
                })
                .collect();
        }
        if let Some(dust) = &intent.dust_actions {
            let mut dust = (**dust).clone();
            dust.registrations = dust
                .registrations
                .distinct_values()
                .into_iter()
                .map(|(reg, _)| DustRegistration {
                    allow_fee_payment: u128::from(segment),
                    ..(*reg).clone()
                })
                .collect();
            intent.dust_actions = Some(Sp::new(dust));
        }
        stx.intents = stx.intents.insert(segment, intent);
    }
    Transaction::Standard(stx)
}
