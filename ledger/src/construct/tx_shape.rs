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
use crate::dust::DUST_SPEND_PROOF_SIZE;
use crate::error::TxShapeCheckError;
use crate::structure::{
    ContractAction, LedgerParameters, MIN_PROOF_SIZE, PROOF_SIZE, PedersenDowngradeable, ProofKind,
    SignatureKind, Transaction, UnshieldedOffer,
};
use base_crypto::cost_model::CostDuration;
use coin_structure::coin::{NIGHT, PublicAddress, TokenType};
use coin_structure::contract::ContractAddress;
use onchain_runtime::context::{ClaimedUnshieldedSpendsKey, Effects};
use onchain_runtime::error::TranscriptRejected;
use serialize::Serializable;
use std::collections::{BTreeMap, BTreeSet};
use storage::Storable;
use storage::db::DB;
use zswap::Offer as ZswapOffer;

/// Counts of a transaction's non-call parts, whose cost transcript
/// partitioning reserves from the guaranteed budget.
///
/// Pass the counts of the final transaction to
/// [`partition_transcripts_with_tx_shape`](super::partition_transcripts_with_tx_shape),
/// so the guaranteed section is sized for it rather than for the estimate.
/// Every part costs time to dismiss and adds bytes, which raise the time the
/// transaction is allowed. Some parts cost more than their bytes allow, such
/// as NIGHT spends; others, such as Zswap parts and Dust spends, can allow
/// more than they cost. Overstating a count is therefore not always the
/// cautious choice: where a count is not known exactly, partition for a
/// [`TransactionShapeRange`] instead.
///
/// Where a part sits within the transaction does not change its cost, except
/// for whether it is in the guaranteed or a fallible section: a fallible
/// part's application cost is not charged to the time to dismiss. The
/// unprefixed unshielded and Zswap counts are the intents' guaranteed
/// unshielded offers and the guaranteed Zswap offer; the `fallible_` counts
/// are the intents' fallible unshielded offers and the fallible Zswap offers.
/// Moving a count to its `fallible_` field lowers the reserve, and a part
/// counted as fallible but placed in the guaranteed section understates it.
///
/// Building a transaction with calls goes:
///
/// 1. Describe the final transaction: start from
///    [`TransactionShape::estimate_from_calls`] and set the counts the estimate
///    gets wrong, or count a draft with [`TransactionShape::of`]. Where a
///    count is uncertain, take a [`TransactionShapeRange`].
/// 2. Partition the calls for it, with the transaction they join:
///    [`StandardTransaction::add_calls_with_tx_shape_range`](crate::structure::StandardTransaction::add_calls_with_tx_shape_range)
///    or [`partition_transcripts_for_tx`](super::partition_transcripts_for_tx).
/// 3. Balance the transaction, then, before proving it,
///    [`TransactionShape::check_tx`] it. If it does not fit, partition again
///    for the shape the check reports, or a range up to it, and rebuild.
///
/// Start from [`TransactionShape::estimate_from_calls`] and set the counts the
/// estimate gets wrong, or from [`TransactionShape::of`] to count a transaction
/// already built. [`TransactionShape::merge`] adds up the shapes of parts built
/// separately. There is deliberately no empty shape to start from: a count
/// added in a later version would read 0 in it, understating every caller that
/// does not know to set it, while both constructors give it a value.
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
    pub fallible_night_spends: usize,
    pub fallible_night_outputs: usize,
    pub fallible_non_night_unshielded_spends: usize,
    pub fallible_non_night_unshielded_outputs: usize,
    pub fallible_zswap_inputs: usize,
    pub fallible_zswap_outputs: usize,
    pub fallible_zswap_transients: usize,
}

