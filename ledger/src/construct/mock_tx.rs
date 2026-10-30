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

use super::{PreTranscript, TransactionShape};
use crate::dust::{
    DUST_SPEND_PROOF_SIZE, DustActions, DustCommitment, DustNullifier, DustPublicKey,
    DustRegistration, DustSpend,
};
use crate::structure::{
    ContractAction, ContractCall, Intent, IntentHash, MIN_PROOF_SIZE, ProofMarker, ProofVersioned,
    StandardTransaction, Transaction, UnshieldedOffer, UtxoOutput, UtxoSpend,
};
use base_crypto::cost_model::RunningCost;
use base_crypto::hash::{HashOutput, PERSISTENT_HASH_BYTES};
use base_crypto::schnorr::{Signature, SigningKey};
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
use storage::arena::Sp;
use storage::db::DB;
use storage::storage::{Array, HashMap};
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
            // The node reads a verifier key once per address and entry point. A
            // `PreTranscript` has no entry point, so each call gets its own address and
            // the mock reserves a read per call, never fewer than the real transaction.
            // The address also keeps identical calls from merging into one node.
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

/// The smallest transaction of shape `tx_shape` that carries `calls` with placeholder proofs.
///
/// Every value takes its smallest encoding, so with the right counts the mock is never
/// larger than the real transaction. Its NIGHT spends and Dust registrations share
/// one key, and the ledger charges a registration for each NIGHT UTXO its key
/// spends, so each registration reserves that charge for every NIGHT spend.
pub(crate) fn mock_tx<D: DB>(
    calls: &[PreTranscript<D>],
    tx_shape: &TransactionShape,
) -> Transaction<Signature, ProofMarker, PureGeneratorPedersen, D> {
    let mut rng = StdRng::seed_from_u64(0);
    let night_key = SigningKey::sample(&mut rng);
    let empty_intent = |segment: u16| Intent {
        guaranteed_unshielded_offer: None,
        fallible_unshielded_offer: None,
        actions: Array::new(),
        dust_actions: None,
        // Identical intents would be serialized once.
        ttl: Timestamp::from_secs(segment.into()),
        binding_commitment: PureGeneratorPedersen::smallest_representable(),
    };
    let first_intent = Intent {
        guaranteed_unshielded_offer: mock_unshielded_offer(tx_shape, &night_key, &mut rng),
        actions: mock_calls(calls),
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
        guaranteed_coins: mock_zswap_offer(tx_shape),
        fallible_coins: HashMap::new(),
        binding_randomness: PedersenRandomness::default(),
    })
}

