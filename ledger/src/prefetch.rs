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

use coin_structure::coin::{Commitment, Nullifier, UserAddress, NIGHT};
use coin_structure::contract::ContractAddress;
use storage::db::DB;
use transient_crypto::merkle_tree::MerkleTreeDigest;

use crate::dust::{DustNullifier, InitialNonce};
use crate::structure::{
    ClaimKind, ContractAction, IntentHash, LedgerState, Transaction, Utxo, VerifiedTransaction,
};

pub struct PrefetchPaths {
    zswap_nullifiers: Vec<Nullifier>,
    zswap_coin_coms: Vec<Commitment>,
    zswap_merkle_roots: Vec<MerkleTreeDigest>,
    contract_addresses: Vec<ContractAddress>,
    utxo_keys: Vec<Utxo>,
    replay_intent_hashes: Vec<IntentHash>,
    dust_nullifiers: Vec<DustNullifier>,
    dust_delegation_addresses: Vec<UserAddress>,
    dust_night_indices: Vec<InitialNonce>,
    reward_addresses: Vec<UserAddress>,
    bridge_addresses: Vec<UserAddress>,
}

impl PrefetchPaths {
    pub fn new() -> Self {
        PrefetchPaths {
            zswap_nullifiers: Vec::new(),
            zswap_coin_coms: Vec::new(),
            zswap_merkle_roots: Vec::new(),
            contract_addresses: Vec::new(),
            utxo_keys: Vec::new(),
            replay_intent_hashes: Vec::new(),
            dust_nullifiers: Vec::new(),
            dust_delegation_addresses: Vec::new(),
            dust_night_indices: Vec::new(),
            reward_addresses: Vec::new(),
            bridge_addresses: Vec::new(),
        }
    }

    pub fn collect<D: DB>(&mut self, tx: &VerifiedTransaction<D>) {
        collect_into(self, tx);
    }
}

pub fn collect_prefetch_paths<D: DB>(tx: &VerifiedTransaction<D>) -> PrefetchPaths {
    let mut paths = PrefetchPaths::new();

    collect_into(&mut paths, tx);

    paths
}

fn collect_into<D: DB>(paths: &mut PrefetchPaths, tx: &VerifiedTransaction<D>) {
    match &tx.inner {
        Transaction::Standard(stx) => {
            // Zswap: guaranteed coins
            if let Some(offer) = &stx.guaranteed_coins {
                collect_zswap_paths(paths, offer);
            }

            // Zswap: fallible coins
            for entry in stx.fallible_coins.iter() {
                collect_zswap_paths(paths, &entry.1);
            }

            // Per-intent paths
            for entry in stx.intents.iter() {
                let segment_id: u16 = *entry.0;
                let intent = &*entry.1;

                // Replay protection (always uses segment_id 0)
                paths.replay_intent_hashes.push(intent.intent_hash(0));

                // Guaranteed unshielded offer
                if let Some(offer) = &intent.guaranteed_unshielded_offer {
                    let intent_hash = intent.intent_hash(0);
                    collect_unshielded_paths(paths, offer, intent_hash);
                }

                // Fallible unshielded offer
                if let Some(offer) = &intent.fallible_unshielded_offer {
                    let intent_hash = intent.intent_hash(segment_id);
                    collect_unshielded_paths(paths, offer, intent_hash);
                }

                // Contract actions
                for action in intent.actions.iter_deref() {
                    if let ContractAction::Call(call) = action {
                        paths.contract_addresses.push(call.address);
                    }
                }

                // Dust actions
                if let Some(da) = &intent.dust_actions {
                    for spend in da.spends.iter_deref() {
                        paths.dust_nullifiers.push(spend.old_nullifier);
                    }
                    for reg in da.registrations.iter_deref() {
                        paths
                            .dust_delegation_addresses
                            .push(UserAddress::from(reg.night_key.clone()));
                    }
                }
            }
        }
        Transaction::ClaimRewards(rewards) => {
            let address = UserAddress::from(rewards.owner.clone());
            match rewards.kind {
                ClaimKind::Reward => paths.reward_addresses.push(address),
                ClaimKind::CardanoBridge => paths.bridge_addresses.push(address),
            }
        }
    }
}

