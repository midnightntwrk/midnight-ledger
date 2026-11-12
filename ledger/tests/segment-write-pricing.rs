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

//! Regression tests for pricing writes per segment. Declared deletions are only
//! checked when their transcript runs, and a failing fallible segment commits
//! none of them, so they must not offset writes in another segment.

use base_crypto::cost_model::{CostDuration, RunningCost};
use base_crypto::time::Timestamp;
use coin_structure::contract::ContractAddress;
use midnight_ledger::construct::{ContractCallPrototype, PreTranscript, partition_transcripts};
use midnight_ledger::error::FeeCalculationError;
use midnight_ledger::semantics::TransactionResult;
use midnight_ledger::structure::{
    ContractDeploy, INITIAL_PARAMETERS, Intent, ProofPreimageMarker, Signature, Transaction,
};
use midnight_ledger::test_utilities::{TestState, test_intents, test_intents_adv};
use midnight_ledger::verify::WellFormedStrictness;
use midnight_ledger_v9 as midnight_ledger;
use onchain_runtime::context::QueryContext;
use onchain_runtime::ops::Op;
use onchain_runtime::result_mode::ResultModeVerify;
use onchain_runtime::state::{ContractOperation, ContractState, StateValue, stval};
use onchain_runtime::transcript::Transcript;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serialize::Deserializable;
use std::borrow::Cow;
use storage::arena::Sp;
use storage::db::InMemoryDB;
use storage::storage::HashMap;
use transient_crypto::commitment::PedersenRandomness;
use transient_crypto::proofs::{KeyLocation, VerifierKey};

type Tx = Transaction<Signature, ProofPreimageMarker, PedersenRandomness, InMemoryDB>;
type Intents = HashMap<
    u16,
    Intent<Signature, ProofPreimageMarker, PedersenRandomness, InMemoryDB>,
    InMemoryDB,
>;
type Program = Vec<Op<ResultModeVerify, InMemoryDB>>;

const ENTRY_POINT: &[u8] = b"run";

/// A placeholder verifier key; contract proofs are not verified in these tests.
fn operation() -> ContractOperation {
    let vk = VerifierKey::deserialize(&mut &b"\x00\x00\x00\x00"[..], 0).unwrap();
    ContractOperation::new(Some(vk), None)
}

fn strictness() -> WellFormedStrictness {
    let mut strictness = WellFormedStrictness::default();
    strictness.enforce_balancing = false;
    strictness.verify_contract_proofs = false;
    strictness.verify_native_proofs = false;
    strictness.verify_signatures = false;
    strictness
}

fn deploy(
    rng: &mut StdRng,
    state: &mut TestState<InMemoryDB>,
    initial: StateValue<InMemoryDB>,
) -> ContractAddress {
    let contract = ContractState::new(
        initial,
        HashMap::new().insert(ENTRY_POINT.into(), operation()),
        Default::default(),
    );
    let deploy = ContractDeploy::new(rng, contract);
    let addr = deploy.address();
    let tx: Tx = Transaction::from_intents(
        "local-test",
        test_intents(rng, vec![], vec![], vec![deploy], Timestamp::from_secs(0)),
    );
    state.assert_apply(&tx, strictness());
    addr
}

/// Replaces the contract state with a map of `entries` entries.
fn grow_program(entries: u32) -> Program {
    vec![
        Op::Pop,
        Op::Push {
            storage: true,
            value: stval!({0u32 => (0u64)}; entries),
        },
    ]
}

/// Replaces the contract state with `Null`, deleting what was there.
fn clear_program() -> Program {
    vec![
        Op::Pop,
        Op::Push {
            storage: true,
            value: StateValue::Null,
        },
    ]
}

/// The transcript, with gas as declared by the construction heuristic.
fn transcript(
    state: &TestState<InMemoryDB>,
    addr: ContractAddress,
    program: Program,
) -> Transcript<InMemoryDB> {
    partition_transcripts(
        &[PreTranscript {
            context: QueryContext::new(state.ledger.index(addr).unwrap().data, addr),
            program,
            comm_comm: None,
        }],
        &INITIAL_PARAMETERS,
    )
    .unwrap()[0]
        .0
        .clone()
        .unwrap()
}

/// A transcript that does nothing, but declares `bytes_deleted` deletions.
fn phantom_deletions(bytes_deleted: u64) -> Transcript<InMemoryDB> {
    Transcript {
        gas: RunningCost {
            compute_time: CostDuration::from_picoseconds(10_000_000_000),
            bytes_deleted,
            ..RunningCost::ZERO
        },
        effects: Default::default(),
        program: vec![Op::Ckpt].into(),
        version: None,
    }
}

fn call(
    rng: &mut StdRng,
    addr: ContractAddress,
    guaranteed: Option<Transcript<InMemoryDB>>,
    fallible: Option<Transcript<InMemoryDB>>,
) -> ContractCallPrototype<InMemoryDB> {
    ContractCallPrototype {
        address: addr,
        entry_point: ENTRY_POINT.into(),
        op: operation(),
        guaranteed_public_transcript: guaranteed,
        fallible_public_transcript: fallible,
        private_transcript_outputs: vec![],
        input: ().into(),
        output: ().into(),
        communication_commitment_rand: rng.r#gen(),
        key_location: KeyLocation(Cow::Borrowed("run")),
    }
}

