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

mod mock_tx;
mod tx_shape;

use mock_tx::{mock_calls, mock_tx};
pub use tx_shape::TransactionShape;

use crate::dust::DustActions;
use crate::error::{MalformedTransaction, PartitionFailure};
use crate::structure::EXPECTED_CONTRACT_DEPTH;
use crate::structure::{
    ContractAction, ContractCall, ContractDeploy, GUARANTEED_SEGMENT, Intent,
    IntentSigningEnvelope, LedgerParameters, MaintenanceUpdate, ProofPreimageMarker,
    ProofPreimageVersioned, Signature, SignatureKind, SignaturesValue, SigningKey, SingleUpdate,
    StandardTransaction, Transaction, UnshieldedOffer,
};
use crate::structure::{PedersenDowngradeable, ProofKind};
use base_crypto::cost_model::CostDuration;
use base_crypto::cost_model::RunningCost;
use base_crypto::fab::AlignedValue;
use base_crypto::time::Timestamp;
use coin_structure::contract::ContractAddress;
use itertools::Itertools;
use onchain_runtime::context::{Effects, QueryContext, QueryResults};
use onchain_runtime::error::TranscriptRejected;
use onchain_runtime::ops::Op;
use onchain_runtime::result_mode::ResultModeVerify;
use onchain_runtime::state::{ContractOperation, ContractState, EntryPointBuf};
use onchain_runtime::transcript::Transcript;
use rand::{CryptoRng, Rng};
use serde::{Deserialize, Serialize};
use serialize::Serializable;
use std::iter::once;
use std::ops::Deref;
use storage::Storable;
use storage::arena::Sp;
use storage::db::DB;
use transient_crypto::commitment::PedersenRandomness;
use transient_crypto::curve::Fr;
use transient_crypto::fab::{AlignedValueExt, ValueReprAlignedValue};
use transient_crypto::hash::transient_commit;
use transient_crypto::proofs::{KeyLocation, ProofPreimage};
use transient_crypto::repr::FieldRepr;
use zswap::{
    Input as ZswapInput, Offer as ZswapOffer, Output as ZswapOutput, Transient as ZswapTransient,
    ZSWAP_TREE_HEIGHT,
};

impl<S: SignatureKind<D>, D: DB> Transaction<S, ProofPreimageMarker, PedersenRandomness, D> {
    pub fn from_intents(
        network_id: impl Into<String>,
        intents: storage::storage::HashMap<
            u16,
            Intent<S, ProofPreimageMarker, PedersenRandomness, D>,
            D,
        >,
    ) -> Self {
        Self::new(network_id, intents, None, storage::storage::HashMap::new())
    }
}

/// Flags whether any of `items` are claimed by some call's *fallible* transcript, and so belong
/// in the fallible Zswap offer rather than the guaranteed one. `claims` says what counts as
/// a claim for the item type at hand.
fn has_claims_in_fallible<T, D: DB>(
    items: &[T],
    fallible_effects: &[&Effects<D>],
    claims: impl Fn(&Effects<D>, &T) -> bool,
) -> Vec<bool> {
    items
        .iter()
        .map(|item| fallible_effects.iter().any(|effects| claims(effects, item)))
        .collect()
}

/// Return items whose fallibility flag matches `fallible`.
fn with_fallibility<T: Clone>(items: &[T], is_fallible: &[bool], fallible: bool) -> Vec<T> {
    items
        .iter()
        .zip(is_fallible)
        .filter(|(_, f)| **f == fallible)
        .map(|(item, _)| item.clone())
        .collect()
}

