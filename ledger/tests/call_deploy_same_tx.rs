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

//! A contract may be called, and its maintenance authority updated, in the
//! intent that deploys it, provided the action comes after the deploy and a
//! call runs only in the fallible phase. This is what lets a contract's
//! initialisation be a circuit call, proven like any other, with the
//! contract's address known to it.
//!
//! The action is checked against the deploy's state, so it must be applied
//! only if that deploy took effect: the intent's atomicity ensures it, and
//! deploys in other intents, which fail independently, are not consulted.

#![deny(warnings)]

use base_crypto::cost_model::RunningCost;
use base_crypto::fab::AlignedValue;
use base_crypto::rng::SplittableRng;
use base_crypto::time::{Duration, Timestamp};
use coin_structure::contract::ContractAddress;
use lazy_static::lazy_static;
use midnight_ledger::construct::{ContractCallPrototype, PreTranscript, partition_transcripts};
use midnight_ledger::error::{MalformedTransaction, TransactionInvalid};
use midnight_ledger::semantics::TransactionResult;
use midnight_ledger::structure::{
    ContractDeploy, ContractOperationVersion, INITIAL_PARAMETERS, Intent, MaintenanceUpdate,
    ProofPreimageMarker, Signature, SingleUpdate, Transaction,
};
use midnight_ledger::test_utilities::{
    Resolver, TestState, contract_operation, test_resolver, tx_prove,
};
use midnight_ledger::verify::WellFormedStrictness;
use midnight_ledger_v10 as midnight_ledger;
use onchain_runtime::context::QueryContext;
use onchain_runtime::ops::{Key, Op, key};
use onchain_runtime::program_fragments::*;
use onchain_runtime::result_mode::{ResultModeGather, ResultModeVerify};
use onchain_runtime::state::{
    ContractMaintenanceAuthority, ContractMaintenanceVerifyingKey, ContractOperation,
    ContractState, StateValue, stval,
};
use onchain_runtime::transcript::Transcript;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serialize::Deserializable;
use std::borrow::Cow;
use storage::arena::Sp;
use storage::db::{DB, InMemoryDB};
use storage::storage::HashMap;
use transient_crypto::commitment::PedersenRandomness;
use transient_crypto::proofs::{KeyLocation, ProofPreimage};

lazy_static! {
    static ref RESOLVER: Resolver = test_resolver("fallible");
}

type TestIntent = Intent<Signature, ProofPreimageMarker, PedersenRandomness, InMemoryDB>;
type TestTransaction = Transaction<Signature, ProofPreimageMarker, PedersenRandomness, InMemoryDB>;

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

/// The initial state of the `fallible` test contract: `attempts`, `first`,
/// `successes`.
fn count_contract(count_op: &ContractOperation) -> ContractState<InMemoryDB> {
    ContractState::new(
        stval!([(0u64), (false), (0u64)]),
        HashMap::new().insert(b"count"[..].into(), count_op.clone()),
        Default::default(),
    )
}

/// The whole of `count`, from its initial state: it increments `attempts`,
/// checkpoints, asserts `!first`, sets it and increments `successes`.
fn count_program() -> Vec<Op<ResultModeVerify, InMemoryDB>> {
    program_with_results(
        &[
            &Counter_increment!([key!(0u8)], false, 1u64)[..],
            &kernel_checkpoint!((), ()),
            &Cell_read!([key!(1u8)], false, bool),
            &Cell_write!([key!(1u8)], false, bool, true),
            &Counter_increment!([key!(2u8)], false, 1u64),
        ]
        .into_iter()
        .flat_map(|x| x.iter())
        .cloned()
        .collect::<Vec<_>>(),
        &[false.into()],
    )
}

