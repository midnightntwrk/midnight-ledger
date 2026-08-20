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
use midnight_ledger::construct::{
    ContractCallExt, ContractCallPrototype, PreTranscript, communication_commitment,
    partition_transcripts,
};
use midnight_ledger::semantics::{ErasedTransactionResult::Success, ZswapLocalStateExt};
use midnight_ledger::structure::Signature;
use midnight_ledger::structure::{
    ContractDeploy, INITIAL_PARAMETERS, LedgerState, ProofKind, ProofMarker,
    ProofPreimageVersioned, Transaction,
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
use transient_crypto::proofs::{KeyLocation, ProofPreimage, Resolver as ResolverTrait};
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
        AggregatedContractProof, AggregationTranscript, AggregationVerifier, AggregationVerify,
        AggregationWitness, InnerCircuitsContext, IvcInstance, ProofAggregation,
    };
    use zkir_v3::ir_aggregation::AggregableIrSource;

    // Skip if the outer aggregation SRS is not available locally.
    // $MIDNIGHT_PP
    let srs_dir = std::path::PathBuf::from(
        "/nix/store/dh2bkkbh384z7f507bd04b0cmwipzjvq-midnight-local-params-10-dust-zswap-v3/"
            .to_string(),
    );
    /* match std::env::var("SRS_DIR") {
        Ok(d) => std::path::PathBuf::from(d),
        Err(_) => {
            eprintln!("proof_aggregation_verify: SRS_DIR not set — skipping");
            return;
        }
    };*/

    // K=20 is required because sha2_256 in aggregation_arch() inflates the outer
    // IVC circuit to ~628K rows, exceeding K=19's 524K-row limit.
    const IVC_K: u32 = 20;
    // K=13 matches the SHA-256 chip's design ("the table fits in a K=13 domain")
    // and matches the multi_circuit_aggregation example's INNER_K=13.
    const INNER_K: u32 = 13;

    // Inner-circuit verifier params (K=13 from SRS_DIR).
    let inner_srs = transient_crypto::aggregation::load_midnight_srs(&srs_dir, INNER_K)
        .unwrap_or_else(|e| {
            panic!("failed to load bls_midnight_2p{INNER_K} from {srs_dir:?}: {e}")
        });
    let inner_params = inner_srs.verifier_params();
    let arch = AggregableIrSource::aggregation_arch();
    let inner_ctx = InnerCircuitsContext::new(arch, INNER_K, inner_params);

    // Outer aggregator SRS (K=20, from SRS_DIR).
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
    // Load setTopic once: build the ContractOperation (for on-chain) and keep
    // ir_source bytes (for aggregation) so we avoid a second resolver round-trip.
    let (set_topic_op, set_topic_ir_bytes) = {
        use transient_crypto::proofs::Resolver as ResolverTrait;
        let mat = RESOLVER
            .resolve_key(KeyLocation(Cow::Borrowed("setTopic")))
            .await
            .expect("resolver error")
            .expect("setTopic not found");
        let mut op = onchain_runtime::state::ContractOperation::new(None, None);
        if let Ok(vk) = serialize::tagged_deserialize::<transient_crypto::proofs::VerifierKey>(
            &mut &mat.verifier_key[..],
        ) {
            op.v3 = Some(vk);
        } else {
            op.v2 = Some(
                serialize::tagged_deserialize(&mut &mat.verifier_key[..])
                    .expect("verifier key should deserialize"),
            );
        }
        (op, mat.ir_source)
    };
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

    // Capture the ProofPreimage before `call` is consumed by test_intents.
    // IrSource proofs (produced by tx_prove_bind below) have N raw public inputs
    // and cannot be fed directly to the IVC aggregator, which requires
    // AggregableIrSource proofs with a single Poseidon-hashed public input.
    // We build the ProofPreimage here so we can re-prove with AggregableIrSource.
    let agg_preimage = {
        let comm_commit = communication_commitment(
            call.input.clone(),
            call.output.clone(),
            call.communication_commitment_rand,
        );
        match <ProofPreimage as ContractCallExt<InMemoryDB>>::construct_proof(&call, comm_commit) {
            ProofPreimageVersioned::V2(p) => (*p).clone(),
            _ => unreachable!("construct_proof always returns V2"),
        }
    };

    let tx = Transaction::from_intents(
        "local-test",
        test_intents(&mut rng, vec![call], Vec::new(), Vec::new(), state.time),
    );
    tx.well_formed(&state.ledger, unbalanced_strictness, state.time)
        .unwrap();
    let tx = tx_prove_bind(rng.split(), &tx, &RESOLVER).await.unwrap();
    let tx = state.balance_tx(rng.split(), tx, &RESOLVER).await.unwrap();
    tx.well_formed(&state.ledger, balanced_strictness, state.time)
        .unwrap();
    // Capture dust spend preimages (with correct binding_input) before they're consumed.
    let dust_preimages = state.captured_dust_preimages.clone();
    state.assert_apply(&tx, balanced_strictness);

    // ── Re-prove setTopic with AggregableIrSource ─────────────────────────────
    // Wrap the IrSource we already loaded at the top in AggregableIrSource.
    // AggregableIrSource::preprocess is identical to IrSource::preprocess; the
    // circuit differs only in that it adds an extra Poseidon step to hash all
    // public inputs down to one.
    let ir_v3: zkir_v3::IrSource =
        serialize::tagged_deserialize(std::io::Cursor::new(&set_topic_ir_bytes[..]))
            .expect("IrSource tagged deserialize must succeed");
    let agg_ir = AggregableIrSource(ir_v3);

    // Load K=13 SRS for inner proof (matching INNER_K=13 in InnerCircuitsContext).
    let inner_srs = std::sync::Arc::new(
        transient_crypto::aggregation::load_midnight_srs(&srs_dir, INNER_K)
            .unwrap_or_else(|e| panic!("failed to load inner bls_midnight_2p{INNER_K}: {e}")),
    );

    let agg_vk = midnight_zk_stdlib::setup_vk(inner_srs.as_ref(), &agg_ir);
    let agg_pk = midnight_zk_stdlib::setup_pk(&agg_ir, &agg_vk);

    let preproc = agg_ir
        .preprocess(&agg_preimage)
        .expect("AggregableIrSource preprocess must succeed");
    let pis = preproc.pis.clone();

    let inner_proof_bytes = midnight_zk_stdlib::prove::<
        AggregableIrSource,
        AggregationTranscript<transient_crypto::curve::outer::Scalar>,
    >(
        inner_srs.as_ref(),
        &agg_pk,
        &agg_ir,
        &pis,
        preproc,
        rng.split(),
    )
    .expect("inner AggregableIrSource prove must succeed");

    // ── Aggregate setTopic ────────────────────────────────────────────────────
    let witness = AggregationWitness::new::<AggregableIrSource>(agg_vk, pis, inner_proof_bytes);
    let mut ivc_proof = aggregator
        .aggregate(witness)
        .expect("IVC aggregation step must succeed");

    // ── Aggregate dust spend proof(s) ─────────────────────────────────────────
    // The dust spend circuit is also a V3 IrSource circuit (spend.bzkir).
    // state.captured_dust_preimages holds the preimages (with the binding_input
    // already set to the override value used during proving) that balance_tx
    // populated when it paid the transaction fee.
    if !dust_preimages.is_empty() {
        let dust_mat = RESOLVER
            .resolve_key(KeyLocation(Cow::Borrowed("midnight/dust/spend")))
            .await
            .expect("resolver error for dust spend")
            .expect("midnight/dust/spend IrSource not found");
        let dust_ir_v3: zkir_v3::IrSource =
            serialize::tagged_deserialize(std::io::Cursor::new(&dust_mat.ir_source[..]))
                .expect("dust IrSource tagged deserialize must succeed");
        let dust_agg_ir = AggregableIrSource(dust_ir_v3);

        let dust_agg_vk = midnight_zk_stdlib::setup_vk(inner_srs.as_ref(), &dust_agg_ir);
        let dust_agg_pk = midnight_zk_stdlib::setup_pk(&dust_agg_ir, &dust_agg_vk);

        for dust_preimage in &dust_preimages {
            let preproc = dust_agg_ir
                .preprocess(dust_preimage)
                .expect("dust AggregableIrSource preprocess must succeed");
            let pis = preproc.pis.clone();
            let proof_bytes = midnight_zk_stdlib::prove::<
                AggregableIrSource,
                AggregationTranscript<transient_crypto::curve::outer::Scalar>,
            >(
                inner_srs.as_ref(),
                &dust_agg_pk,
                &dust_agg_ir,
                &pis,
                preproc,
                rng.split(),
            )
            .expect("dust inner AggregableIrSource prove must succeed");
            let witness = AggregationWitness::new::<AggregableIrSource>(
                dust_agg_vk.clone(),
                pis,
                proof_bytes,
            );
            ivc_proof = aggregator
                .aggregate(witness)
                .expect("dust IVC aggregation step must succeed");
        }
    }

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