impl TransactionShape {
    /// Estimates the shape of the balanced transaction the calls imply:
    ///
    /// - two intents, the calls' and the fee intent, with one Dust spend;
    /// - one UTXO output per unshielded send the calls claim to a user;
    /// - a Zswap input or output per shielded coin the calls claim;
    /// - per token type, one UTXO spend by the wallet when the calls receive
    ///   more than is claimed to them, or claim more than they send or mint;
    /// - for NIGHT, a change output when the wallet spends it or the calls
    ///   send out more than they claim;
    /// - every part in the guaranteed section.
    ///
    /// The estimate does not count change for other tokens, more than one
    /// wallet spend per token type, intents beyond two or fallible parts; set
    /// those counts to expect them. Runs every call to its end, so it fails
    /// where a call does.
    pub fn estimate_from_calls<D: DB>(
        calls: &[PreTranscript<D>],
        params: &LedgerParameters,
    ) -> Result<TransactionShape, TranscriptRejected<D>> {
        let full_runs = calls
            .iter()
            .map(|call| call.run_to_checkpoint(call.total_checkpoints(), params))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(TransactionShape::estimate_from_effects(
            full_runs
                .iter()
                .map(|run| (run.context.address, &run.context.effects)),
        ))
    }

    #[deprecated(note = "renamed to `estimate_from_calls`")]
    pub fn for_calls<D: DB>(
        calls: &[PreTranscript<D>],
        params: &LedgerParameters,
    ) -> Result<TransactionShape, TranscriptRejected<D>> {
        TransactionShape::estimate_from_calls(calls, params)
    }

