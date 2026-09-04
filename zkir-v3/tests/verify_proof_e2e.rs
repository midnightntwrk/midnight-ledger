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

//! End-to-end tests for the `verify_proof` instruction, one per decider kind.
//!
//! Both follow the same shape: prove an inner circuit with a Poseidon
//! transcript, register its verifying key under a decider, then build an
//! outer ZKIR circuit that witnesses the inner public inputs, binds the proof
//! with `inner_proof`, and verifies it with `verify_proof`. Verifying the outer
//! proof runs the pairing check on the accumulator the instruction exposed.
//!
//!   * [`Echo`] defers nothing: [`DeciderKind::None`].
//!   * [`Recursive`] verifies an [`Echo`] proof in-circuit, so it carries the
//!     accumulator that verification defers: [`DeciderKind::Collapsed`].

use std::borrow::Cow;
use std::fs::File;
use std::io::BufReader;

use midnight_circuits::hash::poseidon::PoseidonState;
use midnight_circuits::instructions::{AssignmentInstructions, PublicInputInstructions};
use midnight_circuits::types::{AssignedBit, AssignedNative};
use midnight_curves::Fq;
use midnight_proofs::circuit::{Layouter, Value};
use midnight_proofs::plonk;
use midnight_zk_stdlib::{
    MidnightVK, Relation, ZkStdLib, ZkStdLibArch, optimal_k, prove, setup_pk, setup_vk,
};
use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;
use sha2::Digest;

use midnight_zkir_v3::IrSource;
use midnight_zkir_v3::ir_instructions::decidable::{
    DeciderKind, serialize_vk, trivial_accumulator_pis,
};
use midnight_zkir_v3::ir_instructions::verify_proof::{
    verify_proof_incircuit, verify_proof_offcircuit,
};
use transient_crypto::curve::Fr;
use transient_crypto::proofs::{
    InnerProofWitness, KeyLocation, PARAMS_VERIFIER, ParamsProver, ParamsProverProvider,
    ProofPreimage, Zkir,
};

/// The value [`Echo`] exposes.
const ECHO: u64 = 123;

/// Reads SRS params at runtime from `$MIDNIGHT_PP`.
struct RuntimeParams;

impl ParamsProverProvider for RuntimeParams {
    async fn get_params(&self, k: u8) -> std::io::Result<ParamsProver> {
        let dir = std::env::var("MIDNIGHT_PP")
            .expect("$MIDNIGHT_PP must name a directory of `bls_midnight_2p<k>` files");
        ParamsProver::read(BufReader::new(File::open(format!(
            "{dir}/bls_midnight_2p{k}"
        ))?))
    }
}

// ---------------------------------------------------------------------------
// The two inner circuits
// ---------------------------------------------------------------------------

/// Exposes one field element as its only public input.
#[derive(Clone)]
struct Echo;

impl Relation for Echo {
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
        std_lib.constrain_as_public_input(layouter, &x)
    }

    fn used_chips(&self) -> ZkStdLibArch {
        ZkStdLibArch::default()
    }

    fn write_relation<W: std::io::Write>(&self, _writer: &mut W) -> std::io::Result<()> {
        Ok(())
    }

    fn read_relation<R: std::io::Read>(_reader: &mut R) -> std::io::Result<Self> {
        Ok(Echo)
    }
}

/// Verifies a proof of `inner_vk` in-circuit, and exposes the accumulator that
/// verification defers as the tail of its own instance.
#[derive(Clone)]
struct Recursive {
    /// The verifying key of the proof this one verifies, as registered:
    /// `serialize_vk`'s blob.
    inner_vk: Vec<u8>,
}

impl Relation for Recursive {
    type Instance = Vec<Fq>;
    type Witness = Vec<u8>;
    type Error = plonk::Error;

    fn format_instance(instance: &Vec<Fq>) -> Result<Vec<Fq>, plonk::Error> {
        Ok(instance.clone())
    }

    fn circuit(
        &self,
        std_lib: &ZkStdLib,
        layouter: &mut impl Layouter<Fq>,
        instance: Value<Vec<Fq>>,
        witness: Value<Vec<u8>>,
    ) -> Result<(), plonk::Error> {
        let x: AssignedNative<Fq> = std_lib.assign(layouter, instance.map(|fields| fields[0]))?;
        std_lib.constrain_as_public_input(layouter, &x)?;

        // Verifying it constrains the deferred accumulator as the rest of our
        // public inputs. Nothing here is guarded, so the guard is a fixed one.
        let on: AssignedBit<Fq> = std_lib.assign_fixed(layouter, true)?;
        verify_proof_incircuit(std_lib, layouter, &self.inner_vk, &[&[x]], witness, &on)
    }

