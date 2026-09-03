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

//! Scaffolding for the `unit/` tests: IR and VM logic checked without building
//! a proof. Nothing here parses a VK, so the blobs are arbitrary bytes.

use std::borrow::Cow;

use midnight_zkir_v3::ir::IrMinorVersion;
use midnight_zkir_v3::{Instruction, IrSource};
use sha2::Digest;
use transient_crypto::curve::Fr;
use transient_crypto::proofs::{InnerProofWitness, KeyLocation, ProofPreimage, Zkir};

/// The binding input every preimage here carries. Nothing in `unit/` asserts on
/// it; it just has to be *something*.
pub const BINDING_INPUT: u64 = 99;

/// Two arbitrary VK blobs, distinct in content and length.
///
/// The leading byte is the decider tag (`0x00` = `DeciderKind::None`) and the
/// rest is filler. Nothing here parses a blob as a real key, but the tag is
/// read before anything else is, so an arbitrary first byte would be refused as
/// an unknown decider before the check under test could ever run.
pub const VK_BLOB_A: [u8; 32] = {
    let mut b = [0xaa; 32];
    b[0] = 0x00;
    b
};
pub const VK_BLOB_B: [u8; 48] = {
    let mut b = [0xbb; 48];
    b[0] = 0x00;
    b
};

/// A blob's digest, hex-encoded without the `0x` prefix.
pub fn vk_hash(blob: &[u8]) -> String {
    const_hex::encode(sha2::Sha256::digest(blob))
}

/// Wraps an instruction list in the IR envelope and parses it. The circuit
/// takes no inputs; see [`ir_with_inputs`] for one that does.
pub fn ir(instructions: &str) -> IrSource {
    ir_with_inputs("", false, instructions)
}

/// As [`ir`], with a resolved VK side-table attached.
///
/// The version is bumped by hand because `IrSource::load` accepts only
/// `minor: 0..=0`, so text IR always parses as [`IrMinorVersion::V0`] — and a
/// `V0` carrying `verify_proof_vks` is refused by `Serializable::serialize`.
pub fn ir_with_vks(instructions: &str, vks: Vec<Vec<u8>>) -> IrSource {
    let mut ir = ir(instructions);
    ir.version = IrMinorVersion::V1;
    ir.verify_proof_vks = vks;
    ir
}

/// The full envelope: a circuit's declared `inputs`, whether it commits to
/// communications, and its instructions. Both occupy public inputs ahead of any
/// accumulator, which is what makes them worth varying.
pub fn ir_with_inputs(
    inputs: &str,
    do_communications_commitment: bool,
    instructions: &str,
) -> IrSource {
    let json = format!(
        r#"{{
           "version": {{ "major": 3, "minor": 0 }},
           "inputs": [{inputs}],
           "outputs": [],
           "do_communications_commitment": {do_communications_commitment},
           "instructions": [{instructions}]
        }}"#
    );
    IrSource::load(json.as_bytes()).expect("IR must parse")
}

/// A single `inner_proof` binding, bound to `%p_0`.
pub const BIND_ONE: &str = r#"{ "op": "inner_proof", "guard": "0x01", "output": "%p_0" }"#;

/// `inner_proof` bindings `%p_0..%p_n`, then one `verify_proof` per hash, each
/// over a distinct one-element instance — so a round-trip that mixed instances
/// up between instructions would show.
pub fn bind_and_verify(vk_hashes: &[String]) -> String {
    let binds = (0..vk_hashes.len())
        .map(|i| format!(r#"{{ "op": "inner_proof", "guard": "0x01", "output": "%p_{i}" }}"#))
        .collect::<Vec<_>>();
    let verifies = vk_hashes.iter().enumerate().map(|(i, h)| {
        let instance = 0x7b + i;
        format!(
            r#"{{ "op": "verify_proof", "guard": "0x01", "vk_hash": "0x{h}", "instance": ["0x{instance:02x}"], "proof": "%p_{i}" }}"#
        )
    });
    binds
        .into_iter()
        .chain(verifies)
        .collect::<Vec<_>>()
        .join(",\n")
}

/// [`bind_and_verify`] for the single-`verify_proof` case.
pub fn bind_and_verify_one(vk_hash: &str) -> String {
    bind_and_verify(&[vk_hash.to_string()])
}

/// A preimage carrying `n` proof witnesses and nothing else.
pub fn preimage(n: usize) -> ProofPreimage {
    ProofPreimage {
        binding_input: Fr::from(BINDING_INPUT),
        communications_commitment: None,
        inputs: vec![],
        private_transcript: vec![],
        public_transcript_inputs: vec![],
        public_transcript_outputs: vec![],
        proof_witnesses: (0..n)
            .map(|i| InnerProofWitness::Direct(vec![i as u8; 8]))
            .collect(),
        key_location: KeyLocation(Cow::Borrowed("builtin")),
    }
}

/// Runs `check`, returning the error message. Panics if it succeeded.
pub fn expect_check_err(ir: &IrSource, preimage: ProofPreimage) -> String {
    match ir.check(&preimage) {
        Ok(_) => panic!("check unexpectedly succeeded"),
        Err(e) => format!("{e:#}"),
    }
}

/// The `vk_hash` of every `VerifyProof`, in instruction order.
pub fn vk_hashes(ir: &IrSource) -> Vec<Vec<u8>> {
    ir.instructions
        .iter()
        .filter_map(|ins| match ins {
            Instruction::VerifyProof { vk_hash, .. } => Some(vk_hash.clone()),
            _ => None,
        })
        .collect()
}