    pub(crate) fn estimate_from_effects<'a, D: DB>(
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
            ..TransactionShape::empty()
        }
    }

    /// Counts the non-call parts of `tx` as it is.
    ///
    /// Contract calls, deploys and maintenance updates are not counted. A
    /// rewards claim has none of the parts counted, and counts nothing.
    pub fn of<S, P, B, D>(tx: &Transaction<S, P, B, D>) -> TransactionShape
    where
        S: SignatureKind<D>,
        P: ProofKind<D>,
        B: Storable<D> + Clone,
        D: DB,
    {
        let Transaction::Standard(stx) = tx else {
            return TransactionShape::empty();
        };
        let mut tx_shape = TransactionShape {
            intents: stx.intents.size(),
            ..TransactionShape::empty()
        };
        let mut guaranteed = SectionShape::default();
        let mut fallible = SectionShape::default();
        for intent in stx.intents.values() {
            if let Some(offer) = &intent.guaranteed_unshielded_offer {
                guaranteed.count_unshielded_offer(offer);
            }
            if let Some(offer) = &intent.fallible_unshielded_offer {
                fallible.count_unshielded_offer(offer);
            }
            if let Some(dust_actions) = &intent.dust_actions {
                tx_shape.dust_spends += dust_actions.spends.len();
                for registration in dust_actions.registrations.iter() {
                    if registration.dust_address.is_some() {
                        tx_shape.dust_registrations += 1;
                    } else {
                        tx_shape.dust_deregistrations += 1;
                    }
                }
            }
        }
        if let Some(offer) = &stx.guaranteed_coins {
            guaranteed.count_zswap_offer(offer);
        }
        for offer in stx.fallible_coins.values() {
            fallible.count_zswap_offer(&offer);
        }
        tx_shape.set_guaranteed(guaranteed);
        tx_shape.set_fallible(fallible);
        tx_shape
    }

    /// The shape of a transaction made of the parts both shapes count.
    ///
    /// Intents are added too, which is right for parts in separate intents.
    /// If both shapes count parts of one intent, lower
    /// [`intents`](Self::intents) to match.
    pub fn merge(&self, other: &TransactionShape) -> TransactionShape {
        self.zip_with(other, usize::saturating_add)
    }

    /// The shape whose every count is `f` of the two shapes' counts.
    fn zip_with(
        &self,
        other: &TransactionShape,
        f: impl Fn(usize, usize) -> usize,
    ) -> TransactionShape {
        TransactionShape {
            intents: f(self.intents, other.intents),
            night_spends: f(self.night_spends, other.night_spends),
            night_outputs: f(self.night_outputs, other.night_outputs),
            non_night_unshielded_spends: f(
                self.non_night_unshielded_spends,
                other.non_night_unshielded_spends,
            ),
            non_night_unshielded_outputs: f(
                self.non_night_unshielded_outputs,
                other.non_night_unshielded_outputs,
            ),
            dust_spends: f(self.dust_spends, other.dust_spends),
            zswap_inputs: f(self.zswap_inputs, other.zswap_inputs),
            zswap_outputs: f(self.zswap_outputs, other.zswap_outputs),
            zswap_transients: f(self.zswap_transients, other.zswap_transients),
            dust_registrations: f(self.dust_registrations, other.dust_registrations),
            dust_deregistrations: f(self.dust_deregistrations, other.dust_deregistrations),
            fallible_night_spends: f(self.fallible_night_spends, other.fallible_night_spends),
            fallible_night_outputs: f(self.fallible_night_outputs, other.fallible_night_outputs),
            fallible_non_night_unshielded_spends: f(
                self.fallible_non_night_unshielded_spends,
                other.fallible_non_night_unshielded_spends,
            ),
            fallible_non_night_unshielded_outputs: f(
                self.fallible_non_night_unshielded_outputs,
                other.fallible_non_night_unshielded_outputs,
            ),
            fallible_zswap_inputs: f(self.fallible_zswap_inputs, other.fallible_zswap_inputs),
            fallible_zswap_outputs: f(self.fallible_zswap_outputs, other.fallible_zswap_outputs),
            fallible_zswap_transients: f(
                self.fallible_zswap_transients,
                other.fallible_zswap_transients,
            ),
        }
    }

    /// Checks `tx`, the final transaction of calls partitioned for `self`,
    /// against the time to dismiss the node allows it, before it is proven.
    ///
    /// Partitioning keeps calls guaranteed for the shape it is given, but the
    /// final transaction may differ: the wallet may spend more UTXOs or add
    /// change. This finds out before the transaction is proven and submitted.
    /// On [`TxShapeCheckError::OutsideTimeToDismiss`], partition the calls
    /// again for its `actual` shape, [`TransactionShape::of`]`(tx)`, and
    /// rebuild.
    ///
    /// The check counts each call proof at its smallest size, as partitioning
    /// does, so it accepts no transaction the node would reject for its time
    /// to dismiss, but may reject a proven one whose larger proofs the node
    /// would accept. For a proven transaction,
    /// [`Transaction::cost`]`(params, true)` gives the node's own verdict.
    /// `self` is used only to report how the shapes differ.
    pub fn check_tx<S, P, B, D>(
        &self,
        tx: &Transaction<S, P, B, D>,
        params: &LedgerParameters,
    ) -> Result<(), TxShapeCheckError>
    where
        S: SignatureKind<D>,
        P: ProofKind<D>,
        B: Storable<D> + PedersenDowngradeable<D> + Serializable,
        D: DB,
    {
        let time_to_dismiss = tx
            .time_to_dismiss(params)
            .map_err(TxShapeCheckError::FeeCalculation)?;
        let erased = tx.erase_proofs();
        // The ledger estimates each call proof at `PROOF_SIZE`; the smallest is
        // `MIN_PROOF_SIZE`. Counting a call twice only lowers the bound.
        let calls = erased
            .actions()
            .filter(|(_, action)| matches!(action, ContractAction::Call(_)))
            .count();
        let size = <() as ProofKind<D>>::estimated_tx_size(&erased)
            .saturating_sub(calls.saturating_mul(PROOF_SIZE - MIN_PROOF_SIZE));
        let allowed_time_to_dismiss = CostDuration::max(
            params.limits.time_to_dismiss_per_byte * size as u64,
            params.limits.min_time_to_dismiss,
        );
        if time_to_dismiss > allowed_time_to_dismiss {
            return Err(TxShapeCheckError::OutsideTimeToDismiss {
                expected: Box::new(self.clone()),
                actual: Box::new(TransactionShape::of(tx)),
                time_to_dismiss,
                allowed_time_to_dismiss,
            });
        }
        Ok(())
    }

    /// Every count with its field's name, in declaration order.
    pub(crate) fn counts(&self) -> [(&'static str, usize); 18] {
        [
            ("intents", self.intents),
            ("night_spends", self.night_spends),
            ("night_outputs", self.night_outputs),
            (
                "non_night_unshielded_spends",
                self.non_night_unshielded_spends,
            ),
            (
                "non_night_unshielded_outputs",
                self.non_night_unshielded_outputs,
            ),
            ("dust_spends", self.dust_spends),
            ("zswap_inputs", self.zswap_inputs),
            ("zswap_outputs", self.zswap_outputs),
            ("zswap_transients", self.zswap_transients),
            ("dust_registrations", self.dust_registrations),
            ("dust_deregistrations", self.dust_deregistrations),
            ("fallible_night_spends", self.fallible_night_spends),
            ("fallible_night_outputs", self.fallible_night_outputs),
            (
                "fallible_non_night_unshielded_spends",
                self.fallible_non_night_unshielded_spends,
            ),
            (
                "fallible_non_night_unshielded_outputs",
                self.fallible_non_night_unshielded_outputs,
            ),
            ("fallible_zswap_inputs", self.fallible_zswap_inputs),
            ("fallible_zswap_outputs", self.fallible_zswap_outputs),
            ("fallible_zswap_transients", self.fallible_zswap_transients),
        ]
    }

    /// The fewest bytes each counted part takes in a transaction: its proof
    /// for Zswap parts and Dust spends, otherwise the keys and hashes it
    /// carries, which are fixed in size.
    fn min_part_bytes() -> TransactionShape {
        const KEY: usize = 32;
        // An owner and a token type; a spend adds its output's intent hash.
        const OUTPUT: usize = 2 * KEY;
        const SPEND: usize = 3 * KEY;
        TransactionShape {
            intents: KEY,
            night_spends: SPEND,
            night_outputs: OUTPUT,
            non_night_unshielded_spends: SPEND,
            non_night_unshielded_outputs: OUTPUT,
            dust_spends: DUST_SPEND_PROOF_SIZE,
            zswap_inputs: zswap::INPUT_PROOF_SIZE,
            zswap_outputs: zswap::OUTPUT_PROOF_SIZE,
            zswap_transients: zswap::INPUT_PROOF_SIZE + zswap::OUTPUT_PROOF_SIZE,
            dust_registrations: KEY,
            dust_deregistrations: KEY,
            fallible_night_spends: SPEND,
            fallible_night_outputs: OUTPUT,
            fallible_non_night_unshielded_spends: SPEND,
            fallible_non_night_unshielded_outputs: OUTPUT,
            fallible_zswap_inputs: zswap::INPUT_PROOF_SIZE,
            fallible_zswap_outputs: zswap::OUTPUT_PROOF_SIZE,
            fallible_zswap_transients: zswap::INPUT_PROOF_SIZE + zswap::OUTPUT_PROOF_SIZE,
        }
    }

    /// Whether a transaction of this shape could be at most `byte_limit`
    /// bytes, counting only the smallest its parts can be.
    pub(crate) fn fits_in(&self, byte_limit: usize) -> bool {
        self.counts()
            .into_iter()
            .zip(Self::min_part_bytes().counts())
            .map(|((_, count), (_, bytes))| count.saturating_mul(bytes))
            .fold(0usize, usize::saturating_add)
            <= byte_limit
    }

    /// The shape that counts nothing. Not public: see the type's docs.
    pub(crate) fn empty() -> TransactionShape {
        TransactionShape {
            intents: 0,
            night_spends: 0,
            night_outputs: 0,
            non_night_unshielded_spends: 0,
            non_night_unshielded_outputs: 0,
            dust_spends: 0,
            zswap_inputs: 0,
            zswap_outputs: 0,
            zswap_transients: 0,
            dust_registrations: 0,
            dust_deregistrations: 0,
            fallible_night_spends: 0,
            fallible_night_outputs: 0,
            fallible_non_night_unshielded_spends: 0,
            fallible_non_night_unshielded_outputs: 0,
            fallible_zswap_inputs: 0,
            fallible_zswap_outputs: 0,
            fallible_zswap_transients: 0,
        }
    }

    /// The unshielded and Zswap counts of the guaranteed section.
    pub(crate) fn guaranteed(&self) -> SectionShape {
        SectionShape {
            night_spends: self.night_spends,
            night_outputs: self.night_outputs,
            non_night_unshielded_spends: self.non_night_unshielded_spends,
            non_night_unshielded_outputs: self.non_night_unshielded_outputs,
            zswap_inputs: self.zswap_inputs,
            zswap_outputs: self.zswap_outputs,
            zswap_transients: self.zswap_transients,
        }
    }

    /// The unshielded and Zswap counts of the fallible sections.
    pub(crate) fn fallible(&self) -> SectionShape {
        SectionShape {
            night_spends: self.fallible_night_spends,
            night_outputs: self.fallible_night_outputs,
            non_night_unshielded_spends: self.fallible_non_night_unshielded_spends,
            non_night_unshielded_outputs: self.fallible_non_night_unshielded_outputs,
            zswap_inputs: self.fallible_zswap_inputs,
            zswap_outputs: self.fallible_zswap_outputs,
            zswap_transients: self.fallible_zswap_transients,
        }
    }

    fn set_guaranteed(&mut self, section: SectionShape) {
        self.night_spends = section.night_spends;
        self.night_outputs = section.night_outputs;
        self.non_night_unshielded_spends = section.non_night_unshielded_spends;
        self.non_night_unshielded_outputs = section.non_night_unshielded_outputs;
        self.zswap_inputs = section.zswap_inputs;
        self.zswap_outputs = section.zswap_outputs;
        self.zswap_transients = section.zswap_transients;
    }

    fn set_fallible(&mut self, section: SectionShape) {
        self.fallible_night_spends = section.night_spends;
        self.fallible_night_outputs = section.night_outputs;
        self.fallible_non_night_unshielded_spends = section.non_night_unshielded_spends;
        self.fallible_non_night_unshielded_outputs = section.non_night_unshielded_outputs;
        self.fallible_zswap_inputs = section.zswap_inputs;
        self.fallible_zswap_outputs = section.zswap_outputs;
        self.fallible_zswap_transients = section.zswap_transients;
    }

    /// Folds the Zswap parts given to `add_calls` into the estimate: the
    /// estimate's claimed coins stand in for parts not given, and `fallible`
    /// counts the inputs, outputs and transients given that sit in the
    /// fallible section.
    pub(crate) fn with_zswap_parts(
        mut self,
        inputs: usize,
        outputs: usize,
        transients: usize,
        fallible: (usize, usize, usize),
    ) -> TransactionShape {
        let (fallible_inputs, fallible_outputs, fallible_transients) = fallible;
        let all_inputs = self.zswap_inputs.saturating_sub(transients).max(inputs);
        let all_outputs = self.zswap_outputs.saturating_sub(transients).max(outputs);
        self.zswap_inputs = all_inputs.saturating_sub(fallible_inputs);
        self.zswap_outputs = all_outputs.saturating_sub(fallible_outputs);
        self.zswap_transients = transients.saturating_sub(fallible_transients);
        self.fallible_zswap_inputs = fallible_inputs;
        self.fallible_zswap_outputs = fallible_outputs;
        self.fallible_zswap_transients = fallible_transients;
        self
    }
}

