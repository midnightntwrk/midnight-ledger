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

//! Regression tests for declared transcript gas at and beyond the `2^63` picosecond
//! boundary, which used to wrap negative during fee scaling and be priced as zero.

use base_crypto::cost_model::{CostDuration, RunningCost};
use base_crypto::time::Timestamp;
use midnight_ledger::construct::ContractCallPrototype;
use midnight_ledger::error::{FeeCalculationError, MalformedTransaction};
use midnight_ledger::structure::{INITIAL_PARAMETERS, ProofPreimageMarker, Signature, Transaction};
use midnight_ledger::test_utilities::{TestState, test_intents};
use midnight_ledger::verify::WellFormedStrictness;
use midnight_ledger_v9 as midnight_ledger;
use onchain_runtime::context::Effects;
use onchain_runtime::ops::Op;
use onchain_runtime::state::ContractOperation;
use onchain_runtime::transcript::Transcript;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::borrow::Cow;
use storage::db::InMemoryDB;
use transient_crypto::commitment::PedersenRandomness;
use transient_crypto::proofs::KeyLocation;

type Tx = Transaction<Signature, ProofPreimageMarker, PedersenRandomness, InMemoryDB>;

const SIGN_BOUNDARY: u64 = 1 << 63;

#[derive(Clone, Copy, Debug)]
enum Segment {
    Guaranteed,
    Fallible,
}

fn transcript(compute_ps: u64, read_ps: u64) -> Transcript<InMemoryDB> {
    Transcript {
        gas: RunningCost {
            compute_time: CostDuration::from_picoseconds(compute_ps),
            read_time: CostDuration::from_picoseconds(read_ps),
            bytes_written: 0,
            bytes_deleted: 0,
        },
        effects: Effects::default(),
        program: vec![Op::Ckpt].into(),
        version: None,
    }
}

fn call(
    rng: &mut StdRng,
    segment: Segment,
    compute_ps: u64,
    read_ps: u64,
) -> ContractCallPrototype<InMemoryDB> {
    let t = transcript(compute_ps, read_ps);
    let (guaranteed, fallible) = match segment {
        Segment::Guaranteed => (Some(t), None),
        Segment::Fallible => (None, Some(t)),
    };
    ContractCallPrototype {
        address: rng.r#gen(),
        entry_point: b"run"[..].into(),
        op: ContractOperation::new(None, None),
        guaranteed_public_transcript: guaranteed,
        fallible_public_transcript: fallible,
        private_transcript_outputs: vec![],
        input: ().into(),
        output: ().into(),
        communication_commitment_rand: rng.r#gen(),
        key_location: KeyLocation(Cow::Borrowed("run")),
    }
}

fn tx(rng: &mut StdRng, calls: Vec<ContractCallPrototype<InMemoryDB>>) -> Tx {
    Transaction::from_intents(
        "local-test",
        test_intents(rng, calls, vec![], vec![], Timestamp::from_secs(0)),
    )
}

fn single(rng: &mut StdRng, segment: Segment, compute_ps: u64, read_ps: u64) -> Tx {
    let call = call(rng, segment, compute_ps, read_ps);
    tx(rng, vec![call])
}

#[test]
fn declared_compute_at_sign_boundary_is_not_priced_as_zero() {
    let mut rng = StdRng::seed_from_u64(0x42);
    let params = &INITIAL_PARAMETERS;
    // `2^63 - 1` crosses the boundary once per-call overhead is added.
    for declared in [
        SIGN_BOUNDARY - 1,
        SIGN_BOUNDARY,
        SIGN_BOUNDARY + 1,
        u64::MAX,
    ] {
        for segment in [Segment::Guaranteed, Segment::Fallible] {
            let tx = single(&mut rng, segment, declared, 0);
            let cost = tx.cost(params, false).unwrap();
            assert!(
                // Fixed point scaling saturates just below `2^63` ps, which is still far
                // beyond any block limit.
                cost.compute_time
                    >= CostDuration::from_picoseconds(declared.min(SIGN_BOUNDARY - 1)),
                "{segment:?} declaring {declared} ps was priced at {:?}",
                cost.compute_time
            );
            assert!(
                matches!(
                    tx.fees(params, false),
                    Err(FeeCalculationError::BlockLimitExceeded)
                ),
                "{segment:?} declaring {declared} ps passed the block limit check"
            );
            if let Segment::Guaranteed = segment {
                assert!(
                    tx.time_to_dismiss(params).unwrap()
                        >= CostDuration::from_picoseconds(declared.min(SIGN_BOUNDARY - 1)),
                    "guaranteed compute dropped from time to dismiss for {declared} ps"
                );
                assert!(matches!(
                    tx.cost(params, true),
                    Err(FeeCalculationError::OutsideTimeToDismiss { .. })
                ));
            }
        }
    }
}

