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

//! Scaffolding for the `e2e/` tests: a real inner proof built, verified
//! in-circuit, and carried through to the deferred pairing.
//!
//! [`RsaSignatureRelation`] is the default inner workload; [`SingleScalarRelation`]
//! is deliberately unlike it, for cases needing two distinct keys or a cheaper
//! circuit. [`rsa_inner_proof`] and [`scalar_inner_proofs`] are the usual entry
//! points; the `inner_setup*` / `prove_inner*` pairs underneath are for cases
//! that vary one step, such as a different domain or transcript.
//!
//! Does not reuse `common::TestParams`, which reads `env!("MIDNIGHT_PP")` at
//! build time and would break compilation wherever that is unset.

use std::borrow::Cow;
use std::fs::File;
use std::io::BufReader;
use std::ops::Rem;
use std::sync::OnceLock;
use std::time::Instant;

use midnight_circuits::biguint::AssignedBigUint;
use midnight_circuits::hash::poseidon::PoseidonState;
use midnight_circuits::instructions::{
    AssertionInstructions, AssignmentInstructions, PublicInputInstructions,
};
use midnight_circuits::types::AssignedNative;
use midnight_curves::Fq;
use midnight_proofs::circuit::{Layouter, Value};
use midnight_proofs::plonk;
use midnight_zk_stdlib::{
    MidnightPK, Relation, ZkStdLib, ZkStdLibArch, optimal_k, prove, setup_pk, setup_vk,
};
use num_bigint::BigUint;
use num_traits::{Num, One};
use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;
use sha2::Digest;

use midnight_zkir_v3::IrSource;
use midnight_zkir_v3::ir::IrMinorVersion;
use midnight_zkir_v3::ir_instructions::decidable::{DeciderKind, serialize_vk};
use transient_crypto::curve::Fr;
use transient_crypto::proofs::{
    InnerProofWitness, KeyLocation, PARAMS_VERIFIER, ParamsProver, ParamsProverProvider, Proof,
    ProofPreimage, ProverKey, VerifierKey, Zkir,
};

/// SRS params read at run time from `$MIDNIGHT_PP`, falling back to
/// `~/.cache/midnight/zk-params`.
pub struct RuntimeParams;

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

/// The binding input every outer preimage carries; it occupies slot 0.
pub const BINDING_INPUT: u64 = 99;

/// We assume the RSA public key is of the form `(3, m)`.
pub const E: u64 = 3;

/// Bit width of the modulus, the message and the signature.
pub const NB_BITS: u32 = 1024;

/// The BigUint gadget works in base `2^96`.
pub const LOG2_BASE: u32 = 96;

/// Floor on the SRS size an inner circuit uses. `optimal_k` can report less
/// than the smallest params file on disk (the single-scalar fixture's is 4). An
/// oversized domain is harmless; a missing file is not.
pub const MIN_SRS_K: u8 = 10;

pub type Modulus = BigUint;
pub type Message = BigUint;
pub type Signature = BigUint;

/// Given an RSA public key `(3, m)` and a message as public inputs, prove
/// knowledge of an `s` with `s^3 = msg (mod m)`.
#[derive(Clone, Default)]
pub struct RsaSignatureRelation;

impl Relation for RsaSignatureRelation {
    type Instance = (Modulus, Message);
    type Witness = Signature;
    type Error = plonk::Error;

    fn format_instance((pk, msg): &Self::Instance) -> Result<Vec<Fq>, plonk::Error> {
        Ok([
            AssignedBigUint::<Fq>::as_public_input(pk, NB_BITS),
            AssignedBigUint::<Fq>::as_public_input(msg, NB_BITS),
        ]
        .into_iter()
        .flatten()
        .collect())
    }

    fn circuit(
        &self,
        std_lib: &ZkStdLib,
        layouter: &mut impl Layouter<Fq>,
        instance: Value<Self::Instance>,
        witness: Value<Self::Witness>,
    ) -> Result<(), plonk::Error> {
        let biguint = std_lib.biguint();

        let public_key = biguint.assign_biguint(
            layouter,
            instance.as_ref().map(|(pk, _)| pk.clone()),
            NB_BITS,
        )?;
        let message = biguint.assign_biguint(layouter, instance.map(|(_, msg)| msg), NB_BITS)?;
        let signature = biguint.assign_biguint(layouter, witness, NB_BITS)?;

        biguint.constrain_as_public_input(layouter, &public_key, NB_BITS)?;
        biguint.constrain_as_public_input(layouter, &message, NB_BITS)?;

        let expected_msg = biguint.mod_exp(layouter, &signature, E, &public_key)?;

        biguint.assert_equal(layouter, &message, &expected_msg)
    }