/// One transcript of the whole program, run from the contract's initial state,
/// with the partitioner's headroom: 20% on time and writes, deletions declared
/// at their actual figure. With no guaranteed transcript beside it, it need not
/// start with a checkpoint.
fn fallible_only_transcript(
    contract: &ContractState<InMemoryDB>,
    addr: ContractAddress,
) -> Transcript<InMemoryDB> {
    let program = count_program();
    let results = QueryContext::new(contract.data.clone(), addr)
        .query(
            &program,
            None,
            &INITIAL_PARAMETERS.cost_model.runtime_cost_model,
        )
        .expect("the whole of `count` runs from the initial state");
    let actual = results.gas_cost;
    Transcript {
        gas: RunningCost {
            bytes_deleted: actual.bytes_deleted,
            ..actual * 1.2
        },
        effects: results.context.effects.clone(),
        program: program.into(),
        version: Some(Sp::new(Transcript::<InMemoryDB>::VERSION)),
    }
}

/// The same program split at its checkpoint, as the partitioner would: the
/// first increment guaranteed, the rest fallible. Each part is partitioned on
/// its own, as `fallible.rs` does, since the partitioner keeps a whole call
/// guaranteed whenever it fits the budget.
fn split_transcripts(
    contract: &ContractState<InMemoryDB>,
    addr: ContractAddress,
) -> (Transcript<InMemoryDB>, Transcript<InMemoryDB>) {
    let part = |program: Vec<Op<ResultModeVerify, InMemoryDB>>| {
        partition_transcripts(
            &[PreTranscript {
                context: QueryContext::new(contract.data.clone(), addr),
                program,
                comm_comm: None,
            }],
            &INITIAL_PARAMETERS,
        )
        .unwrap()
        .remove(0)
        .0
        .expect("a non-empty program fits the guaranteed budget")
    };
    let program = count_program();
    let checkpoint = program
        .iter()
        .position(|op| matches!(op, Op::Ckpt))
        .expect("`count` checkpoints");
    (
        part(program[..checkpoint].to_vec()),
        part(program[checkpoint..].to_vec()),
    )
}

fn count_call<R: Rng + ?Sized>(
    rng: &mut R,
    addr: ContractAddress,
    count_op: &ContractOperation,
    guaranteed: Option<Transcript<InMemoryDB>>,
    fallible: Option<Transcript<InMemoryDB>>,
) -> ContractCallPrototype<InMemoryDB> {
    ContractCallPrototype {
        inner_proofs: vec![],
        address: addr,
        entry_point: b"count"[..].into(),
        op: count_op.clone(),
        input: ().into(),
        output: ().into(),
        guaranteed_public_transcript: guaranteed,
        fallible_public_transcript: fallible,
        private_transcript_outputs: vec![],
        communication_commitment_rand: rng.r#gen(),
        key_location: KeyLocation(Cow::Borrowed("count")),
    }
}

fn empty_intent<R: Rng + rand::CryptoRng + ?Sized>(rng: &mut R, tblock: Timestamp) -> TestIntent {
    Intent::empty(rng, tblock + Duration::from_secs(3600))
}

fn transaction(intents: &[(u16, TestIntent)]) -> TestTransaction {
    let mut map = HashMap::new();
    for (segment, intent) in intents {
        map = map.insert(*segment, intent.clone());
    }
    Transaction::from_intents("local-test", map)
}

fn strictness() -> WellFormedStrictness {
    let mut strictness = WellFormedStrictness::default();
    strictness.enforce_balancing = false;
    strictness
}

async fn setup() -> (
    StdRng,
    TestState<InMemoryDB>,
    ContractOperation,
    ContractState<InMemoryDB>,
    ContractDeploy<InMemoryDB>,
    ContractAddress,
) {
    let mut rng = StdRng::seed_from_u64(0x42);
    let state: TestState<InMemoryDB> = TestState::new(&mut rng);
    let count_op = contract_operation(&RESOLVER, "count").await;
    let contract = count_contract(&count_op);
    let deploy = ContractDeploy::new(&mut rng, contract.clone());
    let addr = deploy.address();
    (rng, state, count_op, contract, deploy, addr)
}