fn collect_zswap_paths<D: DB>(
    paths: &mut PrefetchPaths,
    offer: &zswap::Offer<(), D>,
) {
    for input in offer.inputs.iter_deref() {
        paths.zswap_nullifiers.push(input.nullifier);
        paths.zswap_merkle_roots.push(input.merkle_tree_root);
    }
    for output in offer.outputs.iter_deref() {
        paths.zswap_coin_coms.push(output.coin_com);
    }
    for transient in offer.transient.iter_deref() {
        paths.zswap_nullifiers.push(transient.nullifier);
        paths.zswap_coin_coms.push(transient.coin_com);
    }
}

fn collect_unshielded_paths<D: DB>(
    paths: &mut PrefetchPaths,
    offer: &crate::structure::UnshieldedOffer<(), D>,
    intent_hash: IntentHash,
) {
    for input in offer.inputs.iter_deref() {
        // NIGHT-type inputs: prefetch dust night_indices
        if input.type_ == NIGHT {
            paths.dust_night_indices.push(input.initial_nonce());
        }
        paths.utxo_keys.push(Utxo::from(input.clone()));
    }
    for (output_no, output) in offer.outputs.iter_deref().enumerate() {
        // NIGHT-type outputs: prefetch dust address_delegation
        if output.type_ == NIGHT {
            paths.dust_delegation_addresses.push(output.owner);
        }
        paths.utxo_keys.push(Utxo {
            value: output.value,
            owner: output.owner,
            type_: output.type_,
            intent_hash,
            output_no: output_no as u32,
        });
    }
}

pub fn execute_prefetch<D: DB>(state: &LedgerState<D>, paths: &PrefetchPaths) {
    use rayon::iter::{IntoParallelRefIterator, ParallelIterator};

    rayon::scope(|s| {
        s.spawn(|_| {
            paths.zswap_nullifiers.par_iter().for_each(|nullifier| {
                state.zswap.nullifiers.contains_key(nullifier);
            });
        });
        s.spawn(|_| {
            paths.zswap_coin_coms.par_iter().for_each(|com| {
                state.zswap.coin_coms_set.contains_key(com);
            });
        });
        s.spawn(|_| {
            paths.zswap_merkle_roots.par_iter().for_each(|root| {
                state.zswap.past_roots.contains(root);
            });
        });
        s.spawn(|_| {
            paths.contract_addresses.par_iter().for_each(|addr| {
                state.contract.get(addr);
            });
        });
        s.spawn(|_| {
            paths.utxo_keys.par_iter().for_each(|utxo| {
                state.utxo.utxos.contains_key(utxo);
            });
        });
        s.spawn(|_| {
            paths.replay_intent_hashes.par_iter().for_each(|hash| {
                state.replay_protection.time_filter_map.contains(hash);
            });
        });
        s.spawn(|_| {
            paths.dust_nullifiers.par_iter().for_each(|nullifier| {
                state.dust.utxo.nullifiers.member(nullifier);
            });
        });
        s.spawn(|_| {
            paths.dust_delegation_addresses.par_iter().for_each(|addr| {
                state.dust.generation.address_delegation.contains_key(addr);
            });
        });
        s.spawn(|_| {
            paths.dust_night_indices.par_iter().for_each(|nonce| {
                state.dust.generation.night_indices.contains_key(nonce);
            });
        });
        s.spawn(|_| {
            paths.reward_addresses.par_iter().for_each(|addr| {
                state.unclaimed_block_rewards.get(addr);
            });
        });
        s.spawn(|_| {
            paths.bridge_addresses.par_iter().for_each(|addr| {
                state.bridge_receiving.get(addr);
            });
        });
    });
}
