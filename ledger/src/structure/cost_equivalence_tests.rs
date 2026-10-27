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

//! Transaction costs are computed per distinct node and counted once per
//! path to it; these tests compare them against the per-element loops they
//! replaced.

#![allow(deprecated)]

use super::*;
use crate::dust::{DustActions, DustPublicKey, DustRegistration};
use base_crypto::fab::AlignedValue;
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

fn old_validation_cost_impl(
    tx: &Tx,
    model: &TransactionCostModel,
    per_call_vk_read: impl Fn(
        &ContractAddress,
        &EntryPointBuf,
        Option<ContractOperationVersion>,
    ) -> RunningCost,
) -> SyntheticCost {
    let mut result = match tx {
        Transaction::Standard(stx) => {
            let unique_calls = tx
                .calls()
                .map(|(_, call)| {
                    (
                        call.address,
                        call.entry_point,
                        <() as ProofKind<D>>::proof_version_to_operation_version(&call.proof),
                    )
                })
                .collect::<BTreeSet<_>>();
            let mut cost = model.baseline_cost;
            for (address, entry_point, version) in &unique_calls {
                cost += per_call_vk_read(address, entry_point, version.clone());
            }
            let offers = stx
                .guaranteed_coins
                .iter()
                .map(|o| (**o).clone())
                .chain(stx.fallible_coins.values())
                .collect::<Vec<_>>();
            let zswap_inputs = offers
                .iter()
                .map(|offer| offer.inputs.len() + offer.transient.len())
                .sum::<usize>();
            let zswap_outputs = offers
                .iter()
                .map(|offer| offer.outputs.len() + offer.transient.len())
                .sum::<usize>();
            cost += model.proof_verify(zswap::INPUT_PIS) * zswap_inputs;
            cost += model.proof_verify(zswap::OUTPUT_PIS) * zswap_outputs;
            for intent in stx.intents.values() {
                // Binding commitment check
                cost.compute_time += model.runtime_cost_model.pedersen_valid;
                // Unshielded offer validation
                cost.compute_time += intent
                    .guaranteed_unshielded_offer
                    .iter()
                    .chain(intent.fallible_unshielded_offer.iter())
                    .map(|o| o.signatures.len())
                    .sum::<usize>()
                    * model.runtime_cost_model.signature_verify_constant;
                for action in intent.actions.iter() {
                    match &*action {
                        ContractAction::Call(call) => {
                            cost.compute_time += model.runtime_cost_model.verifier_key_load;
                            cost += model.proof_verify(call.public_inputs_len());
                        }
                        ContractAction::Maintain(upd) => {
                            cost.compute_time += model.runtime_cost_model.signature_verify_constant
                                * upd.signatures.len();
                        }
                        _ => {}
                    }
                }
                if let Some(dust_actions) = intent.dust_actions {
                    cost += model.proof_verify(DUST_SPEND_PIS) * dust_actions.spends.len();
                    cost.compute_time += model.runtime_cost_model.signature_verify_constant
                        * dust_actions.registrations.len();
                }
            }
            // Compute time for Pedersen check
            cost.compute_time += offers.iter().map(|o| o.deltas.len()).sum::<usize>()
                * (model.runtime_cost_model.hash_to_curve + model.runtime_cost_model.ec_mul);
            cost.compute_time += model.runtime_cost_model.ec_mul;
            let mut res = SyntheticCost::from(cost);
            res.block_usage = tx.est_size() as u64;
            res
        }
        Transaction::ClaimRewards(_) => (RunningCost {
            compute_time: model.runtime_cost_model.signature_verify_constant,
            ..RunningCost::ZERO
        } + model.baseline_cost)
            .into(),
    };
    result.compute_time = result.compute_time * model.validation_factor;
    result
}

