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

//! `inner_proof` consumes one entry of `ProofPreimage::inner_proofs` per
//! instruction, whatever its guard, so the vector's length is fixed by the
//! circuit and not by the path taken. Its pairing with `verify_proof` is
//! checked before either pass runs.
//!
//! Every guard here is a constant `0x00`, which is what keeps these tests fast:
//! a guarded-off `verify_proof` returns the trivial accumulator without reading
//! its verifying key, so the side-table entry can be a stub. Guarded-on
//! verification needs a real key and a real inner proof, and is covered by
//! `verify_proof_e2e`.

use std::borrow::Cow;

use sha2::Digest;

use midnight_zkir_v3::IrSource;
use transient_crypto::curve::Fr;
use transient_crypto::proofs::{InnerProofWitness, KeyLocation, ProofPreimage, Zkir};

/// Stands in for a verifying key. Never parsed, because nothing here is
/// verified; `verify_proof_vks` is only indexed by digest.
const STUB_VK: &[u8] = &[0u8, 1, 2, 3];

fn preimage(inner_proofs: Vec<InnerProofWitness>) -> ProofPreimage {
    ProofPreimage {
        binding_input: Fr::from(7u64),
        communications_commitment: None,
        inputs: vec![],
        private_transcript: vec![],
        public_transcript_inputs: vec![],
        public_transcript_outputs: vec![],
        inner_proofs,
        key_location: KeyLocation(Cow::Borrowed("builtin")),
    }
}

fn blank() -> InnerProofWitness {
    InnerProofWitness::Direct(vec![])
}

/// The rejection message, so each test can say *why* it expected a rejection
/// rather than accept any error at all.
fn rejection(instructions: Vec<String>, witnesses: usize) -> String {
    load(instructions)
        .check(&preimage(vec![blank(); witnesses]))
        .expect_err("must be rejected")
        .to_string()
}

fn inner(name: &str, guard: &str) -> String {
    format!(r#"{{ "op": "inner_proof", "guard": "{guard}", "output": "{name}" }}"#)
}

fn verify(name: &str, guard: &str) -> String {
    format!(
        r#"{{ "op": "verify_proof", "guard": "{guard}", "vk_hash": "0x{vk}",
              "instance": [], "proof": "{name}" }}"#,
        vk = const_hex::encode(sha2::Sha256::digest(STUB_VK)),
    )
}

/// A well-formed, guarded-off `inner_proof` / `verify_proof` pair.
fn pair(name: &str) -> Vec<String> {
    vec![inner(name, "0x00"), verify(name, "0x00")]
}

fn load(instructions: Vec<String>) -> IrSource {
    let ir_json = format!(
        r#"{{
           "version": {{ "major": 3, "minor": 1 }},
           "inputs": [],
           "outputs": [],
           "do_communications_commitment": false,
           "instructions": [{}]
        }}"#,
        instructions.join(",\n")
    );
    let mut ir = IrSource::load(ir_json.as_bytes()).expect("IR must parse");
    ir.verify_proof_vks = vec![STUB_VK.to_vec()];
    ir
}

#[test]
fn one_proof_witness_per_instruction_whatever_the_guard() {
    let ir = load([pair("%p_0"), pair("%p_1")].concat());

    // Neither proof is verified, yet both slots must still be supplied: the
    // count follows the instruction list, not the guards.
    ir.check(&preimage(vec![blank(), blank()]))
        .expect("two instructions, two witnesses");

    assert!(ir.check(&preimage(vec![blank()])).is_err(), "too few");
    assert!(
        ir.check(&preimage(vec![blank(), blank(), blank()]))
            .is_err(),
        "too many"
    );
}

#[test]
fn a_verify_proof_must_name_a_proof_bound_before_it() {
    let msg = rejection(vec![verify("%p_9", "0x00")], 0);
    assert!(msg.contains("no preceding `inner_proof` binds"), "{msg}");

    // Binding it later does not help: both passes resolve in instruction order.
    let msg = rejection(vec![verify("%p_0", "0x00"), inner("%p_0", "0x00")], 1);
    assert!(msg.contains("no preceding `inner_proof` binds"), "{msg}");
}

#[test]
fn the_two_guards_must_agree() {
    let msg = rejection(vec![inner("%p_0", "0x01"), verify("%p_0", "0x00")], 1);
    assert!(msg.contains("guarded differently"), "{msg}");
}

#[test]
fn a_bound_proof_is_verified_exactly_once() {
    let twice = vec![
        inner("%p_0", "0x00"),
        verify("%p_0", "0x00"),
        verify("%p_0", "0x00"),
    ];
    let msg = rejection(twice, 1);
    assert!(msg.contains("verified more than once"), "{msg}");

    let msg = rejection(vec![inner("%p_0", "0x00")], 1);
    assert!(msg.contains("no `verify_proof` uses"), "{msg}");
}
