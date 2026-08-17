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

//! End-to-end example for the `verify_proof` instruction (Model A: fixed VK,
//! full binary).
//!
//! Flow:
//!   1. Build a minimal inner circuit and prove it with a **Poseidon**
//!      transcript — what the in-circuit verifier expects, NOT ZKIR's default
//!      Blake2b transcript.
//!   2. Serialize its `MidnightVK` (Processed) as the VK blob, and take the raw
//!      proof bytes.
//!   3. Build an outer ZKIR circuit that binds the proof with `inner_proof` and
//!      verifies it with `verify_proof`, against that VK and the inner public
//!      input (as hex in the IR).
//!   4. keygen the outer circuit (which records the accumulator's public-input
//!      offset in the verifying key); then prove it and verify, where verifying
//!      runs the deferred pairing check on the inner proof's accumulator.
//!
//! This test is `#[ignore]`d: the outer circuit does in-circuit BLS12-381 proof
//! verification, so it is large and needs prover SRS params at a high `k`
//! (point `MIDNIGHT_PP` at a directory of `bls_midnight_2p<k>` files, or rely on
//! `~/.cache/midnight/zk-params`), which are not present in CI. Run with:
//!   `MIDNIGHT_PP=<dir> cargo +1.95.0 test -p midnight-zkir-v3 \
//!        --test verify_proof_e2e -- --ignored --nocapture`

use std::fs::File;
use std::io::BufReader;

use midnight_circuits::hash::poseidon::PoseidonState;
use sha2::Digest;
use midnight_circuits::instructions::{AssignmentInstructions, PublicInputInstructions};
use midnight_circuits::types::AssignedNative;
use midnight_curves::Fq;
use midnight_proofs::circuit::{Layouter, Value};
use midnight_proofs::plonk;
use midnight_proofs::utils::SerdeFormat;
use midnight_zk_stdlib::{Relation, ZkStdLib, ZkStdLibArch, prove, setup_pk, setup_vk};
use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;

use std::borrow::Cow;

use midnight_zkir_v3::IrSource;
use transient_crypto::curve::Fr;
use transient_crypto::proofs::{
    KeyLocation, PARAMS_VERIFIER, ParamsProver, ParamsProverProvider, ProofPreimage, Zkir,
};

/// Reads SRS params at runtime from `$MIDNIGHT_PP` (falling back to
/// `~/.cache/midnight/zk-params`), so this file compiles without the build-time
/// `env!("MIDNIGHT_PP")` the shared test harness uses.
struct RuntimeParams;

impl ParamsProverProvider for RuntimeParams {
    async fn get_params(&self, k: u8) -> std::io::Result<ParamsProver> {
        let dir = std::env::var("MIDNIGHT_PP").unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_default();
            format!("{home}/.cache/midnight/zk-params")
        });
        ParamsProver::read(BufReader::new(File::open(format!(
            "{dir}/bls_midnight_2p{k}"
        ))?))
    }
}

/// A minimal inner circuit: witnesses one field element and exposes it as the
/// single public input. Just enough to produce a real, verifiable proof.
#[derive(Clone)]
struct InnerRelation;

impl Relation for InnerRelation {
    type Instance = Fq;
    type Witness = ();
    type Error = plonk::Error;

    fn format_instance(instance: &Fq) -> Result<Vec<Fq>, plonk::Error> {
        Ok(vec![*instance])
    }

    fn circuit(
        &self,
        std_lib: &ZkStdLib,
        layouter: &mut impl Layouter<Fq>,
        instance: Value<Fq>,
        _witness: Value<()>,
    ) -> Result<(), plonk::Error> {
        let x: AssignedNative<Fq> = std_lib.assign(layouter, instance)?;
        std_lib.constrain_as_public_input(layouter, &x)?;
        Ok(())
    }

    fn used_chips(&self) -> ZkStdLibArch {
        ZkStdLibArch::default()
    }

    fn write_relation<W: std::io::Write>(&self, _writer: &mut W) -> std::io::Result<()> {
        Ok(())
    }

    fn read_relation<R: std::io::Read>(_reader: &mut R) -> std::io::Result<Self> {
        Ok(InnerRelation)
    }
}