fn old_application_cost(tx: &Tx, model: &TransactionCostModel) -> (SyntheticCost, SyntheticCost) {
    let mut g_cost = model.baseline_cost;
    let mut f_cost = RunningCost::ZERO;
    for (_, intent) in tx.intents() {
        // Replay protection state update
        g_cost += model.time_filter_map_lookup() + model.cell_read(PERSISTENT_HASH_BYTES as u64);
        g_cost += model.time_filter_map_insert(false)
            + model.cell_write(PERSISTENT_HASH_BYTES as u64, false);
        // utxo processing
        for (cost, offer) in [
            (&mut g_cost, intent.guaranteed_unshielded_offer.as_ref()),
            (&mut f_cost, intent.fallible_unshielded_offer.as_ref()),
        ]
        .into_iter()
        .filter_map(|(c, o)| o.map(|o| (c, o)))
        {
            let inputs = offer.inputs.len();
            let outputs = offer.outputs.len();
            // UTXO membership test
            *cost += model.map_index(EXPECTED_UTXO_DEPTH) * inputs;
            // UTXO removal
            *cost += (model.map_remove(EXPECTED_UTXO_DEPTH, true)
                + model.cell_delete(UTXO_SIZE as u64))
                * inputs;
            // UTXO insertion
            *cost += (model.map_insert(EXPECTED_UTXO_DEPTH, false)
                + model.cell_write(UTXO_SIZE as u64, false))
                * outputs;
            let night_inputs = offer.inputs.iter().filter(|i| i.type_ == NIGHT).count();
            let night_outputs = offer.outputs.iter().filter(|o| o.type_ == NIGHT).count();
            // Generating dtime update
            *cost += (model.merkle_tree_insert_unamortized(32, false)
                + model.cell_write(DUST_GENERATION_INFO_SIZE as u64, false))
                * night_inputs;
            // Night generates Dust address table read
            *cost += (model.map_index(EXPECTED_UTXO_DEPTH) + model.cell_read(FR_BYTES as u64))
                * night_outputs;
            // Generation tree insertion & first-free update
            *cost += (model.cell_read(8) + model.cell_write(8, true)) * night_outputs;
            *cost += (model.merkle_tree_insert_amortized(32, false)
                + model.cell_write(DUST_GENERATION_INFO_SIZE as u64, false))
                * night_outputs;
            // Night indicies insertion
            *cost += (model.map_insert(EXPECTED_UTXO_DEPTH, false) + model.cell_write(8, false))
                * night_outputs;
            // Commitment merkle tree insertion & first-free update
            *cost += (model.cell_read(8) + model.cell_write(8, true)) * night_outputs;
            *cost += (model.merkle_tree_insert_amortized(EXPECTED_UTXO_DEPTH, false)
                + model.cell_write(FR_BYTES as u64, false))
                * night_outputs;
            // Commitment computation
            *cost += RunningCost::compute(
                model.runtime_cost_model.transient_hash
                    * FRESH_DUST_COMMITMENT_HASHES
                    * night_outputs,
            );
        }
        let dust_spends = intent
            .dust_actions
            .as_ref()
            .map(|a| a.spends.len())
            .unwrap_or(0);
        // Nullifier membership test
        g_cost += model.map_index(32) * dust_spends;
        // Nullifier set insertion
        g_cost += (model.map_insert(EXPECTED_UTXO_DEPTH, false)
            + model.cell_write(FR_BYTES as u64, false))
            * dust_spends;
        // Commitment merkle tree insertion & first-free update
        g_cost += (model.cell_read(8) + model.cell_write(8, true)) * dust_spends;
        g_cost += (model.merkle_tree_insert_amortized(EXPECTED_UTXO_DEPTH, false)
            + model.cell_write(FR_BYTES as u64, false))
            * dust_spends;
        // Dust Merkle roots lookup
        g_cost += model.time_filter_map_lookup() * 2u64;
        for reg in intent
            .dust_actions
            .iter()
            .flat_map(|a| a.registrations.iter())
        {
            // Update the dust address registration table
            if reg.dust_address.is_some() {
                g_cost += model.map_insert(EXPECTED_GENERATION_DEPTH, false)
                    + model.cell_write(FR_BYTES as u64, false);
            } else {
                g_cost += model.map_remove(EXPECTED_GENERATION_DEPTH, true)
                    + model.cell_delete(FR_BYTES as u64);
            }
            // For each guaranteed night input with a matching address in
            // the intent, we read its ctime, and check it in the
            // generation indicies table.
            if reg.dust_address.is_some() {
                let night_inputs = intent
                    .guaranteed_unshielded_offer
                    .iter()
                    .flat_map(|o| o.inputs.iter())
                    .filter(|i| i.owner == reg.night_key && i.type_ == NIGHT)
                    .count();
                // Night indicies set check
                g_cost +=
                    (model.map_index(EXPECTED_UTXO_DEPTH) + model.cell_read(8)) * night_inputs;
                // Generation tree index
                g_cost += (model.merkle_tree_index(EXPECTED_UTXO_DEPTH)
                    + model.cell_read(DUST_GENERATION_INFO_SIZE as u64))
                    * night_inputs;
            }
        }
        // Contract actions
        for action in intent.actions.iter() {
            match &*action {
                ContractAction::Call(call) => {
                    let base_cost = if call.guaranteed_transcript.is_some() {
                        &mut g_cost
                    } else {
                        &mut f_cost
                    };
                    // Fetch / store state
                    *base_cost += model.map_index(EXPECTED_CONTRACT_DEPTH)
                        + model.map_insert(EXPECTED_CONTRACT_DEPTH, true)
                        + model.map_index(1)
                        + model.map_insert(1, true);
                    // Declared transcript costs
                    //
                    // NOTE: This is taken at face-value here. During
                    // execution, the declared cost (A `RunningCost`) is
                    // checked against the real cost at each operation, and
                    // aborted if it exceeds it (with the exception of
                    // `bytes_deleted`, which is checked after completion,
                    // and must be *at least* as declared).
                    for (cost, transcript) in [
                        (&mut g_cost, call.guaranteed_transcript.as_ref()),
                        (&mut f_cost, call.fallible_transcript.as_ref()),
                    ]
                    .into_iter()
                    .filter_map(|(c, t)| t.map(|t| (c, t)))
                    {
                        *cost += transcript.gas;
                        // VM stack setup / destroy cost
                        // Left out of scope here to avoid going to deep into
                        // stack structure.
                        *cost += model.stack_setup_cost_for_effects(&transcript.effects);
                    }
                }
                ContractAction::Deploy(deploy) => {
                    // Contract exists check
                    f_cost += model.map_index(EXPECTED_CONTRACT_DEPTH);
                    // Contract insert
                    f_cost += model.map_insert(EXPECTED_CONTRACT_DEPTH, false)
                        + model.tree_copy(Sp::new(deploy.initial_state.clone()));
                }
                ContractAction::Maintain(upd) => {
                    // Contract state fetch
                    f_cost += model.map_index(EXPECTED_CONTRACT_DEPTH);
                    // Maintainance update counter update
                    f_cost += model.map_index(1) * 2u64
                        + model.map_insert(1, true) * 2u64
                        + model.cell_read(8)
                        + model.cell_write(8, true);
                    // Carrying out the updates
                    for part in upd.updates.iter() {
                        match &*part {
                            SingleUpdate::ReplaceAuthority(auth) => {
                                f_cost += model.tree_copy::<_, D>(Sp::new(auth.clone()))
                                    + model.map_insert(1, true)
                            }
                            SingleUpdate::VerifierKeyRemove(..) => {
                                f_cost += model.map_remove(EXPECTED_OPERATIONS_DEPTH, true)
                                    + model.cell_delete(VERIFIER_KEY_SIZE as u64)
                                    + model.map_insert(1, true)
                            }
                            SingleUpdate::VerifierKeyInsert(ep, vk) => {
                                f_cost += model.map_insert(EXPECTED_OPERATIONS_DEPTH, false)
                                    + model.cell_write(
                                        (ep.serialized_size() + vk.serialized_size()) as u64,
                                        false,
                                    )
                                    + model.map_insert(1, true)
                            }
                            SingleUpdate::IrRemove(ep) => {
                                f_cost += model.map_insert(EXPECTED_OPERATIONS_DEPTH, true)
                                    + model.cell_delete(ep.serialized_size() as u64)
                                    + model.map_insert(1, true)
                            }
                            SingleUpdate::IrInsert(ep, ir) => {
                                f_cost += model.map_insert(EXPECTED_OPERATIONS_DEPTH, true)
                                    + model.cell_write(
                                        (ep.serialized_size() + ir.serialized_size()) as u64,
                                        false,
                                    )
                                    + model.map_insert(1, true)
                            }
                        }
                    }
                    // Inserting the new state
                    f_cost += model.map_insert(EXPECTED_CONTRACT_DEPTH, true);
                }
            }
        }
    }
    match tx {
        Transaction::Standard(stx) => {
            let offers = stx
                .guaranteed_coins
                .iter()
                .map(|o| (GUARANTEED_SEGMENT, (**o).clone()))
                .chain(
                    stx.fallible_coins
                        .iter()
                        .map(|pair| (*pair.0, (*pair.1).clone())),
                );
            for (segment, offer) in offers {
                let mut offer_cost = RunningCost::ZERO;
                let inputs = offer.inputs.len() + offer.transient.len();
                let outputs = offer.outputs.len() + offer.transient.len();
                // Roots set test
                offer_cost += model.map_index(EXPECTED_TIME_FILTER_MAP_DEPTH) * inputs;
                // Nullifier set test
                offer_cost += model.map_index(EXPECTED_UTXO_DEPTH) * inputs;
                // Nullifier set insertion
                offer_cost += (model.map_insert(EXPECTED_UTXO_DEPTH, false)
                    + model.cell_write(PERSISTENT_HASH_BYTES as u64, false))
                    * inputs;
                // Commitment set test
                offer_cost += model.map_index(EXPECTED_UTXO_DEPTH) * outputs;
                // Commitment set insertion
                offer_cost += model.map_insert(EXPECTED_UTXO_DEPTH, false) * outputs;
                // First free update
                offer_cost += (model.cell_read(8) + model.cell_write(8, true)) * outputs;
                // Merkle tree insertion
                offer_cost +=
                    model.merkle_tree_insert_amortized(EXPECTED_UTXO_DEPTH, false) * outputs;
                if segment == GUARANTEED_SEGMENT {
                    g_cost += offer_cost;
                } else {
                    f_cost += offer_cost;
                    // Because we also try to apply it in the guaranteed segment.
                    g_cost.compute_time += offer_cost.compute_time;
                }
            }
        }
        Transaction::ClaimRewards(_) => {
            // Claim check
            g_cost += model.map_index(EXPECTED_UTXO_DEPTH);
            // Claim update
            g_cost += model.map_insert(EXPECTED_UTXO_DEPTH, true);
            // Replay protection update
            g_cost +=
                model.time_filter_map_lookup() + model.cell_read(PERSISTENT_HASH_BYTES as u64);
            g_cost += model.time_filter_map_insert(false)
                + model.cell_write(PERSISTENT_HASH_BYTES as u64, false);
            // Utxo update
            g_cost += model.map_insert(EXPECTED_UTXO_DEPTH, false);
        }
    }
    g_cost.compute_time = g_cost.compute_time * model.guaranteed_factor;
    f_cost.compute_time = f_cost.compute_time * model.fallible_factor;
    (g_cost.into(), (g_cost + f_cost).into())
}

