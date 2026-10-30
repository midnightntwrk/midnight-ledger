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

use super::PreTranscript;
use crate::structure::LedgerParameters;
use coin_structure::coin::{NIGHT, PublicAddress, TokenType};
use coin_structure::contract::ContractAddress;
use onchain_runtime::context::{ClaimedUnshieldedSpendsKey, Effects};
use onchain_runtime::error::TranscriptRejected;
use std::collections::{BTreeMap, BTreeSet};
use storage::db::DB;

/// Counts of a transaction's non-call parts, whose cost
/// [`partition_transcripts_with_tx_shape`](super::partition_transcripts_with_tx_shape)
/// reserves from the guaranteed budget.
///
/// Pass the counts of the final transaction, so the guaranteed section is
/// sized for it rather than for the derived default. Raising a count moves the
/// calls toward the fallible section, except for Dust spends, Dust
/// deregistrations, Dust registrations when no NIGHT is spent, Zswap parts and
/// non-NIGHT outputs: their bytes can add more allowance than they cost.
/// Overstating those is caught by `well_formed`.
///
/// To adjust a count, start from [`TransactionShape::for_calls`] and set the
/// field.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct TransactionShape {
    pub intents: usize,
    pub night_spends: usize,
    pub night_outputs: usize,
    pub non_night_unshielded_spends: usize,
    pub non_night_unshielded_outputs: usize,
    pub dust_spends: usize,
    pub zswap_inputs: usize,
    pub zswap_outputs: usize,
    pub zswap_transients: usize,
    pub dust_registrations: usize,
    pub dust_deregistrations: usize,
}

impl TransactionShape {
    /// The shape of the balanced transaction the calls imply:
    ///
    /// - two intents, the calls' and the fee intent, with one Dust spend;
    /// - one UTXO output per unshielded send the calls claim to a user;
    /// - a Zswap input or output per shielded coin the calls claim;
    /// - per token type, one UTXO spend by the wallet when the calls receive
    ///   more than is claimed to them, or claim more than they send or mint;
    /// - for NIGHT, a change output when the wallet spends it or the calls
    ///   send out more than they claim.
    ///
    /// Change for other tokens is not counted; set
    /// `non_night_unshielded_outputs` to expect it.
    pub fn for_calls<D: DB>(
        calls: &[PreTranscript<D>],
        params: &LedgerParameters,
    ) -> Result<TransactionShape, TranscriptRejected<D>> {
        let full_runs = calls
            .iter()
            .map(|call| call.run_to_ckpt_no(call.no_checkpoints(), params))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(TransactionShape::for_effects(
            full_runs
                .iter()
                .map(|run| (run.context.address, &run.context.effects)),
        ))
    }

    pub(crate) fn for_effects<'a, D: DB>(
        calls: impl IntoIterator<Item = (ContractAddress, &'a Effects<D>)>,
    ) -> TransactionShape {
        let night = TokenType::Unshielded(NIGHT);
        let mut balances = BTreeMap::<TokenType, TokenBalance>::new();
        let mut claimed_nullifiers = BTreeSet::new();
        let mut claimed_commitments = BTreeSet::new();
        for (address, effects) in calls {
            // A mint adds to the section's balance as a send does.
            for (domain_sep, amount) in effects.unshielded_mints.clone() {
                let token_type =
                    TokenType::Unshielded(address.custom_unshielded_token_type(domain_sep));
                balances.entry(token_type).or_default().send(amount.into());
            }
            for (token_type, amount) in effects.unshielded_outputs.clone() {
                balances.entry(token_type).or_default().send(amount);
            }
            for (token_type, amount) in effects.unshielded_inputs.clone() {
                balances.entry(token_type).or_default().receive(amount);
            }
            for (ClaimedUnshieldedSpendsKey(token_type, recipient), amount) in
                effects.claimed_unshielded_spends.clone()
            {
                balances
                    .entry(token_type)
                    .or_default()
                    .claim(recipient, amount);
            }
            claimed_nullifiers.extend(effects.claimed_nullifiers.iter().map(|n| **n));
            claimed_commitments.extend(effects.claimed_shielded_receives.iter().map(|c| **c));
            claimed_commitments.extend(effects.claimed_shielded_spends.iter().map(|c| **c));
        }
        let night_balance = balances.remove(&night).unwrap_or_default();
        let non_night_balances = balances;
        TransactionShape {
            // The calls' intent and the one balancing adds.
            intents: 2,
            night_spends: usize::from(night_balance.needs_wallet_spend()),
            night_outputs: night_balance.user_outputs
                + usize::from(night_balance.needs_change_output()),
            non_night_unshielded_spends: non_night_balances
                .values()
                .filter(|balance| balance.needs_wallet_spend())
                .count(),
            non_night_unshielded_outputs: non_night_balances
                .values()
                .map(|balance| balance.user_outputs)
                .sum(),
            dust_spends: 1,
            zswap_inputs: claimed_nullifiers.len(),
            zswap_outputs: claimed_commitments.len(),
            zswap_transients: 0,
            dust_registrations: 0,
            dust_deregistrations: 0,
        }
    }

