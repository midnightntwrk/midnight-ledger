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

//! A contract operation and a proof written by ledger 9, read and verified by
//! this ledger. `ledger-9/README.md` says how the bytes were made.

#![deny(warnings)]
#![cfg(feature = "proof-verifying")]

use base_crypto::hash::HashOutput;
use coin_structure::contract::ContractAddress;
use midnight_ledger::error::MalformedTransaction;
use midnight_ledger::structure::{ContractCall, ProofKind, ProofMarker, ProofVersioned};
use midnight_ledger::verify::ProofVerificationMode;
use midnight_ledger_v10 as midnight_ledger;
use onchain_runtime::state::ContractOperation;
use serialize::{Deserializable, tagged_deserialize, tagged_serialize};
use storage::db::InMemoryDB;
use transient_crypto::curve::Fr;

/// The operation called, as ledger 9 stored it in the contract's state.
const OPERATION: &[u8] = include_bytes!("ledger-9/store.operation.bin");
const PROOF: &[u8] = include_bytes!("ledger-9/store.proof.bin");
const PUBLIC_INPUTS: &[u8] = include_bytes!("ledger-9/store.public-inputs.bin");

/// Reads the operation the way a ledger 9 state is carried over: the ledger 9
/// fields `v2`, `v3` and `ir` as they are, and an empty `v4`.
fn operation() -> ContractOperation {
    let mut reader = OPERATION
        .strip_prefix(b"midnight:contract-operation[v6]:")
        .expect("operation should carry its ledger 9 tag");
    let v2 = Deserializable::deserialize(&mut reader, 0).unwrap();
    let v3 = Deserializable::deserialize(&mut reader, 0).unwrap();
    let ir = Deserializable::deserialize(&mut reader, 0).unwrap();
    assert!(reader.is_empty());
    let mut op = ContractOperation::new(None, ir);
    op.v2 = v2;
    op.v3 = v3;
    op
}

fn proof() -> ProofVersioned {
    tagged_deserialize(&mut &PROOF[..]).unwrap()
}

fn public_inputs() -> Vec<Fr> {
    tagged_deserialize(&mut &PUBLIC_INPUTS[..]).unwrap()
}

fn verify(
    op: &ContractOperation,
    proof: ProofVersioned,
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
        &proof,
        public_inputs,
        &call,
        ProofVerificationMode::Real,
    )
}

#[test]
fn ledger_9_proof_keeps_its_bytes_and_verifies() {
    let proof = proof();
    let mut written = Vec::new();
    tagged_serialize(&proof, &mut written).unwrap();
    assert_eq!(written, PROOF);
    verify(&operation(), proof, public_inputs()).unwrap();
}

#[test]
fn ledger_9_proof_fails_on_other_public_inputs() {
    let mut public_inputs = public_inputs();
    public_inputs[0] = public_inputs[0] + Fr::from(1u64);
    assert!(matches!(
        verify(&operation(), proof(), public_inputs),
        Err(MalformedTransaction::InvalidProof(_))
    ));
}

/// A proof is checked against the key of its own version only.
#[test]
fn proof_needs_the_key_of_its_own_version() {
    let ProofVersioned::V3(proof) = proof() else {
        panic!("ledger 9 proof should be a V3 proof");
    };
    // Keys are parsed on first use, so an empty one does here.
    let latest_key = Deserializable::deserialize(&mut &b"\x00"[..], 0).unwrap();
    let v2_proof = transient_crypto_2::proofs::Proof(proof.0.clone());
    let v4_proof = transient_crypto::proofs::Proof::from_bytes(proof.0.clone());
    let cases = [
        (operation(), ProofVersioned::V2(v2_proof)),
        (operation(), ProofVersioned::V4(v4_proof)),
        (
            ContractOperation::new(Some(latest_key), None),
            ProofVersioned::V3(proof),
        ),
    ];
    for (op, proof) in cases {
        assert!(matches!(
            verify(&op, proof, public_inputs()),
            Err(MalformedTransaction::VerifierKeyNotPresent { .. })
        ));
    }
}
