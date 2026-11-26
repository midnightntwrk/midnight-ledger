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

use super::tx_shape::SectionShape;
use super::{PreTranscript, TransactionShape};
use crate::dust::{
    DUST_SPEND_PROOF_SIZE, DustActions, DustCommitment, DustNullifier, DustPublicKey,
    DustRegistration, DustSpend,
};
use crate::structure::{
    ContractAction, ContractCall, Intent, IntentHash, MIN_PROOF_SIZE, ProofKind, ProofMarker,
    ProofVersioned, SignatureKind, StandardTransaction, Transaction, UnshieldedOffer, UtxoOutput,
    UtxoSpend,
};
use crate::structure::{Signature, SigningKey};
use base_crypto::cost_model::RunningCost;
use base_crypto::hash::{HashOutput, PERSISTENT_HASH_BYTES};
use base_crypto::time::Timestamp;
use coin_structure::coin::{Commitment, NIGHT, Nullifier, UnshieldedTokenType, UserAddress};
use coin_structure::contract::ContractAddress;
use onchain_runtime::context::Effects;
use onchain_runtime::state::EntryPointBuf;
use onchain_runtime::transcript::Transcript;
use rand::SeedableRng;
use rand::rngs::StdRng;
use std::iter::{once, repeat_n};
use std::sync::Arc;
use storage::Storable;
use storage::arena::Sp;
use storage::db::DB;
use storage::storage::Array;
use transient_crypto::commitment::{Pedersen, PedersenRandomness, PureGeneratorPedersen};
use transient_crypto::curve::Fr;
use transient_crypto::proofs::Proof;
use zswap::{
    Input as ZswapInput, Offer as ZswapOffer, Output as ZswapOutput, Transient as ZswapTransient,
};

const NON_NIGHT_PLACEHOLDER_TYPE: UnshieldedTokenType = UnshieldedTokenType(HashOutput([0xff; 32]));

/// The calls as an intent stores them, with the smallest bodies they can have and a
/// placeholder proof of `MIN_PROOF_SIZE` bytes each.
pub(crate) fn mock_calls<'a, D: DB + 'a>(
    calls: impl IntoIterator<Item = &'a PreTranscript<D>>,
) -> Array<ContractAction<ProofMarker, D>, D> {
    calls
        .into_iter()
        .enumerate()
        .map(|(i, call)| {
            // The node reads a verifier key once per address, entry point and proof
            // version. A `PreTranscript` has no entry point, so each call gets its own
            // address and the mock reserves a read per call, never fewer than the real
            // transaction. The address also keeps identical calls from merging into
            // one node.
            ContractAction::Call(Sp::new(ContractCall {
                address: ContractAddress(distinct_hash(i)),
                entry_point: EntryPointBuf(Vec::new()),
                guaranteed_transcript: None,
                fallible_transcript: Some(Sp::new(Transcript {
                    gas: RunningCost::ZERO,
                    effects: Effects::default(),
                    program: Array::from(&call.program),
                    version: Some(Sp::new(Transcript::<D>::VERSION)),
                })),
                communication_commitment: call.comm_comm.unwrap_or_default(),
                proof: ProofVersioned::V2(Proof(vec![0; MIN_PROOF_SIZE])),
            }))
        })
        .collect()
}

/// `actions`, the transaction's contract actions other than the calls being
/// partitioned, as the mock carries them: each call keeps its transcripts and
/// gets a placeholder proof of `MIN_PROOF_SIZE` bytes.
pub(crate) fn mock_actions<P: ProofKind<D>, D: DB>(
    actions: impl IntoIterator<Item = ContractAction<P, D>>,
) -> Vec<ContractAction<ProofMarker, D>> {
    actions
        .into_iter()
        .map(|action| match action {
            ContractAction::Call(call) => ContractAction::Call(Sp::new(ContractCall {
                address: call.address,
                entry_point: call.entry_point.clone(),
                guaranteed_transcript: call.guaranteed_transcript.clone(),
                fallible_transcript: call.fallible_transcript.clone(),
                communication_commitment: call.communication_commitment,
                proof: ProofVersioned::V2(Proof(vec![0; MIN_PROOF_SIZE])),
            })),
            ContractAction::Deploy(deploy) => ContractAction::Deploy(deploy),
            ContractAction::Maintain(update) => ContractAction::Maintain(update),
        })
        .collect()
}

/// The contract actions of `tx`, as [`mock_actions`] carries them.
pub(crate) fn mock_actions_of<S, P, B, D>(
    tx: &StandardTransaction<S, P, B, D>,
) -> Vec<ContractAction<ProofMarker, D>>
where
    S: SignatureKind<D>,
    P: ProofKind<D>,
    B: Storable<D> + Clone,
    D: DB,
{
    mock_actions(
        tx.intents
            .values()
            .flat_map(|intent| Vec::from(&intent.actions)),
    )
}