/// The deploy, then a fallible-only call on it, in one intent: the call is
/// verified against the deploy's verifier key and applied on the deployed
/// state, atomically with the deploy.
#[tokio::test]
async fn deploy_then_call_in_one_intent_lands() {
    let (mut rng, mut state, count_op, contract, deploy, addr) = setup().await;
    let call = count_call(
        &mut rng,
        addr,
        &count_op,
        None,
        Some(fallible_only_transcript(&contract, addr)),
    );
    let intent = empty_intent(&mut rng, state.time)
        .add_deploy(deploy)
        .add_call::<ProofPreimage>(call);
    let tx = tx_prove(rng.split(), &transaction(&[(1, intent)]), &RESOLVER)
        .await
        .unwrap();

    assert!(state.ledger.index(addr).is_none());
    state.assert_apply(&tx, strictness());
    assert_eq!(
        state.ledger.index(addr).unwrap().data.get_ref(),
        &stval!([(1u64), (true), (1u64)])
    );
}

/// The call placed before the deploy in the intent's actions would be applied
/// before the contract exists; it is rejected rather than failing on
/// application.
#[tokio::test]
async fn call_before_deploy_is_rejected() {
    let (mut rng, state, count_op, contract, deploy, addr) = setup().await;
    let call = count_call(
        &mut rng,
        addr,
        &count_op,
        None,
        Some(fallible_only_transcript(&contract, addr)),
    );
    let intent = empty_intent(&mut rng, state.time)
        .add_call::<ProofPreimage>(call)
        .add_deploy(deploy);
    let tx = tx_prove(rng.split(), &transaction(&[(1, intent)]), &RESOLVER)
        .await
        .unwrap();

    assert!(matches!(
        tx.well_formed(&state.ledger, strictness(), state.time),
        Err(MalformedTransaction::ActionPrecedesDeploy { address }) if address == addr
    ));
}

/// A call with a guaranteed transcript runs before any deploy, whatever its
/// position, so it cannot target a contract deployed in the same intent.
#[tokio::test]
async fn guaranteed_call_on_same_intent_deploy_is_rejected() {
    let (mut rng, state, count_op, contract, deploy, addr) = setup().await;
    let (guaranteed, fallible) = split_transcripts(&contract, addr);
    for (guaranteed, fallible) in [
        (Some(guaranteed.clone()), Some(fallible)),
        (Some(guaranteed), None),
    ] {
        let call = count_call(&mut rng, addr, &count_op, guaranteed, fallible);
        let intent = empty_intent(&mut rng, state.time)
            .add_deploy(deploy.clone())
            .add_call::<ProofPreimage>(call);
        let tx = tx_prove(rng.split(), &transaction(&[(1, intent)]), &RESOLVER)
            .await
            .unwrap();

        assert!(matches!(
            tx.well_formed(&state.ledger, strictness(), state.time),
            Err(MalformedTransaction::ActionPrecedesDeploy { address }) if address == addr
        ));
    }
}

/// A call on a contract that neither the chain nor the transaction holds is
/// rejected as before.
#[tokio::test]
async fn call_on_undeployed_contract_is_still_rejected() {
    let (mut rng, state, count_op, contract, _deploy, addr) = setup().await;
    let call = count_call(
        &mut rng,
        addr,
        &count_op,
        None,
        Some(fallible_only_transcript(&contract, addr)),
    );
    let intent = empty_intent(&mut rng, state.time).add_call::<ProofPreimage>(call);
    let tx = tx_prove(rng.split(), &transaction(&[(1, intent)]), &RESOLVER)
        .await
        .unwrap();

    assert!(matches!(
        tx.well_formed(&state.ledger, strictness(), state.time),
        Err(MalformedTransaction::ContractNotPresent(address)) if address == addr
    ));
}