    fn used_chips(&self) -> ZkStdLibArch {
        ZkStdLibArch {
            nr_pow2range_cols: 4,
            ..ZkStdLibArch::default()
        }
    }

    fn write_relation<W: std::io::Write>(&self, _writer: &mut W) -> std::io::Result<()> {
        Ok(())
    }

    fn read_relation<R: std::io::Read>(_reader: &mut R) -> std::io::Result<Self> {
        Ok(RsaSignatureRelation)
    }
}

/// Witnesses one field element and exposes it. Deliberately unlike
/// [`RsaSignatureRelation`] — different architecture, and an instance of 1
/// element against RSA's 22.
#[derive(Clone, Default)]
pub struct SingleScalarRelation;

impl Relation for SingleScalarRelation {
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
        Ok(SingleScalarRelation)
    }
}

/// A deterministic RNG, so every test in this suite is reproducible.
pub fn test_rng() -> ChaCha20Rng {
    ChaCha20Rng::from_seed([7; 32])
}

/// A fixed RSA key pair: the public modulus `m = p*q` and the private exponent
/// `d = 3^-1 mod phi(m)`. `p` and `q` are the two 512-bit primes from the
/// `midnight-zk` RSA example.
pub fn rsa_key() -> (Modulus, BigUint) {
    let p = BigUint::from_str_radix(
        "81e05798232330a8c7059621c812dc9d2bba37edbd0e79f101eef1db373c1272\
         4595480ae6a9dbbf158fa65d6910b8aea7b3be2eede9123ede8d84ec9e8ee907",
        16,
    )
    .unwrap();
    let q = BigUint::from_str_radix(
        "acd6fd3c0d70502e8ecefb20259fbf4783a614a0fb1a33701e3adc84947326a7\
         54f8a632e5f6cd718a681cde953024b3612bb0646f180b6fd063b1ef4e10d4a5",
        16,
    )
    .unwrap();

    let modulus = &p * &q;
    let phi = (&p - BigUint::one()) * (&q - BigUint::one());
    let d = BigUint::from(E)
        .modinv(&phi)
        .expect("e must be invertible mod phi");

    (modulus, d)
}

/// An arbitrary fixed message, reduced into `Z_modulus`. `tag` selects between
/// distinct messages under the same key.
pub fn message(modulus: &Modulus, tag: u8) -> Message {
    let hex = format!(
        "6d69646e696768742d6c65646765722d7273612d696e6e65722d70726f6f662d\
         746573742d766563746f722d646f2d6e6f742d7573652d696e2d70726f64{tag:02x}"
    );
    BigUint::from_str_radix(&hex, 16).unwrap().rem(modulus)
}

/// Signs `msg` under the private exponent `d`, i.e. produces `s` with
/// `s^3 = msg (mod modulus)`.
pub fn sign(msg: &Message, d: &BigUint, modulus: &Modulus) -> Signature {
    let signature = msg.modpow(d, modulus);
    debug_assert_eq!(&signature.modpow(&BigUint::from(E), modulus), msg);
    signature
}

/// The statement every test in this suite pins its outer circuit to: message
/// number 0 under [`rsa_key`], with its valid signature.
pub fn pinned_statement() -> ((Modulus, Message), Signature) {
    let (modulus, d) = rsa_key();
    let msg = message(&modulus, 0);
    let signature = sign(&msg, &d, &modulus);
    ((modulus, msg), signature)
}

/// Derives `k` from the relation, reads the matching SRS, and generates the key
/// pair. The VK blob is `serialize_vk`'s output: a leading decider tag byte,
/// then the `MidnightVK` in `Processed` form.
///
/// Every relation here is an ordinary circuit that defers nothing of its own,
/// so the kind is always [`DeciderKind::None`]. `verify_proof_e2e.rs` covers
/// [`DeciderKind::Collapsed`].
pub async fn inner_setup_for<R: Relation + Default>(
    label: &str,
) -> (ParamsProver, MidnightPK<R>, Vec<u8>) {
    let derived = u8::try_from(optimal_k(&R::default())).expect("inner k fits in u8");
    let k = derived.max(MIN_SRS_K);
    let label = if k == derived {
        label.to_string()
    } else {
        format!("{label}, derived {derived}, clamped to {MIN_SRS_K}")
    };
    inner_setup_at::<R>(&label, k).await
}

