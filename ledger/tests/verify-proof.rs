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

//! A contract call whose circuit verifies another proof in-circuit.
//!
//! zkir-v3's own end-to-end tests take a `verify_proof` circuit as far as its
//! outer proof verifying. This one carries it the rest of the way: through a
//! deployed contract operation, a proven transaction, and the ledger's
//! well-formedness check, which is where the accumulator the instruction
//! defers meets the pairing check that discharges it.
//!
//! Compact cannot emit `verify_proof` yet, so the contract's circuit is
//! hand-rolled ZKIR, keyed at run time rather than loaded from the test
//! artifacts. The proof it verifies comes from
//! [`zkir_v3::testing`], which holds the only prover here that writes the
//! transcript the in-circuit verifier reads.

use base_crypto::data_provider::{FetchMode, MidnightDataProvider, OutputMode};
use base_crypto::fab::AlignedValue;
use base_crypto::rng::SplittableRng;
use base_crypto::time::Timestamp;
use midnight_ledger::construct::{ContractCallPrototype, PreTranscript, partition_transcripts};
use midnight_ledger::dust::{DUST_EXPECTED_FILES, DustResolver};
use midnight_ledger::structure::{ContractDeploy, INITIAL_PARAMETERS, ProofVersioned, Transaction};
use midnight_ledger::test_utilities::{PUBLIC_PARAMS, Resolver, TestState, test_intents, tx_prove};
use midnight_ledger::verify::WellFormedStrictness;
use midnight_ledger_v10 as midnight_ledger;
use onchain_runtime::context::QueryContext;
use onchain_runtime::ops::Op;
use onchain_runtime::state::{ContractOperation, ContractState, StateValue};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serialize::{Serializable, Tagged};
use sha2::Digest;
use std::borrow::Cow;
use std::collections::HashMap as StdHashMap;
use storage::db::InMemoryDB;
use storage::storage::HashMap;
use transient_crypto::curve::Fr;
use transient_crypto::proofs::{
    InnerProofWitness, KeyLocation, ProvingKeyMaterial, VerifierKey, Zkir,
};
use zkir_v3::IrSource;

const OUTER_KEY: &str = "verify-proof-outer";

/// The inner proof's statement, and so the instance the contract's circuit
/// binds it to.
const INNER_INSTANCE: u64 = 42;

/// The contract's circuit: witness the inner proof's public inputs, bind the
/// proof, verify it.
///
/// Its own public inputs are what [`ContractCall::public_inputs`] builds: the
/// binding input, the communications commitment, then the transcript's fields.
/// ZKIR emits the first two from the preimage; the third is the one zero a
/// `Noop { n: 1 }` field-reprs to, declared by the `impact` instruction.
fn outer_ir(vk_blob: Vec<u8>, instance_len: usize) -> IrSource {
    let names: Vec<String> = (0..instance_len).map(|i| format!("\"%i_{i}\"")).collect();
    let witnesses: Vec<String> = names
        .iter()
        .map(|name| {
            format!(
                r#"{{ "op": "private_input", "guard": "0x01",
                      "type": "Scalar<BLS12-381>", "output": {name} }}"#
            )
        })
        .collect();
    let ir_json = format!(
        r#"{{
           "version": {{ "major": 3, "minor": 1 }},
           "inputs": [],
           "outputs": [],
           "do_communications_commitment": true,
           "instructions": [
               {witnesses},
               {{ "op": "inner_proof", "guard": "0x01", "output": "%p" }},
               {{ "op": "verify_proof", "guard": "0x01",
                  "vk_hash": "0x{vk_hash}",
                  "instance": [{names}], "proof": "%p" }},
               {{ "op": "impact", "guard": "0x01", "inputs": ["0x00"] }}
           ]
        }}"#,
        witnesses = witnesses.join(",\n               "),
        names = names.join(", "),
        vk_hash = hex::encode(sha2::Sha256::digest(&vk_blob)),
    );
    let mut ir = IrSource::load(ir_json.as_bytes()).expect("outer IR must parse");
    // `minor: 1` above: the side-table only exists in that wire shape.
    ir.verify_proof_vks = vec![vk_blob];
    assert_eq!(ir.accumulator_count(), 1);
    ir
}