impl<S: SignatureKind<D>, D: DB>
    StandardTransaction<S, ProofPreimageMarker, PedersenRandomness, D>
{
    pub fn new(
        network_id: impl Into<String>,
        intents: storage::storage::HashMap<
            u16,
            Intent<S, ProofPreimageMarker, PedersenRandomness, D>,
            D,
        >,
        guaranteed_coins: Option<ZswapOffer<ProofPreimage, D>>,
        fallible_coins: storage::storage::HashMap<u16, ZswapOffer<ProofPreimage, D>, D>,
    ) -> Self {
        let mut res = StandardTransaction {
            network_id: network_id.into(),
            binding_randomness: Default::default(),
            intents,
            guaranteed_coins: guaranteed_coins.map(|x| Sp::new(x)),
            fallible_coins: fallible_coins.into_iter().collect(),
        };
        res.recompute_binding_randomness();
        res
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add_calls<P>(
        &self,
        rng: &mut (impl Rng + CryptoRng),
        segment: SegmentSpecifier,
        calls: &[PrePartitionContractCall<D>],
        params: &LedgerParameters,
        ttl: Timestamp,
        zswap_inputs: &[ZswapInput<ProofPreimage, D>],
        zswap_outputs: &[ZswapOutput<ProofPreimage, D>],
        zswap_transients: &[ZswapTransient<ProofPreimage, D>],
    ) -> Result<Self, PartitionFailure<D>>
    where
        P: ContractCallExt<D>,
    {
        self.add_calls_inner::<P>(
            rng,
            segment,
            calls,
            params,
            ttl,
            zswap_inputs,
            zswap_outputs,
            zswap_transients,
            |derived| {
                derived.with_zswap_parts(
                    zswap_inputs.len(),
                    zswap_outputs.len(),
                    zswap_transients.len(),
                )
            },
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add_calls_with_tx_shape<P>(
        &self,
        rng: &mut (impl Rng + CryptoRng),
        segment: SegmentSpecifier,
        calls: &[PrePartitionContractCall<D>],
        params: &LedgerParameters,
        ttl: Timestamp,
        zswap_inputs: &[ZswapInput<ProofPreimage, D>],
        zswap_outputs: &[ZswapOutput<ProofPreimage, D>],
        zswap_transients: &[ZswapTransient<ProofPreimage, D>],
        expected_tx_shape: &TransactionShape,
    ) -> Result<Self, PartitionFailure<D>>
    where
        P: ContractCallExt<D>,
    {
        self.add_calls_inner::<P>(
            rng,
            segment,
            calls,
            params,
            ttl,
            zswap_inputs,
            zswap_outputs,
            zswap_transients,
            |_| expected_tx_shape.clone(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn add_calls_inner<P>(
        &self,
        rng: &mut (impl Rng + CryptoRng),
        segment: SegmentSpecifier,
        calls: &[PrePartitionContractCall<D>],
        params: &LedgerParameters,
        ttl: Timestamp,
        zswap_inputs: &[ZswapInput<ProofPreimage, D>],
        zswap_outputs: &[ZswapOutput<ProofPreimage, D>],
        zswap_transients: &[ZswapTransient<ProofPreimage, D>],
        choose_tx_shape: impl FnOnce(TransactionShape) -> TransactionShape,
    ) -> Result<Self, PartitionFailure<D>>
    where
        P: ContractCallExt<D>,
    {
        let pre_transcripts = calls
            .iter()
            .map(|call| PreTranscript {
                comm_comm: Some(communication_commitment(
                    call.input.clone(),
                    call.output.clone(),
                    call.communication_commitment_rand,
                )),
                ..call.pre_transcript.clone()
            })
            .collect::<Vec<_>>();
        let partitioned = partition_transcripts_inner(&pre_transcripts, params, choose_tx_shape)?;
        let segment = match segment {
            SegmentSpecifier::First => 1,
            SegmentSpecifier::GuaranteedOnly
                if partitioned.iter().any(|(_, snd)| snd.is_some()) =>
            {
                return Err(PartitionFailure::GuaranteedOnlyUnsatisfied);
            }
            SegmentSpecifier::GuaranteedOnly | SegmentSpecifier::Random => {
                rng.gen_range(2..u16::MAX)
            }
            SegmentSpecifier::Specific(GUARANTEED_SEGMENT) => {
                return Err(PartitionFailure::IllegalSegmentZero);
            }
            SegmentSpecifier::Specific(seg) => seg,
        };
        let prototypes = calls
            .iter()
            .zip(partitioned)
            .map(|(call, (guaranteed, fallible))| ContractCallPrototype {
                address: call.address,
                entry_point: call.entry_point.clone(),
                op: call.op.clone(),
                guaranteed_public_transcript: guaranteed,
                fallible_public_transcript: fallible,
                private_transcript_outputs: call.private_transcript_outputs.clone(),
                input: call.input.clone(),
                output: call.output.clone(),
                communication_commitment_rand: call.communication_commitment_rand,
                key_location: call.key_location.clone(),
            })
            .collect::<Vec<_>>();
        let fallible_effects = prototypes
            .iter()
            .filter_map(|proto| proto.fallible_public_transcript.as_ref())
            .map(|transcript| &transcript.effects)
            .collect::<Vec<_>>();
        let zswap_input_is_fallible =
            has_claims_in_fallible(zswap_inputs, &fallible_effects, |effects, inp| {
                effects.claimed_nullifiers.member(&inp.nullifier)
            });
        let zswap_output_is_fallible =
            has_claims_in_fallible(zswap_outputs, &fallible_effects, |effects, out| {
                effects
                    .claimed_shielded_spends
                    .union(&effects.claimed_shielded_receives)
                    .member(&out.coin_com)
            });
        let zswap_transient_is_fallible =
            has_claims_in_fallible(zswap_transients, &fallible_effects, |effects, trans| {
                effects
                    .claimed_shielded_spends
                    .union(&effects.claimed_shielded_receives)
                    .member(&trans.coin_com)
                    || effects.claimed_nullifiers.member(&trans.nullifier)
            });
        let intent_before = self
            .intents
            .get(&segment)
            .map(|i| (*i).clone())
            .unwrap_or_else(|| Intent::empty(rng, ttl));
        let intent = prototypes
            .into_iter()
            .fold(intent_before, |i, proto| i.add_call::<P>(proto));
        let mut res = self.set_intent(segment, intent);
        let offer_with_fallibility = |fallible: bool| {
            ZswapOffer::new(
                with_fallibility(zswap_inputs, &zswap_input_is_fallible, fallible),
                with_fallibility(zswap_outputs, &zswap_output_is_fallible, fallible),
                with_fallibility(zswap_transients, &zswap_transient_is_fallible, fallible),
            )
        };
        let guaranteed_coins =
            offer_with_fallibility(false).map(|o| o.retarget_segment(GUARANTEED_SEGMENT));
        let fallible_coins = offer_with_fallibility(true).map(|o| o.retarget_segment(segment));
        let merge_into = |old: Option<&ZswapOffer<ProofPreimage, D>>,
                          new: ZswapOffer<ProofPreimage, D>| {
            match old {
                Some(old) => old.merge(&new).map_err(PartitionFailure::Merge),
                None => Ok(new),
            }
        };
        if let Some(offer) = guaranteed_coins {
            let merged = merge_into(res.guaranteed_coins.as_deref(), offer)?;
            res.guaranteed_coins = Some(Sp::new(merged));
        }
        if let Some(offer) = fallible_coins {
            let merged = merge_into(res.fallible_coins.get(&segment).as_deref(), offer)?;
            res.fallible_coins = res.fallible_coins.insert(segment, merged);
        }
        res.recompute_binding_randomness();
        Ok(res)
    }

    pub fn set_intent(
        &self,
        segment: u16,
        intent: Intent<S, ProofPreimageMarker, PedersenRandomness, D>,
    ) -> Self {
        let mut res = self.clone();
        res.intents = self.intents.insert(segment, intent);
        res.recompute_binding_randomness();
        res
    }

    pub fn recompute_binding_randomness(&mut self) {
        self.binding_randomness = self
            .guaranteed_coins
            .as_ref()
            .map(|o| o.binding_randomness())
            .unwrap_or_else(|| PedersenRandomness::from(0))
            + self
                .fallible_coins
                .values()
                .fold(PedersenRandomness::from(0), |acc, o| {
                    acc + o.binding_randomness()
                })
            + self
                .intents
                .values()
                .map(|i| i.binding_randomness())
                .fold(0.into(), |a, b| a + b);
    }
}

impl<D: DB> ContractDeploy<D> {
    pub fn new<R: Rng + CryptoRng + ?Sized>(rng: &mut R, initial_state: ContractState<D>) -> Self {
        ContractDeploy {
            initial_state,
            nonce: rng.r#gen(),
        }
    }
}

impl<D: DB> MaintenanceUpdate<D> {
    pub fn new(address: ContractAddress, updates: Vec<SingleUpdate>, counter: u32) -> Self {
        MaintenanceUpdate {
            address,
            updates: updates.into(),
            counter,
            signatures: vec![].into(),
        }
    }

    pub fn add_signature(mut self, idx: u32, signature: Signature) -> Self {
        self.signatures = self
            .signatures
            .push(SignaturesValue(idx, signature))
            .iter_deref()
            .cloned()
            .sorted()
            .collect();
        self
    }
}

impl<S: SignatureKind<D>, D: DB> Intent<S, ProofPreimageMarker, PedersenRandomness, D> {
    pub fn empty<R: Rng + CryptoRng + ?Sized>(
        rng: &mut R,
        ttl: Timestamp,
    ) -> Intent<S, ProofPreimageMarker, PedersenRandomness, D> {
        Intent::new(rng, None, None, vec![], vec![], vec![], None, ttl)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new<R: Rng + CryptoRng + ?Sized>(
        rng: &mut R,
        guaranteed_unshielded_offer: Option<UnshieldedOffer<S, D>>,
        fallible_unshielded_offer: Option<UnshieldedOffer<S, D>>,
        calls: Vec<ContractCallPrototype<D>>,
        updates: Vec<MaintenanceUpdate<D>>,
        deploys: Vec<ContractDeploy<D>>,
        dust_actions: Option<DustActions<S, ProofPreimageMarker, D>>,
        ttl: Timestamp,
    ) -> Intent<S, ProofPreimageMarker, PedersenRandomness, D> {
        let intent = Intent {
            guaranteed_unshielded_offer: guaranteed_unshielded_offer.map(|x| Sp::new(x)),
            fallible_unshielded_offer: fallible_unshielded_offer.map(|x| Sp::new(x)),
            actions: vec![].into(),
            dust_actions: dust_actions.map(Sp::new),
            ttl,
            binding_commitment: rng.r#gen(),
        };

        let intent = calls
            .into_iter()
            .fold(intent, |acc, x| acc.add_call::<ProofPreimage>(x));

        let intent = updates
            .into_iter()
            .fold(intent, |acc, x| acc.add_maintenance_update(x));

        deploys.into_iter().fold(intent, |acc, x| acc.add_deploy(x))
    }
}

impl<
    S: SignatureKind<D>,
    P: ProofKind<D>,
    B: Storable<D> + PedersenDowngradeable<D> + Serializable,
    D: DB,
> Intent<S, P, B, D>
{
    #[allow(clippy::result_large_err)]
    pub fn sign(
        mut self,
        rng: &mut (impl Rng + CryptoRng),
        segment_id: u16,
        guaranteed_signing_keys: &[SigningKey],
        fallible_signing_keys: &[SigningKey],
        dust_registration_signing_keys: &[SigningKey],
    ) -> Result<Self, MalformedTransaction<D>>
    where
        UnshieldedOffer<S, D>: Clone,
        P: ProofKind<D>,
    {
        let data = self
            .erase_proofs()
            .erase_signatures()
            .data_to_sign(segment_id);

        let mut sign_unshielded_offers = |unshielded_offer: &mut Option<
            Sp<UnshieldedOffer<S, D>, D>,
        >,
                                          signing_keys: &[SigningKey]|
         -> Result<(), MalformedTransaction<D>> {
            if let Some(offer) = unshielded_offer {
                let signatures: Vec<<S as SignatureKind<D>>::Signature<IntentSigningEnvelope<D>>> =
                    offer
                        .inputs
                        .iter()
                        .zip(signing_keys)
                        .map(|(spend, sk)| {
                            if spend.owner != sk.verifying_key() {
                                return Err(MalformedTransaction::<D>::IntentSignatureKeyMismatch);
                            }
                            Ok(S::sign(sk, rng, &data))
                        })
                        .collect::<Result<Vec<_>, _>>()?;

                let mut new = (*offer).deref().clone();
                new.add_signatures(signatures);
                *unshielded_offer = Some(Sp::new(new.clone()));
            }
            Ok(())
        };

        sign_unshielded_offers(
            &mut self.guaranteed_unshielded_offer,
            guaranteed_signing_keys,
        )?;
        sign_unshielded_offers(&mut self.fallible_unshielded_offer, fallible_signing_keys)?;

        if let Some(da) = self.dust_actions {
            let registrations = da
                .registrations
                .iter()
                .zip(dust_registration_signing_keys.iter())
                .map(|(reg, sk)| {
                    let mut reg = (*reg).clone();
                    if reg.night_key != sk.verifying_key() {
                        return Err(MalformedTransaction::<D>::IntentSignatureKeyMismatch);
                    }
                    reg.signature = Some(Sp::new(S::sign(sk, rng, &data)));
                    Ok(reg)
                })
                .collect::<Result<_, _>>()?;
            self.dust_actions = Some(Sp::new(DustActions {
                registrations,
                spends: da.spends.clone(),
                ctime: da.ctime,
            }));
        }

        Ok(self)
    }
}

impl<S: SignatureKind<D>, D: DB> Transaction<S, ProofPreimageMarker, PedersenRandomness, D> {
    pub fn new(
        network_id: impl Into<String>,
        intents: storage::storage::HashMap<
            u16,
            Intent<S, ProofPreimageMarker, PedersenRandomness, D>,
            D,
        >,
        guaranteed_coins: Option<ZswapOffer<ProofPreimage, D>>,
        fallible_coins: storage::storage::HashMap<u16, ZswapOffer<ProofPreimage, D>, D>,
    ) -> Self {
        let mut stx = StandardTransaction {
            network_id: network_id.into(),
            intents,
            guaranteed_coins: guaranteed_coins
                .map(|x| Sp::new(x.retarget_segment(GUARANTEED_SEGMENT))),
            fallible_coins: fallible_coins
                .into_iter()
                .map(|(seg, offer)| (seg, offer.retarget_segment(seg)))
                .collect(),
            binding_randomness: Default::default(),
        };
        stx.recompute_binding_randomness();
        Transaction::Standard(stx)
    }
}

#[derive_where::derive_where(Clone, Debug)]
pub struct PrePartitionContractCall<D: DB> {
    pub address: ContractAddress,
    pub entry_point: EntryPointBuf,
    pub op: ContractOperation,
    pub pre_transcript: PreTranscript<D>,
    pub private_transcript_outputs: Vec<AlignedValue>,
    pub input: AlignedValue,
    pub output: AlignedValue,
    pub communication_commitment_rand: Fr,
    pub key_location: KeyLocation,
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
#[serde(tag = "tag", content = "value", rename_all = "camelCase")]
pub enum SegmentSpecifier {
    First,
    GuaranteedOnly,
    Random,
    Specific(u16),
}

#[derive(Clone, Debug)]
pub struct ContractCallPrototype<D: DB> {
    pub address: ContractAddress,
    pub entry_point: EntryPointBuf,
    pub op: ContractOperation,
    pub guaranteed_public_transcript: Option<Transcript<D>>,
    pub fallible_public_transcript: Option<Transcript<D>>,
    pub private_transcript_outputs: Vec<AlignedValue>,
    pub input: AlignedValue,
    pub output: AlignedValue,
    pub communication_commitment_rand: Fr,
    pub key_location: KeyLocation,
}

pub trait ContractCallExt<D: DB> {
    type AssociatedProof;
    fn construct_proof(
        call: &ContractCallPrototype<D>,
        communication_commitment: Fr,
    ) -> ProofPreimageVersioned;
}

impl<D: DB> ContractCallExt<D> for ProofPreimage {
    type AssociatedProof = ProofPreimage;
    fn construct_proof(
        call: &ContractCallPrototype<D>,
        communication_commitment: Fr,
    ) -> ProofPreimageVersioned {
        let inputs = ValueReprAlignedValue(call.input.clone()).field_vec();
        let mut private_transcript = Vec::with_capacity(
            call.private_transcript_outputs
                .iter()
                .map(|o| o.value_only_field_size())
                .sum(),
        );
        for o in call.private_transcript_outputs.iter() {
            o.value_only_field_repr(&mut private_transcript);
        }
        let public_transcript_iter = call
            .guaranteed_public_transcript
            .iter()
            .flat_map(|t| t.program.iter_deref())
            .chain(
                call.fallible_public_transcript
                    .iter()
                    .flat_map(|t| t.program.iter_deref()),
            );

        let public_transcript_ops: Vec<_> = public_transcript_iter.collect();

        let mut public_transcript_inputs =
            Vec::with_capacity(public_transcript_ops.iter().map(|op| op.field_size()).sum());

        for op in &public_transcript_ops {
            op.field_repr(&mut public_transcript_inputs);
        }

        let mut public_transcript_outputs = Vec::new();
        for op in &public_transcript_ops {
            if let Op::Popeq { result, .. } = op {
                result.value_only_field_repr(&mut public_transcript_outputs);
            }
        }

        // This gets populated correctly during proving, ensuring it correctly uses the updated
        // transcripts and costs.
        let binding_input = 0u8.into();

        let proof = ProofPreimage {
            inputs,
            private_transcript,
            public_transcript_inputs,
            public_transcript_outputs,
            binding_input,
            communications_commitment: Some((
                communication_commitment,
                call.communication_commitment_rand,
            )),
            key_location: call.key_location.clone(),
        };

        ProofPreimageVersioned::V2(std::sync::Arc::new(proof))
    }
}

impl<D: DB> ContractCall<ProofPreimageMarker, D> {
    pub fn new(
        address: ContractAddress,
        entry_point: EntryPointBuf,
        guaranteed_transcript: Option<Transcript<D>>,
        fallible_transcript: Option<Transcript<D>>,
        communication_commitment: Fr,
        proof: ProofPreimageVersioned,
    ) -> Self {
        ContractCall {
            address,
            entry_point,
            guaranteed_transcript: guaranteed_transcript.map(Sp::new),
            fallible_transcript: fallible_transcript.map(Sp::new),
            communication_commitment,
            proof,
        }
    }
}

impl<S: SignatureKind<D>, D: DB> Intent<S, ProofPreimageMarker, PedersenRandomness, D> {
    pub fn add_call<P>(&self, call: ContractCallPrototype<D>) -> Self
    where
        P: ContractCallExt<D>,
    {
        let mut io_repr = Vec::with_capacity(
            call.input.value_only_field_size() + call.output.value_only_field_size(),
        );
        call.input.value_only_field_repr(&mut io_repr);
        call.output.value_only_field_repr(&mut io_repr);
        let communication_commitment =
            transient_commit(&io_repr, call.communication_commitment_rand);

        let proof = P::construct_proof(&call, communication_commitment);
        let call = ContractCall {
            address: call.address,
            entry_point: call.entry_point,
            guaranteed_transcript: call.guaranteed_public_transcript.map(|x| Sp::new(x)),
            fallible_transcript: call.fallible_public_transcript.map(|x| Sp::new(x)),
            communication_commitment,
            proof,
        };
        // We add the call:
        // - Directly before the first call *claimed* by it
        // - At the end otherwise.
        let mut actions = Vec::new();
        let mut already_inserted = false;
        fn references<D: DB>(
            caller: &ContractCall<ProofPreimageMarker, D>,
            callee: &ContractAction<ProofPreimageMarker, D>,
        ) -> bool {
            match callee {
                ContractAction::Call(call) => caller
                    .guaranteed_transcript
                    .iter()
                    .chain(caller.fallible_transcript.iter())
                    .flat_map(|t| {
                        t.effects
                            .claimed_contract_calls
                            .iter()
                            .map(|x| (*x).deref().into_inner())
                    })
                    .any(|(_seq, addr, ep_hash, cc)| {
                        addr == call.address
                            && cc == call.communication_commitment
                            && ep_hash == call.entry_point.ep_hash()
                    }),
                _ => false,
            }
        }
        for c in self.actions.iter_deref().cloned() {
            if !already_inserted && references(&call, &c) {
                actions.push(call.clone().into());
                already_inserted = true;
            }
            actions.push(c);
        }
        if !already_inserted {
            actions.push(call.into());
        }
        Intent {
            guaranteed_unshielded_offer: self.guaranteed_unshielded_offer.clone(),
            fallible_unshielded_offer: self.fallible_unshielded_offer.clone(),
            actions: actions.into(),
            dust_actions: self.dust_actions.clone(),
            ttl: self.ttl,
            binding_commitment: self.binding_commitment,
        }
    }

    pub fn add_deploy(&self, deploy: ContractDeploy<D>) -> Self {
        Intent {
            guaranteed_unshielded_offer: self.guaranteed_unshielded_offer.clone(),
            fallible_unshielded_offer: self.fallible_unshielded_offer.clone(),
            actions: self
                .actions
                .iter_deref()
                .cloned()
                .chain(once(deploy.into()))
                .collect(),
            dust_actions: self.dust_actions.clone(),
            ttl: self.ttl,
            binding_commitment: self.binding_commitment,
        }
    }

    pub fn add_maintenance_update(&self, upd: MaintenanceUpdate<D>) -> Self {
        Intent {
            guaranteed_unshielded_offer: self.guaranteed_unshielded_offer.clone(),
            fallible_unshielded_offer: self.fallible_unshielded_offer.clone(),
            actions: self
                .actions
                .iter_deref()
                .cloned()
                .chain(once(upd.into()))
                .collect(),
            dust_actions: self.dust_actions.clone(),
            ttl: self.ttl,
            binding_commitment: self.binding_commitment,
        }
    }

    pub fn binding_randomness(&self) -> PedersenRandomness {
        self.binding_commitment
    }
}

#[derive(Debug)]
pub struct PreTranscript<D: DB> {
    pub context: QueryContext<D>,
    pub program: Vec<Op<ResultModeVerify, D>>,
    pub comm_comm: Option<Fr>,
}

impl<D: DB> Clone for PreTranscript<D> {
    fn clone(&self) -> Self {
        PreTranscript {
            context: self.context.clone(),
            program: self.program.clone(),
            comm_comm: self.comm_comm,
        }
    }
}

const LAST_COMMITMENT_INDEX: u64 = (1 << ZSWAP_TREE_HEIGHT) - 1;

impl<D: DB> PreTranscript<D> {
    /// The chain assigns an output's index on inclusion, so the client rehearses
    /// with a stand-in, typically the output's ordinal. A contract that stores
    /// the index then writes as many bytes as the stand-in takes, and the real
    /// index may take more. Packing the indices onto the last leaves of the tree,
    /// in their original order, makes each as wide as an index can get, so the
    /// rehearsal declares enough bytes for any tree.
    fn with_widest_commitment_indices(&self) -> Self {
        let mut context = self.context.clone();
        let mut by_index = context.call_context.com_indices.iter().collect::<Vec<_>>();
        by_index.sort_by_key(|(commitment, index)| (**index, *commitment));
        let last_leaves = (LAST_COMMITMENT_INDEX + 1 - by_index.len() as u64)..;
        context.call_context.com_indices = by_index
            .into_iter()
            .map(|(commitment, _)| commitment)
            .zip(last_leaves)
            .collect();
        PreTranscript {
            context,
            program: self.program.clone(),
            comm_comm: self.comm_comm,
        }
    }

    fn no_checkpoints(&self) -> usize {
        self.program
            .iter()
            .filter(|op| matches!(op, Op::Ckpt))
            .count()
    }

    // 0-indexed!
    fn run_to_ckpt_no(
        &self,
        mut n: usize,
        params: &LedgerParameters,
    ) -> Result<QueryResults<ResultModeVerify, D>, TranscriptRejected<D>> {
        n += 1;
        let prog = self
            .program
            .iter()
            .cloned()
            .take_while(|op| {
                if matches!(op, Op::Ckpt) {
                    n -= 1;
                    n != 0
                } else {
                    true
                }
            })
            .collect::<Vec<_>>();
        self.context
            .query(&prog, None, &params.cost_model.runtime_cost_model)
    }

    // 1-indexed!
    #[allow(clippy::type_complexity)]
    fn split_at(
        &self,
        mut n: usize,
        params: &LedgerParameters,
    ) -> Result<(Option<Transcript<D>>, Option<Transcript<D>>), TranscriptRejected<D>> {
        let mut prog_guaranteed = Vec::new();
        let mut prog_fallible = Vec::new();
        for op in self.program.iter() {
            if n > 0 && matches!(op, Op::Ckpt) {
                n -= 1;
            }
            if n == 0 {
                prog_fallible.push(op.clone());
            } else {
                prog_guaranteed.push(op.clone());
            }
        }
        let guaranteed_res = self.context.query(
            &prog_guaranteed,
            None,
            &params.cost_model.runtime_cost_model,
        )?;
        let mut continuation_context = guaranteed_res.context.clone();
        continuation_context.effects = Effects::default();
        let fallible_res = continuation_context.query(
            &prog_fallible,
            None,
            &params.cost_model.runtime_cost_model,
        )?;
        let mk_transcript = |prog: Vec<Op<ResultModeVerify, D>>,
                             res: QueryResults<ResultModeVerify, D>| {
            if prog.is_empty() {
                None
            } else {
                Some(Transcript {
                    gas: res.gas_heuristic(params, false),
                    effects: res.context.effects.clone(),
                    program: prog.into(),
                    version: Some(Sp::new(Transcript::<D>::VERSION)),
                })
            }
        };
        Ok((
            mk_transcript(prog_guaranteed, guaranteed_res),
            mk_transcript(prog_fallible, fallible_res),
        ))
    }
}

trait QueryResultsExt {
    fn gas_heuristic(&self, params: &LedgerParameters, include_external: bool) -> RunningCost;
}

// gas_heuristic and the reserve, `mock_tx(calls, tx_shape).time_to_dismiss`,
// together approximate Transaction::time_to_dismiss for use during transcript
// partitioning, before the final Transaction exists. The reserve covers the
// validation cost of every part, the calls' proof verification and VK reads
// included, and the application cost of the non-call parts. gas_heuristic
// covers, per call kept guaranteed, its gas with 20% overhead and its
// application cost: state fetch/store and stack setup for its effects.

impl<D: DB> QueryResultsExt for QueryResults<ResultModeVerify, D> {
    fn gas_heuristic(&self, params: &LedgerParameters, include_external: bool) -> RunningCost {
        const TIME_OVERHEAD: f64 = 1.2;
        // Declared deletions are a lower bound, and any shortfall is priced as
        // net growth; overhead is therefore applied to net growth, not churn.
        const NET_WRITE_OVERHEAD: f64 = 1.2;
        const DELETION_UNDERESTIMATE: f64 = 0.95;
        let actual = self.gas_cost;
        let churned = actual.bytes_written.min(actual.bytes_deleted);
        let net_written = actual.bytes_written - churned;
        let transcript_gas_cost_with_overhead = RunningCost {
            bytes_written: churned
                .saturating_add((net_written as f64 * NET_WRITE_OVERHEAD).ceil() as u64),
            bytes_deleted: (actual.bytes_deleted as f64 * DELETION_UNDERESTIMATE).floor() as u64,
            ..actual * TIME_OVERHEAD
        };
        if !include_external {
            return transcript_gas_cost_with_overhead;
        }
        let model = &params.cost_model;

        let mut application = RunningCost::ZERO;
        application += model.map_index(EXPECTED_CONTRACT_DEPTH)
            + model.map_insert(EXPECTED_CONTRACT_DEPTH, true)
            + model.map_index(1)
            + model.map_insert(1, true);
        application += model.stack_setup_cost_for_effects(&self.context.effects);
        application.compute_time = application.compute_time * model.guaranteed_factor;

        transcript_gas_cost_with_overhead + application
    }
}

pub fn communication_commitment(input: AlignedValue, output: AlignedValue, rand: Fr) -> Fr {
    transient_commit(
        &ValueReprAlignedValue(AlignedValue::concat([&input, &output])),
        rand,
    )
}

pub type TranscriptPair<D> = (Option<Transcript<D>>, Option<Transcript<D>>);

pub fn partition_transcripts<D: DB>(
    calls: &[PreTranscript<D>],
    params: &LedgerParameters,
) -> Result<Vec<TranscriptPair<D>>, PartitionFailure<D>> {
    partition_transcripts_inner(calls, params, |derived| derived)
}

/// Splits each call's transcript into its guaranteed and fallible parts.
/// `expected_tx_shape`'s parts add their bytes to the time-to-dismiss allowance
/// and their cost is reserved from it.
pub fn partition_transcripts_with_tx_shape<D: DB>(
    calls: &[PreTranscript<D>],
    params: &LedgerParameters,
    expected_tx_shape: &TransactionShape,
) -> Result<Vec<TranscriptPair<D>>, PartitionFailure<D>> {
    partition_transcripts_inner(calls, params, |_| expected_tx_shape.clone())
}

/// `choose_tx_shape` gets the shape the calls imply and returns the one to size
/// the partition for.
fn partition_transcripts_inner<D: DB>(
    calls: &[PreTranscript<D>],
    params: &LedgerParameters,
    choose_tx_shape: impl FnOnce(TransactionShape) -> TransactionShape,
) -> Result<Vec<TranscriptPair<D>>, PartitionFailure<D>> {
    if calls.is_empty() {
        return Ok(Vec::new());
    }
    let n = calls.len();
    let calls = calls
        .iter()
        .map(PreTranscript::with_widest_commitment_indices)
        .collect::<Vec<_>>();
    // Step 1: Generate a call graph between `calls`. Assert that this is a forest (no cycles,
    //      no multiple parents).

    // Gather full runs to observe all calls
    let no_ckpts = calls
        .iter()
        .map(PreTranscript::no_checkpoints)
        .collect::<Vec<_>>();
    let full_runs = calls
        .iter()
        .zip(no_ckpts.iter())
        .map(|(pt, n)| pt.run_to_ckpt_no(*n, params))
        .collect::<Result<Vec<_>, _>>()?;
    // Graph is a Vec of Vecs, where call_graph[i] = [a, b, c] means that calls[i] calls calls[a],
    // calls[b] and calls[c]
    let call_graph = full_runs
        .iter()
        .map(|qr| {
            let claimed_commitments = qr
                .context
                .effects
                .claimed_contract_calls
                .iter()
                .map(|sp| (*sp).deref().into_inner().3)
                .collect::<Vec<_>>();
            (0..n)
                .filter(|i| {
                    calls[*i]
                        .comm_comm
                        .map(|comm| claimed_commitments.contains(&comm))
                        .unwrap_or(false)
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    // Identify cycles by starting at each node in turn, and asserting that the set of reachable
    // nodes does not contain itself
    //
    // Note that this also identifies some non-cycles, such as A -> B, A -> C, B -> D, B -> D,
    // but this is fine as these are also not allowed.
    for i in 0..n {
        let mut visited = vec![i];
        let mut reachable = call_graph[i].clone();
        while let Some(next) = reachable.pop() {
            if visited.contains(&next) {
                return Err(PartitionFailure::NonForest);
            }
            visited.push(next);
            reachable.extend(&call_graph[next]);
        }
    }

    // Step 2: Identify root nodes in the DAG.
    // Identify multiple parents at the same time. For each node, count how many other nodes
    // reference it. If that's 0, it's a root node. If it's >1, we don't have a forest.
    let n_callers = (0..n)
        .map(|i| (0..n).filter(|j| call_graph[*j].contains(&i)).count())
        .collect::<Vec<_>>();
    if n_callers.iter().any(|n| *n > 1) {
        return Err(PartitionFailure::NonForest);
    }
    let root_nodes = n_callers
        .iter()
        .enumerate()
        .filter(|(_, n)| **n == 0)
        .map(|(i, _)| i)
        .collect::<Vec<_>>();

    // Step 3: For each root node, compute the guaranteed section budget of its closure.
    let closures = root_nodes
        .iter()
        .map(|r| {
            let mut visited = vec![];
            let mut frontier = vec![*r];
            while let Some(item) = frontier.pop() {
                visited.push(item);
                frontier.extend(&call_graph[item]);
            }
            visited
        })
        .collect::<Vec<_>>();
    let tx_shape = choose_tx_shape(TransactionShape::for_effects(
        full_runs
            .iter()
            .map(|run| (run.context.address, &run.context.effects)),
    ));
    let mock = mock_tx(&calls, &tx_shape);
    let reserve = mock
        .time_to_dismiss(params)
        .expect("mock transactions are disjoint by construction");
    let allowance = CostDuration::max(
        params.limits.time_to_dismiss_per_byte * mock.serialized_size() as u64,
        params.limits.min_time_to_dismiss,
    );
    if reserve >= allowance {
        return Ok(calls
            .iter()
            .map(|call| call.split_at(0, params))
            .collect::<Result<Vec<_>, _>>()?);
    }
    let calls_budget = allowance - reserve;
    let closure_sizes = closures
        .iter()
        .map(|closure| mock_calls(closure.iter().map(|i| &calls[*i])).serialized_size())
        .collect::<Vec<_>>();
    let total_size = closure_sizes.iter().sum::<usize>() as f64;
    let closure_budgets = closure_sizes
        .iter()
        .map(|size| calls_budget * (*size as f64 / total_size))
        .collect::<Vec<_>>();

    // Step 4: Partition the root nodes:
    //      4a: Split root nodes on `ckpt`s.
    //      4b. Run up to `ckpt` and determine calls that are included. Run those entirely.
    //      4c. Determine the latest `ckpt` for which this fits into the previously computed
    //          budget.

    // preliminary_results is a vec that, for each root, contains a pair consisting of:
    // the number of checkpoints to include in the guaranteed section (0 indicating entirely in the
    // fallible section), and a vec of the indices of the callees that are included in the
    // guaranteed section (a subset of the corresponding entry in call_graph).
    let preliminary_results = root_nodes
        .iter()
        .enumerate()
        .map(|(root_n, &root)| {
            // Run through the number of sections to put into the guaranteed transcript.
            // Start with the number of checkpoints + 1 (we have one more section than checkpoints),
            // end at 1. If none pass, 0 make it into the guaranteed section.
            for n in (1..no_ckpts[root] + 2).rev() {
                let partial_res = calls[root].run_to_ckpt_no(n - 1, params)?;
                let claimed = partial_res
                    .context
                    .effects
                    .claimed_contract_calls
                    .iter()
                    .map(|sp| (*sp).deref().into_inner().3)
                    .collect::<Vec<_>>();
                let claimed_idx = calls
                    .iter()
                    .enumerate()
                    .filter(|(_, pt)| {
                        pt.comm_comm
                            .map(|cc| claimed.contains(&cc))
                            .unwrap_or(false)
                    })
                    .map(|(i, _)| i)
                    .collect::<Vec<_>>();
                let mut required_budget = partial_res.gas_heuristic(params, true).max_time();
                let mut frontier = claimed_idx.clone();
                while let Some(next) = frontier.pop() {
                    required_budget += full_runs[next].gas_heuristic(params, true).max_time();
                    frontier.extend(&call_graph[next]);
                }
                if required_budget <= closure_budgets[root_n] {
                    return Ok((n, claimed_idx));
                }
            }
            Ok::<_, PartitionFailure<D>>((0, vec![]))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut sections_in_guaranteed = vec![0; n];
    for (root_n, &root) in root_nodes.iter().enumerate() {
        sections_in_guaranteed[root] = preliminary_results[root_n].0;
        let mut frontier = preliminary_results[root_n].1.clone();
        while let Some(next) = frontier.pop() {
            sections_in_guaranteed[next] = no_ckpts[next] + 1;
            frontier.extend(&call_graph[next]);
        }
    }

    Ok(sections_in_guaranteed
        .into_iter()
        .enumerate()
        .map(|(i, sections)| calls[i].split_at(sections, params))
        .collect::<Result<Vec<_>, _>>()?)
}

#[cfg(test)]
mod partition_tests {
    use super::{has_claims_in_fallible, with_fallibility};
    use base_crypto::hash::HashOutput;
    use coin_structure::coin::{Commitment as CoinCommitment, Nullifier};
    use onchain_runtime::context::Effects;
    use storage::db::InMemoryDB;

    fn nul(b: u8) -> Nullifier {
        Nullifier(HashOutput([b; 32]))
    }

    fn com(b: u8) -> CoinCommitment {
        CoinCommitment(HashOutput([b; 32]))
    }

    /// Effects claiming one nullifier and one shielded spend.
    fn effects(n: u8, c: u8) -> Effects<InMemoryDB> {
        let mut e = Effects::<InMemoryDB>::default();
        e.claimed_nullifiers = e.claimed_nullifiers.insert(nul(n));
        e.claimed_shielded_spends = e.claimed_shielded_spends.insert(com(c));
        e
    }

    #[test]
    fn has_claims_in_fallible_matches_any_claiming_transcript() {
        let a = effects(1, 10);
        let b = effects(2, 20);
        let all = [&a, &b];

        // Claimed by the first, the second, and by neither.
        let items = [nul(1), nul(2), nul(3)];
        let flags = has_claims_in_fallible(&items, &all, |e, n| e.claimed_nullifiers.member(n));
        assert_eq!(flags, vec![true, true, false]);

        // No fallible transcripts at all => nothing is fallible.
        let none: [&Effects<InMemoryDB>; 0] = [];
        let flags = has_claims_in_fallible(&items, &none, |e, n| e.claimed_nullifiers.member(n));
        assert_eq!(flags, vec![false, false, false]);
    }

    /// A nullifier claim must not make an output fallible, nor vice versa: the flags are
    /// per-item-kind, and crossing them would move coins into the wrong offer.
    #[test]
    fn has_claims_in_fallible_does_not_cross_item_kinds() {
        let a = effects(1, 10);
        let all = [&a];

        let commitments = [com(10), com(1)];
        let flags = has_claims_in_fallible(&commitments, &all, |e, c| {
            e.claimed_shielded_spends.member(c) || e.claimed_shielded_receives.member(c)
        });
        // com(1) shares its bytes with the claimed *nullifier*, but is not a claimed spend.
        assert_eq!(flags, vec![true, false]);
    }

    #[test]
    fn with_fallibility_partitions_and_preserves_order() {
        let items = ['a', 'b', 'c', 'd'];
        let flags = [false, true, false, true];
        assert_eq!(with_fallibility(&items, &flags, false), vec!['a', 'c']);
        assert_eq!(with_fallibility(&items, &flags, true), vec!['b', 'd']);

        // Every item lands in exactly one of the two offers.
        let guaranteed = with_fallibility(&items, &flags, false);
        let fallible = with_fallibility(&items, &flags, true);
        assert_eq!(guaranteed.len() + fallible.len(), items.len());
    }
}

#[cfg(test)]
mod gas_heuristic_tests {
    use super::{LAST_COMMITMENT_INDEX, PreTranscript, QueryResultsExt};
    use crate::structure::INITIAL_PARAMETERS;
    use base_crypto::cost_model::{CostDuration, RunningCost, SyntheticCost};
    use base_crypto::fab::AlignedValue;
    use base_crypto::hash::HashOutput;
    use coin_structure::coin::Commitment;
    use coin_structure::contract::ContractAddress;
    use onchain_runtime::context::{QueryContext, QueryResults};
    use onchain_runtime::ops::{Key, Op, expect_checked};
    use onchain_runtime::result_mode::ResultModeVerify;
    use onchain_runtime::state::{ChargedState, StateValue};
    use proptest::prelude::*;
    use storage::arena::Sp;
    use storage::db::InMemoryDB;
    use storage::storage::Map;

    fn declared(actual: RunningCost) -> RunningCost {
        let res = QueryResults::<ResultModeVerify, InMemoryDB> {
            context: QueryContext::new(
                ChargedState::new(StateValue::Null),
                ContractAddress::default(),
            ),
            events: Vec::new(),
            gas_cost: actual,
        };
        res.gas_heuristic(&INITIAL_PARAMETERS, false)
    }

    proptest! {
        // The declaration must survive the post-execution check. Padding writes in
        // proportion to churn would price churned bytes as net growth.
        #[test]
        fn declared_writes_pad_net_growth_only(
            written in 0u64..10_000_000,
            deleted in 0u64..10_000_000,
        ) {
            let actual = RunningCost {
                read_time: CostDuration::ZERO,
                compute_time: CostDuration::ZERO,
                bytes_written: written,
                bytes_deleted: deleted,
            };
            let declared = declared(actual);
            prop_assert!(declared.bytes_deleted <= actual.bytes_deleted);
            prop_assert!(declared.bytes_written >= actual.bytes_written);

            let actual_net = SyntheticCost::from(actual).bytes_written;
            prop_assert!(declared.bytes_written <= actual.bytes_written + actual_net.div_ceil(5));
        }
    }

    const CONTEXT_COM_INDICES: u8 = 1;

    fn store_coin_index(commitment: Commitment) -> Vec<Op<ResultModeVerify, InMemoryDB>> {
        vec![
            Op::Dup { n: 2 },
            Op::Idx {
                cached: false,
                push_path: false,
                path: [
                    Key::Value(CONTEXT_COM_INDICES.into()),
                    Key::Value(expect_checked(AlignedValue::from(commitment))),
                ]
                .into_iter()
                .collect(),
            },
            Op::Addi { immediate: 0 },
            Op::Swap { n: 0 },
            Op::Pop,
        ]
    }

    fn coin_context(
        commitment: Commitment,
        stored_index: u64,
        context_index: u64,
    ) -> QueryContext<InMemoryDB> {
        let mut context = QueryContext::new(
            ChargedState::new(StateValue::Cell(Sp::new(AlignedValue::from(stored_index)))),
            ContractAddress::default(),
        );
        context.call_context.com_indices = Map::new().insert(commitment, context_index);
        context
    }

    fn overwrite_coin_index(
        context: QueryContext<InMemoryDB>,
        commitment: Commitment,
    ) -> RunningCost {
        context
            .query(
                &store_coin_index(commitment),
                None,
                &INITIAL_PARAMETERS.cost_model.runtime_cost_model,
            )
            .unwrap()
            .gas_cost
    }

    #[test]
    fn declared_writes_cover_the_real_commitment_index() {
        const HELD_COIN_INDEX: u64 = 1;
        const OUTPUT_ORDINAL: u64 = 0;
        const FRESH_TREE_INDEX: u64 = 2;

        let commitment = Commitment(HashOutput([7; 32]));
        let rehearsal = PreTranscript {
            context: coin_context(commitment, HELD_COIN_INDEX, OUTPUT_ORDINAL),
            program: store_coin_index(commitment),
            comm_comm: None,
        };
        let ordinal_declared =
            declared(overwrite_coin_index(rehearsal.context.clone(), commitment));
        let widened_declared = declared(overwrite_coin_index(
            rehearsal.with_widest_commitment_indices().context,
            commitment,
        ));

        let fresh_tree = overwrite_coin_index(
            coin_context(commitment, HELD_COIN_INDEX, FRESH_TREE_INDEX),
            commitment,
        );
        assert!(fresh_tree.within_bounds(&widened_declared));
        assert!(
            fresh_tree.within_bounds(&ordinal_declared),
            "this assertion is here just to bring attention, but it might be fine that this fails"
        );
        let full_tree = overwrite_coin_index(
            coin_context(commitment, HELD_COIN_INDEX, LAST_COMMITMENT_INDEX),
            commitment,
        );
        assert!(
            !full_tree.within_bounds(&ordinal_declared),
            "the ordinal stand-in alone does not cover the last tree index; if this fails adjust\
            this test; consider removing 'with_widest_commitment_indices' if other code change now\
            covers this case"
        );
        assert!(
            full_tree.within_bounds(&widened_declared),
            "declared {} written bytes, the last tree index writes {}",
            widened_declared.bytes_written,
            full_tree.bytes_written
        );
    }

    #[test]
    fn widest_commitment_indices_keep_their_order_and_end_at_the_last_leaf() {
        let first = Commitment(HashOutput([1; 32]));
        let second = Commitment(HashOutput([2; 32]));
        let third = Commitment(HashOutput([3; 32]));
        let mut context = QueryContext::new(
            ChargedState::new(StateValue::Null),
            ContractAddress::default(),
        );
        context.call_context.com_indices = Map::new()
            .insert(first, 0)
            .insert(second, 1)
            .insert(third, LAST_COMMITMENT_INDEX);
        let widened = PreTranscript {
            context,
            program: Vec::<Op<ResultModeVerify, InMemoryDB>>::new(),
            comm_comm: None,
        }
        .with_widest_commitment_indices();

        let index = |commitment| {
            *widened
                .context
                .call_context
                .com_indices
                .get(&commitment)
                .unwrap()
        };
        assert_eq!(index(first), LAST_COMMITMENT_INDEX - 2);
        assert_eq!(index(second), LAST_COMMITMENT_INDEX - 1);
        assert_eq!(index(third), LAST_COMMITMENT_INDEX);
    }
}