/// A maintenance update follows the same rule: after its deploy in the same
/// intent it is checked against the deploy's authority and lands; before it,
/// it is rejected.
#[tokio::test]
async fn maintenance_update_in_deploying_intent() {
    let mut rng = StdRng::seed_from_u64(0x42);
    let mut state: TestState<InMemoryDB> = TestState::new(&mut rng);
    let authority = ContractMaintenanceAuthority {
        committee: vec![],
        threshold: 0,
        counter: 0,
    };
    let deploy = ContractDeploy::new(
        &mut rng,
        ContractState::new(StateValue::Null, HashMap::new(), authority),
    );
    let addr = deploy.address();
    let next_authority = ContractMaintenanceAuthority {
        committee: vec![],
        threshold: 1,
        counter: 1,
    };
    let update = MaintenanceUpdate::new(
        addr,
        vec![SingleUpdate::ReplaceAuthority(next_authority.clone())],
        0,
    );

    let preceding = empty_intent(&mut rng, state.time)
        .add_maintenance_update(update.clone())
        .add_deploy(deploy.clone());
    assert!(matches!(
        transaction(&[(1, preceding)]).well_formed(&state.ledger, strictness(), state.time),
        Err(MalformedTransaction::ActionPrecedesDeploy { address }) if address == addr
    ));

    let following = empty_intent(&mut rng, state.time)
        .add_deploy(deploy)
        .add_maintenance_update(update);
    let tx = transaction(&[(1, following)]);
    let result = state.apply(&tx, strictness()).expect("well-formed");
    assert!(matches!(result, TransactionResult::Success(..)));
    assert_eq!(
        state.ledger.index(addr).unwrap().maintenance_authority,
        next_authority
    );
}

// The tests below run unproven transactions, so they need no key material.
// What they observe is which operations and which authority an action is
// checked against, and whether it is applied.

type SigningKey = base_crypto::schnorr::SigningKey;

/// An operation with a placeholder verifier key.
fn placeholder_op() -> ContractOperation {
    let mut op = ContractOperation::new(None, None);
    op.v2 = Some(
        transient_crypto_2::proofs::VerifierKey::deserialize(&mut &b"\x00\x00\x00\x00"[..], 0)
            .unwrap(),
    );
    op
}

fn sole_authority(sk: &SigningKey, counter: u32) -> ContractMaintenanceAuthority {
    ContractMaintenanceAuthority {
        committee: vec![ContractMaintenanceVerifyingKey::Schnorr(sk.verifying_key())],
        threshold: 1,
        counter,
    }
}

fn signed_update(
    rng: &mut StdRng,
    sk: &SigningKey,
    addr: ContractAddress,
    updates: Vec<SingleUpdate>,
    counter: u32,
) -> MaintenanceUpdate<InMemoryDB> {
    let update = MaintenanceUpdate::new(addr, updates, counter);
    let data = update.data_to_sign();
    update.add_signature(0, Signature::Schnorr(sk.sign(rng, &data)))
}

fn remove_count_key() -> Vec<SingleUpdate> {
    vec![SingleUpdate::VerifierKeyRemove(
        b"count"[..].into(),
        ContractOperationVersion::V3,
    )]
}

/// The `count` contract under `sk`'s authority, with its deploy and address.
fn owned_count_contract(
    rng: &mut StdRng,
    sk: &SigningKey,
) -> (
    ContractState<InMemoryDB>,
    ContractDeploy<InMemoryDB>,
    ContractAddress,
) {
    let contract = ContractState::new(
        stval!([(0u64), (false), (0u64)]),
        HashMap::new().insert(b"count"[..].into(), placeholder_op()),
        sole_authority(sk, 0),
    );
    let deploy = ContractDeploy::new(rng, contract.clone());
    let addr = deploy.address();
    (contract, deploy, addr)
}

fn unproven_count_call(
    rng: &mut StdRng,
    contract: &ContractState<InMemoryDB>,
    addr: ContractAddress,
) -> ContractCallPrototype<InMemoryDB> {
    count_call(
        rng,
        addr,
        &placeholder_op(),
        None,
        Some(fallible_only_transcript(contract, addr)),
    )
}