/// In a domain of the caller's choosing rather than the relation's own. A
/// different `k` gives a different verifying key, which is how a "wrong-size"
/// inner proof is built.
pub async fn inner_setup_at<R: Relation + Default>(
    label: &str,
    k: u8,
) -> (ParamsProver, MidnightPK<R>, Vec<u8>) {
    let relation = R::default();
    println!("inner circuit k = {k} ({label})");

    // `setup_vk` adopts the SRS's `max_k`, so the SRS read here fixes the VK's.
    let inner_srs = RuntimeParams.get_params(k).await.expect("inner SRS");
    let inner_vk = setup_vk(inner_srs.as_ref(), &relation);
    assert_eq!(
        inner_vk.k(),
        k,
        "inner SRS must match the k it was set up with"
    );
    let inner_pk = setup_pk(&relation, &inner_vk);

    let vk_blob = serialize_vk(&inner_vk, DeciderKind::None).expect("serialize inner vk");

    (inner_srs, inner_pk, vk_blob)
}

/// [`inner_setup_for`] specialised to the RSA fixture.
pub async fn inner_setup() -> (ParamsProver, MidnightPK<RsaSignatureRelation>, Vec<u8>) {
    inner_setup_for::<RsaSignatureRelation>("rsa").await
}

/// Proves with the **Poseidon** transcript the in-circuit verifier requires —
/// not ZKIR's default Blake2b.
pub fn prove_inner_for<R: Relation + Default>(
    label: &str,
    instance: &R::Instance,
    witness: R::Witness,
    inner_pk: &MidnightPK<R>,
    inner_srs: &ParamsProver,
    rng: &mut ChaCha20Rng,
) -> Vec<u8> {
    let t = Instant::now();
    let proof = prove::<R, PoseidonState<Fq>>(
        inner_srs.as_ref(),
        inner_pk,
        &R::default(),
        instance,
        witness,
        rng,
    )
    .expect("inner prove");
    println!(
        "inner prove ({label}): {:.1?} ({} proof bytes)",
        t.elapsed(),
        proof.len()
    );
    proof
}

/// [`prove_inner_for`] specialised to the RSA fixture.
pub fn prove_inner(
    instance: &(Modulus, Message),
    signature: Signature,
    inner_pk: &MidnightPK<RsaSignatureRelation>,
    inner_srs: &ParamsProver,
    rng: &mut ChaCha20Rng,
) -> Vec<u8> {
    prove_inner_for::<RsaSignatureRelation>("rsa", instance, signature, inner_pk, inner_srs, rng)
}

/// A VK blob's digest, hex-encoded without the `0x` prefix. This is what a
/// `verify_proof` instruction stores in place of the key itself.
pub fn vk_hash_hex(vk_blob: &[u8]) -> String {
    const_hex::encode(sha2::Sha256::digest(vk_blob))
}

/// An inner statement as `verify_proof`'s `instance` operand list.
///
/// Operands are `0x`-prefixed *little-endian* hex immediates — what `Operand`'s
/// deserializer feeds to `Fr::from_le_bytes`.
pub fn instance_json(pis: &[Fq]) -> String {
    pis.iter()
        .map(|f| format!("\"0x{}\"", const_hex::encode(Fr(*f).as_le_bytes())))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The outer IR envelope: declared `inputs` (JSON, often empty), whether the
/// circuit compiles a communications commitment, the instruction list, and the
/// resolved VK side-table.
///
/// Attaching the side-table is what a compiler does out-of-band: the hash-only
/// IR gains its keys, and both passes index them by digest so each instruction's
/// `vk_hash` resolves.
pub fn outer_ir_with(
    inputs: &str,
    do_communications_commitment: bool,
    instructions: &str,
    vks: Vec<Vec<u8>>,
) -> IrSource {
    let ir_json = format!(
        r#"{{
           "version": {{ "major": 3, "minor": 0 }},
           "inputs": [{inputs}],
           "outputs": [],
           "do_communications_commitment": {do_communications_commitment},
           "instructions": [{instructions}]
        }}"#
    );
    let mut ir = IrSource::load(ir_json.as_bytes()).expect("outer IR must parse");
    // `load` accepts only `minor: 0..=0`, and a `V0` may not carry a side-table.
    ir.version = IrMinorVersion::V1;
    ir.verify_proof_vks = vks;
    ir
}

