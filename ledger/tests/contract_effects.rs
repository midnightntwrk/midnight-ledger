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

use base_crypto::rng::SplittableRng;
use base_crypto::time::Timestamp;
use lazy_static::lazy_static;
use midnight_ledger::construct::{ContractCallPrototype, PreTranscript, partition_transcripts};
use midnight_ledger::semantics::TransactionResult;
use midnight_ledger::structure::{ContractDeploy, INITIAL_PARAMETERS, Transaction};
use midnight_ledger::test_utilities::{Resolver, TestState, test_resolver, verifier_key};
use midnight_ledger::test_utilities::{test_intents, tx_prove};
use midnight_ledger::verify::WellFormedStrictness;
use onchain_runtime::context::QueryContext;
use onchain_runtime::ops::{Key, Op, key};
use onchain_runtime::program_fragments::*;
use onchain_runtime::result_mode::{ResultModeGather, ResultModeVerify};
use onchain_runtime::state::{ContractOperation, ContractState, StateValue, stval};
use storage::storage::HashMap;
use transient_crypto::proofs::KeyLocation;
//use onchain_runtime::{key, stval};
use base_crypto::fab::AlignedValue;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::borrow::Cow;
use storage::arena::Sp;
use storage::db::{DB, InMemoryDB};

lazy_static! {
    static ref RESOLVER: Resolver = test_resolver("fallible");
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

/// **A real contract call, applied both ways.**
///
/// The gate for the carried post-state. Everything the transport does for unshielded, dust
/// and zswap was checkable by projecting the transaction; a contract's post-state is the
/// *output* of running its transcript, so this is the first family whose producer had to be
/// the applier itself.
///
/// Deploy a counter, call it, then from **one** starting ledger:
///
/// ```text
///   apply_with_effects  →  (state_A, result, effects)
///   apply_effects       →  state_B
///   state_A.state_hash() == state_B.state_hash()
/// ```
///
/// ⚠︎ The controls matter more here than anywhere else in this file's neighbours. A
/// transport that carried *nothing* would leave the contract untouched and still reach a
/// state — just the wrong one — so the test asserts the effects actually carry a contract,
/// that the contract's cell moved, and that the starting ledger is not already the answer.
#[tokio::test]
async fn a_contract_call_replays_from_its_carried_post_state() {
    use midnight_ledger::effects::ContractsPresent;

    let mut rng = StdRng::seed_from_u64(0x42);
    let mut state: TestState<InMemoryDB> = TestState::new(&mut rng);

    // ── deploy ──────────────────────────────────────────────────────────────────────────
    let count_op = ContractOperation::new(verifier_key(&RESOLVER, "count").await);
    let contract = ContractState::new(
        stval!([(0u64), (false), (0u64)]),
        HashMap::new().insert(b"count"[..].into(), count_op.clone()),
        Default::default(),
    );
    let (tx, addr) = {
        let deploy = ContractDeploy::new(&mut rng, contract.clone());
        let addr = deploy.address();
        let tx = tx_prove(
            rng.split(),
            &Transaction::from_intents(
                "local-test",
                test_intents(&mut rng, Vec::new(), Vec::new(), vec![deploy], state.time),
            ),
            &RESOLVER,
        )
        .await
        .unwrap();
        (tx, addr)
    };
    let mut strictness = WellFormedStrictness::default();
    strictness.enforce_balancing = false;
    state.assert_apply(&tx, strictness);

    // ── one call ────────────────────────────────────────────────────────────────────────
    let guaranteed_public_transcript = partition_transcripts(
        &[PreTranscript {
            context: QueryContext::new(state.ledger.index(addr).unwrap().data, addr),
            program: program_with_results(&Counter_increment!([key!(0u8)], false, 1u64), &[]),
            comm_comm: None,
        }],
        &INITIAL_PARAMETERS,
    )
    .unwrap()[0]
        .0
        .clone()
        .unwrap();
    let fallible_public_transcript = partition_transcripts(
        &[PreTranscript {
            context: QueryContext::new(state.ledger.index(addr).unwrap().data, addr),
            program: program_with_results(
                &[
                    &kernel_checkpoint!((), ())[..],
                    &Cell_read!([key!(1u8)], false, bool),
                    &Cell_write!([key!(1u8)], false, bool, true),
                    &Counter_increment!([key!(2u8)], false, 1u64),
                ]
                .into_iter()
                .flat_map(|x| x.iter())
                .cloned()
                .collect::<Vec<_>>(),
                &[false.into()],
            ),
            comm_comm: None,
        }],
        &INITIAL_PARAMETERS,
    )
    .unwrap()[0]
        .0
        .clone()
        .unwrap();
    let tx = {
        let call = ContractCallPrototype {
            address: addr,
            entry_point: b"count"[..].into(),
            op: count_op.clone(),
            input: ().into(),
            output: ().into(),
            guaranteed_public_transcript: Some(guaranteed_public_transcript),
            fallible_public_transcript: Some(fallible_public_transcript),
            private_transcript_outputs: vec![],
            communication_commitment_rand: rng.r#gen(),
            key_location: KeyLocation(Cow::Borrowed("count")),
        };
        tx_prove(
            rng.split(),
            &Transaction::from_intents(
                "local-test",
                test_intents(&mut rng, vec![call], Vec::new(), Vec::new(), state.time),
            ),
            &RESOLVER,
        )
        .await
        .unwrap()
    };

    let before = state.ledger.clone();
    let context = state.context();
    let vtx = tx
        .well_formed(&before, strictness, state.time)
        .expect("the call must be well formed");

    // ── the two paths, from the same ledger ─────────────────────────────────────────────
    let (full, _result, fx) = before.apply_with_effects(&vtx, &context);

    let carried: usize = std::iter::once(&fx.guaranteed.contracts)
        .chain(fx.fallible.iter().map(|s| &s.contracts))
        .map(|c| match c {
            ContractsPresent::Carried(v) => v.len(),
            _ => 0,
        })
        .sum();
    assert!(
        carried > 0,
        "the recording carried no contract at all, so replaying it proves nothing"
    );
    assert_ne!(
        before.state_hash(),
        full.state_hash(),
        "applying changed nothing, so equal hashes below would compare two no-ops"
    );
    assert_eq!(
        full.index(addr).unwrap().data.get_ref(),
        &stval!([(1u64), (true), (1u64)]),
        "the contract's own cells did not move as the counter test expects"
    );

    let via_effects = before
        .apply_effects(&fx, &context)
        .expect("effects that carry the post-state must apply");

    // ⚠︎ **`state_hash`, not `Debug`.** An earlier version of this test compared the two
    // contract maps as debug strings and failed with `<Lazy Sp>` against `<[01]: b8>` — the
    // carried graph is reconstituted lazily and its children are simply not dereferenced yet.
    // That is not a difference in the state, and a comparison that cannot tell laziness from
    // divergence reports the wrong thing twice: it fails when nothing is wrong, and it would
    // have passed if both sides were equally lazy and equally wrong.
    assert_eq!(
        full.state_hash(),
        via_effects.state_hash(),
        "the effects path reached a different ledger than the full path\n  full: {:?}\n  fx:   {:?}",
        full.contract,
        via_effects.contract
    );
    // And the carried contract really does hold the post-state, forced this time.
    assert_eq!(
        via_effects.index(addr).unwrap().data.get_ref(),
        &stval!([(1u64), (true), (1u64)]),
        "the carried contract does not hold what the call produced"
    );
}