/// The smallest transaction of shape `tx_shape` that carries `calls` with placeholder proofs,
/// and the transaction's `other_actions`.
///
/// The calls, the other actions and every other part sit in the first intent, the fallible parts in
/// its fallible section and in a fallible Zswap offer for its segment. Every
/// value takes its smallest encoding, so with the right counts the mock is never
/// larger than the real transaction. Its NIGHT spends and Dust registrations share
/// one key, and the ledger charges a registration for each NIGHT UTXO its key
/// spends, so each registration reserves that charge for every NIGHT spend.
pub(crate) fn mock_tx<D: DB>(
    calls: &[PreTranscript<D>],
    other_actions: &[ContractAction<ProofMarker, D>],
    tx_shape: &TransactionShape,
) -> Transaction<Signature, ProofMarker, PureGeneratorPedersen, D> {
    let mut rng = StdRng::seed_from_u64(0);
    let night_key = SigningKey::Schnorr(base_crypto::schnorr::SigningKey::sample(&mut rng));
    let empty_intent = |segment: u16| Intent {
        guaranteed_unshielded_offer: None,
        fallible_unshielded_offer: None,
        actions: Array::new(),
        dust_actions: None,
        // Identical intents would be serialized once.
        ttl: Timestamp::from_secs(segment.into()),
        binding_commitment: PureGeneratorPedersen::smallest_representable(),
    };
    // Fallible parts are numbered after the guaranteed ones, so no two parts
    // repeat a nullifier, commitment, UTXO or signature.
    let guaranteed = tx_shape.guaranteed();
    let fallible = tx_shape.fallible();
    let fallible_first = Offsets {
        spends: guaranteed.night_spends + guaranteed.non_night_unshielded_spends,
        outputs: guaranteed.night_outputs + guaranteed.non_night_unshielded_outputs,
        zswap: guaranteed.zswap_inputs + guaranteed.zswap_outputs + guaranteed.zswap_transients,
    };
    let first_intent = Intent {
        guaranteed_unshielded_offer: mock_unshielded_offer(
            &guaranteed,
            Offsets::default(),
            &night_key,
            &mut rng,
        ),
        fallible_unshielded_offer: mock_unshielded_offer(
            &fallible,
            fallible_first,
            &night_key,
            &mut rng,
        ),
        actions: mock_calls(calls)
            .iter()
            .map(|call| (*call).clone())
            .chain(other_actions.iter().cloned())
            .collect(),
        dust_actions: mock_dust_actions(tx_shape, &night_key, &mut rng),
        ..empty_intent(1)
    };
    let last_segment = u16::try_from(tx_shape.intents.max(1)).unwrap_or(u16::MAX);
    let intents = once((1, first_intent))
        .chain((2..=last_segment).map(|segment| (segment, empty_intent(segment))))
        .collect();
    Transaction::Standard(StandardTransaction {
        network_id: String::new(),
        intents,
        guaranteed_coins: mock_zswap_offer(&guaranteed, 0).map(Sp::new),
        fallible_coins: mock_zswap_offer(&fallible, fallible_first.zswap)
            .into_iter()
            .map(|offer| (1, offer))
            .collect(),
        binding_randomness: PedersenRandomness::default(),
    })
}

/// The index each section's parts are numbered from.
#[derive(Clone, Copy, Default)]
struct Offsets {
    spends: usize,
    outputs: usize,
    zswap: usize,
}

fn mock_unshielded_offer<D: DB>(
    section: &SectionShape,
    first: Offsets,
    night_key: &SigningKey,
    rng: &mut StdRng,
) -> Option<Sp<UnshieldedOffer<Signature, D>, D>> {
    let spends = repeat_n(NIGHT, section.night_spends)
        .chain(repeat_n(
            NON_NIGHT_PLACEHOLDER_TYPE,
            section.non_night_unshielded_spends,
        ))
        .enumerate()
        .map(|(i, type_)| (first.spends + i, type_))
        .map(|(i, type_)| UtxoSpend {
            value: 0,
            owner: night_key.verifying_key(),
            type_,
            intent_hash: IntentHash(distinct_hash(i)),
            output_no: 0,
        })
        .collect::<Vec<_>>();
    let signatures = (first.spends..first.spends + spends.len())
        .map(|i| night_key.sign(rng, &i.to_le_bytes()))
        .collect::<Vec<_>>();
    let outputs = repeat_n(NIGHT, section.night_outputs)
        .chain(repeat_n(
            NON_NIGHT_PLACEHOLDER_TYPE,
            section.non_night_unshielded_outputs,
        ))
        .enumerate()
        .map(|(i, type_)| (first.outputs + i, type_))
        .map(|(i, type_)| UtxoOutput {
            value: 0,
            owner: UserAddress(distinct_hash(i)),
            type_,
        })
        .collect::<Vec<_>>();
    (!spends.is_empty() || !outputs.is_empty()).then(|| {
        Sp::new(UnshieldedOffer {
            inputs: spends.into(),
            outputs: outputs.into(),
            signatures: signatures.into(),
        })
    })
}