#[test]
fn summed_declared_compute_across_calls_is_not_priced_as_zero() {
    let mut rng = StdRng::seed_from_u64(0x43);
    let params = &INITIAL_PARAMETERS;
    // Each call stays below the boundary; only their sum crosses it.
    let calls = vec![
        call(&mut rng, Segment::Fallible, SIGN_BOUNDARY / 2, 0),
        call(&mut rng, Segment::Fallible, SIGN_BOUNDARY / 2, 0),
    ];
    let tx = tx(&mut rng, calls);
    assert!(
        tx.cost(params, false).unwrap().compute_time
            >= CostDuration::from_picoseconds(SIGN_BOUNDARY)
    );
    assert!(matches!(
        tx.fees(params, false),
        Err(FeeCalculationError::BlockLimitExceeded)
    ));
}

#[test]
fn in_budget_declared_compute_is_charged() {
    let mut rng = StdRng::seed_from_u64(0x44);
    let params = &INITIAL_PARAMETERS;
    let declared = 1_000_000_000; // 1ms
    for segment in [Segment::Guaranteed, Segment::Fallible] {
        let base = single(&mut rng, segment, 0, 0);
        let with_gas = single(&mut rng, segment, declared, 0);
        let base_cost = base.cost(params, false).unwrap();
        let gas_cost = with_gas.cost(params, false).unwrap();
        assert!(
            gas_cost.compute_time
                >= base_cost.compute_time + CostDuration::from_picoseconds(declared),
            "{segment:?}: {:?} vs {:?}",
            gas_cost.compute_time,
            base_cost.compute_time
        );
        assert!(with_gas.fees(params, false).unwrap() > base.fees(params, false).unwrap());
    }
}

fn gas_limit_violation(tx: &Tx, state: &TestState<InMemoryDB>) -> bool {
    let mut strictness = WellFormedStrictness::default();
    strictness.enforce_balancing = false;
    strictness.verify_contract_proofs = false;
    strictness.verify_native_proofs = false;
    strictness.verify_signatures = false;
    matches!(
        tx.well_formed(&state.ledger, strictness, state.time),
        Err(MalformedTransaction::TranscriptGasExceedsBlockLimits { .. })
    )
}

#[test]
fn well_formed_bounds_declared_gas_by_block_limits() {
    let mut rng = StdRng::seed_from_u64(0x45);
    let state: TestState<InMemoryDB> = TestState::new(&mut rng);
    let limits = state.ledger.parameters.limits.block_limits;
    let compute_limit = limits.compute_time.into_picoseconds();
    let read_limit = limits.read_time.into_picoseconds();
    for segment in [Segment::Guaranteed, Segment::Fallible] {
        for (compute, read, rejected) in [
            (compute_limit, read_limit, false),
            (compute_limit + 1, 0, true),
            (0, read_limit + 1, true),
            (SIGN_BOUNDARY, 0, true),
            (u64::MAX, u64::MAX, true),
        ] {
            let tx = single(&mut rng, segment, compute, read);
            assert_eq!(
                gas_limit_violation(&tx, &state),
                rejected,
                "{segment:?} declaring compute {compute} ps / read {read} ps"
            );
        }
    }
}
