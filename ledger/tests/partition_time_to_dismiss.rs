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

//! Checks that a call the transcript partitioner keeps guaranteed passes the
//! node's time-to-dismiss check.
//!
//! The transaction is a token-vault NIGHT deposit paid from one to six NIGHT
//! UTXOs. With the `proving` feature its proofs are real; without it the
//! ledger estimates their size.

#![deny(warnings)]

#[allow(unused_imports)]
mod token_vault_common;

use token_vault_common::*;

use base_crypto::cost_model::CostDuration;
use base_crypto::time::Duration;
use midnight_ledger::construct::{
    PrePartitionContractCall, SegmentSpecifier, TransactionShape, TransactionShapeRange,
    TranscriptPair, partition_transcripts_for_tx, partition_transcripts_with_tx_shape,
    partition_transcripts_with_tx_shape_range,
};
use midnight_ledger::error::{FeeCalculationError, TxShapeCheckError};
use midnight_ledger::structure::ProofKind;
use midnight_ledger::structure::{ContractAction, StandardTransaction};

const NIGHT_DEPOSIT_AMOUNT: u128 = 5_000_000;
const CALL_SEGMENT: u16 = 1;
/// Most NIGHT spends for which the partitioner, given the spend count, keeps
/// the deposit guaranteed.
const PARTITIONER_KEEPS_GUARANTEED_UP_TO: usize = 1;
/// Most NIGHT spends for which the ledger accepts a guaranteed deposit. Without
/// proofs the ledger counts each call proof as 4,832 B, more than the deposit's
/// real 2,912 B, and the larger size allows one more spend.
const LEDGER_ACCEPTS_GUARANTEED_UP_TO: usize = if cfg!(feature = "proving") { 1 } else { 2 };

struct Deposit {
    state: TestState<InMemoryDB>,
    vault: TokenVault,
    pre_transcripts: [PreTranscript<InMemoryDB>; 1],
}

impl Deposit {
    async fn deploy(rng: &mut StdRng) -> Self {
        let mut state = TestState::new(rng);
        state.give_fee_token(rng, 10).await;
        let vault = deploy_token_vault(rng, &mut state).await;
        let mut ops = receive_unshielded_ops(TokenType::Unshielded(NIGHT), NIGHT_DEPOSIT_AMOUNT);
        ops.extend(Counter_increment!(
            [key!(STATE_IDX_TOTAL_UNSHIELDED_DEPOSITS)],
            false,
            1u64
        ));
        let pre_transcripts = [PreTranscript {
            context: QueryContext::new(state.ledger.index(vault.addr).unwrap().data, vault.addr),
            program: program_with_results(&ops, &[]),
            comm_comm: None,
        }];
        Deposit {
            state,
            vault,
            pre_transcripts,
        }
    }

    /// The shape of the transaction `ledger_accepts` builds from `night_spends`
    /// UTXOs. The derived default already counts the fee intent and the change
    /// output.
    fn tx_shape(&self, night_spends: usize) -> TransactionShape {
        let mut tx_shape =
            TransactionShape::estimate_from_calls(&self.pre_transcripts, &INITIAL_PARAMETERS)
                .unwrap();
        tx_shape.night_spends = night_spends;
        tx_shape
    }

    async fn ledger_accepts(
        &self,
        rng: &mut StdRng,
        night_spends: usize,
        split: TranscriptPair<InMemoryDB>,
    ) -> bool {
        let tx = self.balanced_tx(rng, night_spends, split, vec![]).await;
        self.ledger_accepts_tx(rng, night_spends, &tx).await
    }

