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
//! replaced, so the rewrite charges the fees clients built before it compute.
//!
//! Delete this module once the rollout of the rewrite is confirmed. Do not
//! update it to follow later pricing changes: a failure caused by an
//! intended change means the module has served its purpose.

#![allow(deprecated)]

use super::transactions::{generated, recipe};
use crate::structure::*;
use proptest::prelude::*;
use storage::db::InMemoryDB;

type D = InMemoryDB;
type Tx = Transaction<(), (), Pedersen, D>;

fn old_validation_cost(tx: &Tx, model: &TransactionCostModel) -> SyntheticCost {
    let per_call = model.cell_read(VERIFIER_KEY_SIZE as u64)
        + model.map_index(EXPECTED_CONTRACT_DEPTH)
        + model.map_index(EXPECTED_OPERATIONS_DEPTH);
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
            cost += per_call * unique_calls.len();
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

/// The bytes a deploy copies: every distinct node of its state, with its
/// child references.
fn old_tree_copy<T: Storable<D>>(value: Sp<T, D>) -> RunningCost {
    let bytes_written = value
        .serialize_to_node_list()
        .nodes
        .iter()
        .map(|n| n.data.len() as u64 + n.child_indices.len() as u64 * PERSISTENT_HASH_BYTES as u64)
        .sum::<u64>();
    RunningCost {
        bytes_written,
        ..RunningCost::ZERO
    }
}

fn old_application_cost(tx: &Tx, model: &TransactionCostModel) -> (SyntheticCost, SyntheticCost) {
    let mut g_cost = model.baseline_cost;
    let mut f_costs = std::collections::BTreeMap::<Segment, RunningCost>::new();
    for (segment, intent) in tx.intents() {
        let mut f_cost = RunningCost::ZERO;
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
                        + old_tree_copy(Sp::new(deploy.initial_state.clone()));
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
                                f_cost +=
                                    old_tree_copy(Sp::new(auth.clone())) + model.map_insert(1, true)
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
        *f_costs.entry(segment).or_default() += f_cost;
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
                    *f_costs.entry(segment).or_default() += offer_cost;
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
    let g_cost = SyntheticCost::from(g_cost);
    let total_cost = f_costs.into_values().fold(g_cost, |total, mut f_cost| {
        f_cost.compute_time = f_cost.compute_time * model.fallible_factor;
        total + SyntheticCost::from(f_cost)
    });
    (g_cost, total_cost)
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

/// Every call's effect entries, counted on every path to the call.
fn old_effects_check_cost(tx: &Tx, model: &TransactionCostModel) -> SyntheticCost {
    let mut entries = 0u64;
    for (_, intent) in tx.intents() {
        for call in intent.calls() {
            for t in call
                .guaranteed_transcript
                .iter()
                .chain(call.fallible_transcript.iter())
            {
                let e = &t.effects;
                entries += [
                    e.claimed_nullifiers.size(),
                    e.claimed_shielded_receives.size(),
                    e.claimed_shielded_spends.size(),
                    e.claimed_contract_calls.size(),
                    e.shielded_mints.size(),
                    e.unshielded_mints.size(),
                    e.unshielded_inputs.size(),
                    e.unshielded_outputs.size(),
                    e.claimed_unshielded_spends.size(),
                ]
                .iter()
                .sum::<usize>() as u64;
            }
        }
    }
    let mut cost = RunningCost::compute(model.runtime_cost_model.ins_map_constant * entries);
    cost.compute_time = cost.compute_time * model.validation_factor;
    cost.into()
}

proptest! {
    #[test]
    fn costs_match_per_element_loops(r in recipe(), repeats in any::<bool>()) {
        let tx = generated(&r, repeats);
        let model = &INITIAL_PARAMETERS.cost_model;
        let validation = old_validation_cost(&tx, model);
        prop_assert_eq!(tx.validation_cost(model), validation);
        let effects = old_effects_check_cost(&tx, model);
        prop_assert_eq!(tx.effects_check_cost(model), effects);
        let Transaction::Standard(stx) = &tx else { unreachable!() };
        if stx.disjoint_check().is_ok() {
            let application = old_application_cost(&tx, model);
            prop_assert_eq!(tx.application_cost(&INITIAL_PARAMETERS), Ok(application));
            let dismiss = application.0 + validation + effects;
            prop_assert_eq!(tx.time_to_dismiss(&INITIAL_PARAMETERS), Ok(dismiss.max_time()));
            // Each pricing entry point dismisses against the same time.
            let state = LedgerState::<D>::new("local-test");
            for (allowed, within) in [(dismiss.max_time(), true), (dismiss.max_time() - CostDuration::from_picoseconds(1), false)] {
                let mut params = INITIAL_PARAMETERS;
                params.limits.time_to_dismiss_per_byte = CostDuration::ZERO;
                params.limits.min_time_to_dismiss = allowed;
                let within_time = |r: Result<_, FeeCalculationError>| {
                    !matches!(r, Err(FeeCalculationError::OutsideTimeToDismiss { .. }))
                };
                prop_assert_eq!(within_time(tx.cost(&params, true).map(|_| ())), within);
                prop_assert_eq!(within_time(tx.cost_with_state(&params, &state, true).map(|_| ())), within);
                prop_assert_eq!(within_time(tx.fees_with_impl(&params, |_, _| None, true).map(|_| ())), within);
            }
        } else {
            prop_assert_eq!(tx.application_cost(&INITIAL_PARAMETERS), Err(FeeCalculationError::RepeatedEntries));
        }
        prop_assert_eq!(tx.est_size(), old_estimated_tx_size(&tx));
    }
}
