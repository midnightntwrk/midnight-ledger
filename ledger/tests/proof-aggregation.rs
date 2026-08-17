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

#![cfg(feature = "proof-aggregation")]
#![deny(warnings)]
#![allow(unused_imports)]
#![allow(unused_variables)]

use base_crypto::fab::{AlignedValue, Value};
use base_crypto::hash::{HashOutput, persistent_commit};
use base_crypto::rng::SplittableRng;
use base_crypto::time::Timestamp;
use coin_structure::coin::{Info as CoinInfo, QualifiedInfo as QualifiedCoinInfo};
use coin_structure::contract::ContractAddress;
use coin_structure::transfer::{Recipient, SenderEvidence};
use futures::FutureExt;
use lazy_static::lazy_static;
use midnight_ledger::construct::{ContractCallPrototype, PreTranscript, partition_transcripts};
use midnight_ledger::semantics::{ErasedTransactionResult::Success, ZswapLocalStateExt};
use midnight_ledger::structure::Signature;
use midnight_ledger::structure::{
    ContractDeploy, INITIAL_PARAMETERS, LedgerState, ProofKind, ProofMarker, Transaction,
};
use midnight_ledger::test_utilities::{Resolver, contract_operation};
use midnight_ledger::test_utilities::{TestState, tx_prove_bind};
use midnight_ledger::test_utilities::{Tx, TxBound};
use midnight_ledger::test_utilities::{test_intents, test_resolver};
use midnight_ledger::verify::WellFormedStrictness;
use midnight_ledger_v9 as midnight_ledger;
use onchain_runtime::context::QueryContext;
use onchain_runtime::ops::{Key, Op, key};
use onchain_runtime::program_fragments::*;
use onchain_runtime::result_mode::{ResultModeGather, ResultModeVerify};
use onchain_runtime::state::{ContractState, StateValue, stval};
use rand::rngs::StdRng;
use rand::{CryptoRng, Rng, SeedableRng};
use serialize::Serializable;
use std::borrow::Cow;
use std::fs::File;
use std::future::Future;
use std::path::Path;
use storage::arena::Sp;
use storage::db::{DB, InMemoryDB};
use storage::storage::{Array, HashMap};
use transient_crypto::commitment::PedersenRandomness;
use transient_crypto::curve::Fr;
use transient_crypto::fab::ValueReprAlignedValue;
use transient_crypto::merkle_tree::{MerkleTree, leaf_hash};
use transient_crypto::proofs::PARAMS_VERIFIER;
use transient_crypto::proofs::{KeyLocation, ProofPreimage};
use zswap::verify::{OUTPUT_VK, SIGN_VK, SPEND_VK};
use zswap::{
    Delta, Input as ZswapInput, Offer as ZswapOffer, Output as ZswapOutput,
    Transient as ZswapTransient,
};

lazy_static! {
    static ref RESOLVER: Resolver = test_resolver("micro-dao");
}

fn program_with_results<D: DB>(
    prog: &[Op<ResultModeGather, D>],
    results: &[AlignedValue],
) -> Vec<Op<ResultModeVerify, D>> {
    let mut res_iter = results.iter();

    prog.iter()
        .map(|op| op.clone().translate(|()| res_iter.next().unwrap().clone()))
        .filter(|op| match op {
            Op::Idx { path, .. } => !path.is_empty(),
            Op::Ins { n, .. } => *n != 0,
            _ => true,
        })
        .collect::<Vec<_>>()
}

fn context_with_offer<D: DB>(
    ledger: &LedgerState<D>,
    addr: ContractAddress,
    offer: Option<&ZswapOffer<ProofPreimage, D>>,
) -> QueryContext<D> {
    let mut res = QueryContext::new(ledger.index(addr).unwrap().data, addr);
    if let Some(offer) = offer {
        let (_, indices) = ledger.zswap.try_apply(offer, None).unwrap();
        res.call_context.com_indices = indices;
    }
    res
}