    fn used_chips(&self) -> ZkStdLibArch {
        ZkStdLibArch {
            poseidon: true,
            bls12_381: true,
            nr_pow2range_cols: 4,
            ..Default::default()
        }
    }

    fn write_relation<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        writer.write_all(&(self.inner_vk.len() as u64).to_le_bytes())?;
        writer.write_all(&self.inner_vk)
    }

    fn read_relation<R: std::io::Read>(reader: &mut R) -> std::io::Result<Self> {
        let mut len = [0u8; 8];
        reader.read_exact(&mut len)?;
        let mut inner_vk = vec![0u8; u64::from_le_bytes(len) as usize];
        reader.read_exact(&mut inner_vk)?;
        Ok(Recursive { inner_vk })
    }
}

// ---------------------------------------------------------------------------
// Shared plumbing
// ---------------------------------------------------------------------------

/// Keygen and prove `relation`, with the Poseidon transcript the in-circuit
/// verifier expects.
async fn prove_inner<R: Relation>(
    relation: &R,
    instance: &R::Instance,
    witness: R::Witness,
    rng: &mut ChaCha20Rng,
) -> (Vec<u8>, MidnightVK) {
    // The SRS files start at k = 12; a smaller circuit is simply padded up.
    let srs = RuntimeParams
        .get_params(optimal_k(relation).max(12) as u8)
        .await
        .expect("inner SRS");
    let vk = setup_vk(srs.as_ref(), relation);
    let pk = setup_pk(relation, &vk);
    let proof = prove::<R, PoseidonState<Fq>>(srs.as_ref(), &pk, relation, instance, witness, rng)
        .expect("inner prove");
    (proof, vk)
}

/// The outer ZKIR circuit: witness the inner proof's `instance_len` public
/// inputs, bind the proof, verify it.
///
/// ZKIR has no instruction of its own for the inner instance: the fields are
/// ordinary prover witnesses, and `verify_proof` just names the variables they
/// were bound to. Here that is one guarded `private_input` per field, which is
/// what a caller emits; the values travel in `ProofPreimage::private_transcript`.
///
/// Everything is under the input guard `%g`, so one keygen serves both guard
/// values — the guard changes what the accumulator public inputs are, never the
/// shape of the circuit.
fn outer_ir(vk_blob: Vec<u8>, instance_len: usize) -> IrSource {
    let names: Vec<String> = (0..instance_len).map(|i| format!("\"%i_{i}\"")).collect();
    let witnesses: Vec<String> = names
        .iter()
        .map(|name| {
            format!(
                r#"{{ "op": "private_input", "guard": "%g",
                      "type": "Scalar<BLS12-381>", "output": {name} }}"#
            )
        })
        .collect();
    let ir_json = format!(
        r#"{{
           "version": {{ "major": 3, "minor": 1 }},
           "inputs": [{{ "name": "%g", "type": "Scalar<BLS12-381>" }}],
           "outputs": [],
           "do_communications_commitment": false,
           "instructions": [
               {witnesses},
               {{ "op": "inner_proof", "guard": "%g", "output": "%p" }},
               {{ "op": "verify_proof", "guard": "%g",
                  "vk_hash": "0x{vk_hash}",
                  "instance": [{names}], "proof": "%p" }}
           ]
        }}"#,
        witnesses = witnesses.join(",\n               "),
        names = names.join(", "),
        vk_hash = const_hex::encode(sha2::Sha256::digest(&vk_blob)),
    );
    let mut ir = IrSource::load(ir_json.as_bytes()).expect("outer IR must parse");
    // `minor: 1` above: the side-table is only in the `V1` wire shape.
    ir.verify_proof_vks = vec![vk_blob];
    // One accumulator, whatever the decider kind. That is what keeps the
    // exposed shape witness-independent and computable at keygen.
    assert_eq!(ir.accumulator_count(), 1);
    ir
}

/// The outer proof's preimage. Guarded off, there is no inner proof and no
/// inner instance to supply at all.
fn preimage(guard: bool, proof: &[u8], instance: &[Fq]) -> ProofPreimage {
    ProofPreimage {
        binding_input: Fr::from(99u64),
        communications_commitment: None,
        inputs: vec![Fr::from(guard as u64)],
        // The inner instance, as ordinary prover witnesses.
        private_transcript: if guard {
            instance.iter().copied().map(Fr).collect()
        } else {
            vec![]
        },
        public_transcript_inputs: vec![],
        public_transcript_outputs: vec![],
        proof_witnesses: guard
            .then(|| InnerProofWitness::Direct(proof.to_vec()))
            .into_iter()
            .collect(),
        key_location: KeyLocation(Cow::Borrowed("builtin")),
    }
}

// ---------------------------------------------------------------------------
// The two scenarios
// ---------------------------------------------------------------------------

