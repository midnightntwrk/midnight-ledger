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

//! A real verifier key of every supported version, with a proof made against
//! it. `verifier-keys/README.md` says how the bytes were made and that the
//! bytes of a version never change.

#![cfg(feature = "proof-verifying")]

use crate::error::MalformedTransaction;
use crate::structure::{
    ContractCall, ContractOperationVersionedVerifierKey as VersionedKey, ProofKind, ProofMarker,
    ProofVersioned,
};
use crate::verify::ProofVerificationMode;
use base_crypto::hash::HashOutput;
use coin_structure::contract::ContractAddress;
use onchain_runtime::state::ContractOperation;
use serialize::tagged_deserialize;
use storage::db::InMemoryDB;
use transient_crypto::curve::Fr;

struct Fixture {
    key: &'static [u8],
    proof: &'static [u8],
    public_inputs: &'static [u8],
}

macro_rules! fixture {
    ($version:literal) => {
        Fixture {
            key: include_bytes!(concat!("verifier-keys/", $version, "/key.bin")),
            proof: include_bytes!(concat!("verifier-keys/", $version, "/proof.bin")),
            public_inputs: include_bytes!(concat!(
                "verifier-keys/",
                $version,
                "/public-inputs.bin"
            )),
        }
    };
}

const FIXTURES: [Fixture; 3] = [fixture!("v6"), fixture!("v7"), fixture!("v8")];

/// An operation holding the fixture's key and nothing else, and the fixture's
/// proof.
fn operation_and_proof(fixture: &Fixture) -> (ContractOperation, ProofVersioned) {
    let mut op = ContractOperation::new(None, None);
    let mut proof = fixture.proof;
    // No wildcard arm: a new key version has to be handled here, and needs a
    // fixture of its own.
    let proof = match VersionedKey::from_tagged_key(fixture.key).unwrap() {
        VersionedKey::V3(key) => {
            op.v2 = Some(key);
            ProofVersioned::V2(tagged_deserialize(&mut proof).unwrap())
        }
        VersionedKey::V4(key) => {
            op.v3 = Some(key);
            ProofVersioned::V3(tagged_deserialize(&mut proof).unwrap())
        }
        VersionedKey::V5(key) => {
            op.v4 = Some(key);
            ProofVersioned::V4(tagged_deserialize(&mut proof).unwrap())
        }
    };
    (op, proof)
}

fn public_inputs(fixture: &Fixture) -> Vec<Fr> {
    tagged_deserialize(&mut &fixture.public_inputs[..]).unwrap()
}

fn verify(
    op: &ContractOperation,
    proof: &ProofVersioned,
    public_inputs: Vec<Fr>,
) -> Result<(), MalformedTransaction<InMemoryDB>> {
    // Verification reads the call for its error messages only.
    let call = ContractCall::<ProofMarker, InMemoryDB> {
        address: ContractAddress(HashOutput([0; 32])),
        entry_point: b"store".to_vec().into(),
        guaranteed_transcript: None,
        fallible_transcript: None,
        communication_commitment: Fr::default(),
        proof: proof.clone(),
    };
    <ProofMarker as ProofKind<InMemoryDB>>::proof_verify(
        op,
        proof,
        public_inputs,
        &call,
        ProofVerificationMode::Real,
    )
}

#[test]
fn proof_of_every_key_version_verifies() {
    for fixture in &FIXTURES {
        let (op, proof) = operation_and_proof(fixture);
        verify(&op, &proof, public_inputs(fixture)).unwrap();

        let mut other_inputs = public_inputs(fixture);
        other_inputs[0] = other_inputs[0] + Fr::from(1u64);
        assert!(matches!(
            verify(&op, &proof, other_inputs),
            Err(MalformedTransaction::InvalidProof(_))
        ));
    }
}

#[test]
fn proof_needs_the_key_of_its_own_version() {
    for (i, proof_fixture) in FIXTURES.iter().enumerate() {
        let (_, proof) = operation_and_proof(proof_fixture);
        for (j, key_fixture) in FIXTURES.iter().enumerate() {
            if i == j {
                continue;
            }
            let (op, _) = operation_and_proof(key_fixture);
            assert!(matches!(
                verify(&op, &proof, public_inputs(proof_fixture)),
                Err(MalformedTransaction::VerifierKeyNotPresent { .. })
            ));
        }
    }
}
