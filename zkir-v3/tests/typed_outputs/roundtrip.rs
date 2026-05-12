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

//! Positive end-to-end tests for the typed `outputs` field and `Output`
//! terminator. Each test exercises one shape of typed return: one per
//! `IrType`, plus one multi-output case.
//!
//! Every IR sets `do_communications_commitment = true` so that the prover
//! actually invokes `encode_offcircuit` (and `encode_incircuit` in-circuit)
//! on each declared output. Without that flag set, the encoding paths sit
//! in dead code (see the `if self.do_communications_commitment` branches in
//! `IrSource::preprocess` / `IrSource::circuit`) and these tests would only
//! check signature conformance, not the full encode/decode round-trip.
//!
//! The recipe is the same in every test (see
//! `common::assert_typed_output_roundtrip`):
//!   load JSON IR -> compute commitment over inputs ++ encoded expected
//!   outputs -> keygen -> prove -> verify against `[binding, commitment]`.

use midnight_curves::Fr as JubjubFr;
use midnight_zkir_v3::ir_types::IrValue;
use transient_crypto::curve::Fr;

use crate::common::assert_typed_output_roundtrip;

#[actix_rt::test]
async fn native_identity() {
    // Output the input directly. Exercises Output{vals: [<input ref>]}
    // without an intervening compute instruction.
    let input: Fr = 123.into();
    assert_typed_output_roundtrip(
        r#"{
           "version": { "major": 3, "minor": 0 },
           "inputs": [
              { "name": "%v_0", "type": "Scalar<BLS12-381>" }
           ],
           "outputs": ["Scalar<BLS12-381>"],
           "do_communications_commitment": true,
           "instructions": [
               { "op": "output", "vals": ["%v_0"] }
           ]
        }"#,
        vec![input],
        vec![IrValue::Native(input)],
    )
    .await;
}

#[actix_rt::test]
async fn native_via_copy() {
    // Output a value that came from a `copy` instruction. Exercises Output
    // resolving an operand that points at instruction-produced memory.
    let input: Fr = 123.into();
    assert_typed_output_roundtrip(
        r#"{
           "version": { "major": 3, "minor": 0 },
           "inputs": [
              { "name": "%v_0", "type": "Scalar<BLS12-381>" }
           ],
           "outputs": ["Scalar<BLS12-381>"],
           "do_communications_commitment": true,
           "instructions": [
               { "op": "copy", "val": "%v_0", "output": "%v_1" },
               { "op": "output", "vals": ["%v_1"] }
           ]
        }"#,
        vec![input],
        vec![IrValue::Native(input)],
    )
    .await;
}

#[actix_rt::test]
async fn jubjub_scalar() {
    // Output a Scalar<Jubjub> obtained by decoding a Native input.
    // Exercises the canonical-scalar path documented in
    // `encode_incircuit::JubjubScalar`.
    assert_typed_output_roundtrip(
        r#"{
           "version": { "major": 3, "minor": 0 },
           "inputs": [
              { "name": "%v_0", "type": "Scalar<BLS12-381>" }
           ],
           "outputs": ["Scalar<Jubjub>"],
           "do_communications_commitment": true,
           "instructions": [
               { "op": "decode", "type": "Scalar<Jubjub>", "inputs": ["%v_0"], "output": "%s" },
               { "op": "output", "vals": ["%s"] }
           ]
        }"#,
        vec![5.into()],
        vec![IrValue::JubjubScalar(JubjubFr::from(5u64))],
    )
    .await;
}

#[actix_rt::test]
async fn output_opaque_passthrough_from_input() {
    let preimage_bytes: Vec<u8> = b"hello".to_vec();
    // Match the canonical input-vector encoding for an Opaque input:
    // `[byte_len, fr_0, ..., fr_{N-1}]`. For 5 bytes, N = 1.
    let byte_len: Fr = (preimage_bytes.len() as u64).into();
    let preimage_fr = Fr::from_le_bytes(&preimage_bytes).expect("fits in Fr");
    assert_typed_output_roundtrip(
        r#"{
           "version": { "major": 3, "minor": 0 },
           "inputs": [
              { "name": "%s", "type": "Opaque" }
           ],
           "outputs": ["Opaque"],
           "do_communications_commitment": true,
           "instructions": [
               { "op": "output", "vals": ["%s"] }
           ]
        }"#,
        vec![byte_len, preimage_fr],
        vec![IrValue::opaque(preimage_bytes)],
    )
    .await;
}

#[actix_rt::test]
async fn multi_output_native_pair() {
    // Two Native outputs from the same input. Exercises the per-position
    // arity & type loop in the Output arm.
    let input: Fr = 9.into();
    assert_typed_output_roundtrip(
        r#"{
           "version": { "major": 3, "minor": 0 },
           "inputs": [
              { "name": "%v_0", "type": "Scalar<BLS12-381>" }
           ],
           "outputs": ["Scalar<BLS12-381>", "Scalar<BLS12-381>"],
           "do_communications_commitment": true,
           "instructions": [
               { "op": "copy", "val": "%v_0", "output": "%v_1" },
               { "op": "copy", "val": "%v_0", "output": "%v_2" },
               { "op": "output", "vals": ["%v_1", "%v_2"] }
           ]
        }"#,
        vec![input],
        vec![IrValue::Native(input), IrValue::Native(input)],
    )
    .await;
}
