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

use base_crypto::time::Duration;
use midnight_ledger::construct::{
    TransactionShape, TranscriptPair, partition_transcripts_with_tx_shape,
};
use midnight_ledger::error::FeeCalculationError;
use midnight_ledger::structure::ProofKind;

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
            TransactionShape::for_calls(&self.pre_transcripts, &INITIAL_PARAMETERS).unwrap();
        tx_shape.night_spends = night_spends;
        tx_shape
    }

    async fn ledger_accepts(
        &self,
        rng: &mut StdRng,
        night_spends: usize,
        split: TranscriptPair<InMemoryDB>,
    ) -> bool {
        let params = &INITIAL_PARAMETERS;
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
        let offer: UnshieldedOffer<Signature, InMemoryDB> = UnshieldedOffer {
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
            vec![call],
            Vec::new(),
            Vec::new(),
            None,
            state.time + Duration::from_secs(3600),
        )
        .sign(rng, CALL_SEGMENT, &keys, &[], &[])
        .unwrap();
        let tx =
            Transaction::from_intents("local-test", HashMap::new().insert(CALL_SEGMENT, intent));
        let tx = state.balance_tx(rng.split(), tx, &RESOLVER).await.unwrap();
        let tx = tx_prove_bind(rng.split(), &tx, &RESOLVER).await.unwrap();

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
            tx.time_to_dismiss(&params.cost_model)
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