fn intents(rng: &mut StdRng, calls: Vec<ContractCallPrototype<InMemoryDB>>) -> Intents {
    test_intents(rng, calls, vec![], vec![], Timestamp::from_secs(0))
}

fn tx(intents: Intents) -> Tx {
    Transaction::from_intents("local-test", intents)
}

fn net_written(tx: &Tx) -> u64 {
    tx.cost(&INITIAL_PARAMETERS, false).unwrap().bytes_written
}

// Enough entries for the declared net writes to exceed the block limit, while
// the transaction itself stays within the block usage limit.
const OVER_LIMIT_ENTRIES: u32 = 2800;

fn assert_over_block_limit(tx: &Tx) {
    assert!(
        net_written(tx) > INITIAL_PARAMETERS.limits.block_limits.bytes_written,
        "net writes priced at {}",
        net_written(tx)
    );
    assert!(matches!(
        tx.fees(&INITIAL_PARAMETERS, false),
        Err(FeeCalculationError::BlockLimitExceeded)
    ));
}

#[test]
fn fallible_deletions_do_not_offset_guaranteed_writes() {
    let mut rng = StdRng::seed_from_u64(0x50);
    let mut state: TestState<InMemoryDB> = TestState::new(&mut rng);
    let addr = deploy(&mut rng, &mut state, StateValue::Null);

    let grow = transcript(&state, addr, grow_program(OVER_LIMIT_ENTRIES));
    let declared = grow.gas.bytes_written;
    let calls = vec![call(&mut rng, addr, Some(grow.clone()), None)];
    let honest = tx(intents(&mut rng, calls));
    assert_over_block_limit(&honest);

    // The fallible transcript fails its own check, so its deletions never
    // happen, while the guaranteed writes commit.
    let calls = vec![call(
        &mut rng,
        addr,
        Some(grow),
        Some(phantom_deletions(declared)),
    )];
    let attack = tx(intents(&mut rng, calls));
    assert!(net_written(&attack) >= net_written(&honest));
    assert_over_block_limit(&attack);

    // Pricing aside, application is as before: the guaranteed segment commits.
    let res = state.apply(&attack, strictness()).unwrap();
    assert!(
        matches!(&res, TransactionResult::PartialSuccess(segments, _)
            if segments[&0].is_ok() && segments[&1].is_err()),
        "{res:?}"
    );
}

#[test]
fn deletions_do_not_offset_writes_across_fallible_segments() {
    let mut rng = StdRng::seed_from_u64(0x51);
    let mut state: TestState<InMemoryDB> = TestState::new(&mut rng);
    let addr = deploy(&mut rng, &mut state, StateValue::Null);

    let grow = transcript(&state, addr, grow_program(OVER_LIMIT_ENTRIES));
    let declared = grow.gas.bytes_written;
    let grow_call = call(&mut rng, addr, None, Some(grow));
    let honest = tx(intents(&mut rng, vec![grow_call.clone()]));
    assert_over_block_limit(&honest);

    // Segment 2 declares deletions that never happen, and fails on its own.
    let cancel_call = call(&mut rng, addr, None, Some(phantom_deletions(declared)));
    let attack = tx(test_intents_adv(
        intents(&mut rng, vec![grow_call]),
        &mut rng,
        2,
        vec![cancel_call],
        vec![],
        vec![],
        Timestamp::from_secs(0),
    ));
    assert!(net_written(&attack) >= net_written(&honest));
    assert_over_block_limit(&attack);
}

#[test]
fn deletions_offset_writes_within_a_segment() {
    const ENTRIES: u32 = 300;
    let mut rng = StdRng::seed_from_u64(0x52);
    let mut state: TestState<InMemoryDB> = TestState::new(&mut rng);
    let grown = deploy(&mut rng, &mut state, StateValue::Null);
    let cleared = deploy(&mut rng, &mut state, stval!({1u32 => (1u64)}; ENTRIES));

    let grow = transcript(&state, grown, grow_program(ENTRIES));
    let clear = transcript(&state, cleared, clear_program());
    assert!(clear.gas.bytes_deleted > 0);
    let calls = vec![call(&mut rng, grown, Some(grow.clone()), None)];
    let grow_only = tx(intents(&mut rng, calls));

    // Both calls commit together, so the deletions really happen, and still
    // offset the writes.
    let calls = vec![
        call(&mut rng, grown, Some(grow), None),
        call(&mut rng, cleared, Some(clear), None),
    ];
    let both = tx(intents(&mut rng, calls));
    assert!(
        net_written(&both) < net_written(&grow_only),
        "{} vs {}",
        net_written(&both),
        net_written(&grow_only)
    );
    let res = state.apply(&both, strictness()).unwrap();
    assert!(matches!(res, TransactionResult::Success(_)), "{res:?}");
}