/// [`outer_ir_with`] for the common case: no declared inputs, no commitment.
pub fn outer_ir(instructions: &str, vks: Vec<Vec<u8>>) -> IrSource {
    outer_ir_with("", false, instructions, vks)
}

/// Everything a `verify_proof` instruction needs about one inner proof: the key
/// it was made under, the statement it proves, and the proof itself.
pub struct InnerProof {
    pub vk_blob: Vec<u8>,
    pub pis: Vec<Fq>,
    pub proof: Vec<u8>,
}

impl InnerProof {
    /// As an [`outer_ir_for_all`] entry.
    pub fn entry(&self) -> (Vec<u8>, Vec<Fq>) {
        (self.vk_blob.clone(), self.pis.clone())
    }
}

/// Sets up and proves the RSA fixture on [`pinned_statement`] — 22 public
/// inputs, and the workload every single-proof case uses.
pub async fn rsa_inner_proof(rng: &mut ChaCha20Rng) -> InnerProof {
    let (srs, pk, vk_blob) = inner_setup().await;
    let (instance, signature) = pinned_statement();
    let proof = prove_inner(&instance, signature, &pk, &srs, rng);
    let pis = RsaSignatureRelation::format_instance(&instance).expect("format rsa instance");
    InnerProof {
        vk_blob,
        pis,
        proof,
    }
}

/// Sets up and proves [`SingleScalarRelation`] on a fixed instance — 1 public
/// input, and a key deliberately unlike the RSA fixture's.
pub async fn scalar_inner_proof(rng: &mut ChaCha20Rng) -> InnerProof {
    scalar_inner_proofs(&[123], rng).await.pop().expect("one")
}

/// Several statements under *one* key: setup runs once, then a proof per
/// instance. The cheapest fixture here — prefer it whenever a case does not care
/// which relation produced its proofs.
pub async fn scalar_inner_proofs(instances: &[u64], rng: &mut ChaCha20Rng) -> Vec<InnerProof> {
    let (srs, pk, vk_blob) = inner_setup_for::<SingleScalarRelation>("single-scalar").await;
    instances
        .iter()
        .map(|&i| {
            let instance = Fq::from(i);
            let proof = prove_inner_for::<SingleScalarRelation>(
                &format!("single-scalar, instance {i}"),
                &instance,
                (),
                &pk,
                &srs,
                rng,
            );
            InnerProof {
                vk_blob: vk_blob.clone(),
                pis: SingleScalarRelation::format_instance(&instance)
                    .expect("format scalar instance"),
                proof,
            }
        })
        .collect()
}

/// One `inner_proof` binding per proof, then one `verify_proof` per
/// (VK, instance) pair, in order.
pub fn outer_ir_for_all(entries: &[(Vec<u8>, Vec<Fq>)]) -> IrSource {
    let bindings = (0..entries.len())
        .map(|i| format!(r#"{{ "op": "inner_proof", "guard": "0x01", "output": "%p_{i}" }}"#))
        .collect::<Vec<_>>();

    let verifications = entries.iter().enumerate().map(|(i, (vk_blob, pis))| {
        format!(
            r#"{{
                   "op": "verify_proof",
                   "guard": "0x01",
                   "vk_hash": "0x{vk_hash}",
                   "instance": [{instance}],
                   "proof": "%p_{i}"
               }}"#,
            vk_hash = vk_hash_hex(vk_blob),
            instance = instance_json(pis),
        )
    });

    let instructions = bindings
        .into_iter()
        .chain(verifications)
        .collect::<Vec<_>>()
        .join(",\n               ");

    // The side-table is a *set* of the keys the circuit needs: resolution
    // indexes it by digest and rejects a blob listed twice, so an entry reused
    // across instructions contributes one copy, not one per use.
    let mut vks: Vec<Vec<u8>> = Vec::new();
    for (blob, _) in entries {
        if !vks.contains(blob) {
            vks.push(blob.clone());
        }
    }

    outer_ir(&instructions, vks)
}

/// Single-`verify_proof` convenience wrapper over [`outer_ir_for_all`].
pub fn outer_ir_for(vk_blob: &[u8], inner_pis: &[Fq]) -> IrSource {
    outer_ir_for_all(&[(vk_blob.to_vec(), inner_pis.to_vec())])
}

