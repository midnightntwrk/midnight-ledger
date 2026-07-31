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

#![cfg(feature = "proving")]

//! Regression gate for the Noop field-repr allocation DoS.
//!
//! `Op::Noop { n }` serialises in ~6 bytes, but its `FieldRepr` materialises
//! `n` field elements. A crafted transcript carrying `Noop { n: u32::MAX }`
//! forces ~137 GB of allocation inside `Transaction::well_formed`'s
//! fee-costing path, OOM-killing every validating node. The agreed
//! (costing-only) fix computes the public-input length as
//! `2 + Σ op.field_size()` arithmetically, so the amplification is *priced*
//! (and rejected as fee-too-high) without ever materialising the vector.
//!
//! WARNING — run ONLY under a memory cap. If the costing fix
//! (`ContractCall::public_inputs_len`) is ever regressed, this test allocates
//! ~137 GB; on an overcommit system that invokes the kernel OOM killer, which
//! may kill unrelated processes. It is therefore `#[ignore]`d so it never runs
//! in the default suite, and stays a *cap-required* regression guard even
//! though it passes (~16 MiB) with the fix in place.
//! To run it, confine it to a cgroup memory scope (the legitimate proving path
//! peaks at ~30 MiB, so a 2 GiB cap is ~68x above the real workload and ~68x
//! below the bad allocation):
//!
//! ```text
//! cargo test -p midnight-ledger --features proving --test noop-dos --no-run
//! systemd-run --user --scope -p MemoryMax=2G -p MemorySwapMax=0 \
//!   target/debug/deps/noop_dos-<hash> --exact noop_dos --ignored --nocapture
//! ```

use base_crypto::fab::AlignedValue;
use base_crypto::rng::SplittableRng;
use base_crypto::time::Timestamp;
use lazy_static::lazy_static;
use midnight_ledger::construct::{ContractCallPrototype, PreTranscript, partition_transcripts};
use midnight_ledger::structure::{ContractAction, ContractDeploy, INITIAL_PARAMETERS, Transaction};
use midnight_ledger::test_utilities::{
    Resolver, TestState, test_intents, test_resolver, tx_prove, verifier_key,
};
use midnight_ledger::verify::WellFormedStrictness;
use onchain_runtime::context::QueryContext;
use onchain_runtime::ops::{Key, Op, key};
use onchain_runtime::program_fragments::*;
use onchain_runtime::result_mode::{ResultModeGather, ResultModeVerify};
use onchain_runtime::state::{ContractOperation, ContractState, StateValue, stval};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::borrow::Cow;
use std::time::{Duration, Instant};
use storage::arena::Sp;
use storage::db::{DB, InMemoryDB};
use storage::storage::HashMap;
use transient_crypto::proofs::KeyLocation;

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

#[tokio::test]
#[ignore = "RUN ONLY under a memory cap (see module header) — allocates ~137GB if the public_inputs_len costing fix regresses. Passes at ~16 MiB with the fix in place."]
async fn noop_dos() {
    let mut rng = StdRng::seed_from_u64(0x42);
    // Initial states
    let mut state: TestState<InMemoryDB> = TestState::new(&mut rng);

    // Part 1: Deploy
    println!(":: Part 1: Deploy");
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
                test_intents(
                    &mut rng,
                    Vec::new(),
                    Vec::new(),
                    vec![deploy],
                    Timestamp::from_secs(0),
                ),
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

    // Part 2: First application
    println!(":: Part 2: First count");
    let guaranteed_public_transcript = partition_transcripts(
        &[PreTranscript {
            context: QueryContext::new(state.ledger.index(addr).unwrap().data, addr),
            program: program_with_results::<InMemoryDB>(
                &Counter_increment!([key!(0u8)], false, 1u64),
                &[],
            ),
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
    let mut tx = {
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
                test_intents(
                    &mut rng,
                    vec![call],
                    Vec::new(),
                    Vec::new(),
                    Timestamp::from_secs(0),
                ),
            ),
            &RESOLVER,
        )
        .await
        .unwrap()
    };
    // Craft the malicious payload: a single Noop with the maximal counter. On
    // the wire this is ~6 bytes, but its FieldRepr materialises `n` field
    // elements (~137 GB at u32::MAX). We inject it by overwriting the program
    // of the call's *already-valid* fallible transcript -- mirroring an
    // attacker-authored transcript arriving over the wire, and sidestepping the
    // construction-side gas accounting that `partition_transcripts` would
    // re-run (which could reject the payload before it reaches well_formed).
    let poison_program: Vec<Op<ResultModeVerify, InMemoryDB>> = vec![Op::Noop { n: u32::MAX }];

    match &mut tx {
        Transaction::Standard(stx) => {
            // `test_intents` keys the single intent at segment 1.
            let mut intent = (*stx.intents.get(&1u16).expect("intent at segment 1")).clone();
            let mut actions: Vec<_> = (&intent.actions).into();
            let mut poisoned = false;
            for action in actions.iter_mut() {
                if let ContractAction::Call(call_sp) = action {
                    let mut call = (**call_sp).clone();
                    let mut transcript = call
                        .fallible_transcript
                        .as_deref()
                        .expect("count call has a fallible transcript")
                        .clone();
                    transcript.program = poison_program.clone().into();
                    call.fallible_transcript = Some(Sp::new(transcript));
                    *action = ContractAction::Call(Sp::new(call));
                    poisoned = true;
                }
            }
            assert!(poisoned, "expected a Call action to poison");
            intent.actions = actions.into();
            // Write the mutated intent BACK into the transaction. The previous
            // version of this test assigned the payload to a dropped clone, so
            // it never reached well_formed and the test was vacuous.
            stx.intents = stx.intents.insert(1u16, intent);
        }
        _ => unreachable!(),
    }

    // (1) Isolation: computing the transaction cost exercises exactly the path
    // the fix changes (`validation_cost` -> public-input length), with no
    // balancing or proof verification in the way. Post-fix this sums
    // field_size() and returns fast; pre-fix it materialises the ~137 GB
    // field-repr here (caught by the memory cap -- see the module header).
    let t0 = Instant::now();
    let _ = tx.cost(&INITIAL_PARAMETERS, false);
    assert!(
        t0.elapsed() < Duration::from_secs(2),
        "Transaction::cost materialised the Noop field-repr instead of summing field_size()"
    );

    // (2) End-to-end: with balancing enforced, the over-budget cost is surfaced
    // as a well_formed rejection (rather than silently swallowed).
    let mut dos_strictness = WellFormedStrictness::default();
    dos_strictness.enforce_balancing = true;
    let t0 = Instant::now();
    let result = tx.well_formed(&state.ledger, dos_strictness, state.time);
    assert!(
        result.is_err(),
        "well_formed must reject a transcript whose Noop counter blows up the \
         public-input cost, but returned Ok"
    );
    assert!(
        t0.elapsed() < Duration::from_secs(2),
        "well_formed materialised the Noop field-repr instead of summing field_size()"
    );
}
