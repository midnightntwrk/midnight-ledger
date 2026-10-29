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
use coin_structure::coin::{NIGHT, TokenType};
use onchain_runtime::context::Effects;
use onchain_runtime::error::TranscriptRejected;
use storage::db::DB;

/// Counts of a transaction's non-call parts, whose cost
/// [`partition_transcripts_with_tx_shape`](super::partition_transcripts_with_tx_shape)
/// reserves from the guaranteed budget.
///
/// Pass the counts of the final transaction, so the guaranteed section is
/// sized for it rather than for the derived default. Raising a count moves the
/// calls toward the fallible section, except for Dust spends, Zswap parts and
/// non-NIGHT outputs, whose bytes can add more allowance than they cost;
/// overstating those is caught by `well_formed`.
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
}

impl TransactionShape {
    /// The shape of the balanced transaction the calls imply: their intent and
    /// a fee intent with one Dust spend, one UTXO spend per token type they
    /// receive, the outputs they send or mint, and a NIGHT change output when
    /// they spend NIGHT. Change for other tokens is not counted; set
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
            full_runs.iter().map(|run| &run.context.effects),
        ))
    }

    pub(crate) fn for_effects<'a, D: DB>(
        effects: impl IntoIterator<Item = &'a Effects<D>>,
    ) -> TransactionShape {
        let night = TokenType::Unshielded(NIGHT);
        let mut receives_night = false;
        let mut sends_night = false;
        let mut unshielded_spends = 0;
        let mut unshielded_outputs = 0;
        let mut zswap_inputs = 0;
        let mut zswap_outputs = 0;
        for effects in effects {
            receives_night |= effects.unshielded_inputs.contains_key(&night);
            sends_night |= effects.unshielded_outputs.contains_key(&night);
            unshielded_spends += effects.unshielded_inputs.size();
            unshielded_outputs +=
                effects.unshielded_outputs.size() + effects.unshielded_mints.size();
            zswap_inputs += effects.claimed_nullifiers.size();
            zswap_outputs += effects.claimed_shielded_receives.size();
        }
        let night_spends = usize::from(receives_night);
        let change_output = usize::from(receives_night);
        let night_sends = usize::from(sends_night);
        TransactionShape {
            // The calls' intent and the one balancing adds.
            intents: 2,
            night_spends,
            night_outputs: night_sends + change_output,
            non_night_unshielded_spends: unshielded_spends - night_spends,
            non_night_unshielded_outputs: unshielded_outputs - night_sends,
            dust_spends: 1,
            zswap_inputs,
            zswap_outputs,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::mock_tx::mock_tx;
    use super::*;
    use crate::structure::INITIAL_PARAMETERS;
    use base_crypto::hash::HashOutput;
    use coin_structure::coin::{Commitment, Nullifier, UnshieldedTokenType};
    use storage::db::InMemoryDB;

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

    fn sends(token_types: &[TokenType]) -> Effects<InMemoryDB> {
        let mut effects = Effects::default();
        for token_type in token_types {
            effects.unshielded_outputs = effects.unshielded_outputs.insert(*token_type, 1);
        }
        effects
    }

    /// Synthetic calls' effects, with the shape derived from them.
    fn derived_tx_shapes() -> Vec<(&'static str, Vec<Effects<InMemoryDB>>, TransactionShape)> {
        let night = TokenType::Unshielded(NIGHT);
        let no_effects = TransactionShape {
            intents: 2,
            night_spends: 0,
            night_outputs: 0,
            non_night_unshielded_spends: 0,
            non_night_unshielded_outputs: 0,
            dust_spends: 1,
            zswap_inputs: 0,
            zswap_outputs: 0,
        };
        let mut shielded = Effects::default();
        shielded.claimed_nullifiers = shielded
            .claimed_nullifiers
            .insert(Nullifier(HashOutput([1; 32])));
        shielded.claimed_shielded_receives = shielded
            .claimed_shielded_receives
            .insert(Commitment(HashOutput([1; 32])))
            .insert(Commitment(HashOutput([2; 32])));
        let mut mint = Effects::default();
        mint.unshielded_mints = mint.unshielded_mints.insert(HashOutput([1; 32]), 1);
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
                    zswap_outputs: 2,
                    ..no_effects.clone()
                },
            ),
            (
                "mint",
                vec![mint],
                TransactionShape {
                    non_night_unshielded_outputs: 1,
                    ..no_effects.clone()
                },
            ),
        ]
    }

    #[test]
    fn for_effects_counts_the_balanced_transaction() {
        for (case, effects, tx_shape) in derived_tx_shapes() {
            assert_eq!(TransactionShape::for_effects(&effects), tx_shape, "{case}");
        }
    }

    #[test]
    fn reserve_grows_with_each_count() {
        let model = &INITIAL_PARAMETERS.cost_model;
        let reserve = |tx_shape: &TransactionShape| {
            mock_tx::<InMemoryDB>(&[], tx_shape).time_to_dismiss(model)
        };
        let derived = TransactionShape::for_effects(&[Effects::<InMemoryDB>::default()]);
        // Non-NIGHT outputs are left out: inserting a UTXO costs only bytes
        // written, which the reserve, a time, does not count.
        let counts: [fn(&mut TransactionShape) -> &mut usize; 7] = [
            |tx_shape| &mut tx_shape.intents,
            |tx_shape| &mut tx_shape.night_spends,
            |tx_shape| &mut tx_shape.night_outputs,
            |tx_shape| &mut tx_shape.non_night_unshielded_spends,
            |tx_shape| &mut tx_shape.dust_spends,
            |tx_shape| &mut tx_shape.zswap_inputs,
            |tx_shape| &mut tx_shape.zswap_outputs,
        ];
        for count in counts {
            let mut tx_shape = derived.clone();
            *count(&mut tx_shape) += 100;
            assert!(reserve(&tx_shape) > reserve(&derived), "{tx_shape:?}");
        }
    }
}