/// The shapes the final transaction may take, when its counts are not known
/// exactly: every count from [`at_least`](Self::at_least)'s to
/// [`at_most`](Self::at_most)'s.
///
/// Neither overstating nor understating a count is safe in general, as some
/// parts cost more time to dismiss than their bytes allow and others less.
/// Partitioning for a range reserves the cost of `at_most` and allows for the
/// bytes of `at_least`. A transaction's cost and its size both only grow with
/// each count, so the partition fits every transaction whose counts lie in
/// between. An exact shape is the range from it to itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransactionShapeRange {
    at_least: TransactionShape,
    at_most: TransactionShape,
}

impl TransactionShapeRange {
    /// The range of just `shape`.
    pub fn exactly(shape: TransactionShape) -> Self {
        TransactionShapeRange {
            at_least: shape.clone(),
            at_most: shape,
        }
    }

    /// The range spanning both shapes, in either order: each count from the
    /// smaller of theirs to the larger.
    pub fn between(a: &TransactionShape, b: &TransactionShape) -> Self {
        TransactionShapeRange {
            at_least: a.zip_with(b, usize::min),
            at_most: a.zip_with(b, usize::max),
        }
    }

    /// Every count at its smallest.
    pub fn at_least(&self) -> &TransactionShape {
        &self.at_least
    }