fn mock_dust_actions<D: DB>(
    tx_shape: &TransactionShape,
    night_key: &SigningKey,
    rng: &mut StdRng,
) -> Option<Sp<DustActions<Signature, ProofMarker, D>, D>> {
    let spends = (0..tx_shape.dust_spends as u64)
        .map(|i| DustSpend {
            v_fee: 0,
            old_nullifier: DustNullifier(Fr::from(i)),
            new_commitment: DustCommitment(Fr::from(0u64)),
            proof: Proof(vec![0; DUST_SPEND_PROOF_SIZE]),
        })
        .collect::<Vec<_>>();
    // A deregistration is a registration without a Dust address. Deregistrations
    // sharing a key would repeat, and are charged nothing per NIGHT spend, so
    // each gets its own key.
    let dereg_keys = (0..tx_shape.dust_deregistrations)
        .map(|_| SigningKey::Schnorr(base_crypto::schnorr::SigningKey::sample(&mut *rng)))
        .collect::<Vec<_>>();
    let dust_addresses = (0..tx_shape.dust_registrations)
        .map(|i| (night_key, Some(DustPublicKey(Fr::from(i as u64)))))
        .chain(dereg_keys.iter().map(|key| (key, None)));
    let registrations = dust_addresses
        .enumerate()
        .map(|(i, (key, dust_address))| DustRegistration {
            night_key: key.verifying_key(),
            dust_address: dust_address.map(Sp::new),
            allow_fee_payment: 0,
            signature: Some(Sp::new(key.sign(rng, &i.to_le_bytes()))),
        })
        .collect::<Vec<_>>();
    (!spends.is_empty() || !registrations.is_empty()).then(|| {
        Sp::new(DustActions {
            spends: spends.into(),
            registrations: registrations.into(),
            ctime: Timestamp::from_secs(0),
        })
    })
}

/// A Zswap offer of `section`'s Zswap parts, numbered from `first`.
fn mock_zswap_offer<D: DB>(section: &SectionShape, first: usize) -> Option<ZswapOffer<Proof, D>> {
    let input_proof = Arc::new(Proof(vec![0; zswap::INPUT_PROOF_SIZE]));
    let output_proof = Arc::new(Proof(vec![0; zswap::OUTPUT_PROOF_SIZE]));
    let inputs = (first..first + section.zswap_inputs)
        .map(|i| ZswapInput {
            nullifier: Nullifier(distinct_hash(i)),
            value_commitment: Pedersen::default(),
            contract_address: None,
            merkle_tree_root: Default::default(),
            proof: input_proof.clone(),
        })
        .collect::<Vec<_>>();
    let first = first + section.zswap_inputs;
    let outputs = (first..first + section.zswap_outputs)
        .map(|i| ZswapOutput {
            coin_com: Commitment(distinct_hash(i)),
            value_commitment: Pedersen::default(),
            contract_address: None,
            ciphertext: None,
            proof: output_proof.clone(),
        })
        .collect::<Vec<_>>();
    let first = first + section.zswap_outputs;
    let transients = (first..first + section.zswap_transients)
        .map(|i| ZswapTransient {
            nullifier: Nullifier(distinct_hash(i)),
            coin_com: Commitment(distinct_hash(i)),
            value_commitment_input: Pedersen::default(),
            value_commitment_output: Pedersen::default(),
            contract_address: None,
            ciphertext: None,
            proof_input: input_proof.clone(),
            proof_output: output_proof.clone(),
        })
        .collect::<Vec<_>>();
    (!inputs.is_empty() || !outputs.is_empty() || !transients.is_empty()).then(|| ZswapOffer {
        inputs: inputs.into(),
        outputs: outputs.into(),
        transient: transients.into(),
        deltas: Array::new(),
    })
}