#[actix_rt::test]
#[ignore = "outer verifier circuit needs a high-k SRS not available in CI"]
async fn verify_proof_end_to_end() {
    let mut rng = ChaCha20Rng::from_seed([7; 32]);

    // ---- 1. Inner proof, with a Poseidon transcript --------------------
    let inner_instance = Fq::from(123u64);
    let inner_relation = InnerRelation;

    // Discover the inner circuit's k, then read the matching SRS.
    let k = {
        let srs = RuntimeParams.get_params(12).await.expect("inner SRS (probe)");
        setup_vk(srs.as_ref(), &inner_relation).k()
    };
    let inner_srs = RuntimeParams.get_params(k).await.expect("inner SRS");
    let inner_vk = setup_vk(inner_srs.as_ref(), &inner_relation);
    let inner_pk = setup_pk(&inner_relation, &inner_vk);
    let inner_proof = prove::<InnerRelation, PoseidonState<Fq>>(
        inner_srs.as_ref(),
        &inner_pk,
        &inner_relation,
        &inner_instance,
        (),
        &mut rng,
    )
    .expect("inner prove");

    // ---- 2. Serialize the inner VK into the VK blob ---------------------
    // The blob is simply the `MidnightVK` in its `Processed` serialization; the
    // ledger reads it back with `MidnightVK::read` when preparing/verifying the
    // inner proof. (There is no longer a decider discriminant byte.)
    let vk_blob = {
        let mut blob = Vec::new();
        inner_vk
            .write(&mut blob, SerdeFormat::Processed)
            .expect("serialize inner vk");
        blob
    };

    // ---- 3. Outer ZKIR circuit: inner_proof + verify_proof --------------
    // Built via JSON (`IrSource::load`). The canonical IR is hash-only: the
    // instruction stores just `hash(vk_blob)`, the inner public input is the
    // immediate `0x7b` (= 123), and the proof arrives by name from
    // `inner_proof`. The full VK is supplied out-of-band below.
    let vk_hash = sha2::Sha256::digest(&vk_blob);
    let ir_json = format!(
        r#"{{
           "version": {{ "major": 3, "minor": 0 }},
           "inputs": [],
           "outputs": [],
           "do_communications_commitment": false,
           "instructions": [
               {{
                   "op": "inner_proof",
                   "output": "%p_0"
               }},
               {{
                   "op": "verify_proof",
                   "vk_hash": "0x{vk_hash_hex}",
                   "instance": ["0x7b"],
                   "proof": "%p_0"
               }}
           ]
        }}"#,
        vk_hash_hex = const_hex::encode(vk_hash),
    );
    let mut outer_ir = IrSource::load(ir_json.as_bytes()).expect("outer IR must parse");

    // Resolve the full VK out-of-band (at "compile time"): the hash-only IR
    // gains its VK side-table. Both passes index it by digest, so the
    // instruction's `vk_hash` resolves to this blob.
    outer_ir.verify_proof_vks = vec![vk_blob];

    // ---- 4. keygen + prove + verify the outer ZKIR circuit --------------
    let (outer_pk, outer_vk) = outer_ir.keygen(&RuntimeParams).await.expect("outer keygen");

    // The outer circuit takes no inputs (the inner statement is an immediate),
    // so its preimage is just a binding input with empty transcripts — plus the
    // inner proof, supplied here as the (single) opaque proof witness.
    let outer_preimage = ProofPreimage {
        binding_input: Fr::from(99u64),
        communications_commitment: None,
        inputs: vec![],
        private_transcript: vec![],
        public_transcript_inputs: vec![],
        public_transcript_outputs: vec![],
        proof_witnesses: vec![inner_proof],
        key_location: KeyLocation(Cow::Borrowed("builtin")),
    };

    // Prove the outer circuit via the normal ZKIR path (Blake2b transcript). If
    // the inner proof were invalid or incompatible, the off-circuit and
    // in-circuit `verify_proof` preparation would make this fail — so a
    // successful prove is itself evidence the inner proof was verified.
    let (outer_proof, outer_pis, _pi_skips) = outer_ir
        .prove(&mut rng, &RuntimeParams, outer_pk, &outer_preimage)
        .await
        .expect("outer prove");

    // The verifying key carries the inner-proof accumulator offset. Assert that
    // this survives the wire format: serialization is invariant under lazy
    // initialization, and a serialize -> deserialize round-trip is byte-exact.
    // We then verify through the *reloaded* key, so the deferred pairing is
    // driven by offsets that came back off the wire, not from the in-memory VK.
    let reloaded_vk = {
        use serialize::{Deserializable, Serializable};
        let mut bytes = Vec::new();
        Serializable::serialize(&outer_vk, &mut bytes).expect("serialize vk");
        outer_vk.init().expect("init vk");
        let mut bytes_after_init = Vec::new();
        Serializable::serialize(&outer_vk, &mut bytes_after_init).expect("serialize vk");
        assert_eq!(
            bytes, bytes_after_init,
            "VerifierKey serialization must be invariant under initialization"
        );
        let reloaded: transient_crypto::proofs::VerifierKey =
            Deserializable::deserialize(&mut &bytes[..], 0).expect("deserialize vk");
        let mut bytes_reloaded = Vec::new();
        Serializable::serialize(&reloaded, &mut bytes_reloaded).expect("serialize vk");
        assert_eq!(
            bytes, bytes_reloaded,
            "VerifierKey must round-trip byte-for-byte through serialize/deserialize"
        );
        
        // Initialize the wire-parsed key and serialize it again. Unlike the
        // (uninitialized) pass-through above, this re-derives the blob *from the
        // parsed structure* (`vk.write()` + offsets).
        reloaded.init().expect("init reloaded vk");
        let mut bytes_after_reload_init = Vec::new();
        Serializable::serialize(&reloaded, &mut bytes_after_reload_init).expect("serialize vk");
        assert_eq!(
            bytes, bytes_after_reload_init,
            "re-serializing a wire-parsed VerifierKey from its parsed structure must reproduce the bytes"
        );
        reloaded
    };

    // Verify the outer proof. This is the whole story: `verify` runs the Plonk
    // check AND, transparently, the deferred pairing on the inner proof's
    // accumulator (whose public-input offset is recorded in the VerifierKey and
    // whose encoding is carried in `outer_pis`). So a single `verify` fully
    // verifies both the outer circuit and the inner proof. Verification params
    // are constant-size, so the static `PARAMS_VERIFIER` works regardless of the
    // outer circuit's k.
    reloaded_vk
        .verify(&PARAMS_VERIFIER, &outer_proof, outer_pis.into_iter())
        .expect("outer verify (incl. deferred pairing)");
}
