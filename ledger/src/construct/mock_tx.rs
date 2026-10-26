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
use crate::dust::{DUST_SPEND_PROOF_SIZE, DustActions, DustCommitment, DustNullifier, DustSpend};
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
use zswap::{Input as ZswapInput, Offer as ZswapOffer, Output as ZswapOutput};

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
            ContractAction::Call(Sp::new(ContractCall {
                address: ContractAddress::default(),
                entry_point: EntryPointBuf(Vec::new()),
                guaranteed_transcript: None,
                fallible_transcript: Some(Sp::new(Transcript {
                    gas: RunningCost::ZERO,
                    effects: Effects::default(),
                    program: Array::from(&call.program),
                    version: Some(Sp::new(Transcript::<D>::VERSION)),
                })),
                // Real commitments are random, so no two real calls serialize as one
                // shared node
                communication_commitment: call.comm_comm.unwrap_or_else(|| Fr::from(i as u64)),
                proof: ProofVersioned::V2(Proof(vec![0; MIN_PROOF_SIZE])),
            }))
        })
        .collect()
}

/// The smallest transaction of shape `tx_shape` that carries `calls` with placeholder proofs.
///
/// Every value takes its smallest encoding, so a shape with the right counts never
/// predicts more than the real transaction, except that binding commitments take their
/// largest encoding, as the ledger's own estimates do, and that a Zswap transient with
/// neither a contract address nor a ciphertext is slightly smaller than the input and
/// output it counts as. (This is just a few bytes - too small to matter.)
///
/// A Dust spend's bytes can add more allowance than its cost reserves, so a
/// `dust_spends` count that is too high moves calls toward the guaranteed
/// section, as the Zswap counts and the non-NIGHT output count do. Every other
/// count moves calls toward the fallible section.
pub(crate) fn mock_tx<D: DB>(
    calls: &[PreTranscript<D>],
    tx_shape: &TransactionShape,
) -> Transaction<Signature, ProofMarker, PureGeneratorPedersen, D> {
    let mut rng = StdRng::seed_from_u64(0);
    let signing_key = SigningKey::sample(&mut rng);
    let spends = repeat_n(NIGHT, tx_shape.night_spends)
        .chain(repeat_n(
            NON_NIGHT_PLACEHOLDER_TYPE,
            tx_shape.non_night_unshielded_spends,
        ))
        .enumerate()
        .map(|(i, type_)| UtxoSpend {
            value: 0,
            owner: signing_key.verifying_key(),
            type_,
            intent_hash: IntentHash(distinct_hash(i)),
            output_no: 0,
        })
        .collect::<Vec<_>>();
    let signatures = (0..spends.len())
        .map(|i| signing_key.sign(&mut rng, &i.to_le_bytes()))
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
    let unshielded_offer = (!spends.is_empty() || !outputs.is_empty()).then(|| {
        Sp::new(UnshieldedOffer {
            inputs: spends.into(),
            outputs: outputs.into(),
            signatures: signatures.into(),
        })
    });
    let dust_actions = (tx_shape.dust_spends > 0).then(|| {
        Sp::new(DustActions {
            spends: (0..tx_shape.dust_spends as u64)
                .map(|i| DustSpend {
                    v_fee: 0,
                    old_nullifier: DustNullifier(Fr::from(i)),
                    new_commitment: DustCommitment(Fr::from(0u64)),
                    proof: Proof(vec![0; DUST_SPEND_PROOF_SIZE]),
                })
                .collect(),
            registrations: Array::new(),
            ctime: Timestamp::from_secs(0),
        })
    });
    let empty_intent = |segment: u16| Intent {
        guaranteed_unshielded_offer: None,
        fallible_unshielded_offer: None,
        actions: Array::new(),
        dust_actions: None,
        // Identical intents would be serialized once.
        ttl: Timestamp::from_secs(segment.into()),
        binding_commitment: PureGeneratorPedersen::largest_representable(),
    };
    let first_intent = Intent {
        guaranteed_unshielded_offer: unshielded_offer,
        actions: mock_calls(calls),
        dust_actions,
        ..empty_intent(1)
    };
    let last_segment = u16::try_from(tx_shape.intents.max(1)).unwrap_or(u16::MAX);
    let intents = once((1, first_intent))
        .chain((2..=last_segment).map(|segment| (segment, empty_intent(segment))))
        .collect();
    let guaranteed_coins = (tx_shape.zswap_inputs > 0 || tx_shape.zswap_outputs > 0).then(|| {
        let input_proof = Arc::new(Proof(vec![0; zswap::INPUT_PROOF_SIZE]));
        let output_proof = Arc::new(Proof(vec![0; zswap::OUTPUT_PROOF_SIZE]));
        Sp::new(ZswapOffer {
            inputs: (0..tx_shape.zswap_inputs)
                .map(|i| ZswapInput {
                    nullifier: Nullifier(distinct_hash(i)),
                    value_commitment: Pedersen::default(),
                    contract_address: None,
                    merkle_tree_root: Default::default(),
                    proof: input_proof.clone(),
                })
                .collect(),
            outputs: (0..tx_shape.zswap_outputs)
                .map(|i| ZswapOutput {
                    coin_com: Commitment(distinct_hash(i)),
                    value_commitment: Pedersen::default(),
                    contract_address: None,
                    ciphertext: None,
                    proof: output_proof.clone(),
                })
                .collect(),
            transient: Array::new(),
            deltas: Array::new(),
        })
    });
    Transaction::Standard(StandardTransaction {
        network_id: String::new(),
        intents,
        guaranteed_coins,
        fallible_coins: HashMap::new(),
        binding_randomness: PedersenRandomness::default(),
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
        let calls = [(), ()].map(|()| pre_transcript(vec![Op::Noop { n: 1 }], None));
        let tx_shape = TransactionShape {
            intents: 3,
            night_spends: 2,
            night_outputs: 3,
            non_night_unshielded_spends: 4,
            non_night_unshielded_outputs: 5,
            dust_spends: 6,
            zswap_inputs: 7,
            zswap_outputs: 8,
        };
        let Transaction::Standard(stx) = mock_tx(&calls, &tx_shape) else {
            unreachable!()
        };
        let intent = stx.intents.get(&1).unwrap();
        let offer = intent.guaranteed_unshielded_offer.as_ref().unwrap();
        let dust_spends = &intent.dust_actions.as_ref().unwrap().spends;
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
        assert_eq!(distinct(coins.inputs.iter()), tx_shape.zswap_inputs);
        assert_eq!(distinct(coins.outputs.iter()), tx_shape.zswap_outputs);

        // No calls count nothing but two intents and the fee's Dust spend.
        let no_calls: [PreTranscript<InMemoryDB>; 0] = [];
        let mut no_parts = TransactionShape::for_calls(&no_calls, &INITIAL_PARAMETERS).unwrap();
        no_parts.dust_spends = 0;
        let Transaction::Standard(stx) = mock_tx(&no_calls, &no_parts) else {
            unreachable!()
        };
        let intent = stx.intents.get(&1).unwrap();
        assert!(intent.guaranteed_unshielded_offer.is_none());
        assert!(intent.dust_actions.is_none());
        assert!(stx.guaranteed_coins.is_none());
    }
}