    pub(crate) fn with_zswap_parts(
        mut self,
        inputs: usize,
        outputs: usize,
        transients: usize,
    ) -> TransactionShape {
        self.zswap_inputs = self.zswap_inputs.saturating_sub(transients).max(inputs);
        self.zswap_outputs = self.zswap_outputs.saturating_sub(transients).max(outputs);
        self.zswap_transients = transients;
        self
    }
}

#[derive(Default)]
struct TokenBalance {
    sent: u128,
    received: u128,
    claimed: u128,
    claimed_to_contracts: u128,
    user_outputs: usize,
}

impl TokenBalance {
    fn send(&mut self, amount: u128) {
        self.sent = self.sent.saturating_add(amount);
    }

    fn receive(&mut self, amount: u128) {
        self.received = self.received.saturating_add(amount);
    }

    fn claim(&mut self, recipient: PublicAddress, amount: u128) {
        self.claimed = self.claimed.saturating_add(amount);
        match recipient {
            PublicAddress::Contract(_) => {
                self.claimed_to_contracts = self.claimed_to_contracts.saturating_add(amount);
            }
            PublicAddress::User(_) => self.user_outputs += 1,
        }
    }

    fn needs_wallet_spend(&self) -> bool {
        // Only a send pinned to the receive's section can pay for it: a claim
        // naming the receiving contract ties the receive to the send recorded with
        // the claim. Unclaimed sends may land in another section. A claim with no
        // send behind it is caught by the second comparison.
        self.received > self.claimed_to_contracts || self.claimed > self.sent
    }

