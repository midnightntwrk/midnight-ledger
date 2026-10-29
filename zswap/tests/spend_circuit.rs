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

//! Tests that the hand-written `ZswapSpend` relation proves the same statement
//! as `zswap/static/spend.zkir`: its public inputs match those computed by the
//! IR virtual machine, and its proofs verify against its own verifier key.

#![cfg(feature = "proof-verifying")]

use midnight_zswap::spend_circuit::{
    NUM_PUBLIC_INPUTS, SpendWitness, ZSWAP_TREE_HEIGHT, ZswapSpend, split_bytes32,
};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha20Rng;
use std::borrow::Cow;
use std::fs::File;
use std::io::BufReader;
use transient_crypto::curve::{EmbeddedFr, Fr};
use transient_crypto_old::proofs::{ParamsProver, VerifierKey};
use zkir_v2::IrSource;

const PARAMS_DIR: &str = env!("MIDNIGHT_PP");
const ZSWAP_STATIC: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/static");

fn params(k: u8) -> ParamsProver {
    ParamsProver::read(BufReader::new(
        File::open(format!("{PARAMS_DIR}/bls_midnight_2p{k}")).expect("params file must exist"),
    ))
    .expect("params must parse")
}

/// Converts a current `Fr` to the old (v1 pipeline) `Fr`.
fn to_old(f: Fr) -> transient_crypto_old::curve::Fr {
    transient_crypto_old::curve::Fr::from_le_bytes(&f.as_le_bytes()).expect("Fr round-trip")
}

