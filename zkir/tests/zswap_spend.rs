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

//! Tests that the hand-written `ZswapSpend` relation is equivalent to the
//! shipped `zswap/static/spend.*` artifacts: identical verifier key bytes,
//! identical public input layout, and proofs that verify against the shipped
//! verifier key.

use midnight_zkir::IrSource;
use midnight_zkir::zswap_spend::{
    NUM_PUBLIC_INPUTS, SpendWitness, ZSWAP_TREE_HEIGHT, ZswapSpend, split_bytes32,
};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha20Rng;
use std::borrow::Cow;
use std::fs::File;
use std::io::BufReader;
use transient_crypto::curve::{EmbeddedFr, Fr};
use transient_crypto_old::proofs::{ParamsProver, VerifierKey};

const PARAMS_DIR: &str = env!("MIDNIGHT_PP");
const ZSWAP_STATIC: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../zswap/static");

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
        check_root_result: 1.into(),
        segment: Fr::from(rng.r#gen::<u16>() as u64),
    };
    witness.commitment = witness.implied_commitment();
    assert_eq!(witness.path.len(), ZSWAP_TREE_HEIGHT);
    witness
}

/// The public transcript inputs matching `SpendWitness::public_inputs`: the
/// declared public inputs minus the binding input, with the contract-address
/// block removed when it is skipped (user-owned spends).
fn public_transcript_inputs(witness: &SpendWitness) -> Vec<Fr> {
    let mut inputs = witness.public_inputs()[1..].to_vec();
    if witness.is_user {
        // The Cell_write([3], contract_address) block, pis[34..46].
        inputs.drain(33..45);
    }
    inputs
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
        public_transcript_inputs: public_transcript_inputs(witness),
        public_transcript_outputs: vec![witness.check_root_result, witness.segment],
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
fn vk_is_byte_identical_to_shipped_spend_verifier() {
    let relation = ZswapSpend;
    let circuit = midnight_zk_stdlib_v1::MidnightCircuit::from_relation(&relation);
    let k = circuit.min_k() as u8;
    let vk = midnight_zk_stdlib_v1::setup_vk(params(k).as_ref(), &relation);
    let vk = VerifierKey::from(vk);
    let mut bytes = Vec::new();
    serialize::tagged_serialize(&vk, &mut bytes).expect("vk serialization must succeed");
    let expected =
        std::fs::read(format!("{ZSWAP_STATIC}/spend.verifier")).expect("shipped vk must exist");
    assert_eq!(
        bytes, expected,
        "hand-written spend circuit must produce the shipped verifier key, byte for byte"
    );
}

#[test]
fn proofs_verify_against_shipped_verifier_key() {
    let mut rng = ChaCha20Rng::from_seed([0x5e; 32]);
    let shipped_vk: VerifierKey = serialize::tagged_deserialize(
        &mut File::open(format!("{ZSWAP_STATIC}/spend.verifier")).expect("shipped vk must exist"),
    )
    .expect("shipped vk must parse");

    let relation = ZswapSpend;
    let circuit = midnight_zk_stdlib_v1::MidnightCircuit::from_relation(&relation);
    let k = circuit.min_k() as u8;
    let params = params(k);
    let vk = midnight_zk_stdlib_v1::setup_vk(params.as_ref(), &relation);
    let pk = midnight_zk_stdlib_v1::setup_pk(&relation, &vk);

    for is_user in [true, false] {
        let witness = test_witness(&mut rng, is_user);
        assert_matches_ir(&witness);

        let pis = witness.public_inputs();
        assert_eq!(pis.len(), NUM_PUBLIC_INPUTS);
        let pis_old: Vec<_> = pis.iter().map(|pi| to_old(*pi).0).collect();
        let proof =
            midnight_zk_stdlib_v1::prove::<_, transient_crypto_old::proofs::TranscriptHash>(
                params.as_ref(),
                &pk,
                &relation,
                &pis_old,
                witness,
                &mut rng,
            )
            .expect("proving must succeed");

        shipped_vk
            .verify(
                &transient_crypto_old::proofs::PARAMS_VERIFIER,
                &transient_crypto_old::proofs::Proof(proof.clone()),
                pis.iter().copied().map(to_old),
            )
            .expect("proof must verify against the shipped verifier key");

        // A wrong statement must not verify.
        let mut bad_pis = pis.clone();
        bad_pis[0] = bad_pis[0] + 1.into();
        assert!(
            shipped_vk
                .verify(
                    &transient_crypto_old::proofs::PARAMS_VERIFIER,
                    &transient_crypto_old::proofs::Proof(proof),
                    bad_pis.into_iter().map(to_old),
                )
                .is_err()
        );
    }
}