fn mock_unshielded_offer<D: DB>(
    tx_shape: &TransactionShape,
    night_key: &SigningKey,
    rng: &mut StdRng,
) -> Option<Sp<UnshieldedOffer<Signature, D>, D>> {
    let spends = repeat_n(NIGHT, tx_shape.night_spends)
        .chain(repeat_n(
            NON_NIGHT_PLACEHOLDER_TYPE,
            tx_shape.non_night_unshielded_spends,
        ))
        .enumerate()
        .map(|(i, type_)| UtxoSpend {
            value: 0,
            owner: night_key.verifying_key(),
            type_,
            intent_hash: IntentHash(distinct_hash(i)),
            output_no: 0,
        })
        .collect::<Vec<_>>();
    let signatures = (0..spends.len())
        .map(|i| night_key.sign(rng, &i.to_le_bytes()))
        .collect::<Vec<_>>();
    let outputs = repeat_n(NIGHT, tx_shape.night_outputs)
        .chain(repeat_n(
            NON_NIGHT_PLACEHOLDER_TYPE,
            tx_shape.non_night_unshielded_outputs,
        ))
        .enumerate()
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
    // A deregistration is a registration without a Dust address.
    let dust_addresses = (0..tx_shape.dust_registrations)
        .map(|i| Some(DustPublicKey(Fr::from(i as u64))))
        .chain(repeat_n(None, tx_shape.dust_deregistrations));
    let registrations = dust_addresses
        .enumerate()
        .map(|(i, dust_address)| DustRegistration {
            night_key: night_key.verifying_key(),
            dust_address: dust_address.map(Sp::new),
            allow_fee_payment: 0,
            signature: Some(Sp::new(night_key.sign(rng, &i.to_le_bytes()))),
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

fn mock_zswap_offer<D: DB>(tx_shape: &TransactionShape) -> Option<Sp<ZswapOffer<Proof, D>, D>> {
    let input_proof = Arc::new(Proof(vec![0; zswap::INPUT_PROOF_SIZE]));
    let output_proof = Arc::new(Proof(vec![0; zswap::OUTPUT_PROOF_SIZE]));
    let inputs = (0..tx_shape.zswap_inputs)
        .map(|i| ZswapInput {
            nullifier: Nullifier(distinct_hash(i)),
            value_commitment: Pedersen::default(),
            contract_address: None,
            merkle_tree_root: Default::default(),
            proof: input_proof.clone(),
        })
        .collect::<Vec<_>>();
    let outputs = (0..tx_shape.zswap_outputs)
        .map(|i| ZswapOutput {
            coin_com: Commitment(distinct_hash(i)),
            value_commitment: Pedersen::default(),
            contract_address: None,
            ciphertext: None,
            proof: output_proof.clone(),
        })
        .collect::<Vec<_>>();
    let transients = (0..tx_shape.zswap_transients)
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
    (!inputs.is_empty() || !outputs.is_empty() || !transients.is_empty()).then(|| {
        Sp::new(ZswapOffer {
            inputs: inputs.into(),
            outputs: outputs.into(),
            transient: transients.into(),
            deltas: Array::new(),
        })
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
            ..TransactionShape::for_calls(&no_calls, &INITIAL_PARAMETERS).unwrap()
        }
    }

    #[test]
    fn reserve_charges_each_call() {
        let calls: [_; 2] = std::array::from_fn(|_| pre_transcript(vec![Op::Noop { n: 1 }], None));
        let tx_shape = TransactionShape::for_calls(&calls, &INITIAL_PARAMETERS).unwrap();
        let cost_model = &INITIAL_PARAMETERS.cost_model;
        let reserves = [0, 1, 2].map(|n| {
            mock_tx(&calls[..n], &tx_shape)
                .time_to_dismiss(cost_model)
                .into_picoseconds()
        });
        let [no_calls, one_call, two_calls] = reserves;
        assert!(no_calls < one_call, "{reserves:?}");
        // The second call costs as much as the first: the verifier key read the node
        // shares between calls at one address and entry point is reserved per call.
        // The division by the parallelism factor rounds, so the steps may differ by 1 ps.
        assert!(
            (two_calls - one_call).abs_diff(one_call - no_calls) <= 1,
            "{reserves:?}"
        );
    }

    #[test]
    fn reserve_grows_with_each_count() {
        let model = &INITIAL_PARAMETERS.cost_model;
        let reserve = |tx_shape: &TransactionShape| {
            mock_tx::<InMemoryDB>(&[], tx_shape).time_to_dismiss(model)
        };
        // Non-NIGHT outputs are left out: inserting a UTXO costs only bytes
        // written, which time_to_dismiss does not count.
        let counts: [fn(&mut TransactionShape) -> &mut usize; 10] = [
            |tx_shape| &mut tx_shape.intents,
            |tx_shape| &mut tx_shape.night_spends,
            |tx_shape| &mut tx_shape.night_outputs,
            |tx_shape| &mut tx_shape.non_night_unshielded_spends,
            |tx_shape| &mut tx_shape.dust_spends,
            |tx_shape| &mut tx_shape.zswap_inputs,
            |tx_shape| &mut tx_shape.zswap_outputs,
            |tx_shape| &mut tx_shape.zswap_transients,
            |tx_shape| &mut tx_shape.dust_registrations,
            |tx_shape| &mut tx_shape.dust_deregistrations,
        ];
        for count in counts {
            let mut tx_shape = no_parts();
            *count(&mut tx_shape) += 100;
            assert!(reserve(&tx_shape) > reserve(&no_parts()), "{tx_shape:?}");
        }
    }

    #[tokio::test]
    async fn mock_tx_is_a_tight_lower_bound_of_a_real_tx() {
        use crate::structure::ProofKind;
        use crate::test_utilities::{TestState, test_resolver, tx_prove_bind};
        use base_crypto::rng::SplittableRng;
        use base_crypto::time::Duration;
        use rand::Rng;

        let mut rng = StdRng::seed_from_u64(0x42);
        let mut state = TestState::<InMemoryDB>::new(&mut rng);
        state.give_fee_token(&mut rng, 1).await;
        let owner = state.night_key.verifying_key();
        let offer = UnshieldedOffer::<Signature, InMemoryDB> {
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
                owner: UserAddress::from(owner),
                type_: NIGHT,
            }]
            .into(),
            signatures: vec![].into(),
        };
        let intent = Intent::new(
            &mut rng,
            Some(offer),
            None,
            vec![],
            vec![],
            vec![],
            None,
            state.time + Duration::from_secs(3600),
        )
        .sign(&mut rng, 1, &[state.night_key.clone()], &[], &[])
        .unwrap();
        let tx = Transaction::from_intents("local-test", HashMap::new().insert(1, intent));
        let resolver = test_resolver("");
        let tx = state.balance_tx(rng.split(), tx, &resolver).await.unwrap();
        let tx = tx_prove_bind(rng.split(), &tx, &resolver).await.unwrap();
        let tx_shape = TransactionShape {
            intents: 2,
            night_spends: 1,
            night_outputs: 1,
            non_night_unshielded_spends: 0,
            non_night_unshielded_outputs: 0,
            dust_spends: 1,
            zswap_inputs: 0,
            zswap_outputs: 0,
            zswap_transients: 0,
            dust_registrations: 0,
            dust_deregistrations: 0,
        };

        let mock = mock_tx::<InMemoryDB>(&[], &tx_shape);

        let mock_size = if cfg!(feature = "proving") {
            mock.serialized_size()
        } else {
            <() as ProofKind<InMemoryDB>>::estimated_tx_size(&mock.erase_proofs())
        };
        let real_size = ProofKind::<InMemoryDB>::estimated_tx_size(&tx);
        assert!(mock_size <= real_size, "{mock_size} > {real_size}");
        assert!(
            real_size - mock_size <= 256,
            "{real_size} - {mock_size} > 256"
        );
        let cost_model = &INITIAL_PARAMETERS.cost_model;
        assert_eq!(
            mock.time_to_dismiss(cost_model),
            tx.time_to_dismiss(cost_model)
        );
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
        let tx_shape = TransactionShape {
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
        };
        let Transaction::Standard(stx) = mock_tx(&calls, &tx_shape) else {
            unreachable!()
        };
        let intent = stx.intents.get(&1).unwrap();
        let offer = intent.guaranteed_unshielded_offer.as_ref().unwrap();
        let dust_spends = &intent.dust_actions.as_ref().unwrap().spends;
        let dust_registrations = &intent.dust_actions.as_ref().unwrap().registrations;
        let coins = stx.guaranteed_coins.as_ref().unwrap();
        let intents =
            (1..=tx_shape.intents as u16).map(|segment| stx.intents.get(&segment).unwrap());
        assert_eq!(distinct(intents), tx_shape.intents);
        assert_eq!(distinct(intent.actions.iter()), calls.len());
        assert_eq!(
            distinct(offer.inputs.iter()),
            tx_shape.night_spends + tx_shape.non_night_unshielded_spends
        );
        assert_eq!(distinct(offer.signatures.iter()), offer.inputs.len());
        assert_eq!(
            distinct(offer.outputs.iter()),
            tx_shape.night_outputs + tx_shape.non_night_unshielded_outputs
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
        assert_eq!(distinct(coins.inputs.iter()), tx_shape.zswap_inputs);
        assert_eq!(distinct(coins.outputs.iter()), tx_shape.zswap_outputs);
        assert_eq!(distinct(coins.transient.iter()), tx_shape.zswap_transients);

        let Transaction::Standard(stx) = mock_tx::<InMemoryDB>(&[], &no_parts()) else {
            unreachable!()
        };
        let intent = stx.intents.get(&1).unwrap();
        assert!(intent.guaranteed_unshielded_offer.is_none());
        assert!(intent.dust_actions.is_none());
        assert!(stx.guaranteed_coins.is_none());
    }
}
