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

//! A ratchet over proving: a call can still be proven for a contract holding
//! a verifier key of any supported version, from a circuit for every way a
//! Midnight ledger has proven one.
//!
//! Each call is proven through the test prover and applied with real proof
//! verification. The calls' proofs also pin the contract proof size estimates.
#![cfg(feature = "proving")]

use crate::construct::{ContractCallPrototype, PreTranscript, partition_transcripts};
use crate::dust::{DUST_EXPECTED_FILES, DustResolver};
use crate::structure::{
    ContractDeploy, ContractOperationVersion as Version,
    ContractOperationVersionedVerifierKey as VersionedKey, INITIAL_PARAMETERS, MIN_PROOF_SIZE,
    ProofVersioned, Transaction,
};
use crate::test_utilities::{
    PUBLIC_PARAMS, Resolver, TestState, circuits_of_every_ir_version, contract_operation,
    test_intents, tx_prove,
};
use crate::verify::WellFormedStrictness;
use base_crypto::data_provider::{FetchMode, MidnightDataProvider, OutputMode};
use base_crypto::rng::SplittableRng;
use base_crypto::time::Timestamp;
use onchain_runtime::context::QueryContext;
use onchain_runtime::ops::Op;
use onchain_runtime::state::{ContractState, StateValue};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serialize::Serializable;
use std::borrow::Cow;
use std::collections::BTreeSet;
use storage::db::InMemoryDB;
use storage::storage::HashMap;
use transient_crypto::proofs::{KeyLocation, ProvingKeyMaterial};

/// Serves `circuits` by name, and the built-in zswap and dust keys the rest of
/// a transaction needs.
fn resolver(circuits: Vec<(&'static str, ProvingKeyMaterial)>) -> Resolver {
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
            let found = circuits
                .iter()
                .find(|(name, _)| *name == loc)
                .map(|(_, material)| material.clone());
            Box::pin(std::future::ready(Ok(found)))
        }),
    )
}

#[tokio::test]
async fn call_is_proven_for_every_ir_and_verifier_key_version() {
    let circuits = circuits_of_every_ir_version().await;
    let resolver = resolver(circuits.clone());
    let mut rng = StdRng::seed_from_u64(0x42);
    let mut state: TestState<InMemoryDB> = TestState::new(&mut rng);
    let strictness = WellFormedStrictness {
        enforce_balancing: false,
        ..Default::default()
    };

    let mut key_versions = BTreeSet::new();
    let mut proof_sizes = Vec::new();
    for (name, material) in circuits {
        let key = VersionedKey::from_tagged_key(&material.verifier_key).expect("a verifier key");
        key_versions.insert(key.as_version());
        let op = contract_operation(&resolver, name).await;
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
        .unwrap_or_else(|e| panic!("{name}: deploy proving: {e:?}"));
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
            key_location: KeyLocation(Cow::Borrowed(name)),
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
        .unwrap_or_else(|e| panic!("{name}: call proving: {e:?}"));
        proof_sizes.extend(tx.calls().map(|(_, call)| call.proof.serialized_size()));
        state.assert_apply(&tx, strictness);
    }
    assert_eq!(
        key_versions,
        BTreeSet::from([Version::V3, Version::V4, Version::V5])
    );
    let sized =
        |n| ProofVersioned::V2(transient_crypto_2::proofs::Proof(vec![0; n])).serialized_size();
    assert_eq!(proof_sizes.iter().min(), Some(&sized(MIN_PROOF_SIZE)));
}