    /// Every count at its largest.
    pub fn at_most(&self) -> &TransactionShape {
        &self.at_most
    }

    /// Whether every count of `shape` lies in the range.
    pub fn contains(&self, shape: &TransactionShape) -> bool {
        self.at_least
            .counts()
            .into_iter()
            .zip(shape.counts())
            .zip(self.at_most.counts())
            .all(|(((_, low), (_, count)), (_, high))| low <= count && count <= high)
    }
}

impl From<TransactionShape> for TransactionShapeRange {
    fn from(shape: TransactionShape) -> Self {
        TransactionShapeRange::exactly(shape)
    }
}

/// The unshielded and Zswap counts of one of a transaction's sections,
/// guaranteed or fallible.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SectionShape {
    pub(crate) night_spends: usize,
    pub(crate) night_outputs: usize,
    pub(crate) non_night_unshielded_spends: usize,
    pub(crate) non_night_unshielded_outputs: usize,
    pub(crate) zswap_inputs: usize,
    pub(crate) zswap_outputs: usize,
    pub(crate) zswap_transients: usize,
}

impl SectionShape {
    fn count_unshielded_offer<S: SignatureKind<D>, D: DB>(
        &mut self,
        offer: &UnshieldedOffer<S, D>,
    ) {
        for spend in offer.inputs.iter() {
            if spend.type_ == NIGHT {
                self.night_spends += 1;
            } else {
                self.non_night_unshielded_spends += 1;
            }
        }
        for output in offer.outputs.iter() {
            if output.type_ == NIGHT {
                self.night_outputs += 1;
            } else {
                self.non_night_unshielded_outputs += 1;
            }
        }
    }