/// A different hash for each `i`, as identical parts would be serialized once.
fn distinct_hash(i: usize) -> HashOutput {
    let mut hash = [0; PERSISTENT_HASH_BYTES];
    hash[..8].copy_from_slice(&(i as u64).to_le_bytes());
    HashOutput(hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::structure::INITIAL_PARAMETERS;
    use onchain_runtime::context::QueryContext;
    use onchain_runtime::ops::Op;
    use onchain_runtime::result_mode::ResultModeVerify;
    use onchain_runtime::state::{ChargedState, StateValue};
    use serialize::Serializable;
    use storage::db::InMemoryDB;
    use storage::storage::HashMap;

    fn pre_transcript(
        program: Vec<Op<ResultModeVerify, InMemoryDB>>,
        comm_comm: Option<Fr>,
    ) -> PreTranscript<InMemoryDB> {
        PreTranscript {
            context: QueryContext::new(
                ChargedState::new(StateValue::Null),
                ContractAddress::default(),
            ),
            program,
            comm_comm,
        }
    }

    /// Two intents and no parts: the shape of no calls without Dust spend.
    fn no_parts() -> TransactionShape {
        let no_calls: [PreTranscript<InMemoryDB>; 0] = [];
        TransactionShape {
            dust_spends: 0,
            ..TransactionShape::estimate_from_calls(&no_calls, &INITIAL_PARAMETERS).unwrap()
        }
    }

    #[test]
    fn reserve_charges_each_call() {
        let calls: [_; 2] = std::array::from_fn(|_| pre_transcript(vec![Op::Noop { n: 1 }], None));
        let tx_shape = TransactionShape::estimate_from_calls(&calls, &INITIAL_PARAMETERS).unwrap();
        let reserves = [0, 1, 2].map(|n| {
            mock_tx(&calls[..n], &[], &tx_shape)
                .time_to_dismiss(&INITIAL_PARAMETERS)
                .expect("mock transactions are disjoint by construction")
                .into_picoseconds()
        });
        let [no_calls, one_call, two_calls] = reserves;
        assert!(no_calls < one_call, "{reserves:?}");
        // The second call costs as much as the first: the verifier key read the node
        // shares between calls at one address, entry point and proof version is
        // reserved per call. Scaling by the validation factor rounds, so the steps
        // may differ by 1 ps.
        assert!(
            (two_calls - one_call).abs_diff(one_call - no_calls) <= 1,
            "{reserves:?}"
        );
    }

    fn reserve(tx_shape: &TransactionShape) -> base_crypto::cost_model::CostDuration {
        mock_tx::<InMemoryDB>(&[], &[], tx_shape)
            .time_to_dismiss(&INITIAL_PARAMETERS)
            .expect("mock transactions are disjoint by construction")
    }

    type Count = fn(&mut TransactionShape) -> &mut usize;

    /// Each section count, guaranteed and fallible.
    const SECTION_COUNTS: [(&str, Count, Count); 7] = [
        (
            "night_spends",
            |s| &mut s.night_spends,
            |s| &mut s.fallible_night_spends,
        ),
        (
            "night_outputs",
            |s| &mut s.night_outputs,
            |s| &mut s.fallible_night_outputs,
        ),
        (
            "non_night_unshielded_spends",
            |s| &mut s.non_night_unshielded_spends,
            |s| &mut s.fallible_non_night_unshielded_spends,
        ),
        (
            "non_night_unshielded_outputs",
            |s| &mut s.non_night_unshielded_outputs,
            |s| &mut s.fallible_non_night_unshielded_outputs,
        ),
        (
            "zswap_inputs",
            |s| &mut s.zswap_inputs,
            |s| &mut s.fallible_zswap_inputs,
        ),
        (
            "zswap_outputs",
            |s| &mut s.zswap_outputs,
            |s| &mut s.fallible_zswap_outputs,
        ),
        (
            "zswap_transients",
            |s| &mut s.zswap_transients,
            |s| &mut s.fallible_zswap_transients,
        ),
    ];

    #[test]
    fn reserve_grows_with_each_count() {
        let counts: [Count; 4] = [
            |tx_shape| &mut tx_shape.intents,
            |tx_shape| &mut tx_shape.dust_spends,
            |tx_shape| &mut tx_shape.dust_registrations,
            |tx_shape| &mut tx_shape.dust_deregistrations,
        ];
        for count in counts {
            let mut tx_shape = no_parts();
            *count(&mut tx_shape) += 100;
            assert!(reserve(&tx_shape) > reserve(&no_parts()), "{tx_shape:?}");
        }
        for (name, guaranteed, fallible) in SECTION_COUNTS {
            // Inserting a UTXO costs only bytes written, which time_to_dismiss
            // does not count.
            if name != "non_night_unshielded_outputs" {
                let mut tx_shape = no_parts();
                *guaranteed(&mut tx_shape) += 100;
                assert!(
                    reserve(&tx_shape) > reserve(&no_parts()),
                    "guaranteed {name}"
                );
            }
            // A fallible part's validation is still charged to the time to
            // dismiss; outputs have none.
            if !name.ends_with("outputs") || name == "zswap_outputs" {
                let mut tx_shape = no_parts();
                *fallible(&mut tx_shape) += 100;
                assert!(reserve(&tx_shape) > reserve(&no_parts()), "fallible {name}");
            }
        }
    }

    /// Partitioning for a range rests on the reserve and the size both only
    /// growing with each count.
    #[test]
    fn reserve_and_size_only_grow_with_each_count() {
        let size = |tx_shape: &TransactionShape| {
            mock_tx::<InMemoryDB>(&[], &[], tx_shape).serialized_size()
        };
        let all_counts: [Count; 18] = [
            |s| &mut s.intents,
            |s| &mut s.night_spends,
            |s| &mut s.night_outputs,
            |s| &mut s.non_night_unshielded_spends,
            |s| &mut s.non_night_unshielded_outputs,
            |s| &mut s.dust_spends,
            |s| &mut s.zswap_inputs,
            |s| &mut s.zswap_outputs,
            |s| &mut s.zswap_transients,
            |s| &mut s.dust_registrations,
            |s| &mut s.dust_deregistrations,
            |s| &mut s.fallible_night_spends,
            |s| &mut s.fallible_night_outputs,
            |s| &mut s.fallible_non_night_unshielded_spends,
            |s| &mut s.fallible_non_night_unshielded_outputs,
            |s| &mut s.fallible_zswap_inputs,
            |s| &mut s.fallible_zswap_outputs,
            |s| &mut s.fallible_zswap_transients,
        ];
        // From no parts and from every part, raising any count by one.
        for base in [no_parts(), every_count()] {
            for count in all_counts {
                let mut raised = base.clone();
                *count(&mut raised) += 1;
                assert!(reserve(&raised) >= reserve(&base), "{raised:?}");
                assert!(size(&raised) >= size(&base), "{raised:?}");
            }
        }
    }

    /// A fallible part's application cost is not charged to the time to
    /// dismiss, so it never reserves more than the same part kept guaranteed,
    /// and an unshielded spend or NIGHT output reserves less.
    #[test]
    fn fallible_parts_reserve_no_more_than_guaranteed_ones() {
        for (name, guaranteed, fallible) in SECTION_COUNTS {
            let mut guaranteed_shape = no_parts();
            *guaranteed(&mut guaranteed_shape) += 100;
            let mut fallible_shape = no_parts();
            *fallible(&mut fallible_shape) += 100;
            let (guaranteed, fallible) = (reserve(&guaranteed_shape), reserve(&fallible_shape));
            if matches!(
                name,
                "night_spends" | "night_outputs" | "non_night_unshielded_spends"
            ) {
                assert!(
                    fallible < guaranteed,
                    "{name}: {fallible:?} >= {guaranteed:?}"
                );
            } else {
                assert!(
                    fallible <= guaranteed,
                    "{name}: {fallible:?} > {guaranteed:?}"
                );
            }
        }
    }

    fn every_count() -> TransactionShape {
        TransactionShape {
            intents: 3,
            night_spends: 2,
            night_outputs: 3,
            non_night_unshielded_spends: 4,
            non_night_unshielded_outputs: 5,
            dust_spends: 6,
            zswap_inputs: 7,
            zswap_outputs: 8,
            zswap_transients: 9,
            dust_registrations: 10,
            dust_deregistrations: 11,
            fallible_night_spends: 12,
            fallible_night_outputs: 13,
            fallible_non_night_unshielded_spends: 14,
            fallible_non_night_unshielded_outputs: 15,
            fallible_zswap_inputs: 17,
            fallible_zswap_outputs: 18,
            fallible_zswap_transients: 19,
        }
    }

    /// Another call keeps its transcripts and gets a placeholder proof, and its
    /// guaranteed gas is reserved.
    #[test]
    fn other_calls_keep_their_transcripts_and_are_reserved() {
        let transcript = |gas: RunningCost| {
            Sp::new(Transcript::<InMemoryDB> {
                gas,
                effects: Effects::default(),
                program: Array::new(),
                version: Some(Sp::new(Transcript::<InMemoryDB>::VERSION)),
            })
        };
        let call = |gas: RunningCost| {
            ContractAction::<(), InMemoryDB>::Call(Sp::new(ContractCall {
                address: ContractAddress(HashOutput([9; 32])),
                entry_point: EntryPointBuf(b"op".to_vec()),
                guaranteed_transcript: Some(transcript(gas)),
                fallible_transcript: None,
                communication_commitment: Fr::from(1u64),
                proof: (),
            }))
        };
        let cheap = mock_actions([call(RunningCost::ZERO)]);
        let ContractAction::Call(mocked) = &cheap[0] else {
            unreachable!()
        };
        assert_eq!(
            mocked.guaranteed_transcript,
            Some(transcript(RunningCost::ZERO))
        );
        assert_eq!(mocked.fallible_transcript, None);
        assert_eq!(
            mocked.proof,
            ProofVersioned::V2(Proof(vec![0; MIN_PROOF_SIZE]))
        );
        let costly = mock_actions([call(RunningCost::compute(
            base_crypto::cost_model::CostDuration::from_picoseconds(1_000_000_000),
        ))]);
        let reserve = |actions: &[ContractAction<ProofMarker, InMemoryDB>]| {
            mock_tx::<InMemoryDB>(&[], actions, &no_parts())
                .time_to_dismiss(&INITIAL_PARAMETERS)
                .unwrap()
        };
        assert!(reserve(&cheap) > reserve(&[]));
        assert!(reserve(&costly) > reserve(&cheap));
    }

    /// A range reaching counts no transaction within the byte limit could
    /// have keeps everything fallible, without building their mock.
    #[test]
    fn shapes_beyond_the_byte_limit_keep_everything_fallible() {
        use super::super::{TransactionShapeRange, partition_transcripts_with_tx_shape_range};

        let calls = [pre_transcript(vec![Op::Noop { n: 1 }], None)];
        let estimate = TransactionShape::estimate_from_calls(&calls, &INITIAL_PARAMETERS).unwrap();
        let byte_limit = INITIAL_PARAMETERS.limits.transaction_byte_limit as usize;
        assert!(estimate.fits_in(byte_limit));
        let alone = super::super::partition_transcripts(&calls, &INITIAL_PARAMETERS).unwrap();
        assert!(alone[0].0.is_some(), "{alone:?}");

        // As a negative count from JavaScript would arrive, wrapped.
        let mut absurd = estimate.clone();
        absurd.night_outputs = u32::MAX as usize;
        assert!(!absurd.fits_in(byte_limit));
        for range in [
            TransactionShapeRange::between(&estimate, &absurd),
            TransactionShapeRange::exactly(absurd),
        ] {
            let partitioned =
                partition_transcripts_with_tx_shape_range(&calls, &INITIAL_PARAMETERS, &range)
                    .unwrap();
            assert!(partitioned[0].0.is_none(), "{partitioned:?}");
        }

        // The most Zswap inputs the limit could hold still fit.
        let mut most = TransactionShape::empty();
        most.zswap_inputs = byte_limit / zswap::INPUT_PROOF_SIZE;
        assert!(most.fits_in(byte_limit));
        most.zswap_inputs += 1;
        assert!(!most.fits_in(byte_limit));
    }

    /// `for_calls` stays for the callers of 9.1.0.2-rc.0.
    #[test]
    #[allow(deprecated)]
    fn for_calls_is_estimate_from_calls() {
        let calls: [_; 2] = std::array::from_fn(|_| pre_transcript(vec![Op::Noop { n: 1 }], None));
        assert_eq!(
            TransactionShape::for_calls(&calls, &INITIAL_PARAMETERS).unwrap(),
            TransactionShape::estimate_from_calls(&calls, &INITIAL_PARAMETERS).unwrap()
        );
    }

    #[test]
    fn of_counts_what_the_mock_builds() {
        let calls: [_; 2] = std::array::from_fn(|_| pre_transcript(vec![Op::Noop { n: 1 }], None));
        for tx_shape in [every_count(), no_parts()] {
            assert_eq!(
                TransactionShape::of(&mock_tx(&calls, &[], &tx_shape)),
                tx_shape
            );
        }
    }

    /// The non-call part a real transaction is built around, before balancing.
    #[derive(Clone, Copy, Debug)]
    enum RealPart {
        GuaranteedNight,
        FallibleNight,
        GuaranteedZswap,
        FallibleZswap,
        DustRegistration,
    }

    /// A transaction carrying `part`, balanced and proven, and the counts it
    /// should have besides the fee intent and its Dust spend.
    async fn real_tx(
        part: RealPart,
    ) -> (
        crate::test_utilities::TxBound<Signature, InMemoryDB>,
        TransactionShape,
    ) {
        use crate::dust::DustPublicKey;
        use crate::structure::ProofPreimageMarker;
        use crate::test_utilities::{TestState, test_resolver, tx_prove_bind};
        use base_crypto::rng::SplittableRng;
        use base_crypto::time::Duration;
        use coin_structure::coin::Info as CoinInfo;
        use rand::Rng;
        use transient_crypto::commitment::PedersenRandomness;

        let mut rng = StdRng::seed_from_u64(0x42);
        let mut state = TestState::<InMemoryDB>::new(&mut rng);
        state.give_fee_token(&mut rng, 1).await;
        let ttl = state.time + Duration::from_secs(3600);
        let owner = state.night_key.verifying_key();
        let night_offer = |rng: &mut StdRng| UnshieldedOffer::<Signature, InMemoryDB> {
            inputs: vec![UtxoSpend {
                value: 25_000_000_000,
                owner: owner.clone(),
                type_: NIGHT,
                intent_hash: IntentHash(rng.r#gen()),
                output_no: 3,
            }]
            .into(),
            outputs: vec![UtxoOutput {
                value: 10_000_000_000,
                owner: UserAddress::from(owner.clone()),
                type_: NIGHT,
            }]
            .into(),
            signatures: vec![].into(),
        };
        let fee_intent = TransactionShape {
            intents: 2,
            dust_spends: 1,
            ..TransactionShape::empty()
        };
        let night_intent = |rng: &mut StdRng,
                            guaranteed: Option<UnshieldedOffer<Signature, InMemoryDB>>,
                            fallible: Option<UnshieldedOffer<Signature, InMemoryDB>>,
                            dust_actions| {
            Intent::new(
                rng,
                guaranteed,
                fallible,
                vec![],
                vec![],
                vec![],
                dust_actions,
                ttl,
            )
        };
        let (tx, expected) = match part {
            RealPart::GuaranteedNight | RealPart::FallibleNight => {
                let guaranteed = matches!(part, RealPart::GuaranteedNight);
                let offer = night_offer(&mut rng);
                let keys = [state.night_key.clone()];
                let intent = if guaranteed {
                    night_intent(&mut rng, Some(offer), None, None).sign(
                        &mut rng,
                        1,
                        &keys,
                        &[],
                        &[],
                    )
                } else {
                    night_intent(&mut rng, None, Some(offer), None).sign(
                        &mut rng,
                        1,
                        &[],
                        &keys,
                        &[],
                    )
                }
                .unwrap();
                let expected = if guaranteed {
                    TransactionShape {
                        night_spends: 1,
                        night_outputs: 1,
                        ..fee_intent
                    }
                } else {
                    TransactionShape {
                        fallible_night_spends: 1,
                        fallible_night_outputs: 1,
                        ..fee_intent
                    }
                };
                (
                    Transaction::from_intents("local-test", HashMap::new().insert(1, intent)),
                    expected,
                )
            }
            RealPart::GuaranteedZswap | RealPart::FallibleZswap => {
                let token = Default::default();
                state.rewards_shielded(&mut rng, token, 100_000_000);
                let coin = *state.zswap.coins.iter().next().unwrap().1;
                let segment = if matches!(part, RealPart::GuaranteedZswap) {
                    0
                } else {
                    1
                };
                let input = state
                    .zswap
                    .spend(&mut rng, &state.zswap_keys, &coin, Some(segment))
                    .unwrap()
                    .1;
                let nonce = rng.r#gen();
                let output = ZswapOutput::new(
                    &mut rng,
                    &CoinInfo {
                        nonce,
                        value: coin.value,
                        type_: token,
                    },
                    Some(segment),
                    &state.zswap_keys.coin_public_key(),
                    Some(state.zswap_keys.enc_public_key()),
                )
                .unwrap();
                let offer = ZswapOffer {
                    inputs: vec![input].into(),
                    outputs: vec![output].into(),
                    transient: vec![].into(),
                    deltas: vec![].into(),
                };
                let tx = if segment == 0 {
                    StandardTransaction::<
                        Signature,
                        ProofPreimageMarker,
                        PedersenRandomness,
                        InMemoryDB,
                    >::new(
                        "local-test", HashMap::new(), Some(offer), HashMap::new()
                    )
                } else {
                    StandardTransaction::new(
                        "local-test",
                        HashMap::new().insert(1, Intent::empty(&mut rng, ttl)),
                        None,
                        HashMap::new().insert(1, offer),
                    )
                };
                // A guaranteed Zswap offer needs no intent of its own.
                let expected = if segment == 0 {
                    TransactionShape {
                        intents: 1,
                        zswap_inputs: 1,
                        zswap_outputs: 1,
                        ..fee_intent
                    }
                } else {
                    TransactionShape {
                        fallible_zswap_inputs: 1,
                        fallible_zswap_outputs: 1,
                        ..fee_intent
                    }
                };
                (Transaction::Standard(tx), expected)
            }
            RealPart::DustRegistration => {
                let registration = DustRegistration {
                    night_key: owner.clone(),
                    dust_address: Some(Sp::new(DustPublicKey::from(state.dust_key.clone()))),
                    allow_fee_payment: 0,
                    signature: None,
                };
                let actions = DustActions {
                    spends: vec![].into(),
                    registrations: vec![registration].into(),
                    ctime: state.time,
                };
                let keys = [state.night_key.clone()];
                let offer = night_offer(&mut rng);
                let intent = night_intent(&mut rng, Some(offer), None, Some(actions))
                    .sign(&mut rng, 1, &keys, &[], &keys)
                    .unwrap();
                (
                    Transaction::from_intents("local-test", HashMap::new().insert(1, intent)),
                    TransactionShape {
                        night_spends: 1,
                        night_outputs: 1,
                        dust_registrations: 1,
                        ..fee_intent
                    },
                )
            }
        };
        let resolver = test_resolver("");
        let tx = state.balance_tx(rng.split(), tx, &resolver).await.unwrap();
        let tx = tx_prove_bind(rng.split(), &tx, &resolver).await.unwrap();
        (tx, expected)
    }

    /// For each kind of non-call part, in either section, the mock of a real
    /// transaction's shape is no larger than it, by little, and reserves
    /// exactly its time to dismiss.
    #[tokio::test]
    async fn mock_tx_is_a_tight_lower_bound_of_real_txs() {
        use crate::structure::ProofKind;

        for part in [
            RealPart::GuaranteedNight,
            RealPart::FallibleNight,
            RealPart::GuaranteedZswap,
            RealPart::FallibleZswap,
            RealPart::DustRegistration,
        ] {
            let (tx, expected) = real_tx(part).await;
            let tx_shape = TransactionShape::of(&tx);
            assert_eq!(tx_shape, expected, "{part:?}");

            let mock = mock_tx::<InMemoryDB>(&[], &[], &tx_shape);
            let mock_size = if cfg!(feature = "proving") {
                mock.serialized_size()
            } else {
                <() as ProofKind<InMemoryDB>>::estimated_tx_size(&mock.erase_proofs())
            };
            let real_size = ProofKind::<InMemoryDB>::estimated_tx_size(&tx);
            assert!(
                mock_size <= real_size,
                "{part:?}: {mock_size} > {real_size}"
            );
            // The mock's Zswap outputs carry no ciphertext, as an output to a
            // contract has none; a real output to a user does.
            let slack = 256 + 192 * (tx_shape.zswap_outputs + tx_shape.fallible_zswap_outputs);
            assert!(
                real_size - mock_size <= slack,
                "{part:?}: {real_size} - {mock_size} > {slack}"
            );
            assert_eq!(
                mock.time_to_dismiss(&INITIAL_PARAMETERS),
                tx.time_to_dismiss(&INITIAL_PARAMETERS),
                "{part:?}"
            );
        }
    }

    /// How many of `parts` serialize differently from all the others.
    fn distinct<T: Serializable>(
        parts: impl IntoIterator<Item = impl std::ops::Deref<Target = T>>,
    ) -> usize {
        parts
            .into_iter()
            .map(|part| {
                let mut bytes = Vec::new();
                part.serialize(&mut bytes).unwrap();
                bytes
            })
            .collect::<std::collections::HashSet<_>>()
            .len()
    }

    /// The serializer writes identical nodes once, so each part the shape counts
    /// must differ from the others. A zero count must build no container, or the
    /// mock outgrows a real transaction without that part.
    #[test]
    fn each_count_builds_its_own_part() {
        let calls: [_; 2] = std::array::from_fn(|_| pre_transcript(vec![Op::Noop { n: 1 }], None));
        let tx_shape = every_count();
        let Transaction::Standard(stx) = mock_tx(&calls, &[], &tx_shape) else {
            unreachable!()
        };
        let intent = stx.intents.get(&1).unwrap();
        let g_offer = intent.guaranteed_unshielded_offer.as_ref().unwrap();
        let f_offer = intent.fallible_unshielded_offer.as_ref().unwrap();
        let dust_spends = &intent.dust_actions.as_ref().unwrap().spends;
        let dust_registrations = &intent.dust_actions.as_ref().unwrap().registrations;
        let g_coins = stx.guaranteed_coins.as_ref().unwrap();
        let f_coins = stx.fallible_coins.get(&1).unwrap();
        let (g, f) = (tx_shape.guaranteed(), tx_shape.fallible());
        let intents =
            (1..=tx_shape.intents as u16).map(|segment| stx.intents.get(&segment).unwrap());
        assert_eq!(distinct(intents), tx_shape.intents);
        assert_eq!(distinct(intent.actions.iter()), calls.len());
        // Parts are distinct across the sections too.
        assert_eq!(
            distinct(g_offer.inputs.iter().chain(f_offer.inputs.iter())),
            g.night_spends
                + g.non_night_unshielded_spends
                + f.night_spends
                + f.non_night_unshielded_spends
        );
        assert_eq!(
            distinct(g_offer.signatures.iter().chain(f_offer.signatures.iter())),
            g_offer.inputs.len() + f_offer.inputs.len()
        );
        assert_eq!(
            distinct(g_offer.outputs.iter().chain(f_offer.outputs.iter())),
            g.night_outputs
                + g.non_night_unshielded_outputs
                + f.night_outputs
                + f.non_night_unshielded_outputs
        );
        assert_eq!(distinct(dust_spends.iter()), tx_shape.dust_spends);
        assert_eq!(
            distinct(dust_registrations.iter()),
            tx_shape.dust_registrations + tx_shape.dust_deregistrations
        );
        let deregistrations = dust_registrations
            .iter()
            .filter(|registration| registration.dust_address.is_none());
        assert_eq!(deregistrations.count(), tx_shape.dust_deregistrations);
        assert_eq!(
            distinct(g_coins.inputs.iter().chain(f_coins.inputs.iter())),
            g.zswap_inputs + f.zswap_inputs
        );
        assert_eq!(
            distinct(g_coins.outputs.iter().chain(f_coins.outputs.iter())),
            g.zswap_outputs + f.zswap_outputs
        );
        assert_eq!(
            distinct(g_coins.transient.iter().chain(f_coins.transient.iter())),
            g.zswap_transients + f.zswap_transients
        );

        let Transaction::Standard(stx) = mock_tx::<InMemoryDB>(&[], &[], &no_parts()) else {
            unreachable!()
        };
        let intent = stx.intents.get(&1).unwrap();
        assert!(intent.guaranteed_unshielded_offer.is_none());
        assert!(intent.fallible_unshielded_offer.is_none());
        assert!(intent.dust_actions.is_none());
        assert!(stx.guaranteed_coins.is_none());
        assert!(stx.fallible_coins.is_empty());
    }
}