/// Applies `tx`, expecting it well-formed and `segment` to fail as a deploy of
/// a contract already on chain.
fn assert_fails_as_already_deployed(
    state: &mut TestState<InMemoryDB>,
    tx: &TestTransaction,
    segment: u16,
    addr: ContractAddress,
) {
    let TransactionResult::PartialSuccess(segments, _) =
        state.apply(tx, strictness()).expect("well-formed")
    else {
        panic!("expected partial success");
    };
    assert!(matches!(
        segments.get(&segment),
        Some(Err(TransactionInvalid::ContractAlreadyDeployed(address))) if *address == addr
    ));
}

/// A deploy in another intent is not consulted, in either segment order: the
/// call's segment could be applied though the deploy's failed.
#[test]
fn deploy_in_another_intent_is_not_consulted() {
    let mut rng = StdRng::seed_from_u64(0x42);
    let state: TestState<InMemoryDB> = TestState::new(&mut rng);
    let sk = SigningKey::sample(&mut rng);
    let (contract, deploy, addr) = owned_count_contract(&mut rng, &sk);
    for (deploy_segment, call_segment) in [(1, 2), (2, 1)] {
        let call = unproven_count_call(&mut rng, &contract, addr);
        let deploying = empty_intent(&mut rng, state.time).add_deploy(deploy.clone());
        let calling = empty_intent(&mut rng, state.time).add_call::<ProofPreimage>(call);
        let tx = transaction(&[(deploy_segment, deploying), (call_segment, calling)]);
        assert!(matches!(
            tx.well_formed(&state.ledger, strictness(), state.time),
            Err(MalformedTransaction::ContractNotPresent(address)) if address == addr
        ));
    }
}

/// Replaying the deploy of a contract already on chain does not bring back
/// the authority it was deployed with. From another intent the replay is not
/// consulted; in the update's own intent it fails, and the update with it.
#[test]
fn replayed_deploy_does_not_revive_a_replaced_authority() {
    let mut rng = StdRng::seed_from_u64(0x42);
    let mut state: TestState<InMemoryDB> = TestState::new(&mut rng);
    let old_sk = SigningKey::sample(&mut rng);
    let new_sk = SigningKey::sample(&mut rng);
    let (_, deploy, addr) = owned_count_contract(&mut rng, &old_sk);
    let deploying = empty_intent(&mut rng, state.time).add_deploy(deploy.clone());
    state.assert_apply(&transaction(&[(1, deploying)]), strictness());

    let rotated = sole_authority(&new_sk, 1);
    let rotation = signed_update(
        &mut rng,
        &old_sk,
        addr,
        vec![SingleUpdate::ReplaceAuthority(rotated.clone())],
        0,
    );
    let rotating = empty_intent(&mut rng, state.time).add_maintenance_update(rotation);
    state.assert_apply(&transaction(&[(1, rotating)]), strictness());

    // The replaced key signs a further update at the live counter.
    let takeover = signed_update(
        &mut rng,
        &old_sk,
        addr,
        vec![SingleUpdate::ReplaceAuthority(sole_authority(&old_sk, 2))],
        1,
    );

    let redeploying = empty_intent(&mut rng, state.time).add_deploy(deploy.clone());
    let updating = empty_intent(&mut rng, state.time).add_maintenance_update(takeover.clone());
    assert!(matches!(
        transaction(&[(1, redeploying), (2, updating)]).well_formed(
            &state.ledger,
            strictness(),
            state.time
        ),
        Err(MalformedTransaction::InvalidCommitteeSignature { .. })
    ));

    let together = empty_intent(&mut rng, state.time)
        .add_deploy(deploy)
        .add_maintenance_update(takeover);
    assert_fails_as_already_deployed(&mut state, &transaction(&[(1, together)]), 1, addr);
    assert_eq!(
        state.ledger.index(addr).unwrap().maintenance_authority,
        rotated
    );
}