/// Builds the outer circuit for the given (VK blob, inner public inputs)
/// entries, and generates its keys.
pub async fn outer_setup_all(
    entries: &[(Vec<u8>, Vec<Fq>)],
) -> (IrSource, ProverKey<IrSource>, VerifierKey) {
    let ir = outer_ir_for_all(entries);
    let label = format!("{} verify_proof instruction(s)", entries.len());
    let (pk, vk) = outer_keygen(&ir, &label).await;
    (ir, pk, vk)
}

/// Reports the outer circuit's `k` and generates its keys. Split out so a test
/// that builds its own IR can share the timing and reporting.
pub async fn outer_keygen(ir: &IrSource, label: &str) -> (ProverKey<IrSource>, VerifierKey) {
    println!("outer circuit k = {} ({label})", ir.k());

    let t = Instant::now();
    let (pk, vk) = ir.keygen(&RuntimeParams).await.expect("outer keygen");
    println!("outer keygen: {:.1?}", t.elapsed());

    (pk, vk)
}

/// Builds the outer circuit pinned to `instance`, and generates its keys.
pub async fn outer_setup(
    vk_blob: Vec<u8>,
    instance: &(Modulus, Message),
) -> (IrSource, ProverKey<IrSource>, VerifierKey) {
    let inner_pis = RsaSignatureRelation::format_instance(instance).expect("format inner instance");
    assert_eq!(
        inner_pis.len(),
        2 * NB_BITS.div_ceil(LOG2_BASE) as usize,
        "expected 11 limbs each for modulus and message"
    );
    outer_setup_all(&[(vk_blob, inner_pis)]).await
}

/// Everything the single-`verify_proof` RSA circuit needs, built once. Five
/// cases compile that same circuit and differ only in the inner proof they feed
/// it; keygen costs ~40s, so it is shared.
pub struct PinnedFixture {
    pub inner_srs: ParamsProver,
    pub inner_pk: MidnightPK<RsaSignatureRelation>,
    pub vk_blob: Vec<u8>,
    pub instance: (Modulus, Message),
    pub signature: Signature,
    pub ir: IrSource,
    pub pk: ProverKey<IrSource>,
    pub vk: VerifierKey,
}

static PINNED: OnceLock<PinnedFixture> = OnceLock::new();

/// The shared fixture, built on first use. Cases stay separate; they just stop
/// paying for the same keygen. A race would build twice and discard one — the
/// e2e tier runs single-threaded, so it does not arise.
pub async fn pinned_fixture() -> &'static PinnedFixture {
    if let Some(fixture) = PINNED.get() {
        println!("(reusing the pinned outer circuit)");
        return fixture;
    }

    let (inner_srs, inner_pk, vk_blob) = inner_setup().await;
    let (instance, signature) = pinned_statement();
    let (ir, pk, vk) = outer_setup(vk_blob.clone(), &instance).await;

    let _ = PINNED.set(PinnedFixture {
        inner_srs,
        inner_pk,
        vk_blob,
        instance,
        signature,
        ir,
        pk,
        vk,
    });
    PINNED.get().expect("just set")
}

impl PinnedFixture {
    /// A correct inner proof of [`pinned_statement`], freshly proven.
    pub fn correct_proof(&self, rng: &mut ChaCha20Rng) -> Vec<u8> {
        prove_inner(
            &self.instance,
            self.signature.clone(),
            &self.inner_pk,
            &self.inner_srs,
            rng,
        )
    }

    /// The inner statement as `verify_proof` sees it.
    pub fn inner_pis(&self) -> Vec<Fq> {
        RsaSignatureRelation::format_instance(&self.instance).expect("format inner instance")
    }
}

/// Binding input, the inner proofs as opaque witnesses, everything else empty.
pub fn outer_preimage_all(inner_proofs: Vec<Vec<u8>>) -> ProofPreimage {
    ProofPreimage {
        binding_input: Fr::from(BINDING_INPUT),
        communications_commitment: None,
        inputs: vec![],
        private_transcript: vec![],
        public_transcript_inputs: vec![],
        public_transcript_outputs: vec![],
        proof_witnesses: inner_proofs
            .into_iter()
            .map(InnerProofWitness::Direct)
            .collect(),
        key_location: KeyLocation(Cow::Borrowed("builtin")),
    }
}