    fn count_zswap_offer<P: Storable<D>, D: DB>(&mut self, offer: &ZswapOffer<P, D>) {
        self.zswap_inputs += offer.inputs.len();
        self.zswap_outputs += offer.outputs.len();
        self.zswap_transients += offer.transient.len();
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
            dust_spends: 1,
            ..TransactionShape::empty()
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
    fn estimate_counts_the_balanced_transaction() {
        for (case, effects, tx_shape) in derived_tx_shapes() {
            let calls = effects.iter().map(|effects| (CALL_ADDRESS, effects));
            assert_eq!(
                TransactionShape::estimate_from_effects(calls),
                tx_shape,
                "{case}"
            );
        }
    }

    #[test]
    fn zswap_parts_given_raise_the_derived_counts() {
        let derived = TransactionShape {
            zswap_inputs: 2,
            zswap_outputs: 3,
            ..TransactionShape::estimate_from_effects::<InMemoryDB>([])
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
                .with_zswap_parts(inputs, outputs, transients, (0, 0, 0));
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

    #[test]
    fn zswap_parts_given_fallible_are_counted_fallible() {
        let mut derived = TransactionShape::estimate_from_effects::<InMemoryDB>([]);
        derived.zswap_inputs = 2;
        derived.zswap_outputs = 3;
        let tx_shape = derived.with_zswap_parts(4, 5, 1, (1, 2, 1));
        assert_eq!(
            (
                tx_shape.zswap_inputs,
                tx_shape.zswap_outputs,
                tx_shape.zswap_transients,
                tx_shape.fallible_zswap_inputs,
                tx_shape.fallible_zswap_outputs,
                tx_shape.fallible_zswap_transients,
            ),
            (3, 3, 0, 1, 2, 1)
        );
    }

    #[test]
    fn merge_adds_every_count() {
        let tx_shape = |n: usize| TransactionShape {
            intents: n,
            night_spends: n + 1,
            night_outputs: n + 2,
            non_night_unshielded_spends: n + 3,
            non_night_unshielded_outputs: n + 4,
            dust_spends: n + 5,
            zswap_inputs: n + 6,
            zswap_outputs: n + 7,
            zswap_transients: n + 8,
            dust_registrations: n + 9,
            dust_deregistrations: n + 10,
            fallible_night_spends: n + 11,
            fallible_night_outputs: n + 12,
            fallible_non_night_unshielded_spends: n + 13,
            fallible_non_night_unshielded_outputs: n + 14,
            fallible_zswap_inputs: n + 15,
            fallible_zswap_outputs: n + 16,
            fallible_zswap_transients: n + 17,
        };
        // Each count is n + k, so the merge of n = 1 and n = 2 is 3 + 2k:
        // tx_shape(3) plus k for every count.
        let merged = tx_shape(1).merge(&tx_shape(2));
        let expected = TransactionShape {
            intents: 3,
            night_spends: 5,
            night_outputs: 7,
            non_night_unshielded_spends: 9,
            non_night_unshielded_outputs: 11,
            dust_spends: 13,
            zswap_inputs: 15,
            zswap_outputs: 17,
            zswap_transients: 19,
            dust_registrations: 21,
            dust_deregistrations: 23,
            fallible_night_spends: 25,
            fallible_night_outputs: 27,
            fallible_non_night_unshielded_spends: 29,
            fallible_non_night_unshielded_outputs: 31,
            fallible_zswap_inputs: 33,
            fallible_zswap_outputs: 35,
            fallible_zswap_transients: 37,
        };
        assert_eq!(merged, expected);
        assert_eq!(tx_shape(3).merge(&TransactionShape::empty()), tx_shape(3));
    }

    /// The guaranteed and fallible views read and write the fields they name.
    #[test]
    fn sections_map_to_their_fields() {
        let mut tx_shape = TransactionShape::empty();
        let section = SectionShape {
            night_spends: 1,
            night_outputs: 2,
            non_night_unshielded_spends: 3,
            non_night_unshielded_outputs: 4,
            zswap_inputs: 5,
            zswap_outputs: 6,
            zswap_transients: 7,
        };
        tx_shape.set_fallible(section);
        assert_eq!(tx_shape.fallible(), section);
        assert_eq!(tx_shape.guaranteed(), SectionShape::default());
        tx_shape.set_guaranteed(section);
        assert_eq!(tx_shape.guaranteed(), section);
        assert_eq!(
            tx_shape,
            TransactionShape {
                night_spends: 1,
                night_outputs: 2,
                non_night_unshielded_spends: 3,
                non_night_unshielded_outputs: 4,
                zswap_inputs: 5,
                zswap_outputs: 6,
                zswap_transients: 7,
                fallible_night_spends: 1,
                fallible_night_outputs: 2,
                fallible_non_night_unshielded_spends: 3,
                fallible_non_night_unshielded_outputs: 4,
                fallible_zswap_inputs: 5,
                fallible_zswap_outputs: 6,
                fallible_zswap_transients: 7,
                ..TransactionShape::empty()
            }
        );
    }

    #[test]
    fn check_error_names_the_counts_that_differ() {
        let expected = TransactionShape {
            intents: 2,
            night_spends: 1,
            night_outputs: 1,
            dust_spends: 1,
            ..TransactionShape::empty()
        };
        let actual = TransactionShape {
            night_spends: 3,
            fallible_zswap_inputs: 2,
            ..expected.clone()
        };
        let message = crate::error::TxShapeCheckError::OutsideTimeToDismiss {
            expected: Box::new(expected),
            actual: Box::new(actual),
            time_to_dismiss: base_crypto::cost_model::CostDuration::from_picoseconds(2),
            allowed_time_to_dismiss: base_crypto::cost_model::CostDuration::from_picoseconds(1),
        }
        .to_string();
        assert!(
            message.ends_with(
                "(night_spends: expected 1, got 3, fallible_zswap_inputs: expected 0, got 2)"
            ),
            "{message}"
        );
    }

    #[test]
    fn a_range_spans_both_shapes_in_either_order() {
        let a = TransactionShape {
            intents: 1,
            night_spends: 4,
            zswap_outputs: 0,
            ..TransactionShape::empty()
        };
        let b = TransactionShape {
            intents: 3,
            night_spends: 1,
            zswap_outputs: 2,
            ..TransactionShape::empty()
        };
        let range = TransactionShapeRange::between(&a, &b);
        assert_eq!(range, TransactionShapeRange::between(&b, &a));
        assert_eq!(
            range.at_least(),
            &TransactionShape {
                intents: 1,
                night_spends: 1,
                ..TransactionShape::empty()
            }
        );
        assert_eq!(
            range.at_most(),
            &TransactionShape {
                intents: 3,
                night_spends: 4,
                zswap_outputs: 2,
                ..TransactionShape::empty()
            }
        );
        assert!(range.contains(&a) && range.contains(&b));
        let mut outside = a.clone();
        outside.night_spends = 5;
        assert!(!range.contains(&outside));
        let exact = TransactionShapeRange::from(a.clone());
        assert_eq!((exact.at_least(), exact.at_most()), (&a, &a));
        assert!(exact.contains(&a) && !exact.contains(&b));
    }
}