    /// The deposit with `split`'s transcripts, alongside `other_calls`, paid
    /// from `night_spends` NIGHT UTXOs and balanced, before proving.
    async fn balanced_tx(
        &self,
        rng: &mut StdRng,
        night_spends: usize,
        split: TranscriptPair<InMemoryDB>,
        other_calls: Vec<ContractCallPrototype<InMemoryDB>>,
    ) -> Transaction<
        midnight_ledger::structure::Signature,
        ProofPreimageMarker,
        PedersenRandomness,
        InMemoryDB,
    > {
        let mut state = self.state.clone();
        let (guaranteed_public_transcript, fallible_public_transcript) = split;
        let call = ContractCallPrototype {
            address: self.vault.addr,
            entry_point: b"depositUnshielded"[..].into(),
            op: self.vault.operation("depositUnshielded"),
            input: AlignedValue::concat(
                [
                    AlignedValue::from(NIGHT.0),
                    AlignedValue::from(NIGHT_DEPOSIT_AMOUNT),
                ]
                .iter(),
            ),
            output: ().into(),
            guaranteed_public_transcript,
            fallible_public_transcript,
            private_transcript_outputs: vec![],
            communication_commitment_rand: rng.r#gen(),
            key_location: KeyLocation(Cow::Borrowed("depositUnshielded")),
        };

        let owner = state.night_key.verifying_key();
        // Equal UTXOs that cover the deposit, with change.
        let utxo_value = NIGHT_DEPOSIT_AMOUNT / night_spends as u128 + 1;
        let change = utxo_value * night_spends as u128 - NIGHT_DEPOSIT_AMOUNT;
        let mut spends = (0..night_spends)
            .map(|_| UtxoSpend {
                value: utxo_value,
                owner: owner.clone(),
                type_: NIGHT,
                // `well_formed` does not look if the spent UTXOs actually exist
                intent_hash: IntentHash(rng.r#gen()),
                output_no: 0,
            })
            .collect::<Vec<_>>();
        // `well_formed` requires sorted inputs.
        spends.sort();
        let offer: UnshieldedOffer<midnight_ledger::structure::Signature, InMemoryDB> =
            UnshieldedOffer {
                inputs: spends.into(),
                outputs: vec![UtxoOutput {
                    value: change,
                    owner: UserAddress::from(owner),
                    type_: NIGHT,
                }]
                .into(),
                signatures: vec![].into(),
            };
        let keys = vec![state.night_key.clone(); night_spends];
        let intent = Intent::new(
            rng,
            Some(offer),
            None,
            std::iter::once(call).chain(other_calls).collect(),
            Vec::new(),
            Vec::new(),
            None,
            state.time + Duration::from_secs(3600),
        )
        .sign(rng, CALL_SEGMENT, &keys, &[], &[])
        .unwrap();
        let tx =
            Transaction::from_intents("local-test", HashMap::new().insert(CALL_SEGMENT, intent));
        state.balance_tx(rng.split(), tx, &RESOLVER).await.unwrap()
    }

    /// Whether the ledger accepts `tx` once proven, or rejects it for its time
    /// to dismiss.
    async fn ledger_accepts_tx(
        &self,
        rng: &mut StdRng,
        night_spends: usize,
        tx: &Transaction<
            midnight_ledger::structure::Signature,
            ProofPreimageMarker,
            PedersenRandomness,
            InMemoryDB,
        >,
    ) -> bool {
        let params = &INITIAL_PARAMETERS;
        let state = &self.state;
        let tx = tx_prove_bind(rng.split(), tx, &RESOLVER).await.unwrap();

        let dust_spends = tx
            .intents()
            .filter_map(|(_, intent)| intent.dust_actions.clone())
            .map(|actions| actions.spends.len())
            .sum::<usize>();
        assert_eq!(
            (tx.intents().count(), dust_spends),
            (2, 1),
            "balance_tx should add one intent with one Dust spend"
        );
        println!(
            "{night_spends} NIGHT spends: {} B, time to dismiss {:?}",
            ProofKind::<InMemoryDB>::estimated_tx_size(&tx),
            tx.time_to_dismiss(params)
        );
        match tx.well_formed(&state.ledger, WellFormedStrictness::default(), state.time) {
            Ok(_) => true,
            Err(MalformedTransaction::FeeCalculation(
                FeeCalculationError::OutsideTimeToDismiss { .. },
            )) => false,
            Err(e) => panic!("well_formed rejected the transaction: {e:?}"),
        }
    }
}

#[tokio::test]
async fn guaranteed_call_passes_time_to_dismiss() {
    let params = &INITIAL_PARAMETERS;
    let mut rng = StdRng::seed_from_u64(0x43);
    let deposit = Deposit::deploy(&mut rng).await;

    for night_spends in 1..=6 {
        let tx_shape = deposit.tx_shape(night_spends);
        let split =
            partition_transcripts_with_tx_shape(&deposit.pre_transcripts, params, &tx_shape)
                .unwrap()
                .remove(0);
        // The program has no checkpoints, so the whole call lands in one section.
        let guaranteed = split.0.is_some();
        assert_ne!(guaranteed, split.1.is_some());
        if guaranteed {
            assert!(
                deposit.ledger_accepts(&mut rng, night_spends, split).await,
                "{night_spends} NIGHT spends: the ledger rejects the deposit the partitioner kept guaranteed"
            );
        }
        assert_eq!(
            guaranteed,
            night_spends <= PARTITIONER_KEEPS_GUARANTEED_UP_TO,
            "{night_spends} NIGHT spends: unexpected split"
        );
    }

    let default_split = partition_transcripts(&deposit.pre_transcripts, params)
        .unwrap()
        .remove(0);
    assert!(
        default_split.0.is_some(),
        "the default shape should keep the deposit guaranteed"
    );
    for night_spends in LEDGER_ACCEPTS_GUARANTEED_UP_TO..=LEDGER_ACCEPTS_GUARANTEED_UP_TO + 1 {
        assert_eq!(
            deposit
                .ledger_accepts(&mut rng, night_spends, default_split.clone())
                .await,
            night_spends <= LEDGER_ACCEPTS_GUARANTEED_UP_TO,
            "default shape, {night_spends} NIGHT spends: unexpected ledger verdict"
        );
    }
}

/// `check_tx` flags, before proving, a final transaction that spends more NIGHT
/// UTXOs than its calls were partitioned for. It passes none the ledger
/// rejects, and partitioning again for the shape it reports moves the deposit
/// to the fallible section.
#[tokio::test]
async fn check_tx_flags_a_final_tx_the_partition_did_not_expect() {
    let params = &INITIAL_PARAMETERS;
    let mut rng = StdRng::seed_from_u64(0x44);
    let deposit = Deposit::deploy(&mut rng).await;
    let expected = TransactionShape::estimate_from_calls(&deposit.pre_transcripts, params).unwrap();
    assert_eq!(expected.night_spends, 1);
    let split = partition_transcripts_with_tx_shape(&deposit.pre_transcripts, params, &expected)
        .unwrap()
        .remove(0);
    assert!(
        split.0.is_some(),
        "the estimate should keep the deposit guaranteed"
    );

    let mut verdicts = Vec::new();
    for night_spends in 1..=4 {
        let tx = deposit
            .balanced_tx(&mut rng, night_spends, split.clone(), vec![])
            .await;
        let verdict = expected.check_tx(&tx, params);
        match &verdict {
            Ok(()) => assert!(
                deposit.ledger_accepts_tx(&mut rng, night_spends, &tx).await,
                "{night_spends} NIGHT spends: check_tx passed a transaction the ledger rejects"
            ),
            Err(TxShapeCheckError::OutsideTimeToDismiss {
                expected: reported,
                actual,
                ..
            }) => {
                assert_eq!(**reported, expected);
                assert_eq!(**actual, TransactionShape::of(&tx));
                assert_eq!(actual.night_spends, night_spends);
                let resplit =
                    partition_transcripts_with_tx_shape(&deposit.pre_transcripts, params, actual)
                        .unwrap()
                        .remove(0);
                assert!(
                    resplit.0.is_none(),
                    "{night_spends} NIGHT spends: partitioning for the actual shape should move the deposit to the fallible section"
                );
            }
            Err(e) => panic!("{night_spends} NIGHT spends: {e}"),
        }
        verdicts.push(verdict.is_ok());
    }
    assert_eq!(
        verdicts,
        (1..=4)
            .map(|night_spends| night_spends <= PARTITIONER_KEEPS_GUARANTEED_UP_TO)
            .collect::<Vec<_>>()
    );
}

/// A counter increment in the vault, kept guaranteed and declaring
/// `extra_gas` on top of its own gas: an expensive call with few bytes.
fn costly_call(
    deposit: &Deposit,
    rng: &mut StdRng,
    extra_gas: CostDuration,
) -> ContractCallPrototype<InMemoryDB> {
    let ops = Counter_increment!([key!(STATE_IDX_TOTAL_UNSHIELDED_DEPOSITS)], false, 1u64);
    let pre_transcript = PreTranscript {
        context: QueryContext::new(
            deposit.state.ledger.index(deposit.vault.addr).unwrap().data,
            deposit.vault.addr,
        ),
        program: program_with_results(&ops, &[]),
        comm_comm: None,
    };
    let (guaranteed, fallible) = partition_transcripts(&[pre_transcript], &INITIAL_PARAMETERS)
        .unwrap()
        .remove(0);
    let mut guaranteed = guaranteed.expect("a counter increment fits in the guaranteed section");
    assert!(fallible.is_none());
    guaranteed.gas.compute_time += extra_gas;
    ContractCallPrototype {
        address: deposit.vault.addr,
        entry_point: b"depositUnshielded"[..].into(),
        op: deposit.vault.operation("depositUnshielded"),
        input: ().into(),
        output: ().into(),
        guaranteed_public_transcript: Some(guaranteed),
        fallible_public_transcript: None,
        private_transcript_outputs: vec![],
        communication_commitment_rand: rng.r#gen(),
        key_location: KeyLocation(Cow::Borrowed("depositUnshielded")),
    }
}

/// An expensive guaranteed call already in the transaction leaves no room for
/// the deposit. Partitioning that knows of it, through
/// `partition_transcripts_for_tx` or `add_calls`, moves the deposit to the
/// fallible section, and the final transaction fits its time to dismiss;
/// partitioning the deposit alone keeps it guaranteed, and the final
/// transaction does not.
#[tokio::test]
async fn partitioning_reserves_the_other_calls_in_the_tx() {
    let params = &INITIAL_PARAMETERS;
    let mut rng = StdRng::seed_from_u64(0x45);
    let deposit = Deposit::deploy(&mut rng).await;
    let expected = TransactionShape::estimate_from_calls(&deposit.pre_transcripts, params).unwrap();
    let costly = costly_call(
        &deposit,
        &mut rng,
        CostDuration::from_picoseconds(2_000_000_000),
    );
    let ttl = deposit.state.time + Duration::from_secs(3600);
    let with_costly = StandardTransaction::<
        midnight_ledger::structure::Signature,
        ProofPreimageMarker,
        PedersenRandomness,
        InMemoryDB,
    >::new(
        "local-test",
        HashMap::new().insert(
            CALL_SEGMENT,
            Intent::new(
                &mut rng,
                None,
                None,
                vec![costly.clone()],
                vec![],
                vec![],
                None,
                ttl,
            ),
        ),
        None,
        HashMap::new(),
    );

    let alone = partition_transcripts_with_tx_shape(&deposit.pre_transcripts, params, &expected)
        .unwrap()
        .remove(0);
    assert!(alone.0.is_some(), "alone, the deposit should be guaranteed");
    let tx = deposit
        .balanced_tx(&mut rng, 1, alone, vec![costly.clone()])
        .await;
    assert!(
        matches!(
            expected.check_tx(&tx, params),
            Err(TxShapeCheckError::OutsideTimeToDismiss { .. })
        ),
        "the deposit kept guaranteed beside the costly call should not fit"
    );

    let for_tx = partition_transcripts_for_tx(
        &deposit.pre_transcripts,
        params,
        &Transaction::Standard(with_costly.clone()),
        Some(&TransactionShapeRange::exactly(expected.clone())),
    )
    .unwrap()
    .remove(0);
    assert!(
        for_tx.0.is_none(),
        "beside the costly call, the deposit should be fallible"
    );
    let tx = deposit
        .balanced_tx(&mut rng, 1, for_tx, vec![costly.clone()])
        .await;
    assert_eq!(expected.check_tx(&tx, params), Ok(()));

    // `add_calls` partitions against the calls the transaction holds.
    let communication_commitment_rand = rng.r#gen();
    let added = with_costly
        .add_calls::<ProofPreimage>(
            &mut rng,
            SegmentSpecifier::First,
            &[PrePartitionContractCall {
                address: deposit.vault.addr,
                entry_point: b"depositUnshielded"[..].into(),
                op: deposit.vault.operation("depositUnshielded"),
                pre_transcript: deposit.pre_transcripts[0].clone(),
                private_transcript_outputs: vec![],
                input: AlignedValue::concat(
                    [
                        AlignedValue::from(NIGHT.0),
                        AlignedValue::from(NIGHT_DEPOSIT_AMOUNT),
                    ]
                    .iter(),
                ),
                output: ().into(),
                communication_commitment_rand,
                key_location: KeyLocation(Cow::Borrowed("depositUnshielded")),
            }],
            params,
            ttl,
            &[],
            &[],
            &[],
        )
        .unwrap();
    let guaranteed = Vec::from(&added.intents.get(&CALL_SEGMENT).unwrap().actions)
        .iter()
        .filter_map(|action| match action {
            ContractAction::Call(call) => Some(call.guaranteed_transcript.is_some()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        guaranteed,
        vec![true, false],
        "add_calls should keep the costly call and make the deposit fallible"
    );
}

/// Overstating a Zswap output or a Dust spend lets the deposit stay guaranteed
/// in a transaction that does not have it, as their bytes allow more time than
/// they cost. Partitioning for the range from the actual shape to the
/// overstated one fits the actual transaction.
#[tokio::test]
async fn a_shape_range_covers_overstated_counts() {
    let params = &INITIAL_PARAMETERS;
    let mut rng = StdRng::seed_from_u64(0x46);
    let deposit = Deposit::deploy(&mut rng).await;
    let night_spends = 2;
    let actual = deposit.tx_shape(night_spends);
    let alone = partition_transcripts_with_tx_shape(&deposit.pre_transcripts, params, &actual)
        .unwrap()
        .remove(0);
    assert!(
        alone.0.is_none(),
        "{night_spends} NIGHT spends leave no room"
    );

    type Overstate = fn(&mut TransactionShape);
    let overstate: [(&str, Overstate); 2] = [
        ("a Zswap output", |shape| shape.zswap_outputs += 1),
        ("a Dust spend", |shape| shape.dust_spends += 1),
    ];
    for (part, overstate) in overstate {
        let mut overstated = actual.clone();
        overstate(&mut overstated);

        let exact =
            partition_transcripts_with_tx_shape(&deposit.pre_transcripts, params, &overstated)
                .unwrap()
                .remove(0);
        assert!(
            exact.0.is_some(),
            "overstating {part} should keep the deposit guaranteed"
        );
        let tx = deposit
            .balanced_tx(&mut rng, night_spends, exact, vec![])
            .await;
        assert!(
            actual.check_tx(&tx, params).is_err(),
            "overstating {part}: the transaction without it should not fit"
        );

        let range = TransactionShapeRange::between(&actual, &overstated);
        assert!(range.contains(&actual) && range.contains(&overstated));
        let ranged =
            partition_transcripts_with_tx_shape_range(&deposit.pre_transcripts, params, &range)
                .unwrap()
                .remove(0);
        assert!(
            ranged.0.is_none(),
            "the range from the actual shape to one overstating {part} should keep the deposit fallible"
        );
        let tx = deposit
            .balanced_tx(&mut rng, night_spends, ranged, vec![])
            .await;
        assert_eq!(actual.check_tx(&tx, params), Ok(()));
    }
}