/// [`outer_preimage_all`] for a single proof.
pub fn outer_preimage(inner_proof: Vec<u8>) -> ProofPreimage {
    outer_preimage_all(vec![inner_proof])
}

/// [`outer_preimage_all`] for a circuit that declares `inputs` and discloses
/// values through an `Impact`.
///
/// A guarded-*on* `Impact` needs one `public_transcript_inputs` entry per
/// declared input, each equal to what its operand resolves to, or `preprocess`
/// bails with "Public transcript input mismatch". A guarded-*off* one consumes
/// none, so the vector must then be empty.
pub fn outer_preimage_with(
    inner_proofs: Vec<Vec<u8>>,
    inputs: Vec<Fr>,
    public_transcript_inputs: Vec<Fr>,
) -> ProofPreimage {
    let mut preimage = outer_preimage_all(inner_proofs);
    preimage.inputs = inputs;
    preimage.public_transcript_inputs = public_transcript_inputs;
    preimage
}

/// Proves the outer circuit, reporting how long it took.
pub async fn outer_prove(
    ir: &IrSource,
    pk: ProverKey<IrSource>,
    preimage: &ProofPreimage,
    rng: &mut ChaCha20Rng,
) -> (Proof, Vec<Fr>) {
    let (proof, pis, _skips) = outer_prove_with_skips(ir, pk, preimage, rng).await;
    (proof, pis)
}

/// As [`outer_prove`], also returning the public-input skips.
///
/// One entry per `Impact`, in instruction order: `Some(n)` when the guard was
/// off, naming the `n` zero-padded slots the verifier should ignore, and `None`
/// when it was on. `VerifyProof` contributes no entry, which is what keeps an
/// accumulator from being skipped over.
pub async fn outer_prove_with_skips(
    ir: &IrSource,
    pk: ProverKey<IrSource>,
    preimage: &ProofPreimage,
    rng: &mut ChaCha20Rng,
) -> (Proof, Vec<Fr>, Vec<Option<usize>>) {
    let t = Instant::now();
    let (proof, pis, skips) = ir
        .prove(rng, &RuntimeParams, pk, preimage)
        .await
        .expect("outer prove");
    println!("outer prove: {:.1?} (pi skips: {skips:?})", t.elapsed());
    (proof, pis, skips)
}

/// Verifies the outer proof. A single `verify` runs the Plonk check *and* the
/// deferred pairing on every accumulator the proof carries. Verification params
/// are constant-size, so the static `PARAMS_VERIFIER` works regardless of the
/// circuit's `k`.
pub fn outer_verify(vk: &VerifierKey, proof: &Proof, pis: Vec<Fr>) {
    let t = Instant::now();
    vk.verify(&PARAMS_VERIFIER, proof, pis.into_iter())
        .expect("outer verify (incl. deferred pairing)");
    println!("outer verify: {:.1?}", t.elapsed());
}

/// Where a bad inner proof got rejected.
#[derive(Debug, PartialEq, Eq)]
pub enum Rejection {
    /// Preparation could not process the proof at all.
    AtProve,
    /// Proving succeeded; the deferred pairing rejected. The accumulator is
    /// internally consistent, it just does not pair.
    AtVerify,
}

/// Runs the pipeline against a bad inner proof and reports where it was
/// rejected. Panics if the pipeline accepts it.
pub async fn expect_rejected(
    ir: &IrSource,
    pk: ProverKey<IrSource>,
    vk: &VerifierKey,
    inner_proof: Vec<u8>,
    rng: &mut ChaCha20Rng,
) -> Rejection {
    let preimage = outer_preimage(inner_proof);

    // Not `outer_prove`/`outer_verify`: those expect success, and which stage
    // refuses is the point here.
    let t = Instant::now();
    let (proof, pis, _skips) = match ir.prove(rng, &RuntimeParams, pk, &preimage).await {
        Ok(ok) => {
            println!("outer prove: {:.1?}", t.elapsed());
            ok
        }
        Err(e) => {
            println!("rejected at prove after {:.1?} -- {e}", t.elapsed());
            return Rejection::AtProve;
        }
    };

    let t = Instant::now();
    match vk.verify(&PARAMS_VERIFIER, &proof, pis.into_iter()) {
        Ok(()) => panic!("the outer pipeline ACCEPTED a bad inner proof"),
        Err(e) => {
            println!("rejected at verify after {:.1?} -- {e}", t.elapsed());
            Rejection::AtVerify
        }
    }
}