#[tokio::test]
async fn proof_aggregation_verify() {
    use midnight_ledger::structure::{ContractProofEvidence, ProofKind, ProofMarker};
    use transient_crypto::aggregation::{
        AggregatedContractProof, AggregationVerifier, AggregationVerify, AggregationWitness,
        InnerCircuitsContext, IvcInstance, ProofAggregation,
    };
    use zkir_v3::ir_aggregation::AggregableIrSource;

    // Skip if the outer aggregation SRS is not available locally.
    let srs_dir = match std::env::var("SRS_DIR") {
        Ok(d) => std::path::PathBuf::from(d),
        Err(_) => {
            eprintln!("proof_aggregation_verify: SRS_DIR not set — skipping");
            return;
        }
    };

    const IVC_K: u32 = 19;
    const INNER_K: u32 = 14;

    // Inner-circuit verifier params (embedded K=14 Midnight SRS).
    let inner_params = transient_crypto::aggregation::inner_verifier_params();
    let arch = AggregableIrSource::aggregation_arch();
    let inner_ctx = InnerCircuitsContext::new(arch, INNER_K, inner_params);

    // Outer aggregator SRS (K=19, from SRS_DIR).
    let aggregator_srs = transient_crypto::aggregation::load_midnight_srs(&srs_dir, IVC_K)
        .unwrap_or_else(|e| panic!("failed to load midnight-srs-2p{IVC_K} from {srs_dir:?}: {e}"));

    let (mut aggregator, agg_verifier) = ProofAggregation::setup(aggregator_srs, IVC_K, inner_ctx);

    // ── Generate a transaction with real V3 proofs ───────────────────────────
    let mut rng = StdRng::seed_from_u64(0x43);
    lazy_static::initialize(&PARAMS_VERIFIER);
    SPEND_VK.init().ok();
    OUTPUT_VK.init().ok();
    SIGN_VK.init().ok();

    let org_sk: HashOutput = rng.r#gen();
    let sep = b"lares:udao:pk";
    let org_pk = persistent_commit(sep, org_sk);
    let advance_op = contract_operation(&RESOLVER, "advance").await;
    let buy_in_op = contract_operation(&RESOLVER, "buyIn").await;
    let cash_out_op = contract_operation(&RESOLVER, "cashOut").await;
    let set_topic_op = contract_operation(&RESOLVER, "setTopic").await;
    let vote_commit_op = contract_operation(&RESOLVER, "voteCommit").await;
    let vote_reveal_op = contract_operation(&RESOLVER, "voteReveal").await;

    let mut state: TestState<InMemoryDB> = TestState::new(&mut rng);
    state.rewards_shielded(&mut rng, Default::default(), 5_000_000_000);
    state.give_fee_token(&mut rng, 25).await;

    let mut unbalanced_strictness = WellFormedStrictness::default();
    unbalanced_strictness.enforce_balancing = false;
    let balanced_strictness = WellFormedStrictness::default();

    // Part 1: Deploy (no V3 contract-call evidence, but required before any call).
    let contract: ContractState<InMemoryDB> = ContractState::new(
        stval!([
            (org_pk),
            (0u8),
            (Option::<Vec<u8>>::None),
            (Option::<[u8; 32]>::None),
            (0u64),
            (0u64),
            (0u64),
            [{MT(10) {}}, (0u64)],
            [{MT(10) {}}, (0u64), { MerkleTree::<()>::blank(10).root() => null }],
            {},
            {},
            (QualifiedCoinInfo::default()),
            (false)
        ]),
        HashMap::new()
            .insert(b"advance"[..].into(), advance_op.clone())
            .insert(b"buyIn"[..].into(), buy_in_op.clone())
            .insert(b"cashOut"[..].into(), cash_out_op.clone())
            .insert(b"setTopic"[..].into(), set_topic_op.clone())
            .insert(b"voteCommit"[..].into(), vote_commit_op.clone())
            .insert(b"voteReveal"[..].into(), vote_reveal_op.clone()),
        Default::default(),
    );
    let deploy = ContractDeploy::new(&mut rng, contract);
    let tx = Transaction::from_intents(
        "local-test",
        test_intents(&mut rng, Vec::new(), Vec::new(), vec![deploy], state.time),
    );
    tx.well_formed(&state.ledger, unbalanced_strictness, state.time)
        .unwrap();
    let tx = tx_prove_bind(rng.split(), &tx, &RESOLVER).await.unwrap();
    let tx = state.balance_tx(rng.split(), tx, &RESOLVER).await.unwrap();
    let addr = tx.deploys().map(|(_, d)| d).next().unwrap().address();
    dbg!(&tx);
    tx.well_formed(&state.ledger, balanced_strictness, state.time)
        .unwrap();
    let strictness = WellFormedStrictness::default();
    state.assert_apply(&tx, strictness);

    // Part 2: setTopic — produces one V3 contract-call proof.
    let transcripts = partition_transcripts(
        &[PreTranscript {
            context: context_with_offer(&state.ledger, addr, None),
            program: program_with_results(
                &[
                    &Cell_read!([key!(0u8)], false, [u8; 32])[..],
                    &Cell_read!([key!(1u8)], false, u8),
                    &Cell_write!(
                        [key!(2u8)],
                        false,
                        Option<Vec<u8>>,
                        Some(b"test topic".to_vec())
                    ),
                    &Cell_write!(
                        [key!(3u8)],
                        false,
                        Option<[u8; 32]>,
                        Some(state.zswap_keys.coin_public_key().0.0)
                    ),
                    &Cell_write!([key!(1u8)], true, u8, 1u8),
                ]
                .into_iter()
                .flat_map(|x| x.iter())
                .cloned()
                .collect::<Vec<_>>(),
                &[org_pk.into(), 0u8.into()],
            ),
            comm_comm: None,
        }],
        &INITIAL_PARAMETERS,
    )
    .unwrap();
    let call = ContractCallPrototype {
        address: addr,
        entry_point: b"setTopic"[..].into(),
        op: set_topic_op,
        input: (b"test topic".to_vec(), state.zswap_keys.coin_public_key()).into(),
        output: ().into(),
        guaranteed_public_transcript: transcripts[0].0.clone(),
        fallible_public_transcript: transcripts[0].1.clone(),
        private_transcript_outputs: vec![org_sk.into()],
        communication_commitment_rand: rng.r#gen(),
        key_location: KeyLocation(std::borrow::Cow::Borrowed("setTopic")),
    };
    let tx = Transaction::from_intents(
        "local-test",
        test_intents(&mut rng, vec![call], Vec::new(), Vec::new(), state.time),
    );
    tx.well_formed(&state.ledger, unbalanced_strictness, state.time)
        .unwrap();
    let tx = tx_prove_bind(rng.split(), &tx, &RESOLVER).await.unwrap();
    let tx = state.balance_tx(rng.split(), tx, &RESOLVER).await.unwrap();

    // Collect V3 contract-call proof evidence from the setTopic transaction.
    let evidence = tx.collect_proof_evidence(&state.ledger).unwrap();

    // ── Aggregate all V3 proofs ──────────────────────────────────────────────
    let mut last_ivc_proof: Option<Vec<u8>> = None;
    for ev in &evidence {
        if let ContractProofEvidence::V3 { vk, proof, pis } = ev {
            let midnight_vk = vk
                .midnight_vk()
                .expect("verifier key must be valid after proving");
            // AggregableIrSource::Instance = Vec<outer::Scalar>; Fr(pub outer::Scalar) via .0
            let instance: Vec<transient_crypto::curve::outer::Scalar> =
                pis.iter().map(|f| f.0).collect();
            let witness = AggregationWitness::new::<AggregableIrSource>(
                midnight_vk,
                instance,
                proof.0.clone(),
            );
            last_ivc_proof = Some(
                aggregator
                    .aggregate(witness)
                    .expect("IVC aggregation step must succeed"),
            );
        }
    }

    let ivc_proof = last_ivc_proof
        .expect("setTopic transaction must produce at least one V3 proof to aggregate");
    let ivc_instance: IvcInstance<ProofAggregation> = aggregator.instance();

    // ── Verify the aggregated proof ──────────────────────────────────────────

    // DirectVerifier holds IvcInstance directly — no public serialization API exists.
    //
    // SAFETY: `IvcInstance` is `!Send` only because `Box<dyn Statement>` lacks the `Send`
    // bound, but every concrete statement stored here is `TypedStatement<AggregableIrSource>`
    // whose inner type `Vec<outer::Scalar>` is `Send + Sync`. The instance is never actually
    // moved to another thread.
    struct DirectVerifier {
        verifier: AggregationVerifier,
        instance: IvcInstance<ProofAggregation>,
    }
    unsafe impl Send for DirectVerifier {}
    unsafe impl Sync for DirectVerifier {}
    impl AggregationVerify for DirectVerifier {
        fn verify_aggregated_proof(
            &self,
            proof: &AggregatedContractProof,
        ) -> Result<(), anyhow::Error> {
            self.verifier
                .verify_aggregation(&self.instance, &proof.ivc_proof)
                .map_err(|e| anyhow::anyhow!("{e}"))
        }
    }

    let agg_proof = AggregatedContractProof {
        ivc_proof,
        ivc_instance: vec![], // instance is held directly by DirectVerifier
    };
    let verifier = DirectVerifier {
        verifier: agg_verifier,
        instance: ivc_instance,
    };

    let agg_evidence = vec![ContractProofEvidence::Aggregated { proof: agg_proof }];
    <ProofMarker as ProofKind<InMemoryDB>>::verify_aggregated_proofs(&agg_evidence, &verifier)
        .expect("aggregated proof verification must succeed");
}