fn test_witness(rng: &mut impl Rng, is_user: bool) -> SpendWitness {
    let zero32 = [Fr::from(0u64); 2];
    let mut witness = SpendWitness {
        is_user,
        sk: if is_user {
            split_bytes32(&rng.r#gen())
        } else {
            zero32
        },
        contract_address: if is_user {
            zero32
        } else {
            split_bytes32(&rng.r#gen())
        },
        // Fixed up below once the preimage is known.
        commitment: zero32,
        path: core::array::from_fn(|_| (Fr::from(rng.r#gen::<u64>()), rng.r#gen::<bool>())),
        nonce: split_bytes32(&rng.r#gen()),
        token_type: split_bytes32(&rng.r#gen()),
        value: rng.r#gen::<u128>().into(),
        rc: Fr::try_from(rng.r#gen::<EmbeddedFr>()).expect("Fr is larger than EmbeddedFr"),
        binding_input: Fr::from(rng.r#gen::<u64>()),
        check_root_result: true,
        segment: rng.r#gen::<u16>(),
    };
    witness.commitment = witness.implied_commitment();
    assert_eq!(witness.path.len(), ZSWAP_TREE_HEIGHT);
    witness
}

/// Cross-checks the native witness/public-input computation against the IR
/// virtual machine executing `spend.zkir`, which also re-validates every
/// circuit constraint on these values.
fn assert_matches_ir(witness: &SpendWitness) {
    let ir = IrSource::load_from_tagged(
        &mut File::open(format!("{ZSWAP_STATIC}/spend.bzkir")).expect("spend.bzkir must exist"),
    )
    .expect("spend.bzkir must parse");
    let preimage = transient_crypto::proofs::ProofPreimage {
        inputs: witness.flatten(),
        private_transcript: vec![],
        public_transcript_inputs: witness.public_transcript_inputs(),
        public_transcript_outputs: vec![
            Fr::from(witness.check_root_result as u64),
            Fr::from(witness.segment as u64),
        ],
        binding_input: witness.binding_input,
        communications_commitment: None,
        inner_proofs: vec![],
        key_location: transient_crypto::proofs::KeyLocation(Cow::Borrowed("midnight/zswap/spend")),
    };
    let preprocessed = ir.preprocess(&preimage).expect("IR run must succeed");
    let native_pis: Vec<_> = witness.public_inputs().iter().map(|pi| pi.0).collect();
    assert_eq!(
        preprocessed.pis, native_pis,
        "native public inputs must match the IR virtual machine"
    );
}

#[test]
fn public_inputs_match_ir() {
    let mut rng = ChaCha20Rng::from_seed([0x5e; 32]);
    for is_user in [true, false] {
        let witness = test_witness(&mut rng, is_user);
        assert_eq!(witness.public_inputs().len(), NUM_PUBLIC_INPUTS);
        assert_matches_ir(&witness);
    }
}

#[test]
fn proofs_verify_against_own_verifier_key() {
    let mut rng = ChaCha20Rng::from_seed([0x5e; 32]);

    let relation = ZswapSpend;
    let circuit = midnight_zk_stdlib_v1::MidnightCircuit::from_relation(&relation);
    let k = circuit.min_k() as u8;
    let model = midnight_zk_stdlib_v1::cost_model(&relation);
    println!("k={k} rows={}", model.rows);
    let params = params(k);
    let vk = midnight_zk_stdlib_v1::setup_vk(params.as_ref(), &relation);
    let pk = midnight_zk_stdlib_v1::setup_pk(&relation, &vk);
    let vk = VerifierKey::from(vk);
    let mut forge_rng = ChaCha20Rng::from_seed([0x0f; 32]);
    let mut forged_verifies = |witness: SpendWitness| {
        let pis = witness.public_inputs();
        let pis_old: Vec<_> = pis.iter().map(|pi| to_old(*pi).0).collect();
        let forged = midnight_zk_stdlib_v1::prove::<_, transient_crypto_old::proofs::TranscriptHash>(
            params.as_ref(),
            &pk,
            &relation,
            &pis_old,
            witness,
            &mut forge_rng,
        );
        forged.is_ok_and(|proof| {
            vk.verify(
                &transient_crypto_old::proofs::PARAMS_VERIFIER,
                &transient_crypto_old::proofs::Proof(proof),
                pis.iter().copied().map(to_old),
            )
            .is_ok()
        })
    };

    for is_user in [true, false] {
        let witness = test_witness(&mut rng, is_user);

        let pis = witness.public_inputs();
        assert_eq!(pis.len(), NUM_PUBLIC_INPUTS);
        let pis_old: Vec<_> = pis.iter().map(|pi| to_old(*pi).0).collect();
        let proof =
            midnight_zk_stdlib_v1::prove::<_, transient_crypto_old::proofs::TranscriptHash>(
                params.as_ref(),
                &pk,
                &relation,
                &pis_old,
                witness.clone(),
                &mut rng,
            )
            .expect("proving must succeed");

        // A commitment that does not open to the Merkle leaf must not yield a
        // valid proof, even for the statement its own witness implies.
        let mut unopened = witness.clone();
        unopened.nonce = split_bytes32(&rng.r#gen());
        assert!(
            !forged_verifies(unopened),
            "an unopened commitment must not verify"
        );

        // A value of 2^128 or more must not verify, even though its low 128
        // bits open the commitment and the Pedersen commitment uses all bits.
        let mut overflowing = witness;
        overflowing.value = overflowing.value + Fr::from(u128::MAX) + 1.into();
        overflowing.commitment = overflowing.implied_commitment();
        assert!(
            !forged_verifies(overflowing),
            "a value of 2^128 or more must not verify"
        );

        vk.verify(
            &transient_crypto_old::proofs::PARAMS_VERIFIER,
            &transient_crypto_old::proofs::Proof(proof.clone()),
            pis.iter().copied().map(to_old),
        )
        .expect("proof must verify against the circuit's own verifier key");

        // A wrong statement must not verify.
        let mut bad_pis = pis.clone();
        bad_pis[0] = bad_pis[0] + 1.into();
        assert!(
            vk.verify(
                &transient_crypto_old::proofs::PARAMS_VERIFIER,
                &transient_crypto_old::proofs::Proof(proof),
                bad_pis.into_iter().map(to_old),
            )
            .is_err()
        );
    }
}