fn old_estimated_tx_size(tx: &Tx) -> usize {
    let size = tx.serialized_size();
    let calls = tx.calls().count();
    let dust_spends = tx
        .intents()
        .map(|(_, intent)| {
            intent
                .dust_actions
                .as_ref()
                .map(|da| da.spends.len())
                .unwrap_or(0)
        })
        .sum::<usize>();
    let (zswap_inputs, zswap_outputs) = if let Transaction::Standard(stx) = tx {
        let transients = stx.transients().count();
        (
            stx.inputs().count() + transients,
            stx.outputs().count() + transients,
        )
    } else {
        (0, 0)
    };
    size + calls * PROOF_SIZE
        + zswap_inputs * zswap::INPUT_PROOF_SIZE
        + zswap_outputs * zswap::OUTPUT_PROOF_SIZE
        + dust_spends * DUST_SPEND_PROOF_SIZE
}

type Entries = Vec<(u8, u8)>;

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
                        effects: Effects::default(),
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
                        signatures: vec![
                            SignaturesValue(0, Signature::Schnorr(rng.r#gen()));
                            kind as usize
                        ]
                        .into(),
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

/// `tx` without the entries `disjoint_check` rejects repeating: each
/// intent's inputs and registrations are made distinct and unique to it, and
/// the fallible offer stops sharing the guaranteed offer's inputs. Actions,
/// outputs and their sharing are kept.
fn without_repeats(tx: &Tx) -> Tx {
    let Transaction::Standard(mut stx) = tx.clone() else {
        unreachable!()
    };
    for (segment, mut intent) in tx.intents() {
        if let Some(offer) = &intent.guaranteed_unshielded_offer {
            let mut offer = (**offer).clone();
            offer.inputs = distinct(&offer.inputs)
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
        if let Some(dust) = &intent.dust_actions {
            let mut dust = (**dust).clone();
            dust.registrations = distinct(&dust.registrations)
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

proptest! {
    #[test]
    fn costs_match_per_element_loops(r in recipe(), repeats in any::<bool>()) {
        let tx = transaction(&r);
        let tx = if repeats { tx } else { without_repeats(&tx) };
        let model = &INITIAL_PARAMETERS.cost_model;
        let per_call = model.cell_read(VERIFIER_KEY_SIZE as u64)
            + model.map_index(EXPECTED_CONTRACT_DEPTH)
            + model.map_index(EXPECTED_OPERATIONS_DEPTH);
        prop_assert_eq!(
            tx.validation_cost(model),
            old_validation_cost_impl(&tx, model, |_, _, _| per_call)
        );
        let Transaction::Standard(stx) = &tx else { unreachable!() };
        if stx.disjoint_check().is_ok() {
            prop_assert_eq!(tx.application_cost(&INITIAL_PARAMETERS), Ok(old_application_cost(&tx, model)));
        } else {
            prop_assert_eq!(tx.application_cost(&INITIAL_PARAMETERS), Err(FeeCalculationError::RepeatedEntries));
        }
        prop_assert_eq!(tx.est_size(), old_estimated_tx_size(&tx));
    }
}

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
    // transactions full pricing accepts, at the same fee.
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
        let exact = tx.validation_cost(model) + old_application_cost(&tx, model).1;
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
