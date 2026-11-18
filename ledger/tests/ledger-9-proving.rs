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

//! A call to a contract deployed with ledger 9 keys only: its operation holds
//! a `verifier-key[v7]` key in `v3` and nothing in `v4`. The call is proven
//! through the test prover and applied with real proof verification.
//!
//! The circuit is keyed at run time by the IR v3 crate ledger 9 was released
//! with. It is IR 3.1, which only that crate reads.
#![cfg(feature = "proving")]

use base_crypto::data_provider::{FetchMode, MidnightDataProvider, OutputMode};
use base_crypto::rng::SplittableRng;
use base_crypto::time::Timestamp;
use midnight_ledger::construct::{ContractCallPrototype, PreTranscript, partition_transcripts};
use midnight_ledger::dust::{DUST_EXPECTED_FILES, DustResolver};
use midnight_ledger::structure::{ContractDeploy, INITIAL_PARAMETERS, Transaction};
use midnight_ledger::test_utilities::{PUBLIC_PARAMS, Resolver, TestState, test_intents, tx_prove};
use midnight_ledger::verify::WellFormedStrictness;
use midnight_ledger_v10 as midnight_ledger;
use onchain_runtime::context::QueryContext;
use onchain_runtime::ops::Op;
use onchain_runtime::state::{ContractOperation, ContractState, StateValue};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serialize::{Serializable, Tagged, tagged_serialize};
use std::borrow::Cow;
use storage::db::InMemoryDB;
use storage::storage::HashMap;
use transient_crypto::proofs::{KeyLocation, ProvingKeyMaterial};
use transient_crypto_3::proofs::Zkir;

const KEY: &str = "ledger-9-operation";

/// A circuit for a call whose transcript is one `Noop { n: 1 }`: its public
/// inputs are the binding input, the communications commitment over no inputs
/// or outputs, and the one zero the `Noop` field-reprs to.
const IR: &str = r#"{
   "version": { "major": 3, "minor": 1 },
   "inputs": [],
   "outputs": [],
   "do_communications_commitment": true,
   "instructions": [
       { "op": "impact", "guard": "0x01", "inputs": ["0x00"] }
   ]
}"#;

fn tagged<T: Serializable + Tagged>(x: &T) -> Vec<u8> {
    let mut buf = Vec::new();
    tagged_serialize(x, &mut buf).expect("in-memory serialization");
    buf
}

/// Serves the operation's keys, and the built-in zswap and dust keys the rest
/// of the transaction needs.
fn resolver(material: ProvingKeyMaterial) -> Resolver {
    Resolver::new(
        PUBLIC_PARAMS.clone(),
        DustResolver(
            MidnightDataProvider::new(
                FetchMode::OnDemand,
                OutputMode::Log,
                DUST_EXPECTED_FILES.to_owned(),
            )
            .expect("dust data provider"),
        ),
        Box::new(move |KeyLocation(loc)| {
            let found = (loc == KEY).then(|| material.clone());
            Box::pin(std::future::ready(Ok(found)))
        }),
    )
}

/// Deploys a contract whose one operation holds only a ledger 9 key, proves
/// a call to it, and applies the call with real proof verification.
#[tokio::test]
async fn call_to_a_contract_with_only_a_ledger_9_key() {
    let ir = zkir_v3::IrSource::load(IR.as_bytes()).expect("IR must parse");
    let (pk, vk) = ir.keygen(&PUBLIC_PARAMS.0).await.expect("keygen");
    let material = ProvingKeyMaterial {
        prover_key: tagged(&pk),
        verifier_key: tagged(&vk),
        ir_source: tagged(&ir),
    };
    let mut rng = StdRng::seed_from_u64(0x42);
    let resolver = resolver(material);

    let mut state: TestState<InMemoryDB> = TestState::new(&mut rng);
    let mut strictness = WellFormedStrictness::default();
    strictness.enforce_balancing = false;
    let mut op = ContractOperation::new(None, None);
    op.v3 = Some(vk);
    let contract = ContractState::new(
        StateValue::Null,
        HashMap::new().insert(b"call"[..].into(), op.clone()),
        Default::default(),
    );
    let deploy = ContractDeploy::new(&mut rng, contract);
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
        &resolver,
    )
    .await
    .expect("deploy proving");
    state.assert_apply(&tx, strictness);

    let transcripts = partition_transcripts(
        &[PreTranscript {
            context: QueryContext::new(state.ledger.index(addr).unwrap().data, addr),
            program: vec![Op::Noop { n: 1 }],
            comm_comm: None,
        }],
        &INITIAL_PARAMETERS,
    )
    .expect("a one-noop transcript");
    let call = ContractCallPrototype {
        address: addr,
        entry_point: b"call"[..].into(),
        op,
        guaranteed_public_transcript: transcripts[0].0.clone(),
        fallible_public_transcript: transcripts[0].1.clone(),
        private_transcript_outputs: Vec::new(),
        input: ().into(),
        output: ().into(),
        communication_commitment_rand: rng.r#gen(),
        key_location: KeyLocation(Cow::Borrowed(KEY)),
        inner_proofs: Vec::new(),
    };
    let tx = tx_prove(
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
        &resolver,
    )
    .await
    .expect("call proving");
    state.assert_apply(&tx, strictness);
}