/// Nor does it bring back a verifier key the authority has removed.
#[test]
fn replayed_deploy_does_not_revive_a_removed_verifier_key() {
    let mut rng = StdRng::seed_from_u64(0x42);
    let mut state: TestState<InMemoryDB> = TestState::new(&mut rng);
    let sk = SigningKey::sample(&mut rng);
    let (contract, deploy, addr) = owned_count_contract(&mut rng, &sk);
    let deploying = empty_intent(&mut rng, state.time).add_deploy(deploy.clone());
    state.assert_apply(&transaction(&[(1, deploying)]), strictness());

    let removal = signed_update(&mut rng, &sk, addr, remove_count_key(), 0);
    let removing = empty_intent(&mut rng, state.time).add_maintenance_update(removal);
    state.assert_apply(&transaction(&[(1, removing)]), strictness());

    let call = unproven_count_call(&mut rng, &contract, addr);
    let redeploying = empty_intent(&mut rng, state.time).add_deploy(deploy.clone());
    let calling = empty_intent(&mut rng, state.time).add_call::<ProofPreimage>(call);
    assert!(matches!(
        transaction(&[(1, redeploying), (2, calling)]).well_formed(
            &state.ledger,
            strictness(),
            state.time
        ),
        Err(MalformedTransaction::VerifierKeyNotPresent { .. })
    ));

    let call = unproven_count_call(&mut rng, &contract, addr);
    let together = empty_intent(&mut rng, state.time)
        .add_deploy(deploy)
        .add_call::<ProofPreimage>(call);
    assert_fails_as_already_deployed(&mut state, &transaction(&[(1, together)]), 1, addr);
    assert_eq!(
        state.ledger.index(addr).unwrap().data.get_ref(),
        &stval!([(0u64), (false), (0u64)])
    );
}

/// The deploy's state stands for the contract only up to the first maintenance
/// update, so a call or a second update after it is rejected; an update after
/// a call is not affected.
#[test]
fn action_after_update_in_deploying_intent_is_rejected() {
    let mut rng = StdRng::seed_from_u64(0x42);
    let mut state: TestState<InMemoryDB> = TestState::new(&mut rng);
    let sk = SigningKey::sample(&mut rng);
    let (contract, deploy, addr) = owned_count_contract(&mut rng, &sk);
    let removal = signed_update(&mut rng, &sk, addr, remove_count_key(), 0);
    let rotation = signed_update(
        &mut rng,
        &sk,
        addr,
        vec![SingleUpdate::ReplaceAuthority(sole_authority(&sk, 2))],
        1,
    );

    let call = unproven_count_call(&mut rng, &contract, addr);
    let update_then_call = empty_intent(&mut rng, state.time)
        .add_deploy(deploy.clone())
        .add_maintenance_update(removal.clone())
        .add_call::<ProofPreimage>(call);
    let update_then_update = empty_intent(&mut rng, state.time)
        .add_deploy(deploy.clone())
        .add_maintenance_update(removal.clone())
        .add_maintenance_update(rotation);
    for intent in [update_then_call, update_then_update] {
        assert!(matches!(
            transaction(&[(1, intent)]).well_formed(&state.ledger, strictness(), state.time),
            Err(MalformedTransaction::ActionFollowsUpdate { address }) if address == addr
        ));
    }

    let call = unproven_count_call(&mut rng, &contract, addr);
    let call_then_update = empty_intent(&mut rng, state.time)
        .add_deploy(deploy)
        .add_call::<ProofPreimage>(call)
        .add_maintenance_update(removal);
    state.assert_apply(&transaction(&[(1, call_then_update)]), strictness());
    let on_chain = state.ledger.index(addr).unwrap();
    assert_eq!(on_chain.data.get_ref(), &stval!([(1u64), (true), (1u64)]));
    assert!(on_chain.operations.get(&b"count"[..].into()).is_none());
}