/// Aggregates `n` independent dust spend proofs into a single IVC proof, verifies
/// it, and reports the metrics that matter for evaluating aggregation as a
/// fee-payment scaling strategy:
/// - wall-clock time to fold `n` real dust proofs into one aggregated proof,
/// - the resulting aggregated proof's size in bytes, and
/// - how verification time is affected by the number of folded proofs, by
///   comparing against a second aggregation containing just 1 of the same
///   dust proofs (built from the same already-proven inner witness, so the
///   comparison isolates the IVC fold/verify cost from proving cost).
///
/// Each dust proof comes from a *separate* `balance_tx` fee payment (one dust
/// UTXO spent per empty transaction), which is how aggregation would actually
/// be used in production: folding the fee proofs of several transactions in a
/// block into one succinct proof, rather than shipping N separate ones.
#[tokio::test]
async fn aggregate_dust_proofs() {
    use midnight_ledger::structure::{ContractProofEvidence, ProofKind, ProofMarker};
    use std::time::Instant;
    use transient_crypto::aggregation::{
        AggregatedContractProof, AggregationTranscript, AggregationVerifier, AggregationVerify,
        AggregationWitness, InnerCircuitsContext, IvcInstance, ProofAggregation,
    };
    use zkir_v3::ir_aggregation::AggregableIrSource;

    const N: usize = 5;

    // Skip if the outer aggregation SRS is not available locally (see
    // `proof_aggregation_verify` above for context on this path).
    let srs_dir = std::path::PathBuf::from(
        "/nix/store/dh2bkkbh384z7f507bd04b0cmwipzjvq-midnight-local-params-10-dust-zswap-v3"
            .to_string(),
    );

    const IVC_K: u32 = 19;
    const INNER_K: u32 = 13;

    // ── Produce N independent, real dust spend proofs ─────────────────────────
    // Each iteration balances a fresh, otherwise-empty transaction, which still
    // incurs a (small) Dust fee, so `balance_tx` spends exactly one Dust UTXO
    // and captures its proof preimage in `state.captured_dust_preimages`. We
    // loop (rather than assuming 1 UTXO per round) so this doesn't depend on
    // the exact relationship between per-UTXO generation caps and per-tx fees.
    let mut rng = StdRng::seed_from_u64(0x44);
    let mut state: TestState<InMemoryDB> = TestState::new(&mut rng);
    // Headroom for up to ~20 balancing rounds of tiny empty-tx fees, mirroring
    // the over-provisioning `proof_aggregation_verify` uses for a single fee.
    state.give_fee_token(&mut rng, 25).await;

    let strictness = WellFormedStrictness::default();
    let mut dust_preimages: Vec<ProofPreimage> = Vec::new();
    let mut rounds = 0usize;
    while dust_preimages.len() < N {
        rounds += 1;
        assert!(
            rounds <= 30,
            "only accumulated {}/{N} dust spend proofs after {rounds} balancing rounds",
            dust_preimages.len()
        );
        let empty_tx: Transaction<Signature, _, _, _> =
            Transaction::new("local-test", Default::default(), None, Default::default());
        let tx = state
            .balance_tx(rng.split(), empty_tx, &RESOLVER)
            .await
            .unwrap();
        dust_preimages.extend(state.captured_dust_preimages.clone());
        state.assert_apply(&tx, strictness);
    }
    dust_preimages.truncate(N);
    assert_eq!(
        dust_preimages.len(),
        N,
        "expected exactly {N} dust spend proofs"
    );

    // ── Load the dust-spend circuit once, wrapped for aggregation ─────────────
    let dust_mat = RESOLVER
        .resolve_key(KeyLocation(Cow::Borrowed("midnight/dust/spend")))
        .await
        .expect("resolver error for dust spend")
        .expect("midnight/dust/spend IrSource not found");
    let dust_ir_v3: zkir_v3::IrSource =
        serialize::tagged_deserialize(std::io::Cursor::new(&dust_mat.ir_source[..]))
            .expect("dust IrSource tagged deserialize must succeed");
    let dust_agg_ir = AggregableIrSource(dust_ir_v3);

    let inner_srs = std::sync::Arc::new(
        transient_crypto::aggregation::load_midnight_srs(&srs_dir, INNER_K)
            .unwrap_or_else(|e| panic!("failed to load inner bls_midnight_2p{INNER_K}: {e}")),
    );
    let dust_agg_vk = midnight_zk_stdlib::setup_vk(inner_srs.as_ref(), &dust_agg_ir);
    let dust_agg_pk = midnight_zk_stdlib::setup_pk(&dust_agg_ir, &dust_agg_vk);

    // ── Aggregate all N dust proofs into one, timing the whole pipeline ───────
    let inner_arch = AggregableIrSource::aggregation_arch();
    let inner_params = inner_srs.verifier_params();
    let inner_ctx = InnerCircuitsContext::new(inner_arch, INNER_K, inner_params);
    let aggregator_srs = transient_crypto::aggregation::load_midnight_srs(&srs_dir, IVC_K)
        .unwrap_or_else(|e| panic!("failed to load midnight-srs-2p{IVC_K} from {srs_dir:?}: {e}"));
    let (mut aggregator, agg_verifier) = ProofAggregation::setup(aggregator_srs, IVC_K, inner_ctx);

    // Stash the first dust proof's (vk, pis, inner-proof-bytes) so the size-1
    // baseline below can reuse it without re-running the expensive halo2
    // proving step a second time — only the (cheap) IVC fold and verification
    // steps are re-run for the comparison.
    let mut first_witness_ingredients = None;

    let agg_start = Instant::now();
    let mut ivc_proof = None;
    for (i, dust_preimage) in dust_preimages.iter().enumerate() {
        let preproc = dust_agg_ir
            .preprocess(dust_preimage)
            .expect("dust AggregableIrSource preprocess must succeed");
        let pis = preproc.pis.clone();
        let proof_bytes = midnight_zk_stdlib::prove::<
            AggregableIrSource,
            AggregationTranscript<transient_crypto::curve::outer::Scalar>,
        >(
            inner_srs.as_ref(),
            &dust_agg_pk,
            &dust_agg_ir,
            &pis,
            preproc,
            rng.split(),
        )
        .expect("dust inner AggregableIrSource prove must succeed");
        if i == 0 {
            first_witness_ingredients =
                Some((dust_agg_vk.clone(), pis.clone(), proof_bytes.clone()));
        }
        let witness =
            AggregationWitness::new::<AggregableIrSource>(dust_agg_vk.clone(), pis, proof_bytes);
        ivc_proof = Some(
            aggregator
                .aggregate(witness)
                .expect("dust IVC aggregation step must succeed"),
        );
    }
    let aggregation_time = agg_start.elapsed();
    let ivc_proof = ivc_proof.expect("must aggregate at least one dust proof");
    let proof_size_bytes = ivc_proof.len();

    let ivc_instance: IvcInstance<ProofAggregation> = aggregator.instance();

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
        ivc_instance: vec![],
    };
    let verifier = DirectVerifier {
        verifier: agg_verifier,
        instance: ivc_instance,
    };

    let agg_evidence = vec![ContractProofEvidence::Aggregated { proof: agg_proof }];
    let verify_start = Instant::now();
    <ProofMarker as ProofKind<InMemoryDB>>::verify_aggregated_proofs(&agg_evidence, &verifier)
        .expect("aggregated proof verification must succeed");
    let verify_time = verify_start.elapsed();

    // ── Baseline: aggregate just 1 of the same dust proofs ────────────────────
    // Same circuit, same vk/pk, same already-computed inner proof — only the
    // IVC fold count differs (1 vs N) — to isolate how *that* affects the
    // resulting proof's size and verification time.
    let (vk0, pis0, proof_bytes0) =
        first_witness_ingredients.expect("at least one dust proof must have been folded");

    let inner_params_1 = inner_srs.verifier_params();
    let inner_ctx_1 = InnerCircuitsContext::new(
        AggregableIrSource::aggregation_arch(),
        INNER_K,
        inner_params_1,
    );
    let aggregator_srs_1 = transient_crypto::aggregation::load_midnight_srs(&srs_dir, IVC_K)
        .unwrap_or_else(|e| panic!("failed to load midnight-srs-2p{IVC_K} from {srs_dir:?}: {e}"));
    let (mut aggregator_1, agg_verifier_1) =
        ProofAggregation::setup(aggregator_srs_1, IVC_K, inner_ctx_1);

    let agg_start_1 = Instant::now();
    let witness_1 = AggregationWitness::new::<AggregableIrSource>(vk0, pis0, proof_bytes0);
    let ivc_proof_1 = aggregator_1
        .aggregate(witness_1)
        .expect("single-proof IVC aggregation step must succeed");
    let aggregation_time_1 = agg_start_1.elapsed();
    let proof_size_bytes_1 = ivc_proof_1.len();

    let ivc_instance_1: IvcInstance<ProofAggregation> = aggregator_1.instance();
    let agg_proof_1 = AggregatedContractProof {
        ivc_proof: ivc_proof_1,
        ivc_instance: vec![],
    };
    let verifier_1 = DirectVerifier {
        verifier: agg_verifier_1,
        instance: ivc_instance_1,
    };
    let agg_evidence_1 = vec![ContractProofEvidence::Aggregated { proof: agg_proof_1 }];
    let verify_start_1 = Instant::now();
    <ProofMarker as ProofKind<InMemoryDB>>::verify_aggregated_proofs(&agg_evidence_1, &verifier_1)
        .expect("single-proof aggregated verification must succeed");
    let verify_time_1 = verify_start_1.elapsed();

    // ── Report ─────────────────────────────────────────────────────────────
    println!(
        "[proof aggregation] N={N} dust proofs: aggregation took {aggregation_time:?}, \
         resulting proof = {proof_size_bytes} bytes, verification took {verify_time:?}"
    );
    println!(
        "[proof aggregation] N=1 dust proof (baseline): aggregation took {aggregation_time_1:?}, \
         resulting proof = {proof_size_bytes_1} bytes, verification took {verify_time_1:?}"
    );
    println!(
        "[proof aggregation] verification time for N={N} vs N=1: {:.2}x \
         (IVC aggregation makes verification ~O(1) in the number of folded proofs, \
         so this should stay close to 1x rather than scale with N)",
        verify_time.as_secs_f64() / verify_time_1.as_secs_f64().max(f64::EPSILON)
    );
}
