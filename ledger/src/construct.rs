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

use crate::dust::DustActions;
use crate::error::{MalformedTransaction, PartitionFailure};
use crate::structure::SegIntent;
use crate::structure::{
    ContractAction, ContractCall, ContractDeploy, Intent, LedgerParameters, MaintenanceUpdate,
    PROOF_SIZE, ProofPreimageMarker, ProofPreimageVersioned, SignatureKind, SignaturesValue,
    SingleUpdate, StandardTransaction, Transaction, UnshieldedOffer,
};
use crate::structure::{PedersenDowngradeable, ProofKind};
use base_crypto::cost_model::CostDuration;
use base_crypto::cost_model::RunningCost;
use base_crypto::fab::AlignedValue;
use base_crypto::signatures::Signature;
use base_crypto::signatures::SigningKey;
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
use zswap::Offer;
use zswap::Offer as ZswapOffer;

impl<S: SignatureKind<D>, D: DB> Transaction<S, ProofPreimageMarker, PedersenRandomness, D> {
    pub fn from_intents(
        network_id: impl Into<String>,
        intents: storage::storage::HashMap<
            u16,
            Intent<S, ProofPreimageMarker, PedersenRandomness, D>,
            D,
        >,
    ) -> Self {
        Self::new(network_id, intents, None, std::collections::HashMap::new())
    }
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
        guaranteed_coins: Option<Offer<ProofPreimage, D>>,
        fallible_coins: std::collections::HashMap<u16, Offer<ProofPreimage, D>>,
    ) -> Self {
        StandardTransaction {
            network_id: network_id.into(),
            binding_randomness: Self::binding_randomness(
                &guaranteed_coins,
                &fallible_coins,
                &intents,
            ),
            intents,
            guaranteed_coins: guaranteed_coins.map(|x| Sp::new(x)),
            fallible_coins: fallible_coins.into_iter().collect(),
        }
    }

    fn binding_randomness(
        guaranteed_coins: &Option<Offer<ProofPreimage, D>>,
        fallible_coins: &std::collections::HashMap<u16, Offer<ProofPreimage, D>>,
        intents: &storage::storage::HashMap<
            u16,
            Intent<S, ProofPreimageMarker, PedersenRandomness, D>,
            D,
        >,
    ) -> PedersenRandomness {
        guaranteed_coins
            .as_ref()
            .map(|o| o.binding_randomness())
            .unwrap_or_else(|| PedersenRandomness::from(0))
            + fallible_coins
                .values()
                .fold(PedersenRandomness::from(0), |acc, o| {
                    acc + o.binding_randomness()
                })
            + intents
                .values()
                .map(|i| i.binding_randomness())
                .fold(0.into(), |a, b| a + b)
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

        let mut sign_unshielded_offers =
            |unshielded_offer: &mut Option<Sp<UnshieldedOffer<S, D>, D>>,
             signing_keys: &[SigningKey]|
             -> Result<(), MalformedTransaction<D>> {
                if let Some(offer) = unshielded_offer {
                    let signatures: Vec<<S as SignatureKind<D>>::Signature<SegIntent<D>>> = offer
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
                    let mut reg = (&*reg).clone();
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
        fallible_coins: std::collections::HashMap<u16, Offer<ProofPreimage, D>>,
    ) -> Self {
        let binding_randomness =
            StandardTransaction::binding_randomness(&guaranteed_coins, &fallible_coins, &intents);
        Transaction::Standard(StandardTransaction {
            network_id: network_id.into(),
            intents,
            guaranteed_coins: guaranteed_coins.map(|x| Sp::new(x)),
            fallible_coins: fallible_coins.into_iter().collect(),
            binding_randomness,
        })
    }
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

        ProofPreimageVersioned::V1(proof)
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
pub struct PreTranscript<'a, D: DB> {
    pub context: &'a QueryContext<D>,
    pub program: &'a [Op<ResultModeVerify, D>],
    pub comm_comm: Option<Fr>,
}

impl<D: DB> PreTranscript<'_, D> {
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

    fn guaranteed_budget(&self, params: &LedgerParameters) -> CostDuration {
        let est_size = PROOF_SIZE
            + Serializable::serialized_size(&(
                ContractAddress::default(),
                Effects::<D>::default(),
                &self.program.iter().collect::<Vec<_>>(),
                &self.comm_comm,
            ));
        params.limits.time_to_dismiss_per_byte * est_size as u64
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
        for op in self.program {
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
                    gas: res.gas_heuristic(),
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
    fn gas_heuristic(&self) -> RunningCost;
}

impl<D: DB> QueryResultsExt for QueryResults<ResultModeVerify, D> {
    fn gas_heuristic(&self) -> RunningCost {
        self.gas_cost * 1.2
    }
}

pub fn communication_commitment(input: AlignedValue, output: AlignedValue, rand: Fr) -> Fr {
    transient_commit(&AlignedValue::concat([&input, &output]), rand)
}

pub type TranscriptPair<D> = (Option<Transcript<D>>, Option<Transcript<D>>);

pub fn partition_transcripts<D: DB>(
    calls: &[PreTranscript<'_, D>],
    params: &LedgerParameters,
) -> Result<Vec<TranscriptPair<D>>, PartitionFailure<D>> {
    let n = calls.len();
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
    let closure_budgets = closures
        .iter()
        .map(|closure| {
            closure
                .iter()
                .map(|i| calls[*i].guaranteed_budget(params))
                .sum::<CostDuration>()
        })
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
                let mut required_budget = partial_res.gas_heuristic().max_time();
                let mut frontier = claimed_idx.clone();
                while let Some(next) = frontier.pop() {
                    required_budget += full_runs[next].gas_heuristic().max_time();
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