/// An inner proof that defers nothing of its own.
#[actix_rt::test]
#[ignore = "in-circuit proof verification needs a high-k SRS, not available in CI"]
async fn verify_proof_without_a_decider() {
    let mut rng = ChaCha20Rng::from_seed([7; 32]);
    let instance = [Fq::from(ECHO)];
    let (proof, vk) = prove_inner(&Echo, &instance[0], (), &mut rng).await;

    let ir = outer_ir(
        serialize_vk(&vk, DeciderKind::None).expect("serialize inner vk"),
        instance.len(),
    );
    let (pk, outer_vk) = ir.keygen(&RuntimeParams).await.expect("outer keygen");

    // Proving already runs both `verify_proof` passes, so it fails if the inner
    // proof does not check out; verifying then discharges the accumulator that
    // verification deferred.
    let (outer_proof, pis, _) = ir
        .prove(
            &mut rng,
            &RuntimeParams,
            pk.clone(),
            &preimage(true, &proof, &instance),
        )
        .await
        .expect("outer prove");
    outer_vk
        .verify(&PARAMS_VERIFIER, &outer_proof, pis.into_iter())
        .expect("outer verify");

    // Guarded off, with no inner proof and no instance supplied, the exposed
    // accumulator is the trivial one, whose pairing holds by construction.
    let (guarded, guarded_pis, _) = ir
        .prove(&mut rng, &RuntimeParams, pk, &preimage(false, &[], &[]))
        .await
        .expect("outer prove (guard off)");
    assert_eq!(
        guarded.accumulators[0],
        trivial_accumulator_pis()
            .into_iter()
            .map(Fr)
            .collect::<Vec<_>>(),
    );
    outer_vk
        .verify(&PARAMS_VERIFIER, &guarded, guarded_pis.into_iter())
        .expect("outer verify (guard off)");
}

/// An inner proof carrying an accumulator of its own, which the instruction
/// folds into the one it exposes.
#[actix_rt::test]
#[ignore = "in-circuit proof verification needs a high-k SRS, not available in CI"]
async fn verify_proof_with_a_collapsed_decider() {
    let mut rng = ChaCha20Rng::from_seed([17; 32]);

    // The proof to be verified in-circuit, and the accumulator verifying it
    // defers.
    let (echo_proof, echo_vk) = prove_inner(&Echo, &Fq::from(ECHO), (), &mut rng).await;
    let echo_blob = serialize_vk(&echo_vk, DeciderKind::None).expect("serialize echo vk");
    let deferred = verify_proof_offcircuit(&echo_blob, &[Fq::from(ECHO)], &echo_proof, true)
        .expect("the accumulator the echo proof defers");

    // The proof that verifies it, carrying that accumulator in its instance
    // tail.
    let recursive = Recursive {
        inner_vk: echo_blob,
    };
    let instance: Vec<Fq> = std::iter::once(Fq::from(ECHO)).chain(deferred).collect();
    let (proof, vk) = prove_inner(&recursive, &instance, echo_proof, &mut rng).await;

    let ir = outer_ir(
        serialize_vk(&vk, DeciderKind::Collapsed).expect("serialize recursive vk"),
        instance.len(),
    );
    let (pk, outer_vk) = ir.keygen(&RuntimeParams).await.expect("outer keygen");

    let (outer_proof, pis, _) = ir
        .prove(
            &mut rng,
            &RuntimeParams,
            pk.clone(),
            &preimage(true, &proof, &instance),
        )
        .await
        .expect("outer prove");

    // What the instruction exposed is the fold, not the recursive proof's own
    // accumulator: the carried one is in there too.
    let own = verify_proof_offcircuit(
        &serialize_vk(&vk, DeciderKind::None).expect("serialize recursive vk as None"),
        &instance,
        &proof,
        true,
    )
    .expect("the recursive proof's own accumulator");
    let exposed: Vec<Fq> = outer_proof.accumulators[0].iter().map(|f| f.0).collect();
    assert_ne!(
        exposed, own,
        "the carried accumulator must have been folded in"
    );

    // One pairing, discharging both.
    outer_vk
        .verify(&PARAMS_VERIFIER, &outer_proof, pis.into_iter())
        .expect("outer verify");

    // Guarded off, as above: the trivial accumulator, and no witnesses at all.
    let (guarded, guarded_pis, _) = ir
        .prove(&mut rng, &RuntimeParams, pk, &preimage(false, &[], &[]))
        .await
        .expect("outer prove (guard off)");
    assert_eq!(
        guarded.accumulators[0],
        trivial_accumulator_pis()
            .into_iter()
            .map(Fr)
            .collect::<Vec<_>>(),
    );
    outer_vk
        .verify(&PARAMS_VERIFIER, &guarded, guarded_pis.into_iter())
        .expect("outer verify (guard off)");
}