    fn needs_change_output(&self) -> bool {
        self.needs_wallet_spend() || self.sent > self.claimed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base_crypto::hash::HashOutput;
    use coin_structure::coin::{Commitment, Nullifier, UnshieldedTokenType, UserAddress};
    use storage::db::InMemoryDB;

    const CALL_ADDRESS: ContractAddress = ContractAddress(HashOutput([7; 32]));

    fn non_night(byte: u8) -> TokenType {
        TokenType::Unshielded(UnshieldedTokenType(HashOutput([byte; 32])))
    }

    fn receives(token_types: &[TokenType]) -> Effects<InMemoryDB> {
        let mut effects = Effects::default();
        for token_type in token_types {
            effects.unshielded_inputs = effects.unshielded_inputs.insert(*token_type, 1);
        }
        effects
    }

    fn user(byte: u8) -> PublicAddress {
        PublicAddress::User(UserAddress(HashOutput([byte; 32])))
    }

    fn claims(sends: &[(TokenType, PublicAddress)]) -> Effects<InMemoryDB> {
        let mut effects = Effects::default();
        for (token_type, recipient) in sends {
            let sent = effects
                .unshielded_outputs
                .get(token_type)
                .map_or(0, |amount| *amount);
            effects.unshielded_outputs = effects.unshielded_outputs.insert(*token_type, sent + 1);
            effects.claimed_unshielded_spends = effects
                .claimed_unshielded_spends
                .insert(ClaimedUnshieldedSpendsKey(*token_type, *recipient), 1);
        }
        effects
    }

    fn unclaimed_sends(token_types: &[TokenType]) -> Effects<InMemoryDB> {
        let mut effects = Effects::default();
        for token_type in token_types {
            effects.unshielded_outputs = effects.unshielded_outputs.insert(*token_type, 1);
        }
        effects
    }

    fn sends(token_types: &[TokenType]) -> Effects<InMemoryDB> {
        claims(
            &token_types
                .iter()
                .map(|t| (*t, user(1)))
                .collect::<Vec<_>>(),
        )
    }

    /// Synthetic calls' effects, with the shape derived from them.
    fn derived_tx_shapes() -> Vec<(&'static str, Vec<Effects<InMemoryDB>>, TransactionShape)> {
        let night = TokenType::Unshielded(NIGHT);
        let contract = PublicAddress::Contract(CALL_ADDRESS);
        let no_effects = TransactionShape {
            intents: 2,
            night_spends: 0,
            night_outputs: 0,
            non_night_unshielded_spends: 0,
            non_night_unshielded_outputs: 0,
            dust_spends: 1,
            zswap_inputs: 0,
            zswap_outputs: 0,
            zswap_transients: 0,
            dust_registrations: 0,
            dust_deregistrations: 0,
        };
        let mut shielded = Effects::default();
        shielded.claimed_nullifiers = shielded
            .claimed_nullifiers
            .insert(Nullifier(HashOutput([1; 32])));
        shielded.claimed_shielded_receives = shielded
            .claimed_shielded_receives
            .insert(Commitment(HashOutput([1; 32])))
            .insert(Commitment(HashOutput([2; 32])));
        shielded.claimed_shielded_spends = shielded
            .claimed_shielded_spends
            .insert(Commitment(HashOutput([3; 32])));
        let mut mint = Effects::default();
        mint.unshielded_mints = mint.unshielded_mints.insert(HashOutput([1; 32]), 1);
        let minted =
            TokenType::Unshielded(CALL_ADDRESS.custom_unshielded_token_type(HashOutput([1; 32])));
        // As Compact's `mintUnshieldedToken` records them.
        let mut mint_to_user = mint.clone();
        mint_to_user.claimed_unshielded_spends = mint_to_user
            .claimed_unshielded_spends
            .insert(ClaimedUnshieldedSpendsKey(minted, user(1)), 1);
        let mut mint_to_self = mint.clone();
        mint_to_self.claimed_unshielded_spends = mint_to_self
            .claimed_unshielded_spends
            .insert(ClaimedUnshieldedSpendsKey(minted, contract), 1);
        mint_to_self.unshielded_inputs = mint_to_self.unshielded_inputs.insert(minted, 1);
        let coin = Commitment(HashOutput([4; 32]));
        let mut coin_sender = Effects::default();
        coin_sender.claimed_shielded_spends = coin_sender.claimed_shielded_spends.insert(coin);
        let mut coin_receiver = Effects::default();
        coin_receiver.claimed_shielded_receives =
            coin_receiver.claimed_shielded_receives.insert(coin);
        let mut claims_without_sends = Effects::default();
        claims_without_sends.claimed_unshielded_spends = claims_without_sends
            .claimed_unshielded_spends
            .insert(ClaimedUnshieldedSpendsKey(night, user(1)), 1)
            .insert(ClaimedUnshieldedSpendsKey(non_night(1), user(1)), 1);
        vec![
            ("no effects", vec![Effects::default()], no_effects.clone()),
            (
                "NIGHT spend",
                vec![receives(&[night])],
                TransactionShape {
                    night_spends: 1,
                    night_outputs: 1,
                    ..no_effects.clone()
                },
            ),
            (
                "NIGHT spend and non-NIGHT send",
                vec![receives(&[night]), sends(&[non_night(1)])],
                TransactionShape {
                    night_spends: 1,
                    night_outputs: 1,
                    non_night_unshielded_outputs: 1,
                    ..no_effects.clone()
                },
            ),
            (
                "two non-NIGHT spends",
                vec![receives(&[non_night(1), non_night(2)])],
                TransactionShape {
                    non_night_unshielded_spends: 2,
                    ..no_effects.clone()
                },
            ),
            (
                "NIGHT and non-NIGHT sends",
                vec![sends(&[night, non_night(1)])],
                TransactionShape {
                    night_outputs: 1,
                    non_night_unshielded_outputs: 1,
                    ..no_effects.clone()
                },
            ),
            (
                "shielded input and outputs",
                vec![shielded],
                TransactionShape {
                    zswap_inputs: 1,
                    zswap_outputs: 3,
                    ..no_effects.clone()
                },
            ),
            (
                "NIGHT sent to two users",
                vec![claims(&[(night, user(1)), (night, user(2))])],
                TransactionShape {
                    night_outputs: 2,
                    ..no_effects.clone()
                },
            ),
            (
                "send to a contract",
                vec![claims(&[(non_night(1), contract)])],
                no_effects.clone(),
            ),
            (
                "NIGHT received and sent to a user",
                vec![receives(&[night]), sends(&[night])],
                TransactionShape {
                    night_spends: 1,
                    night_outputs: 2,
                    ..no_effects.clone()
                },
            ),
            (
                "sends to a contract that receives them",
                vec![
                    claims(&[(night, contract), (non_night(1), contract)]),
                    receives(&[night, non_night(1)]),
                ],
                no_effects.clone(),
            ),
            (
                "claims to a user without a send",
                vec![claims_without_sends],
                TransactionShape {
                    night_spends: 1,
                    night_outputs: 2,
                    non_night_unshielded_spends: 1,
                    non_night_unshielded_outputs: 1,
                    ..no_effects.clone()
                },
            ),
            // Change is counted only for NIGHT.
            ("mint alone", vec![mint], no_effects.clone()),
            (
                "mint to a user",
                vec![mint_to_user],
                TransactionShape {
                    non_night_unshielded_outputs: 1,
                    ..no_effects.clone()
                },
            ),
            (
                "mint to the minting contract",
                vec![mint_to_self],
                no_effects.clone(),
            ),
            (
                "send without a claim",
                vec![unclaimed_sends(&[non_night(1)])],
                no_effects.clone(),
            ),
            (
                "NIGHT sent without a claim",
                vec![unclaimed_sends(&[night])],
                TransactionShape {
                    night_outputs: 1,
                    ..no_effects.clone()
                },
            ),
            (
                "two calls receiving NIGHT",
                vec![receives(&[night]), receives(&[night])],
                TransactionShape {
                    night_spends: 1,
                    night_outputs: 1,
                    ..no_effects.clone()
                },
            ),
            (
                "two calls sending NIGHT",
                vec![sends(&[night]), sends(&[night])],
                TransactionShape {
                    night_outputs: 2,
                    ..no_effects.clone()
                },
            ),
            (
                "two calls receiving one non-NIGHT token",
                vec![receives(&[non_night(1)]), receives(&[non_night(1)])],
                TransactionShape {
                    non_night_unshielded_spends: 1,
                    ..no_effects.clone()
                },
            ),
            (
                "one call sending a coin to another",
                vec![coin_sender, coin_receiver],
                TransactionShape {
                    zswap_outputs: 1,
                    ..no_effects.clone()
                },
            ),
        ]
    }

    #[test]
    fn for_effects_counts_the_balanced_transaction() {
        for (case, effects, tx_shape) in derived_tx_shapes() {
            let calls = effects.iter().map(|effects| (CALL_ADDRESS, effects));
            assert_eq!(TransactionShape::for_effects(calls), tx_shape, "{case}");
        }
    }

    #[test]
    fn zswap_parts_given_raise_the_derived_counts() {
        let derived = TransactionShape {
            zswap_inputs: 2,
            zswap_outputs: 3,
            ..TransactionShape::for_effects::<InMemoryDB>([])
        };
        // (inputs, outputs, transients) given, and the counts that result.
        let cases = [
            ((0, 0, 0), (2, 3, 0)),
            ((4, 5, 0), (4, 5, 0)),
            // A transient stands in for a claimed input and a claimed output.
            ((1, 2, 1), (1, 2, 1)),
            ((0, 0, 5), (0, 0, 5)),
        ];
        for ((inputs, outputs, transients), expected) in cases {
            let tx_shape = derived
                .clone()
                .with_zswap_parts(inputs, outputs, transients);
            assert_eq!(
                (
                    tx_shape.zswap_inputs,
                    tx_shape.zswap_outputs,
                    tx_shape.zswap_transients
                ),
                expected,
                "given {inputs} inputs, {outputs} outputs and {transients} transients"
            );
        }
    }
}
