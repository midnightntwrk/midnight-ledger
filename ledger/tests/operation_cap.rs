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

//! Contracts hold at most `MAX_CONTRACT_OPERATIONS` operations: deploys over
//! the cap are rejected, and maintenance may not grow a contract past it.

#![deny(warnings)]

use coin_structure::contract::ContractAddress;
use midnight_ledger::test_utilities::{TestState, test_intents};
use midnight_ledger::{
    error::{MalformedContractDeploy, MalformedTransaction, TransactionInvalid},
    semantics::TransactionResult,
    structure::{
        ContractDeploy, MAX_CONTRACT_OPERATIONS, MaintenanceUpdate, ProofPreimageMarker, Signature,
        SingleUpdate, Transaction,
    },
    verify::WellFormedStrictness,
};
use midnight_ledger_v9 as midnight_ledger;
use onchain_runtime::state::{
    ContractMaintenanceAuthority, ContractOperation, ContractState, EntryPointBuf, IrBuf,
    StateValue,
};
use proptest::prelude::*;
use rand::{SeedableRng, rngs::StdRng};
use storage::arena::Sp;
use storage::db::InMemoryDB;
use storage::storage::HashMap;
use transient_crypto::commitment::PedersenRandomness;

type D = InMemoryDB;
type Tx = Transaction<Signature, ProofPreimageMarker, PedersenRandomness, D>;

fn entry_point(i: u32) -> EntryPointBuf {
    EntryPointBuf(i.to_le_bytes().to_vec())
}

fn ir() -> IrBuf {
    b"ir"[..].to_owned().into()
}

// IR-only operations, as maintenance inserts them, under an authority that
// needs no signatures.
fn contract(operations: u32) -> ContractState<D> {
    let ops = (0..operations).fold(HashMap::new(), |m, i| {
        let mut op = ContractOperation::new(None, None);
        op.ir = Some(Sp::new(ir()));
        m.insert(entry_point(i), op)
    });
    ContractState::new(
        StateValue::Null,
        ops,
        ContractMaintenanceAuthority {
            committee: vec![],
            threshold: 0,
            counter: 0,
        },
    )
}

fn strictness() -> WellFormedStrictness {
    let mut strictness = WellFormedStrictness::default();
    strictness.enforce_balancing = false;
    strictness
}

// Counts on both sides of the cap, mostly within a few of it.
fn operations() -> impl Strategy<Value = u32> {
    let cap = MAX_CONTRACT_OPERATIONS as u32;
    prop_oneof![1 => 0..4u32, 3 => cap - 3..cap + 4]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn deploys_over_the_cap_are_rejected(operations in operations(), seed in any::<u64>()) {
        let mut rng = StdRng::seed_from_u64(seed);
        let state: TestState<D> = TestState::new(&mut rng);
        let deploy = ContractDeploy::new(&mut rng, contract(operations));
        let tx: Tx = Transaction::new(
            "local-test",
            test_intents(&mut rng, Vec::new(), Vec::new(), vec![deploy], state.time),
            None,
            HashMap::new(),
        );
        let rejected = matches!(
            tx.well_formed(&state.ledger, strictness(), state.time),
            Err(MalformedTransaction::MalformedContractDeploy(
                MalformedContractDeploy::TooManyOperations { .. }
            ))
        );
        prop_assert_eq!(rejected, operations as u64 > MAX_CONTRACT_OPERATIONS);
    }

    #[test]
    fn maintenance_may_not_grow_a_contract_past_the_cap(
        before in operations(),
        inserted in 0..4u32,
        removed in 0..4u32,
        seed in any::<u64>(),
    ) {
        let mut rng = StdRng::seed_from_u64(seed);
        let mut state: TestState<D> = TestState::new(&mut rng);
        let removed = removed.min(before);
        let address = ContractAddress(rng.r#gen());
        // Placed directly: a contract over the cap can only predate it.
        state.ledger.contract = state.ledger.contract.insert(address, contract(before));
        let updates = (0..removed)
            .map(|i| SingleUpdate::IrRemove(entry_point(i)))
            .chain((0..inserted).map(|i| SingleUpdate::IrInsert(entry_point(before + i), ir())))
            .collect::<Vec<_>>();
        let update = MaintenanceUpdate::new(address, updates, 0);
        let tx: Tx = Transaction::new(
            "local-test",
            test_intents(&mut rng, Vec::new(), vec![update], Vec::new(), state.time),
            None,
            HashMap::new(),
        );
        let after = (before - removed + inserted) as u64;
        let rejected = match state.apply(&tx, strictness()) {
            Ok(TransactionResult::Success(_)) => false,
            Ok(TransactionResult::PartialSuccess(results, _)) => results.values().any(|r| {
                matches!(r, Err(TransactionInvalid::ContractTooManyOperations { .. }))
            }),
            other => return Err(TestCaseError::fail(format!("unexpected result: {other:?}"))),
        };
        prop_assert_eq!(rejected, after > MAX_CONTRACT_OPERATIONS && after > before as u64);
    }
}