/// Keys a circuit and packages what a [`Resolver`] serves for it.
async fn key_material(ir: &IrSource) -> (ProvingKeyMaterial, VerifierKey) {
    fn tagged<T: Serializable + Tagged>(x: &T) -> Vec<u8> {
        let mut buf = Vec::new();
        serialize::tagged_serialize(x, &mut buf).expect("in-memory serialization");
        buf
    }
    let (pk, vk) = ir.keygen(&*PUBLIC_PARAMS).await.expect("keygen");
    let material = ProvingKeyMaterial {
        prover_key: tagged(&pk),
        verifier_key: tagged(&vk),
        ir_source: tagged(ir),
    };
    (material, vk)
}

/// Serves the run-time circuit, and the built-in zswap and dust keys the rest
/// of the transaction needs.
fn resolver(materials: StdHashMap<String, ProvingKeyMaterial>) -> Resolver {
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
            let found = materials.get(loc.as_ref()).cloned();
            Box::pin(std::future::ready(Ok(found)))
        }),
    )
}

#[tokio::test]
async fn contract_call_verifying_an_inner_proof() {
    let mut rng = StdRng::seed_from_u64(0x42);

    // The proof the contract will verify. It cannot come from ZKIR: a ZKIR
    // proof is written to a Blake2b transcript, and the in-circuit verifier
    // reads a Poseidon one.
    let inner = zkir_v3::testing::echo_proof(&*PUBLIC_PARAMS, Fr::from(INNER_INSTANCE), &mut rng)
        .await
        .expect("inner prove");

    // The contract's circuit, keyed against the same parameters.
    let outer = outer_ir(inner.vk_blob.clone(), inner.instance.len());
    // The params package ships up to 2^18, which is what a circuit holding one
    // `verify_proof` needs; a bigger one would have to be fetched.
    assert!(outer.k() <= 18, "outer circuit k = {}", outer.k());
    let (outer_material, outer_vk) = key_material(&outer).await;

    let resolver = resolver(StdHashMap::from([(OUTER_KEY.to_owned(), outer_material)]));

    // Deploy it.
    let mut state: TestState<InMemoryDB> = TestState::new(&mut rng);
    let mut strictness = WellFormedStrictness::default();
    strictness.enforce_balancing = false;
    let op = ContractOperation::new(Some(outer_vk), None);
    let contract = ContractState::new(
        StateValue::Null,
        HashMap::new().insert(b"verify"[..].into(), op.clone()),
        Default::default(),
    );
    let (tx, addr) = {
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
        (tx, addr)
    };
    state.assert_apply(&tx, strictness);
    assert!(state.ledger.index(addr).is_some());

    // Call it. The inner proof is the witness; its statement is the private
    // transcript the circuit binds as the instance to verify against.
    let tx = {
        // A call must carry a transcript. One `Noop` is the least a contract
        // can say, and field-reprs to the single zero the circuit declares.
        let transcripts = partition_transcripts(
            &[PreTranscript {
                context: QueryContext::new(state.ledger.index(addr).unwrap().data, addr),
                program: vec![Op::Noop { n: 1 }].into(),
                comm_comm: None,
            }],
            &INITIAL_PARAMETERS,
        )
        .expect("a one-noop transcript");
        let call = ContractCallPrototype {
            address: addr,
            entry_point: b"verify"[..].into(),
            op: op.clone(),
            guaranteed_public_transcript: transcripts[0].0.clone(),
            fallible_public_transcript: transcripts[0].1.clone(),
            private_transcript_outputs: vec![AlignedValue::from(INNER_INSTANCE)],
            input: ().into(),
            output: ().into(),
            communication_commitment_rand: rng.r#gen(),
            key_location: KeyLocation(Cow::Borrowed(OUTER_KEY)),
            inner_proofs: vec![InnerProofWitness::Direct(inner.proof.clone())],
        };
        let pre_tx = Transaction::from_intents(
            "local-test",
            test_intents(
                &mut rng,
                vec![call],
                Vec::new(),
                Vec::new(),
                Timestamp::from_secs(0),
            ),
        );
        tx_prove(rng.split(), &pre_tx, &resolver)
            .await
            .expect("call proving")
    };
    // Nothing here is vacuous only if the proof really carries what
    // `verify_proof` deferred, and well-formedness really discharges it.
    let (_, call) = tx.calls().next().expect("the transaction has one call");
    match &call.proof {
        ProofVersioned::V4(proof) => assert_eq!(
            proof.accumulators.len(),
            1,
            "one accumulator, for the circuit's one `verify_proof`"
        ),
        other => panic!("expected a latest-format proof, got {other:?}"),
    }

    tx.well_formed(&state.ledger, strictness, Timestamp::from_secs(0))
        .expect("a call carrying a verified inner proof is well-formed");
    state.assert_apply(&tx, strictness);
}
