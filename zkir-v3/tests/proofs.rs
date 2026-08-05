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

#[path = "common/mod.rs"]
mod common;

#[cfg(test)]
mod proof_tests {
    use super::common::{TestParams, TestResolver};
    use group::{Group, ff::Field};
    use midnight_curves::{JubjubSubgroup, curve25519, k256, p256};
    use midnight_zkir_v3::{
        Identifier, IrSource, Preprocessed, ir_instructions::encode::encode_offcircuit,
        ir_types::IrValue,
    };
    use rand::{SeedableRng, rngs::OsRng};
    use rand_chacha::ChaCha20Rng;
    #[cfg(feature = "proptest")]
    use serialize::randomised_serialization_test;
    use serialize::{Deserializable, Serializable};
    use std::borrow::Cow;
    use std::collections::HashMap;
    use transient_crypto::curve::EmbeddedGroupAffine;
    use transient_crypto::hash::transient_hash;
    use transient_crypto::proofs::Proof;
    #[cfg(feature = "proptest")]
    use transient_crypto::proofs::{
        KeyLocation, PARAMS_VERIFIER, ProofPreimage, VerifierKey, Zkir,
    };

    type ProverKey = transient_crypto::proofs::ProverKey<IrSource>;

    #[actix_rt::test]
    async fn test_extension_attack() {
        let ir_raw = r#"{
           "version": { "major": 3, "minor": 0 },
           "inputs": [
              { "name": "%v_0", "type": "Scalar<BLS12-381>" }
           ],
           "outputs": [],
           "do_communications_commitment": false,
           "instructions": [
               { "op": "assert", "cond": "%v_0" }
           ]
        }"#;
        let ir = IrSource::load(ir_raw.as_bytes()).unwrap();

        let (pk, vk) = ir.keygen(&TestParams).await.unwrap();
        const N: u64 = 512;
        let proof = ir
            .prove_unchecked(
                &mut ChaCha20Rng::from_seed([42; 32]),
                &TestParams,
                pk,
                Preprocessed {
                    memory: HashMap::from([(
                        Identifier("v0".to_string()),
                        IrValue::Native(1.into()),
                    )]),
                    pis: (0..N).map(Into::into).collect(),
                    pi_skips: vec![],
                    binding_input: 0.into(),
                    comm_comm: None,
                },
            )
            .await;
        // Either proving should have failed, or verification should fail:
        let verify =
            proof.and_then(|proof| vk.verify(&PARAMS_VERIFIER, &proof, (0..N).map(Into::into)));
        assert!(verify.is_err());
    }

    #[actix_rt::test]
    async fn test_minimal_proof() {
        let ir_raw = r#"{
           "version": { "major": 3, "minor": 0 },
           "inputs": [
              { "name": "%v_0", "type": "Scalar<BLS12-381>" }
           ],
           "outputs": [],
           "do_communications_commitment": false,
           "instructions": [
               { "op": "assert", "cond": "%v_0" }
           ]
        }"#;
        let ir = IrSource::load(ir_raw.as_bytes()).unwrap();

        let (pk, vk) = ir.keygen(&TestParams).await.unwrap();
        let mut pk_data = Vec::new();
        let mut vk_data = Vec::new();
        Serializable::serialize(&pk, &mut pk_data).unwrap();
        Serializable::serialize(&vk, &mut vk_data).unwrap();
        let pk_fmt = format!("{:#?}", pk);
        let vk_fmt = format!("{:#?}", vk);
        let pk: ProverKey = Deserializable::deserialize(&mut &pk_data[..], 0).unwrap();
        let vk: VerifierKey = Deserializable::deserialize(&mut &vk_data[..], 0).unwrap();
        pk.init().unwrap();
        vk.init().unwrap();
        dbg!(pk_fmt == format!("{:#?}", pk));
        dbg!(vk_fmt == format!("{:#?}", vk));
        let preimage = ProofPreimage {
            binding_input: 42.into(),
            communications_commitment: None,
            inputs: vec![1.into()],
            private_transcript: vec![],
            public_transcript_inputs: vec![],
            public_transcript_outputs: vec![],
            key_location: KeyLocation(Cow::Borrowed("builtin")),
        };
        let (proof, _) = preimage
            .prove::<IrSource>(
                &mut ChaCha20Rng::from_seed([42; 32]),
                &TestParams,
                &TestResolver {
                    pk: pk.clone(),
                    vk: vk.clone(),
                    ir: ir.clone(),
                },
            )
            .await
            .unwrap();
        vk.verify(&PARAMS_VERIFIER, &proof, [42.into()].into_iter())
            .unwrap();
        assert!(
            vk.verify(&PARAMS_VERIFIER, &proof, [43.into()].into_iter())
                .is_err()
        );
    }

    #[actix_rt::test]
    async fn test_htc_proof() {
        let ir_raw = r#"{
           "version": { "major": 3, "minor": 0 },
           "inputs": [
              { "name": "%v_0", "type": "Scalar<BLS12-381>" },
              { "name": "%v_1", "type": "Scalar<BLS12-381>" },
              { "name": "%v_2", "type": "Scalar<BLS12-381>" }
           ],
           "outputs": [],
           "do_communications_commitment": false,
           "instructions": [
               { "op": "hash_to_curve", "inputs": ["%v_0", "%v_1", "%v_2"], "output": "%p_0" }
           ]
        }"#;
        let ir = IrSource::load(ir_raw.as_bytes()).unwrap();

        let (pk, vk) = ir.keygen(&TestParams).await.unwrap();
        let mut pk_data = Vec::new();
        let mut vk_data = Vec::new();
        Serializable::serialize(&pk, &mut pk_data).unwrap();
        Serializable::serialize(&vk, &mut vk_data).unwrap();
        let pk_fmt = format!("{:#?}", pk);
        let pk: ProverKey = Deserializable::deserialize(&mut &pk_data[..], 0).unwrap();
        pk.init().unwrap();
        dbg!(pk_fmt == format!("{:#?}", pk));
        let preimage = ProofPreimage {
            binding_input: 42.into(),
            communications_commitment: None,
            inputs: vec![1.into(), 2.into(), 3.into()],
            private_transcript: vec![],
            public_transcript_inputs: vec![],
            public_transcript_outputs: vec![],
            key_location: KeyLocation(Cow::Borrowed("builtin")),
        };
        let (proof, _) = preimage
            .prove::<IrSource>(
                &mut ChaCha20Rng::from_seed([42; 32]),
                &TestParams,
                &TestResolver {
                    pk: pk.clone(),
                    vk: vk.clone(),
                    ir: ir.clone(),
                },
            )
            .await
            .unwrap();
        vk.verify(&PARAMS_VERIFIER, &proof, [42.into()].into_iter())
            .unwrap();
    }

    // Note: The impact instruction here doesn't correspond to real Impact VM bytecode.
    // Real impact instructions contain encoded opcodes (0x10 for push, 0x30 for dup, etc.).
    // We're keeping this simplified form for historical reasons - it still exercises the
    // prover's public input handling even if it's not a semantically valid Impact program.
    #[actix_rt::test]
    async fn test_hash_proof() {
        let ir_raw = r#"{
           "version": { "major": 3, "minor": 0 },
           "inputs": [
              { "name": "%v_0", "type": "Scalar<BLS12-381>" },
              { "name": "%v_1", "type": "Scalar<BLS12-381>" },
              { "name": "%v_2", "type": "Scalar<BLS12-381>" }
           ],
           "outputs": [],
           "do_communications_commitment": false,
           "instructions": [
               { "op": "transient_hash", "inputs": ["%v_0", "%v_1", "%v_2"], "output": "%v_3" },
               { "op": "impact", "guard": "0x01", "inputs": ["%v_3"] }
           ]
        }"#;
        let ir = IrSource::load(ir_raw.as_bytes()).unwrap();
        let x = transient_hash(&[1.into(), 2.into(), 3.into()]);

        let (pk, vk) = ir.keygen(&TestParams).await.unwrap();
        let mut pk_data = Vec::new();
        let mut vk_data = Vec::new();
        Serializable::serialize(&pk, &mut pk_data).unwrap();
        Serializable::serialize(&vk, &mut vk_data).unwrap();
        let pk_fmt = format!("{:#?}", pk);
        let pk: ProverKey = Deserializable::deserialize(&mut &pk_data[..], 0).unwrap();
        pk.init().unwrap();
        dbg!(pk_fmt == format!("{:#?}", pk));
        let preimage = ProofPreimage {
            binding_input: 42.into(),
            communications_commitment: None,
            inputs: vec![1.into(), 2.into(), 3.into()],
            private_transcript: vec![],
            public_transcript_inputs: vec![x],
            public_transcript_outputs: vec![],
            key_location: KeyLocation(Cow::Borrowed("builtin")),
        };
        let (proof, _) = preimage
            .prove::<IrSource>(
                &mut ChaCha20Rng::from_seed([42; 32]),
                &TestParams,
                &TestResolver {
                    pk: pk.clone(),
                    vk: vk.clone(),
                    ir: ir.clone(),
                },
            )
            .await
            .unwrap();
        vk.verify(&PARAMS_VERIFIER, &proof, [42.into(), x].into_iter())
            .unwrap();
    }

    #[actix_rt::test]
    async fn test_std_hashes_proof() {
        let ir_raw = r#"{
           "version": { "major": 3, "minor": 0 },
           "inputs": [
              { "name": "%v_0", "type": "Scalar<BLS12-381>" }
           ],
           "outputs": [],
           "do_communications_commitment": false,
           "instructions": [
               { "op": "persistent_hash", "alignment": [ { "tag": "atom", "value": { "tag": "bytes", "length": 1 } } ], "inputs": ["%v_0"], "output": "%v_1" },
               { "op": "keccak256", "alignment": [ { "tag": "atom", "value": { "tag": "bytes", "length": 1 } } ], "inputs": ["%v_0"], "output": "%v_2" }
           ]
        }"#;
        let ir = IrSource::load(ir_raw.as_bytes()).unwrap();

        let (pk, vk) = ir.keygen(&TestParams).await.unwrap();
        let mut pk_data = Vec::new();
        let mut vk_data = Vec::new();
        Serializable::serialize(&pk, &mut pk_data).unwrap();
        Serializable::serialize(&vk, &mut vk_data).unwrap();
        let pk_fmt = format!("{:#?}", pk);
        let pk: ProverKey = Deserializable::deserialize(&mut &pk_data[..], 0).unwrap();
        pk.init().unwrap();
        dbg!(pk_fmt == format!("{:#?}", pk));
        let preimage = ProofPreimage {
            binding_input: 42.into(),
            communications_commitment: None,
            inputs: vec![(42).into()],
            private_transcript: vec![],
            public_transcript_inputs: vec![],
            public_transcript_outputs: vec![],
            key_location: KeyLocation(Cow::Borrowed("builtin")),
        };
        let (proof, _) = preimage
            .prove::<IrSource>(
                &mut ChaCha20Rng::from_seed([42; 32]),
                &TestParams,
                &TestResolver {
                    pk: pk.clone(),
                    vk: vk.clone(),
                    ir: ir.clone(),
                },
            )
            .await
            .unwrap();
        vk.verify(&PARAMS_VERIFIER, &proof, [42.into()].into_iter())
            .unwrap();
    }

    #[actix_rt::test]
    async fn test_ec_proof() {
        let ir_raw = r#"{
           "version": { "major": 3, "minor": 0 },
           "inputs": [
              { "name": "%p0", "type": "Point<Jubjub>" },
              { "name": "%s0", "type": "Scalar<BLS12-381>" },
              { "name": "%s1", "type": "Scalar<Jubjub>" }
           ],
           "outputs": [],
           "do_communications_commitment": false,
           "instructions": [
               { "op": "jubjub_scalar_from_native", "native": "%s0", "output": "%s0d" },
               { "op": "encode", "input": "%s0d", "outputs": ["%s0e"] },
               { "op": "ec_mul", "a": "%p0", "scalar": "%s0d", "output": "%p1" },
               { "op": "ec_mul_generator", "scalar": "%s1", "output": "%p2" },
               { "op": "add", "a": "%p1", "b": "%p2", "output": "%p3" },
               { "op": "private_input", "type": "Point<Jubjub>", "guard": null, "output": "%p4" },
               { "op": "ec_mul", "a": "%p4", "scalar": "%s0d", "output": "%p5" }
           ]
        }"#;
        let ir = IrSource::load(ir_raw.as_bytes()).unwrap();

        let (pk, vk) = ir.keygen(&TestParams).await.unwrap();
        let mut pk_data = Vec::new();
        let mut vk_data = Vec::new();
        Serializable::serialize(&pk, &mut pk_data).unwrap();
        Serializable::serialize(&vk, &mut vk_data).unwrap();
        let pk_fmt = format!("{:#?}", pk);
        let pk: ProverKey = Deserializable::deserialize(&mut &pk_data[..], 0).unwrap();
        pk.init().unwrap();
        dbg!(pk_fmt == format!("{:#?}", pk));
        let mut pk_data = Vec::new();
        let mut vk_data = Vec::new();
        Serializable::serialize(&pk, &mut pk_data).unwrap();
        Serializable::serialize(&vk, &mut vk_data).unwrap();
        let pk_fmt = format!("{:#?}", pk);
        let pk: ProverKey = Deserializable::deserialize(&mut &pk_data[..], 0).unwrap();
        pk.init().unwrap();
        dbg!(pk_fmt == format!("{:#?}", pk));
        let p = EmbeddedGroupAffine::generator();
        let q: EmbeddedGroupAffine = JubjubSubgroup::random(OsRng).into();
        let preimage = ProofPreimage {
            binding_input: 42.into(),
            communications_commitment: None,
            inputs: vec![p.x().unwrap(), p.y().unwrap(), (-1).into(), 63.into()],
            private_transcript: vec![q.x().unwrap(), q.y().unwrap()],
            public_transcript_inputs: vec![],
            public_transcript_outputs: vec![],
            key_location: KeyLocation(Cow::Borrowed("builtin")),
        };
        let (proof, _) = preimage
            .prove::<IrSource>(
                &mut ChaCha20Rng::from_seed([42; 32]),
                &TestParams,
                &TestResolver {
                    pk: pk.clone(),
                    vk: vk.clone(),
                    ir: ir.clone(),
                },
            )
            .await
            .unwrap();
        vk.verify(&PARAMS_VERIFIER, &proof, [42.into()].into_iter())
            .unwrap();
    }

    #[actix_rt::test]
    async fn test_divmod_proof() {
        let ir_raw = r#"{
           "version": { "major": 3, "minor": 0 },
           "inputs": [
              { "name": "%v_0", "type": "Scalar<BLS12-381>" }
           ],
           "outputs": [],
           "do_communications_commitment": false,
           "instructions": [
               { "op": "div_mod_power_of_two", "val": "%v_0", "bits": 3, "outputs": ["%v_1", "%v_2"] },
               { "op": "private_input", "type": "Scalar<BLS12-381>", "guard": null, "output": "%v_3" },
               { "op": "private_input", "type": "Scalar<BLS12-381>", "guard": null, "output": "%v_4" },
               { "op": "constrain_eq", "a": "%v_1", "b": "%v_3" },
               { "op": "constrain_eq", "a": "%v_2", "b": "%v_4" },
               { "op": "reconstitute_field", "divisor": "%v_1", "modulus": "%v_2", "bits": 3, "output": "%v_5" },
               { "op": "constrain_eq", "a": "%v_5", "b": "%v_0" }
           ]
        }"#;
        let ir = IrSource::load(ir_raw.as_bytes()).unwrap();

        let (pk, vk) = ir.keygen(&TestParams).await.unwrap();
        let mut pk_data = Vec::new();
        let mut vk_data = Vec::new();
        Serializable::serialize(&pk, &mut pk_data).unwrap();
        Serializable::serialize(&vk, &mut vk_data).unwrap();
        let pk_fmt = format!("{:#?}", pk);
        let vk_fmt = format!("{:#?}", vk);
        let pk: ProverKey = Deserializable::deserialize(&mut &pk_data[..], 0).unwrap();
        let vk: VerifierKey = Deserializable::deserialize(&mut &vk_data[..], 0).unwrap();
        pk.init().unwrap();
        vk.init().unwrap();
        dbg!(pk_fmt == format!("{:#?}", pk));
        dbg!(vk_fmt == format!("{:#?}", vk));
        let preimage = ProofPreimage {
            binding_input: 42.into(),
            communications_commitment: None,
            inputs: vec![20.into()],
            private_transcript: vec![2.into(), 4.into()],
            public_transcript_inputs: vec![],
            public_transcript_outputs: vec![],
            key_location: KeyLocation(Cow::Borrowed("builtin")),
        };
        let (proof, _) = preimage
            .prove::<IrSource>(
                &mut ChaCha20Rng::from_seed([42; 32]),
                &TestParams,
                &TestResolver {
                    pk: pk.clone(),
                    vk: vk.clone(),
                    ir: ir.clone(),
                },
            )
            .await
            .unwrap();
        vk.verify(&PARAMS_VERIFIER, &proof, [42.into()].into_iter())
            .unwrap();
    }

    #[actix_rt::test]
    async fn test_keygen_and_serialize_eq() {
        let ir_raw = r#"{
           "version": { "major": 3, "minor": 0 },
           "inputs": [
              { "name": "%v_0", "type": "Scalar<BLS12-381>" }
           ],
           "outputs": [],
           "do_communications_commitment": false,
           "instructions": [
               { "op": "assert", "cond": "%v_0" }
           ]
        }"#;
        let ir = IrSource::load(ir_raw.as_bytes()).unwrap();
        let vk_kzg1 = ir.keygen_vk(&TestParams).await.unwrap();
        let vk_kzg2 = ir.keygen_vk(&TestParams).await.unwrap();
        assert_eq!(&vk_kzg1, &vk_kzg2);
        let mut bytes = Vec::new();
        serialize::tagged_serialize(&vk_kzg1, &mut bytes).unwrap();
        let vk_kzg3: VerifierKey = serialize::tagged_deserialize(&mut &bytes[..]).unwrap();
        assert_eq!(&vk_kzg1, &vk_kzg3);
    }

    #[cfg(feature = "proptest")]
    randomised_serialization_test!(VerifierKey);
    #[cfg(feature = "proptest")]
    randomised_serialization_test!(Proof);

    #[actix_rt::test]
    async fn test_immediate_values() {
        // v_2 = v_0 + 5, constrain_eq(v_1, v_2)
        let ir_raw = r#"{
           "version": { "major": 3, "minor": 0 },
           "inputs": [
              { "name": "%v_0", "type": "Scalar<BLS12-381>" },
              { "name": "%v_1", "type": "Scalar<BLS12-381>" }
           ],
           "outputs": [],
           "do_communications_commitment": false,
           "instructions": [
               { "op": "add", "a": "%v_0", "b": "0x05", "output": "%v_2" },
               { "op": "constrain_eq", "a": "%v_1", "b": "%v_2" }
           ]
        }"#;
        let ir = IrSource::load(ir_raw.as_bytes()).unwrap();

        let (pk, vk) = ir.keygen(&TestParams).await.unwrap();

        // Test with v_0 = 10, v_1 = 15
        let preimage = ProofPreimage {
            binding_input: 42.into(),
            communications_commitment: None,
            inputs: vec![10.into(), 15.into()],
            private_transcript: vec![],
            public_transcript_inputs: vec![],
            public_transcript_outputs: vec![],
            key_location: KeyLocation(Cow::Borrowed("builtin")),
        };
        let (proof, _) = preimage
            .prove::<IrSource>(
                &mut ChaCha20Rng::from_seed([42; 32]),
                &TestParams,
                &TestResolver {
                    pk: pk.clone(),
                    vk: vk.clone(),
                    ir: ir.clone(),
                },
            )
            .await
            .unwrap();
        vk.verify(&PARAMS_VERIFIER, &proof, [42.into()].into_iter())
            .unwrap();
    }

    #[actix_rt::test]
    async fn test_immediate_add_and_cond_select() {
        // v_2 = v_0 + 1, v_3 = test_eq(v_1, v_2), assert(v_3), v_4 = v_3 ? 2 : 3
        let ir_raw = r#"{
           "version": { "major": 3, "minor": 0 },
           "inputs": [
              { "name": "%v_0", "type": "Scalar<BLS12-381>" },
              { "name": "%v_1", "type": "Scalar<BLS12-381>" }
           ],
           "outputs": [],
           "do_communications_commitment": false,
           "instructions": [
               { "op": "add", "a": "%v_0", "b": "0x01", "output": "%v_2" },
               { "op": "test_eq", "a": "%v_1", "b": "%v_2", "output": "%v_3" },
               { "op": "assert", "cond": "%v_3" },
               { "op": "cond_select", "bit": "%v_3", "a": "0x02", "b": "0x03", "output": "%v_4" }
           ]
        }"#;
        let ir = IrSource::load(ir_raw.as_bytes()).unwrap();

        let (pk, vk) = ir.keygen(&TestParams).await.unwrap();

        // v_0 = 5, v_1 = 6
        let preimage = ProofPreimage {
            binding_input: 99.into(),
            communications_commitment: None,
            inputs: vec![5.into(), 6.into()],
            private_transcript: vec![],
            public_transcript_inputs: vec![],
            public_transcript_outputs: vec![],
            key_location: KeyLocation(Cow::Borrowed("builtin")),
        };
        let (proof, _) = preimage
            .prove::<IrSource>(
                &mut ChaCha20Rng::from_seed([42; 32]),
                &TestParams,
                &TestResolver {
                    pk: pk.clone(),
                    vk: vk.clone(),
                    ir: ir.clone(),
                },
            )
            .await
            .unwrap();
        vk.verify(&PARAMS_VERIFIER, &proof, [99.into()].into_iter())
            .unwrap();
    }

    #[actix_rt::test]
    async fn test_immediate_copy() {
        // v_1 = copy(0x42), constrain_eq(v_0, v_1)
        let ir_raw = r#"{
           "version": { "major": 3, "minor": 0 },
           "inputs": [
              { "name": "%v_0", "type": "Scalar<BLS12-381>" }
           ],
           "outputs": [],
           "do_communications_commitment": false,
           "instructions": [
               { "op": "copy", "val": "0x42", "output": "%v_1" },
               { "op": "constrain_eq", "a": "%v_0", "b": "%v_1" }
           ]
        }"#;
        let ir = IrSource::load(ir_raw.as_bytes()).unwrap();

        let (pk, vk) = ir.keygen(&TestParams).await.unwrap();

        // Input must be 0x42 = 66 for proof to succeed
        let preimage = ProofPreimage {
            binding_input: 123.into(),
            communications_commitment: None,
            inputs: vec![66.into()],
            private_transcript: vec![],
            public_transcript_inputs: vec![],
            public_transcript_outputs: vec![],
            key_location: KeyLocation(Cow::Borrowed("builtin")),
        };
        let (proof, _) = preimage
            .prove::<IrSource>(
                &mut ChaCha20Rng::from_seed([42; 32]),
                &TestParams,
                &TestResolver {
                    pk: pk.clone(),
                    vk: vk.clone(),
                    ir: ir.clone(),
                },
            )
            .await
            .unwrap();
        vk.verify(&PARAMS_VERIFIER, &proof, [123.into()].into_iter())
            .unwrap();
    }

    // Note: Same as test_hash_proof - the impact instruction here is not real Impact VM
    // bytecode, just a simplified test case kept for historical reasons.
    #[actix_rt::test]
    async fn test_immediate_with_public_inputs() {
        let ir_raw = r#"{
           "version": { "major": 3, "minor": 0 },
           "inputs": [
              { "name": "%v_0", "type": "Scalar<BLS12-381>" },
              { "name": "%v_1", "type": "Scalar<BLS12-381>" }
           ],
           "outputs": [],
           "do_communications_commitment": false,
           "instructions": [
               { "op": "constrain_bits", "val": "%v_0", "bits": 8 },
               { "op": "constrain_bits", "val": "%v_1", "bits": 248 },
               { "op": "cond_select", "bit": "%v_0", "a": "0x00", "b": "0x01", "output": "%v_2" },
               { "op": "assert", "cond": "%v_2" },
               { "op": "impact", "guard": "0x01", "inputs": ["0x30"] }
           ]
        }"#;
        let ir = IrSource::load(ir_raw.as_bytes()).unwrap();

        let (pk, vk) = ir.keygen(&TestParams).await.unwrap();

        let preimage = ProofPreimage {
            binding_input: 48.into(),
            communications_commitment: None,
            inputs: vec![0.into(), 42.into()],
            private_transcript: vec![],
            public_transcript_inputs: vec![48.into()],
            public_transcript_outputs: vec![],
            key_location: KeyLocation(Cow::Borrowed("builtin")),
        };
        let (proof, _) = preimage
            .prove::<IrSource>(
                &mut ChaCha20Rng::from_seed([42; 32]),
                &TestParams,
                &TestResolver {
                    pk: pk.clone(),
                    vk: vk.clone(),
                    ir: ir.clone(),
                },
            )
            .await
            .unwrap();
        vk.verify(&PARAMS_VERIFIER, &proof, [48.into(), 48.into()].into_iter())
            .unwrap();
    }

    // Regression test: a guarded-off `impact` must contribute a *zeroed* public
    // input, and the `prepare` and `synthesize` runs must agree on this. A guard
    // of "0x00" zeroes the `0x30` input, so the public input vector is
    // [binding_input, 0] and the value is recorded as skipped.
    #[actix_rt::test]
    async fn test_impact_guarded_off_zeroes_public_input() {
        let ir_raw = r#"{
           "version": { "major": 3, "minor": 0 },
           "inputs": [
              { "name": "%v_0", "type": "Scalar<BLS12-381>" },
              { "name": "%v_1", "type": "Scalar<BLS12-381>" }
           ],
           "outputs": [],
           "do_communications_commitment": false,
           "instructions": [
               { "op": "constrain_bits", "val": "%v_0", "bits": 8 },
               { "op": "constrain_bits", "val": "%v_1", "bits": 248 },
               { "op": "cond_select", "bit": "%v_0", "a": "0x00", "b": "0x01", "output": "%v_2" },
               { "op": "assert", "cond": "%v_2" },
               { "op": "impact", "guard": "0x00", "inputs": ["0x30"] }
           ]
        }"#;
        let ir = IrSource::load(ir_raw.as_bytes()).unwrap();

        let (pk, vk) = ir.keygen(&TestParams).await.unwrap();

        // The impact is guarded off, so nothing is contributed to the public
        // transcript inputs.
        let preimage = ProofPreimage {
            binding_input: 48.into(),
            communications_commitment: None,
            inputs: vec![0.into(), 42.into()],
            private_transcript: vec![],
            public_transcript_inputs: vec![],
            public_transcript_outputs: vec![],
            key_location: KeyLocation(Cow::Borrowed("builtin")),
        };
        let (proof, _) = preimage
            .prove::<IrSource>(
                &mut ChaCha20Rng::from_seed([42; 32]),
                &TestParams,
                &TestResolver {
                    pk: pk.clone(),
                    vk: vk.clone(),
                    ir: ir.clone(),
                },
            )
            .await
            .unwrap();
        // The guarded-off impact input is zeroed in the public input vector.
        vk.verify(&PARAMS_VERIFIER, &proof, [48.into(), 0.into()].into_iter())
            .unwrap();
    }

    #[actix_rt::test]
    async fn test_immediate_little_endian_encoding() {
        let ir_raw = r#"{
           "version": { "major": 3, "minor": 0 },
           "inputs": [
              { "name": "%v_0", "type": "Scalar<BLS12-381>" }
           ],
           "outputs": [],
           "do_communications_commitment": false,
           "instructions": [
               { "op": "constrain_eq", "a": "%v_0", "b": "0x0001" }
           ]
        }"#;
        let ir = IrSource::load(ir_raw.as_bytes()).unwrap();

        let (pk, vk) = ir.keygen(&TestParams).await.unwrap();

        // v_0 must be 256 (little-endian interpretation of 0x0001)
        let preimage = ProofPreimage {
            binding_input: 77.into(),
            communications_commitment: None,
            inputs: vec![256.into()],
            private_transcript: vec![],
            public_transcript_inputs: vec![],
            public_transcript_outputs: vec![],
            key_location: KeyLocation(Cow::Borrowed("builtin")),
        };
        let (proof, _) = preimage
            .prove::<IrSource>(
                &mut ChaCha20Rng::from_seed([42; 32]),
                &TestParams,
                &TestResolver {
                    pk: pk.clone(),
                    vk: vk.clone(),
                    ir: ir.clone(),
                },
            )
            .await
            .unwrap();
        vk.verify(&PARAMS_VERIFIER, &proof, [77.into()].into_iter())
            .unwrap();

        // Test 0x0100 is interpreted as 1 (bytes [01, 00] = 1 + 256*0)
        let ir_raw2 = r#"{
           "version": { "major": 3, "minor": 0 },
           "inputs": [
              { "name": "%v_0", "type": "Scalar<BLS12-381>" }
           ],
           "outputs": [],
           "do_communications_commitment": false,
           "instructions": [
               { "op": "constrain_eq", "a": "%v_0", "b": "0x0100" }
           ]
        }"#;
        let ir2 = IrSource::load(ir_raw2.as_bytes()).unwrap();
        let (pk2, vk2) = ir2.keygen(&TestParams).await.unwrap();

        // v_0 must be 1 (little-endian interpretation of 0x0100)
        let preimage2 = ProofPreimage {
            binding_input: 88.into(),
            communications_commitment: None,
            inputs: vec![1.into()],
            private_transcript: vec![],
            public_transcript_inputs: vec![],
            public_transcript_outputs: vec![],
            key_location: KeyLocation(Cow::Borrowed("builtin")),
        };
        let (proof2, _) = preimage2
            .prove::<IrSource>(
                &mut ChaCha20Rng::from_seed([42; 32]),
                &TestParams,
                &TestResolver {
                    pk: pk2.clone(),
                    vk: vk2.clone(),
                    ir: ir2.clone(),
                },
            )
            .await
            .unwrap();
        vk2.verify(&PARAMS_VERIFIER, &proof2, [88.into()].into_iter())
            .unwrap();
    }

    #[actix_rt::test]
    async fn test_jubjub_point_ops() {
        // Exercises test_eq (asserted), constrain_eq, cond_select, and neg on JubjubPoint
        // in a single circuit so every op is actively tested without dead values.
        let ir_raw = r#"{
           "version": { "major": 3, "minor": 0 },
           "inputs": [
              { "name": "%p0", "type": "Point<Jubjub>" },
              { "name": "%p1", "type": "Point<Jubjub>" },
              { "name": "%bit", "type": "Scalar<BLS12-381>" }
           ],
           "outputs": [
           ],
           "do_communications_commitment": false,
           "instructions": [
               { "op": "test_eq", "a": "%p0", "b": "%p1", "output": "%v0" },
               { "op": "assert", "cond": "%v0" },
               { "op": "constrain_eq", "a": "%p0", "b": "%p1" },
               { "op": "cond_select", "bit": "%bit", "a": "%p0", "b": "%p1", "output": "%p2" },
               { "op": "constrain_eq", "a": "%p2", "b": "%p0" },
               { "op": "neg", "a": "%p0", "output": "%p0_neg" },
               { "op": "private_input", "type": "Point<Jubjub>", "guard": null, "output": "%p0_neg_priv" },
               { "op": "constrain_eq", "a": "%p0_neg", "b": "%p0_neg_priv" }
           ]
        }"#;
        let ir = IrSource::load(ir_raw.as_bytes()).unwrap();

        let (pk, vk) = ir.keygen(&TestParams).await.unwrap();

        // p0 == p1 == generator, bit == 1
        let p = EmbeddedGroupAffine::generator();
        let neg_p: EmbeddedGroupAffine = (-JubjubSubgroup::generator()).into();
        let preimage = ProofPreimage {
            binding_input: 42.into(),
            communications_commitment: None,
            inputs: vec![
                p.x().unwrap(),
                p.y().unwrap(),
                p.x().unwrap(),
                p.y().unwrap(),
                1.into(),
            ],
            private_transcript: vec![neg_p.x().unwrap(), neg_p.y().unwrap()],
            public_transcript_inputs: vec![],
            public_transcript_outputs: vec![],
            key_location: KeyLocation(Cow::Borrowed("builtin")),
        };
        let (proof, _) = preimage
            .prove::<IrSource>(
                &mut ChaCha20Rng::from_seed([42; 32]),
                &TestParams,
                &TestResolver {
                    pk: pk.clone(),
                    vk: vk.clone(),
                    ir: ir.clone(),
                },
            )
            .await
            .unwrap();
        vk.verify(&PARAMS_VERIFIER, &proof, [42.into()].into_iter())
            .unwrap();
    }

    #[actix_rt::test]
    async fn test_jubjub_point_test_eq_unequal() {
        let ir_raw = r#"{
           "version": { "major": 3, "minor": 0 },
           "inputs": [
              { "name": "%p0", "type": "Point<Jubjub>" },
              { "name": "%p1", "type": "Point<Jubjub>" }
           ],
           "outputs": [
           ],
           "do_communications_commitment": false,
           "instructions": [
               { "op": "test_eq", "a": "%p0", "b": "%p1", "output": "%v0" },
               { "op": "not", "a": "%v0", "output": "%v1" },
               { "op": "assert", "cond": "%v1" }
           ]
        }"#;
        let ir = IrSource::load(ir_raw.as_bytes()).unwrap();

        let (pk, vk) = ir.keygen(&TestParams).await.unwrap();

        let p = EmbeddedGroupAffine::generator();
        let q: EmbeddedGroupAffine = JubjubSubgroup::random(OsRng).into();
        let preimage = ProofPreimage {
            binding_input: 42.into(),
            communications_commitment: None,
            inputs: vec![
                p.x().unwrap(),
                p.y().unwrap(),
                q.x().unwrap(),
                q.y().unwrap(),
            ],
            private_transcript: vec![],
            public_transcript_inputs: vec![],
            public_transcript_outputs: vec![],
            key_location: KeyLocation(Cow::Borrowed("builtin")),
        };
        let (proof, _) = preimage
            .prove::<IrSource>(
                &mut ChaCha20Rng::from_seed([42; 32]),
                &TestParams,
                &TestResolver {
                    pk: pk.clone(),
                    vk: vk.clone(),
                    ir: ir.clone(),
                },
            )
            .await
            .unwrap();
        vk.verify(&PARAMS_VERIFIER, &proof, [42.into()].into_iter())
            .unwrap();
    }

    #[actix_rt::test]
    async fn test_jubjub_point_constrain_eq_fails_on_unequal() {
        let ir_raw = r#"{
           "version": { "major": 3, "minor": 0 },
           "inputs": [
              { "name": "%p0", "type": "Point<Jubjub>" },
              { "name": "%p1", "type": "Point<Jubjub>" }
           ],
           "outputs": [
           ],
           "do_communications_commitment": false,
           "instructions": [
               { "op": "constrain_eq", "a": "%p0", "b": "%p1" }
           ]
        }"#;
        let ir = IrSource::load(ir_raw.as_bytes()).unwrap();

        let (pk, vk) = ir.keygen(&TestParams).await.unwrap();

        // Different points: constrain_eq should fail
        let p = EmbeddedGroupAffine::generator();
        let q: EmbeddedGroupAffine = JubjubSubgroup::random(OsRng).into();
        let preimage_fail = ProofPreimage {
            binding_input: 42.into(),
            communications_commitment: None,
            inputs: vec![
                p.x().unwrap(),
                p.y().unwrap(),
                q.x().unwrap(),
                q.y().unwrap(),
            ],
            private_transcript: vec![],
            public_transcript_inputs: vec![],
            public_transcript_outputs: vec![],
            key_location: KeyLocation(Cow::Borrowed("builtin")),
        };
        let result = preimage_fail
            .prove::<IrSource>(
                &mut ChaCha20Rng::from_seed([42; 32]),
                &TestParams,
                &TestResolver {
                    pk: pk.clone(),
                    vk: vk.clone(),
                    ir: ir.clone(),
                },
            )
            .await;
        assert!(
            result.is_err(),
            "constrain_eq on different JubjubPoints should fail"
        );
    }

    #[actix_rt::test]
    async fn test_jubjub_point_cond_select_fails_when_bit_zero() {
        let ir_raw = r#"{
           "version": { "major": 3, "minor": 0 },
           "inputs": [
              { "name": "%p0", "type": "Point<Jubjub>" },
              { "name": "%p1", "type": "Point<Jubjub>" },
              { "name": "%bit", "type": "Scalar<BLS12-381>" }
           ],
           "outputs": [
           ],
           "do_communications_commitment": false,
           "instructions": [
               { "op": "cond_select", "bit": "%bit", "a": "%p0", "b": "%p1", "output": "%p2" },
               { "op": "constrain_eq", "a": "%p2", "b": "%p0" }
           ]
        }"#;
        let ir = IrSource::load(ir_raw.as_bytes()).unwrap();

        let (pk, vk) = ir.keygen(&TestParams).await.unwrap();

        let p = EmbeddedGroupAffine::generator();
        let q: EmbeddedGroupAffine = JubjubSubgroup::random(OsRng).into();

        // bit=0 selects p1 (!=p0), constrain_eq(p2, p0) should fail
        let preimage_fail = ProofPreimage {
            binding_input: 42.into(),
            communications_commitment: None,
            inputs: vec![
                p.x().unwrap(),
                p.y().unwrap(),
                q.x().unwrap(),
                q.y().unwrap(),
                0.into(),
            ],
            private_transcript: vec![],
            public_transcript_inputs: vec![],
            public_transcript_outputs: vec![],
            key_location: KeyLocation(Cow::Borrowed("builtin")),
        };
        let result = preimage_fail
            .prove::<IrSource>(
                &mut ChaCha20Rng::from_seed([42; 32]),
                &TestParams,
                &TestResolver {
                    pk: pk.clone(),
                    vk: vk.clone(),
                    ir: ir.clone(),
                },
            )
            .await;
        assert!(
            result.is_err(),
            "cond_select with bit=0 should select p1, failing constrain_eq against p0"
        );
    }

    #[test]
    fn test_invalid_operand_no_percent_prefix() {
        // Variables without '%' prefix should fail to deserialize
        let ir_raw = r#"{
           "version": { "major": 3, "minor": 0 },
           "inputs": [
              { "name": "%v_0", "type": "Scalar<BLS12-381>" }
           ],
           "outputs": [],
           "do_communications_commitment": false,
           "instructions": [
               { "op": "assert", "cond": "v_0" }
           ]
        }"#;
        let result = IrSource::load(ir_raw.as_bytes());
        assert!(
            result.is_err(),
            "Should reject identifier without '%' prefix"
        );
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Invalid operand format"),
            "Error message: {}",
            err
        );
        assert!(
            err.contains("Variables must start with '%'"),
            "Error message: {}",
            err
        );
    }

    #[test]
    fn test_invalid_operand_odd_length_hex() {
        // Hex immediates with odd length should fail to deserialize
        let ir_raw = r#"{
           "version": { "major": 3, "minor": 0 },
           "inputs": [
              { "name": "%v_0", "type": "Scalar<BLS12-381>" }
           ],
           "outputs": [],
           "do_communications_commitment": false,
           "instructions": [
               { "op": "copy", "val": "0x1", "output": "%v_1" }
           ]
        }"#;
        let result = IrSource::load(ir_raw.as_bytes());
        assert!(result.is_err(), "Should reject odd-length hex string");
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("odd number of digits"),
            "Error message: {}",
            err
        );
    }

    #[test]
    fn test_invalid_operand_malformed_identifier() {
        // Random strings that don't follow conventions should be rejected
        let ir_raw = r#"{
           "version": { "major": 3, "minor": 0 },
           "inputs": [
              { "name": "foo", "type": "Scalar<BLS12-381>" }
           ],
           "outputs": [],
           "do_communications_commitment": false,
           "instructions": [
               { "op": "assert", "cond": "foo" }
           ]
        }"#;
        let result = IrSource::load(ir_raw.as_bytes());
        assert!(result.is_err(), "Should reject malformed identifier");
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Invalid operand format"),
            "Error message: {}",
            err
        );
    }

    #[actix_rt::test]
    async fn test_secp256k1_proof() {
        // Single circuit exercising all three Secp256k1 types.
        // Base, Scalar and Point values are typed inputs; arithmetic results
        // are checked via private inputs.
        let ir_raw = r#"{
           "version": { "major": 3, "minor": 0 },
           "inputs": [
              { "name": "%id", "type": "Point<Secp256k1>"  },
              { "name": "%p0", "type": "Point<Secp256k1>"  },
              { "name": "%p1", "type": "Point<Secp256k1>"  },
              { "name": "%b0", "type": "Base<Secp256k1>"   },
              { "name": "%b1", "type": "Base<Secp256k1>"   },
              { "name": "%s0", "type": "Scalar<Secp256k1>" },
              { "name": "%s1", "type": "Scalar<Secp256k1>" }
           ],
           "outputs": [],
           "do_communications_commitment": false,
           "instructions": [
               { "op": "add", "a": "%p0", "b": "%p1", "output": "%p2" },
               { "op": "add", "a": "%b0", "b": "%b1", "output": "%b2" },
               { "op": "add", "a": "%s0", "b": "%s1", "output": "%s2" },
               { "op": "mul", "a": "%b0", "b": "%b1", "output": "%b_prod" },
               { "op": "mul", "a": "%s0", "b": "%s1", "output": "%s_prod" },
               { "op": "neg", "a": "%p0", "output": "%p0_neg" },
               { "op": "neg", "a": "%b0", "output": "%b0_neg" },
               { "op": "neg", "a": "%s0", "output": "%s0_neg" },
               { "op": "inv", "a": "%b0", "output": "%b0_inv" },
               { "op": "inv", "a": "%s0", "output": "%s0_inv" },
               { "op": "private_input", "type": "Point<Secp256k1>",  "guard": null, "output": "%p2_priv"     },
               { "op": "private_input", "type": "Base<Secp256k1>",   "guard": null, "output": "%b_prod_priv" },
               { "op": "private_input", "type": "Scalar<Secp256k1>", "guard": null, "output": "%s_prod_priv" },
               { "op": "private_input", "type": "Point<Secp256k1>",  "guard": null, "output": "%p0_neg_priv" },
               { "op": "private_input", "type": "Base<Secp256k1>",   "guard": null, "output": "%b0_neg_priv" },
               { "op": "private_input", "type": "Scalar<Secp256k1>", "guard": null, "output": "%s0_neg_priv" },
               { "op": "private_input", "type": "Base<Secp256k1>",   "guard": null, "output": "%b0_inv_priv" },
               { "op": "private_input", "type": "Scalar<Secp256k1>", "guard": null, "output": "%s0_inv_priv" },
               { "op": "constrain_eq", "a": "%p2",     "b": "%p2_priv"     },
               { "op": "constrain_eq", "a": "%b_prod", "b": "%b_prod_priv" },
               { "op": "constrain_eq", "a": "%p0_neg", "b": "%p0_neg_priv" },
               { "op": "constrain_eq", "a": "%b0_neg", "b": "%b0_neg_priv" },
               { "op": "test_eq",      "a": "%s_prod", "b": "%s_prod_priv", "output": "%sp_eq" },
               { "op": "assert",       "cond": "%sp_eq" },
               { "op": "test_eq",      "a": "%s0_neg", "b": "%s0_neg_priv", "output": "%sn_eq" },
               { "op": "assert",       "cond": "%sn_eq" },
               { "op": "constrain_eq", "a": "%b0_inv", "b": "%b0_inv_priv" },
               { "op": "test_eq",      "a": "%s0_inv", "b": "%s0_inv_priv", "output": "%si_eq" },
               { "op": "assert",       "cond": "%si_eq" }
           ]
        }"#;
        let ir = IrSource::load(ir_raw.as_bytes()).unwrap();

        let id = k256::K256::identity();
        let p0 = k256::K256::random(OsRng);
        let p1 = k256::K256::random(OsRng);
        let b0 = k256::Fp::random(OsRng);
        let b1 = k256::Fp::random(OsRng);
        let s0 = k256::Fq::random(OsRng);
        let s1 = k256::Fq::random(OsRng);

        let encode = |v: IrValue| -> Vec<transient_crypto::curve::Fr> {
            encode_offcircuit(&v)
                .into_iter()
                .map(|x| x.try_into().unwrap())
                .collect()
        };

        let inputs: Vec<transient_crypto::curve::Fr> = [
            encode(IrValue::Secp256k1Point(id)),
            encode(IrValue::Secp256k1Point(p0)),
            encode(IrValue::Secp256k1Point(p1)),
            encode(IrValue::Secp256k1Base(b0)),
            encode(IrValue::Secp256k1Base(b1)),
            encode(IrValue::Secp256k1Scalar(s0)),
            encode(IrValue::Secp256k1Scalar(s1)),
        ]
        .concat();

        let private_transcript: Vec<transient_crypto::curve::Fr> = [
            encode(IrValue::Secp256k1Point(p0 + p1)),
            encode(IrValue::Secp256k1Base(b0 * b1)),
            encode(IrValue::Secp256k1Scalar(s0 * s1)),
            encode(IrValue::Secp256k1Point(-p0)),
            encode(IrValue::Secp256k1Base(-b0)),
            encode(IrValue::Secp256k1Scalar(-s0)),
            encode(IrValue::Secp256k1Base(Option::from(b0.invert()).unwrap())),
            encode(IrValue::Secp256k1Scalar(Option::from(s0.invert()).unwrap())),
        ]
        .concat();

        let (pk, vk) = ir.keygen(&TestParams).await.unwrap();
        let preimage = ProofPreimage {
            binding_input: 42.into(),
            communications_commitment: None,
            inputs,
            private_transcript,
            public_transcript_inputs: vec![],
            public_transcript_outputs: vec![],
            key_location: KeyLocation(Cow::Borrowed("builtin")),
        };
        let (proof, _) = preimage
            .prove::<IrSource>(
                &mut ChaCha20Rng::from_seed([42; 32]),
                &TestParams,
                &TestResolver {
                    pk: pk.clone(),
                    vk: vk.clone(),
                    ir: ir.clone(),
                },
            )
            .await
            .unwrap();
        vk.verify(&PARAMS_VERIFIER, &proof, [42.into()].into_iter())
            .unwrap();
    }

    #[actix_rt::test]
    async fn test_coordinates_proof() {
        // Exercises affine coordinates polymorphically across the
        // supported curve point types. Each extracted coordinate is
        // checked against a private input carrying the expected value.
        // A point is then reconstructed from the extracted coordinates
        // and compared to the original point.
        use midnight_zkir_v3::ir_instructions::into_coordinates::into_coordinates_offcircuit;

        let ir_raw = r#"{
           "version": { "major": 3, "minor": 0 },
           "inputs": [
              { "name": "%jp", "type": "Point<Jubjub>"    },
              { "name": "%sp", "type": "Point<Secp256k1>" }
           ],
           "outputs": [],
           "do_communications_commitment": false,
           "instructions": [
               { "op": "into_coordinates", "point": "%jp", "outputs": ["%jx", "%jy"] },
               { "op": "into_coordinates", "point": "%sp", "outputs": ["%sx", "%sy"] },
               { "op": "private_input", "type": "Scalar<BLS12-381>", "guard": null, "output": "%jx_exp" },
               { "op": "private_input", "type": "Scalar<BLS12-381>", "guard": null, "output": "%jy_exp" },
               { "op": "private_input", "type": "Base<Secp256k1>",   "guard": null, "output": "%sx_exp" },
               { "op": "private_input", "type": "Base<Secp256k1>",   "guard": null, "output": "%sy_exp" },
               { "op": "constrain_eq", "a": "%jx", "b": "%jx_exp" },
               { "op": "constrain_eq", "a": "%jy", "b": "%jy_exp" },
               { "op": "constrain_eq", "a": "%sx", "b": "%sx_exp" },
               { "op": "constrain_eq", "a": "%sy", "b": "%sy_exp" },
               { "op": "from_coordinates", "inputs": ["%jx", "%jy"], "output": "%jp_reconstructed" },
               { "op": "from_coordinates", "inputs": ["%sx", "%sy"], "output": "%sp_reconstructed" },
               { "op": "constrain_eq", "a": "%jp_reconstructed", "b": "%jp" },
               { "op": "constrain_eq", "a": "%sp_reconstructed", "b": "%sp" }
           ]
        }"#;
        let ir = IrSource::load(ir_raw.as_bytes()).unwrap();

        let jp = JubjubSubgroup::random(OsRng);
        let sp = k256::K256::random(OsRng);

        let encode = |v: IrValue| -> Vec<transient_crypto::curve::Fr> {
            encode_offcircuit(&v)
                .into_iter()
                .map(|x| x.try_into().unwrap())
                .collect()
        };

        let inputs: Vec<transient_crypto::curve::Fr> = [
            encode(IrValue::JubjubPoint(jp)),
            encode(IrValue::Secp256k1Point(sp)),
        ]
        .concat();

        let (px, py) = into_coordinates_offcircuit(&IrValue::JubjubPoint(jp)).unwrap();
        let (sx, sy) = into_coordinates_offcircuit(&IrValue::Secp256k1Point(sp)).unwrap();

        let private_transcript: Vec<transient_crypto::curve::Fr> =
            [encode(px), encode(py), encode(sx), encode(sy)].concat();

        let (pk, vk) = ir.keygen(&TestParams).await.unwrap();
        let preimage = ProofPreimage {
            binding_input: 42.into(),
            communications_commitment: None,
            inputs,
            private_transcript,
            public_transcript_inputs: vec![],
            public_transcript_outputs: vec![],
            key_location: KeyLocation(Cow::Borrowed("builtin")),
        };
        let (proof, _) = preimage
            .prove::<IrSource>(
                &mut ChaCha20Rng::from_seed([42; 32]),
                &TestParams,
                &TestResolver {
                    pk: pk.clone(),
                    vk: vk.clone(),
                    ir: ir.clone(),
                },
            )
            .await
            .unwrap();
        vk.verify(&PARAMS_VERIFIER, &proof, [42.into()].into_iter())
            .unwrap();
    }

    #[actix_rt::test]
    async fn test_bytes32_proof() {
        // Exercises into_bytes32 / from_bytes32 across all three currently
        // supported types (Native, Secp256k1Base, Secp256k1Scalar):
        //   1. Round-trips a typed value through into_bytes32 then
        //      from_bytes32 and checks it matches the original.
        //   2. Converts a fixed, non-canonical 32-byte string (all 0xff,
        //      which exceeds every one of these fields' moduli) via
        //      from_bytes32 and checks the in-circuit result against the
        //      off-circuit reference implementation, exercising the
        //      modular-reduction behavior documented on the instruction.
        use midnight_zkir_v3::ir_instructions::from_bytes32::from_bytes32_offcircuit;
        use midnight_zkir_v3::ir_types::IrType;

        let ir_raw = r#"{
           "version": { "major": 3, "minor": 0 },
           "inputs": [
              { "name": "%native",      "type": "Scalar<BLS12-381>" },
              { "name": "%secp_base",   "type": "Base<Secp256k1>"   },
              { "name": "%secp_scalar", "type": "Scalar<Secp256k1>" },
              { "name": "%raw",         "type": "Bytes<32>"         }
           ],
           "outputs": [],
           "do_communications_commitment": false,
           "instructions": [
               { "op": "into_bytes32", "input": "%native",      "output": "%native_bytes" },
               { "op": "into_bytes32", "input": "%secp_base",   "output": "%base_bytes"   },
               { "op": "into_bytes32", "input": "%secp_scalar", "output": "%scalar_bytes" },

               { "op": "from_bytes32", "bytes": "%native_bytes", "type": "Scalar<BLS12-381>", "output": "%native_back" },
               { "op": "from_bytes32", "bytes": "%base_bytes",   "type": "Base<Secp256k1>",   "output": "%base_back"   },
               { "op": "from_bytes32", "bytes": "%scalar_bytes", "type": "Scalar<Secp256k1>", "output": "%scalar_back" },

               { "op": "constrain_eq", "a": "%native_back", "b": "%native"      },
               { "op": "constrain_eq", "a": "%base_back",   "b": "%secp_base"   },
               { "op": "constrain_eq", "a": "%scalar_back", "b": "%secp_scalar" },

               { "op": "from_bytes32", "bytes": "%raw", "type": "Scalar<BLS12-381>", "output": "%raw_native" },
               { "op": "from_bytes32", "bytes": "%raw", "type": "Base<Secp256k1>",   "output": "%raw_base"   },
               { "op": "from_bytes32", "bytes": "%raw", "type": "Scalar<Secp256k1>", "output": "%raw_scalar" },

               { "op": "private_input", "type": "Scalar<BLS12-381>", "guard": null, "output": "%raw_native_exp" },
               { "op": "private_input", "type": "Base<Secp256k1>",   "guard": null, "output": "%raw_base_exp"   },
               { "op": "private_input", "type": "Scalar<Secp256k1>", "guard": null, "output": "%raw_scalar_exp" },

               { "op": "constrain_eq", "a": "%raw_native", "b": "%raw_native_exp" },
               { "op": "constrain_eq", "a": "%raw_base",   "b": "%raw_base_exp"   },
               { "op": "constrain_eq", "a": "%raw_scalar", "b": "%raw_scalar_exp" }
           ]
        }"#;
        let ir = IrSource::load(ir_raw.as_bytes()).unwrap();

        let native_val: transient_crypto::curve::Fr = rand::random();
        let base_val = k256::Fp::random(OsRng);
        let scalar_val = k256::Fq::random(OsRng);
        let raw_bytes = [0xffu8; 32];

        let encode = |v: IrValue| -> Vec<transient_crypto::curve::Fr> {
            encode_offcircuit(&v)
                .into_iter()
                .map(|x| x.try_into().unwrap())
                .collect()
        };

        let inputs: Vec<transient_crypto::curve::Fr> = [
            encode(IrValue::Native(native_val)),
            encode(IrValue::Secp256k1Base(base_val)),
            encode(IrValue::Secp256k1Scalar(scalar_val)),
            encode(IrValue::Bytes32(raw_bytes)),
        ]
        .concat();

        let raw_native_exp = from_bytes32_offcircuit(&IrType::Native, &raw_bytes).unwrap();
        let raw_base_exp = from_bytes32_offcircuit(&IrType::Secp256k1Base, &raw_bytes).unwrap();
        let raw_scalar_exp = from_bytes32_offcircuit(&IrType::Secp256k1Scalar, &raw_bytes).unwrap();

        let private_transcript: Vec<transient_crypto::curve::Fr> = [
            encode(raw_native_exp),
            encode(raw_base_exp),
            encode(raw_scalar_exp),
        ]
        .concat();

        let (pk, vk) = ir.keygen(&TestParams).await.unwrap();
        let preimage = ProofPreimage {
            binding_input: 42.into(),
            communications_commitment: None,
            inputs,
            private_transcript,
            public_transcript_inputs: vec![],
            public_transcript_outputs: vec![],
            key_location: KeyLocation(Cow::Borrowed("builtin")),
        };
        let (proof, _) = preimage
            .prove::<IrSource>(
                &mut ChaCha20Rng::from_seed([42; 32]),
                &TestParams,
                &TestResolver {
                    pk: pk.clone(),
                    vk: vk.clone(),
                    ir: ir.clone(),
                },
            )
            .await
            .unwrap();
        vk.verify(&PARAMS_VERIFIER, &proof, [42.into()].into_iter())
            .unwrap();
    }

    #[actix_rt::test]
    async fn test_bytes32_low_high_proof() {
        // Exercises bytes32_into_low_high / bytes32_from_low_high:
        //   1. Splits a Bytes32 into its low (first 31 bytes) and high (byte 31) native
        //      field elements and checks each against the off-circuit reference.
        //   2. Reconstructs the original Bytes32 via bytes32_from_low_high and checks
        //      equality with the original value, exercising the full roundtrip.
        use midnight_zkir_v3::ir_instructions::from_bytes32::from_bytes32_offcircuit;
        use midnight_zkir_v3::ir_types::IrType;

        let ir_raw = r#"{
           "version": { "major": 3, "minor": 0 },
           "inputs": [
              { "name": "%b", "type": "Bytes<32>" }
           ],
           "outputs": [],
           "do_communications_commitment": false,
           "instructions": [
               { "op": "bytes32_into_low_high", "bytes": "%b", "outputs": ["%lo", "%hi"] },
               { "op": "bytes32_from_low_high", "inputs": ["%lo", "%hi"], "output": "%b_back" },
               { "op": "constrain_eq", "a": "%b_back", "b": "%b" },
               { "op": "private_input", "type": "Scalar<BLS12-381>", "guard": null, "output": "%lo_exp" },
               { "op": "private_input", "type": "Scalar<BLS12-381>", "guard": null, "output": "%hi_exp" },
               { "op": "constrain_eq", "a": "%lo", "b": "%lo_exp" },
               { "op": "constrain_eq", "a": "%hi", "b": "%hi_exp" }
           ]
        }"#;
        let ir = IrSource::load(ir_raw.as_bytes()).unwrap();

        // bytes with a non-zero MSB (byte 31 == 32) to exercise the high part.
        let bytes: [u8; 32] = std::array::from_fn(|i| (i + 1) as u8);

        let encode = |v: IrValue| -> Vec<transient_crypto::curve::Fr> {
            encode_offcircuit(&v)
                .into_iter()
                .map(|x| x.try_into().unwrap())
                .collect()
        };

        let inputs: Vec<transient_crypto::curve::Fr> = encode(IrValue::Bytes32(bytes));

        // Compute expected lo and hi using the same logic as the off-circuit VM.
        let mut lo_bytes = bytes;
        lo_bytes[31] = 0;
        let lo_exp = from_bytes32_offcircuit(&IrType::Native, &lo_bytes).unwrap();
        let mut hi_bytes = [0u8; 32];
        hi_bytes[0] = bytes[31];
        let hi_exp = from_bytes32_offcircuit(&IrType::Native, &hi_bytes).unwrap();

        let private_transcript: Vec<transient_crypto::curve::Fr> =
            [encode(lo_exp), encode(hi_exp)].concat();

        let (pk, vk) = ir.keygen(&TestParams).await.unwrap();
        let preimage = ProofPreimage {
            binding_input: 42.into(),
            communications_commitment: None,
            inputs,
            private_transcript,
            public_transcript_inputs: vec![],
            public_transcript_outputs: vec![],
            key_location: KeyLocation(Cow::Borrowed("builtin")),
        };
        let (proof, _) = preimage
            .prove::<IrSource>(
                &mut ChaCha20Rng::from_seed([42; 32]),
                &TestParams,
                &TestResolver {
                    pk: pk.clone(),
                    vk: vk.clone(),
                    ir: ir.clone(),
                },
            )
            .await
            .unwrap();
        vk.verify(&PARAMS_VERIFIER, &proof, [42.into()].into_iter())
            .unwrap();
    }

    #[actix_rt::test]
    async fn test_reverse_bytes_proof() {
        // Exercises reverse_bytes:
        //   1. Reverses the byte order of a Bytes32 and checks the result against
        //      an off-circuit reference computed by reversing the input bytes.
        //   2. Reverses the reversed value again and checks it equals the
        //      original, exercising the involutive round-trip.
        let ir_raw = r#"{
           "version": { "major": 3, "minor": 0 },
           "inputs": [
              { "name": "%b", "type": "Bytes<32>" }
           ],
           "outputs": [],
           "do_communications_commitment": false,
           "instructions": [
               { "op": "reverse_bytes", "bytes": "%b",   "output": "%rev"      },
               { "op": "reverse_bytes", "bytes": "%rev", "output": "%rev_rev"  },
               { "op": "constrain_eq", "a": "%rev_rev", "b": "%b" },
               { "op": "private_input", "type": "Bytes<32>", "guard": null, "output": "%rev_exp" },
               { "op": "constrain_eq", "a": "%rev", "b": "%rev_exp" }
           ]
        }"#;
        let ir = IrSource::load(ir_raw.as_bytes()).unwrap();

        let bytes: [u8; 32] = std::array::from_fn(|i| (i + 1) as u8);
        let mut rev_bytes = bytes;
        rev_bytes.reverse();

        let encode = |v: IrValue| -> Vec<transient_crypto::curve::Fr> {
            encode_offcircuit(&v)
                .into_iter()
                .map(|x| x.try_into().unwrap())
                .collect()
        };

        let inputs: Vec<transient_crypto::curve::Fr> = encode(IrValue::Bytes32(bytes));
        let private_transcript: Vec<transient_crypto::curve::Fr> =
            encode(IrValue::Bytes32(rev_bytes));

        let (pk, vk) = ir.keygen(&TestParams).await.unwrap();
        let preimage = ProofPreimage {
            binding_input: 42.into(),
            communications_commitment: None,
            inputs,
            private_transcript,
            public_transcript_inputs: vec![],
            public_transcript_outputs: vec![],
            key_location: KeyLocation(Cow::Borrowed("builtin")),
        };
        let (proof, _) = preimage
            .prove::<IrSource>(
                &mut ChaCha20Rng::from_seed([42; 32]),
                &TestParams,
                &TestResolver {
                    pk: pk.clone(),
                    vk: vk.clone(),
                    ir: ir.clone(),
                },
            )
            .await
            .unwrap();
        vk.verify(&PARAMS_VERIFIER, &proof, [42.into()].into_iter())
            .unwrap();
    }

    #[actix_rt::test]
    async fn test_native_inv_proof() {
        // Verifies native field inversion: v0 * inv(v0) == 1.
        let ir_raw = r#"{
           "version": { "major": 3, "minor": 0 },
           "inputs": [
              { "name": "%v0", "type": "Scalar<BLS12-381>" }
           ],
           "outputs": [],
           "do_communications_commitment": false,
           "instructions": [
               { "op": "inv", "a": "%v0",          "output": "%v0_inv" },
               { "op": "mul", "a": "%v0", "b": "%v0_inv", "output": "%one"   },
               { "op": "constrain_eq", "a": "%one", "b": "0x01" }
           ]
        }"#;
        let ir = IrSource::load(ir_raw.as_bytes()).unwrap();
        let (pk, vk) = ir.keygen(&TestParams).await.unwrap();

        let preimage = ProofPreimage {
            binding_input: 42.into(),
            communications_commitment: None,
            inputs: vec![7.into()],
            private_transcript: vec![],
            public_transcript_inputs: vec![],
            public_transcript_outputs: vec![],
            key_location: KeyLocation(Cow::Borrowed("builtin")),
        };
        let (proof, _) = preimage
            .prove::<IrSource>(
                &mut ChaCha20Rng::from_seed([42; 32]),
                &TestParams,
                &TestResolver {
                    pk,
                    vk: vk.clone(),
                    ir,
                },
            )
            .await
            .unwrap();
        vk.verify(&PARAMS_VERIFIER, &proof, [42.into()].into_iter())
            .unwrap();
    }

    #[actix_rt::test]
    async fn test_secp256r1_proof() {
        // Single circuit exercising all three Secp256r1 types.
        // Base, Scalar and Point values are typed inputs; arithmetic results
        // are checked via private inputs.
        let ir_raw = r#"{
           "version": { "major": 3, "minor": 0 },
           "inputs": [
              { "name": "%id", "type": "Point<Secp256r1>"  },
              { "name": "%p0", "type": "Point<Secp256r1>"  },
              { "name": "%p1", "type": "Point<Secp256r1>"  },
              { "name": "%b0", "type": "Base<Secp256r1>"   },
              { "name": "%b1", "type": "Base<Secp256r1>"   },
              { "name": "%s0", "type": "Scalar<Secp256r1>" },
              { "name": "%s1", "type": "Scalar<Secp256r1>" }
           ],
           "outputs": [],
           "do_communications_commitment": false,
           "instructions": [
               { "op": "add", "a": "%p0", "b": "%p1", "output": "%p2" },
               { "op": "add", "a": "%b0", "b": "%b1", "output": "%b2" },
               { "op": "add", "a": "%s0", "b": "%s1", "output": "%s2" },
               { "op": "mul", "a": "%b0", "b": "%b1", "output": "%b_prod" },
               { "op": "mul", "a": "%s0", "b": "%s1", "output": "%s_prod" },
               { "op": "neg", "a": "%p0", "output": "%p0_neg" },
               { "op": "neg", "a": "%b0", "output": "%b0_neg" },
               { "op": "neg", "a": "%s0", "output": "%s0_neg" },
               { "op": "inv", "a": "%b0", "output": "%b0_inv" },
               { "op": "inv", "a": "%s0", "output": "%s0_inv" },
               { "op": "private_input", "type": "Point<Secp256r1>",  "guard": null, "output": "%p2_priv"     },
               { "op": "private_input", "type": "Base<Secp256r1>",   "guard": null, "output": "%b_prod_priv" },
               { "op": "private_input", "type": "Scalar<Secp256r1>", "guard": null, "output": "%s_prod_priv" },
               { "op": "private_input", "type": "Point<Secp256r1>",  "guard": null, "output": "%p0_neg_priv" },
               { "op": "private_input", "type": "Base<Secp256r1>",   "guard": null, "output": "%b0_neg_priv" },
               { "op": "private_input", "type": "Scalar<Secp256r1>", "guard": null, "output": "%s0_neg_priv" },
               { "op": "private_input", "type": "Base<Secp256r1>",   "guard": null, "output": "%b0_inv_priv" },
               { "op": "private_input", "type": "Scalar<Secp256r1>", "guard": null, "output": "%s0_inv_priv" },
               { "op": "constrain_eq", "a": "%p2",     "b": "%p2_priv"     },
               { "op": "constrain_eq", "a": "%b_prod", "b": "%b_prod_priv" },
               { "op": "constrain_eq", "a": "%p0_neg", "b": "%p0_neg_priv" },
               { "op": "constrain_eq", "a": "%b0_neg", "b": "%b0_neg_priv" },
               { "op": "test_eq",      "a": "%s_prod", "b": "%s_prod_priv", "output": "%sp_eq" },
               { "op": "assert",       "cond": "%sp_eq" },
               { "op": "test_eq",      "a": "%s0_neg", "b": "%s0_neg_priv", "output": "%sn_eq" },
               { "op": "assert",       "cond": "%sn_eq" },
               { "op": "constrain_eq", "a": "%b0_inv", "b": "%b0_inv_priv" },
               { "op": "test_eq",      "a": "%s0_inv", "b": "%s0_inv_priv", "output": "%si_eq" },
               { "op": "assert",       "cond": "%si_eq" }
           ]
        }"#;
        let ir = IrSource::load(ir_raw.as_bytes()).unwrap();

        let id = p256::P256::identity();
        let p0 = p256::P256::random(OsRng);
        let p1 = p256::P256::random(OsRng);
        let b0 = p256::Fp::random(OsRng);
        let b1 = p256::Fp::random(OsRng);
        let s0 = p256::Fq::random(OsRng);
        let s1 = p256::Fq::random(OsRng);

        let encode = |v: IrValue| -> Vec<transient_crypto::curve::Fr> {
            encode_offcircuit(&v)
                .into_iter()
                .map(|x| x.try_into().unwrap())
                .collect()
        };

        let inputs: Vec<transient_crypto::curve::Fr> = [
            encode(IrValue::Secp256r1Point(id)),
            encode(IrValue::Secp256r1Point(p0)),
            encode(IrValue::Secp256r1Point(p1)),
            encode(IrValue::Secp256r1Base(b0)),
            encode(IrValue::Secp256r1Base(b1)),
            encode(IrValue::Secp256r1Scalar(s0)),
            encode(IrValue::Secp256r1Scalar(s1)),
        ]
        .concat();

        let private_transcript: Vec<transient_crypto::curve::Fr> = [
            encode(IrValue::Secp256r1Point(p0 + p1)),
            encode(IrValue::Secp256r1Base(b0 * b1)),
            encode(IrValue::Secp256r1Scalar(s0 * s1)),
            encode(IrValue::Secp256r1Point(-p0)),
            encode(IrValue::Secp256r1Base(-b0)),
            encode(IrValue::Secp256r1Scalar(-s0)),
            encode(IrValue::Secp256r1Base(Option::from(b0.invert()).unwrap())),
            encode(IrValue::Secp256r1Scalar(Option::from(s0.invert()).unwrap())),
        ]
        .concat();

        let (pk, vk) = ir.keygen(&TestParams).await.unwrap();
        let preimage = ProofPreimage {
            binding_input: 42.into(),
            communications_commitment: None,
            inputs,
            private_transcript,
            public_transcript_inputs: vec![],
            public_transcript_outputs: vec![],
            key_location: KeyLocation(Cow::Borrowed("builtin")),
        };
        let (proof, _) = preimage
            .prove::<IrSource>(
                &mut ChaCha20Rng::from_seed([42; 32]),
                &TestParams,
                &TestResolver {
                    pk: pk.clone(),
                    vk: vk.clone(),
                    ir: ir.clone(),
                },
            )
            .await
            .unwrap();
        vk.verify(&PARAMS_VERIFIER, &proof, [42.into()].into_iter())
            .unwrap();
    }

    #[actix_rt::test]
    async fn test_secp256r1_ec_mul_proof() {
        // Proves p0 * s0 via in-circuit ec_mul on Point<Secp256r1>; the result is
        // checked against a private input carrying the off-circuit product.
        let ir_raw = r#"{
           "version": { "major": 3, "minor": 0 },
           "inputs": [
              { "name": "%p0", "type": "Point<Secp256r1>"  },
              { "name": "%s0", "type": "Scalar<Secp256r1>" }
           ],
           "outputs": [],
           "do_communications_commitment": false,
           "instructions": [
               { "op": "ec_mul", "a": "%p0", "scalar": "%s0", "output": "%p1" },
               { "op": "private_input", "type": "Point<Secp256r1>", "guard": null, "output": "%p1_priv" },
               { "op": "constrain_eq", "a": "%p1", "b": "%p1_priv" }
           ]
        }"#;
        let ir = IrSource::load(ir_raw.as_bytes()).unwrap();

        let p0 = p256::P256::random(OsRng);
        let s0 = p256::Fq::random(OsRng);

        let encode = |v: IrValue| -> Vec<transient_crypto::curve::Fr> {
            encode_offcircuit(&v)
                .into_iter()
                .map(|x| x.try_into().unwrap())
                .collect()
        };

        let inputs: Vec<transient_crypto::curve::Fr> = [
            encode(IrValue::Secp256r1Point(p0)),
            encode(IrValue::Secp256r1Scalar(s0)),
        ]
        .concat();

        let private_transcript: Vec<transient_crypto::curve::Fr> =
            encode(IrValue::Secp256r1Point(p0 * s0));

        let (pk, vk) = ir.keygen(&TestParams).await.unwrap();
        let preimage = ProofPreimage {
            binding_input: 42.into(),
            communications_commitment: None,
            inputs,
            private_transcript,
            public_transcript_inputs: vec![],
            public_transcript_outputs: vec![],
            key_location: KeyLocation(Cow::Borrowed("builtin")),
        };
        let (proof, _) = preimage
            .prove::<IrSource>(
                &mut ChaCha20Rng::from_seed([42; 32]),
                &TestParams,
                &TestResolver {
                    pk: pk.clone(),
                    vk: vk.clone(),
                    ir: ir.clone(),
                },
            )
            .await
            .unwrap();
        vk.verify(&PARAMS_VERIFIER, &proof, [42.into()].into_iter())
            .unwrap();
    }

    #[actix_rt::test]
    async fn test_secp256r1_coordinates_proof() {
        // Secp256r1 counterpart of test_coordinates_proof: extracts affine
        // coordinates of a Point<Secp256r1>, checks them against private inputs
        // carrying the expected values, then reconstructs the point from the
        // extracted coordinates and compares it to the original.
        use midnight_zkir_v3::ir_instructions::into_coordinates::into_coordinates_offcircuit;

        let ir_raw = r#"{
           "version": { "major": 3, "minor": 0 },
           "inputs": [
              { "name": "%pp", "type": "Point<Secp256r1>" }
           ],
           "outputs": [],
           "do_communications_commitment": false,
           "instructions": [
               { "op": "into_coordinates", "point": "%pp", "outputs": ["%px", "%py"] },
               { "op": "private_input", "type": "Base<Secp256r1>", "guard": null, "output": "%px_exp" },
               { "op": "private_input", "type": "Base<Secp256r1>", "guard": null, "output": "%py_exp" },
               { "op": "constrain_eq", "a": "%px", "b": "%px_exp" },
               { "op": "constrain_eq", "a": "%py", "b": "%py_exp" },
               { "op": "from_coordinates", "inputs": ["%px", "%py"], "output": "%pp_reconstructed" },
               { "op": "constrain_eq", "a": "%pp_reconstructed", "b": "%pp" }
           ]
        }"#;
        let ir = IrSource::load(ir_raw.as_bytes()).unwrap();

        let pp = p256::P256::random(OsRng);

        let encode = |v: IrValue| -> Vec<transient_crypto::curve::Fr> {
            encode_offcircuit(&v)
                .into_iter()
                .map(|x| x.try_into().unwrap())
                .collect()
        };

        let inputs: Vec<transient_crypto::curve::Fr> = encode(IrValue::Secp256r1Point(pp));

        let (px, py) = into_coordinates_offcircuit(&IrValue::Secp256r1Point(pp)).unwrap();

        let private_transcript: Vec<transient_crypto::curve::Fr> =
            [encode(px), encode(py)].concat();

        let (pk, vk) = ir.keygen(&TestParams).await.unwrap();
        let preimage = ProofPreimage {
            binding_input: 42.into(),
            communications_commitment: None,
            inputs,
            private_transcript,
            public_transcript_inputs: vec![],
            public_transcript_outputs: vec![],
            key_location: KeyLocation(Cow::Borrowed("builtin")),
        };
        let (proof, _) = preimage
            .prove::<IrSource>(
                &mut ChaCha20Rng::from_seed([42; 32]),
                &TestParams,
                &TestResolver {
                    pk: pk.clone(),
                    vk: vk.clone(),
                    ir: ir.clone(),
                },
            )
            .await
            .unwrap();
        vk.verify(&PARAMS_VERIFIER, &proof, [42.into()].into_iter())
            .unwrap();
    }

    #[actix_rt::test]
    async fn test_secp256r1_bytes32_proof() {
        // Secp256r1 counterpart of test_bytes32_proof, exercising into_bytes32 /
        // from_bytes32 on Secp256r1Base and Secp256r1Scalar:
        //   1. Round-trips a typed value through into_bytes32 then
        //      from_bytes32 and checks it matches the original.
        //   2. Converts a fixed, non-canonical 32-byte string (all 0xff,
        //      which exceeds both fields' moduli) via from_bytes32 and checks
        //      the in-circuit result against the off-circuit reference
        //      implementation, exercising the modular-reduction behavior
        //      documented on the instruction.
        use midnight_zkir_v3::ir_instructions::from_bytes32::from_bytes32_offcircuit;
        use midnight_zkir_v3::ir_types::IrType;

        let ir_raw = r#"{
           "version": { "major": 3, "minor": 0 },
           "inputs": [
              { "name": "%secp256r1_base",   "type": "Base<Secp256r1>"   },
              { "name": "%secp256r1_scalar", "type": "Scalar<Secp256r1>" },
              { "name": "%raw",         "type": "Bytes<32>"    }
           ],
           "outputs": [],
           "do_communications_commitment": false,
           "instructions": [
               { "op": "into_bytes32", "input": "%secp256r1_base",   "output": "%base_bytes"   },
               { "op": "into_bytes32", "input": "%secp256r1_scalar", "output": "%scalar_bytes" },

               { "op": "from_bytes32", "bytes": "%base_bytes",   "type": "Base<Secp256r1>",   "output": "%base_back"   },
               { "op": "from_bytes32", "bytes": "%scalar_bytes", "type": "Scalar<Secp256r1>", "output": "%scalar_back" },

               { "op": "constrain_eq", "a": "%base_back",   "b": "%secp256r1_base"   },
               { "op": "constrain_eq", "a": "%scalar_back", "b": "%secp256r1_scalar" },

               { "op": "from_bytes32", "bytes": "%raw", "type": "Base<Secp256r1>",   "output": "%raw_base"   },
               { "op": "from_bytes32", "bytes": "%raw", "type": "Scalar<Secp256r1>", "output": "%raw_scalar" },

               { "op": "private_input", "type": "Base<Secp256r1>",   "guard": null, "output": "%raw_base_exp"   },
               { "op": "private_input", "type": "Scalar<Secp256r1>", "guard": null, "output": "%raw_scalar_exp" },

               { "op": "constrain_eq", "a": "%raw_base",   "b": "%raw_base_exp"   },
               { "op": "constrain_eq", "a": "%raw_scalar", "b": "%raw_scalar_exp" }
           ]
        }"#;
        let ir = IrSource::load(ir_raw.as_bytes()).unwrap();

        let base_val = p256::Fp::random(OsRng);
        let scalar_val = p256::Fq::random(OsRng);
        let raw_bytes = [0xffu8; 32];

        let encode = |v: IrValue| -> Vec<transient_crypto::curve::Fr> {
            encode_offcircuit(&v)
                .into_iter()
                .map(|x| x.try_into().unwrap())
                .collect()
        };

        let inputs: Vec<transient_crypto::curve::Fr> = [
            encode(IrValue::Secp256r1Base(base_val)),
            encode(IrValue::Secp256r1Scalar(scalar_val)),
            encode(IrValue::Bytes32(raw_bytes)),
        ]
        .concat();

        let raw_base_exp = from_bytes32_offcircuit(&IrType::Secp256r1Base, &raw_bytes).unwrap();
        let raw_scalar_exp = from_bytes32_offcircuit(&IrType::Secp256r1Scalar, &raw_bytes).unwrap();

        let private_transcript: Vec<transient_crypto::curve::Fr> =
            [encode(raw_base_exp), encode(raw_scalar_exp)].concat();

        let (pk, vk) = ir.keygen(&TestParams).await.unwrap();
        let preimage = ProofPreimage {
            binding_input: 42.into(),
            communications_commitment: None,
            inputs,
            private_transcript,
            public_transcript_inputs: vec![],
            public_transcript_outputs: vec![],
            key_location: KeyLocation(Cow::Borrowed("builtin")),
        };
        let (proof, _) = preimage
            .prove::<IrSource>(
                &mut ChaCha20Rng::from_seed([42; 32]),
                &TestParams,
                &TestResolver {
                    pk: pk.clone(),
                    vk: vk.clone(),
                    ir: ir.clone(),
                },
            )
            .await
            .unwrap();
        vk.verify(&PARAMS_VERIFIER, &proof, [42.into()].into_iter())
            .unwrap();
    }

    #[actix_rt::test]
    async fn test_secp256r1_point_constrain_eq_fails_on_unequal() {
        let ir_raw = r#"{
           "version": { "major": 3, "minor": 0 },
           "inputs": [
              { "name": "%p0", "type": "Point<Secp256r1>" },
              { "name": "%p1", "type": "Point<Secp256r1>" }
           ],
           "outputs": [
           ],
           "do_communications_commitment": false,
           "instructions": [
               { "op": "constrain_eq", "a": "%p0", "b": "%p1" }
           ]
        }"#;
        let ir = IrSource::load(ir_raw.as_bytes()).unwrap();

        let (pk, vk) = ir.keygen(&TestParams).await.unwrap();

        // Different points: constrain_eq should fail
        let p = p256::P256::random(OsRng);
        let q = p256::P256::random(OsRng);
        assert_ne!(p, q);

        let encode = |v: IrValue| -> Vec<transient_crypto::curve::Fr> {
            encode_offcircuit(&v)
                .into_iter()
                .map(|x| x.try_into().unwrap())
                .collect()
        };

        let preimage_fail = ProofPreimage {
            binding_input: 42.into(),
            communications_commitment: None,
            inputs: [
                encode(IrValue::Secp256r1Point(p)),
                encode(IrValue::Secp256r1Point(q)),
            ]
            .concat(),
            private_transcript: vec![],
            public_transcript_inputs: vec![],
            public_transcript_outputs: vec![],
            key_location: KeyLocation(Cow::Borrowed("builtin")),
        };
        let result = preimage_fail
            .prove::<IrSource>(
                &mut ChaCha20Rng::from_seed([42; 32]),
                &TestParams,
                &TestResolver {
                    pk: pk.clone(),
                    vk: vk.clone(),
                    ir: ir.clone(),
                },
            )
            .await;
        assert!(
            result.is_err(),
            "constrain_eq on different Secp256r1 points should fail"
        );
    }

    #[actix_rt::test]
    async fn test_curve25519_proof() {
        // Single circuit exercising all three Curve25519 types.
        // Base, Scalar and Point values are typed inputs; arithmetic results
        // are checked via private inputs.
        let ir_raw = r#"{
           "version": { "major": 3, "minor": 0 },
           "inputs": [
              { "name": "%id", "type": "Point<Curve25519>"  },
              { "name": "%p0", "type": "Point<Curve25519>"  },
              { "name": "%p1", "type": "Point<Curve25519>"  },
              { "name": "%b0", "type": "Base<Curve25519>"   },
              { "name": "%b1", "type": "Base<Curve25519>"   },
              { "name": "%s0", "type": "Scalar<Curve25519>" },
              { "name": "%s1", "type": "Scalar<Curve25519>" }
           ],
           "outputs": [],
           "do_communications_commitment": false,
           "instructions": [
               { "op": "add", "a": "%p0", "b": "%p1", "output": "%p2" },
               { "op": "add", "a": "%b0", "b": "%b1", "output": "%b2" },
               { "op": "add", "a": "%s0", "b": "%s1", "output": "%s2" },
               { "op": "mul", "a": "%b0", "b": "%b1", "output": "%b_prod" },
               { "op": "mul", "a": "%s0", "b": "%s1", "output": "%s_prod" },
               { "op": "neg", "a": "%p0", "output": "%p0_neg" },
               { "op": "neg", "a": "%b0", "output": "%b0_neg" },
               { "op": "neg", "a": "%s0", "output": "%s0_neg" },
               { "op": "inv", "a": "%b0", "output": "%b0_inv" },
               { "op": "inv", "a": "%s0", "output": "%s0_inv" },
               { "op": "private_input", "type": "Point<Curve25519>",  "guard": null, "output": "%p2_priv"     },
               { "op": "private_input", "type": "Base<Curve25519>",   "guard": null, "output": "%b_prod_priv" },
               { "op": "private_input", "type": "Scalar<Curve25519>", "guard": null, "output": "%s_prod_priv" },
               { "op": "private_input", "type": "Point<Curve25519>",  "guard": null, "output": "%p0_neg_priv" },
               { "op": "private_input", "type": "Base<Curve25519>",   "guard": null, "output": "%b0_neg_priv" },
               { "op": "private_input", "type": "Scalar<Curve25519>", "guard": null, "output": "%s0_neg_priv" },
               { "op": "private_input", "type": "Base<Curve25519>",   "guard": null, "output": "%b0_inv_priv" },
               { "op": "private_input", "type": "Scalar<Curve25519>", "guard": null, "output": "%s0_inv_priv" },
               { "op": "constrain_eq", "a": "%p2",     "b": "%p2_priv"     },
               { "op": "constrain_eq", "a": "%b_prod", "b": "%b_prod_priv" },
               { "op": "constrain_eq", "a": "%p0_neg", "b": "%p0_neg_priv" },
               { "op": "constrain_eq", "a": "%b0_neg", "b": "%b0_neg_priv" },
               { "op": "test_eq",      "a": "%s_prod", "b": "%s_prod_priv", "output": "%sp_eq" },
               { "op": "assert",       "cond": "%sp_eq" },
               { "op": "test_eq",      "a": "%s0_neg", "b": "%s0_neg_priv", "output": "%sn_eq" },
               { "op": "assert",       "cond": "%sn_eq" },
               { "op": "constrain_eq", "a": "%b0_inv", "b": "%b0_inv_priv" },
               { "op": "test_eq",      "a": "%s0_inv", "b": "%s0_inv_priv", "output": "%si_eq" },
               { "op": "assert",       "cond": "%si_eq" }
           ]
        }"#;
        let ir = IrSource::load(ir_raw.as_bytes()).unwrap();

        let id = curve25519::Curve25519Subgroup::identity();
        let p0 = curve25519::Curve25519Subgroup::random(OsRng);
        let p1 = curve25519::Curve25519Subgroup::random(OsRng);
        let b0 = curve25519::Fp::random(OsRng);
        let b1 = curve25519::Fp::random(OsRng);
        let s0 = <curve25519::Scalar as Field>::random(OsRng);
        let s1 = <curve25519::Scalar as Field>::random(OsRng);

        let encode = |v: IrValue| -> Vec<transient_crypto::curve::Fr> {
            encode_offcircuit(&v)
                .into_iter()
                .map(|x| x.try_into().unwrap())
                .collect()
        };

        let inputs: Vec<transient_crypto::curve::Fr> = [
            encode(IrValue::Curve25519Point(id)),
            encode(IrValue::Curve25519Point(p0)),
            encode(IrValue::Curve25519Point(p1)),
            encode(IrValue::Curve25519Base(b0)),
            encode(IrValue::Curve25519Base(b1)),
            encode(IrValue::Curve25519Scalar(s0)),
            encode(IrValue::Curve25519Scalar(s1)),
        ]
        .concat();

        let private_transcript: Vec<transient_crypto::curve::Fr> = [
            encode(IrValue::Curve25519Point(p0 + p1)),
            encode(IrValue::Curve25519Base(b0 * b1)),
            encode(IrValue::Curve25519Scalar(s0 * s1)),
            encode(IrValue::Curve25519Point(-p0)),
            encode(IrValue::Curve25519Base(-b0)),
            encode(IrValue::Curve25519Scalar(-s0)),
            encode(IrValue::Curve25519Base(Option::from(b0.invert()).unwrap())),
            encode(IrValue::Curve25519Scalar(Field::invert(&s0).unwrap())),
        ]
        .concat();

        let (pk, vk) = ir.keygen(&TestParams).await.unwrap();
        let preimage = ProofPreimage {
            binding_input: 42.into(),
            communications_commitment: None,
            inputs,
            private_transcript,
            public_transcript_inputs: vec![],
            public_transcript_outputs: vec![],
            key_location: KeyLocation(Cow::Borrowed("builtin")),
        };
        let (proof, _) = preimage
            .prove::<IrSource>(
                &mut ChaCha20Rng::from_seed([42; 32]),
                &TestParams,
                &TestResolver {
                    pk: pk.clone(),
                    vk: vk.clone(),
                    ir: ir.clone(),
                },
            )
            .await
            .unwrap();
        vk.verify(&PARAMS_VERIFIER, &proof, [42.into()].into_iter())
            .unwrap();
    }

    #[actix_rt::test]
    async fn test_curve25519_ec_mul_proof() {
        // Proves p0 * s0 via in-circuit ec_mul on Point<Curve25519>; the
        // result is checked against a private input carrying the off-circuit
        // product.
        let ir_raw = r#"{
           "version": { "major": 3, "minor": 0 },
           "inputs": [
              { "name": "%p0", "type": "Point<Curve25519>"  },
              { "name": "%s0", "type": "Scalar<Curve25519>" }
           ],
           "outputs": [],
           "do_communications_commitment": false,
           "instructions": [
               { "op": "ec_mul", "a": "%p0", "scalar": "%s0", "output": "%p1" },
               { "op": "private_input", "type": "Point<Curve25519>", "guard": null, "output": "%p1_priv" },
               { "op": "constrain_eq", "a": "%p1", "b": "%p1_priv" }
           ]
        }"#;
        let ir = IrSource::load(ir_raw.as_bytes()).unwrap();

        let p0 = curve25519::Curve25519Subgroup::random(OsRng);
        let s0 = <curve25519::Scalar as Field>::random(OsRng);

        let encode = |v: IrValue| -> Vec<transient_crypto::curve::Fr> {
            encode_offcircuit(&v)
                .into_iter()
                .map(|x| x.try_into().unwrap())
                .collect()
        };

        let inputs: Vec<transient_crypto::curve::Fr> = [
            encode(IrValue::Curve25519Point(p0)),
            encode(IrValue::Curve25519Scalar(s0)),
        ]
        .concat();

        let private_transcript: Vec<transient_crypto::curve::Fr> =
            encode(IrValue::Curve25519Point(p0 * s0));

        let (pk, vk) = ir.keygen(&TestParams).await.unwrap();
        let preimage = ProofPreimage {
            binding_input: 42.into(),
            communications_commitment: None,
            inputs,
            private_transcript,
            public_transcript_inputs: vec![],
            public_transcript_outputs: vec![],
            key_location: KeyLocation(Cow::Borrowed("builtin")),
        };
        let (proof, _) = preimage
            .prove::<IrSource>(
                &mut ChaCha20Rng::from_seed([42; 32]),
                &TestParams,
                &TestResolver {
                    pk: pk.clone(),
                    vk: vk.clone(),
                    ir: ir.clone(),
                },
            )
            .await
            .unwrap();
        vk.verify(&PARAMS_VERIFIER, &proof, [42.into()].into_iter())
            .unwrap();
    }

    #[actix_rt::test]
    async fn test_curve25519_coordinates_proof() {
        // Curve25519 counterpart of test_coordinates_proof: extracts affine
        // coordinates of a Point<Curve25519>, checks them against private
        // inputs carrying the expected values, then reconstructs the point
        // from the extracted coordinates and compares it to the original.
        use midnight_zkir_v3::ir_instructions::into_coordinates::into_coordinates_offcircuit;

        let ir_raw = r#"{
           "version": { "major": 3, "minor": 0 },
           "inputs": [
              { "name": "%pp", "type": "Point<Curve25519>" }
           ],
           "outputs": [],
           "do_communications_commitment": false,
           "instructions": [
               { "op": "into_coordinates", "point": "%pp", "outputs": ["%px", "%py"] },
               { "op": "private_input", "type": "Base<Curve25519>", "guard": null, "output": "%px_exp" },
               { "op": "private_input", "type": "Base<Curve25519>", "guard": null, "output": "%py_exp" },
               { "op": "constrain_eq", "a": "%px", "b": "%px_exp" },
               { "op": "constrain_eq", "a": "%py", "b": "%py_exp" },
               { "op": "from_coordinates", "inputs": ["%px", "%py"], "output": "%pp_reconstructed" },
               { "op": "constrain_eq", "a": "%pp_reconstructed", "b": "%pp" }
           ]
        }"#;
        let ir = IrSource::load(ir_raw.as_bytes()).unwrap();

        let pp = curve25519::Curve25519Subgroup::random(OsRng);

        let encode = |v: IrValue| -> Vec<transient_crypto::curve::Fr> {
            encode_offcircuit(&v)
                .into_iter()
                .map(|x| x.try_into().unwrap())
                .collect()
        };

        let inputs: Vec<transient_crypto::curve::Fr> = encode(IrValue::Curve25519Point(pp));

        let (px, py) = into_coordinates_offcircuit(&IrValue::Curve25519Point(pp)).unwrap();

        let private_transcript: Vec<transient_crypto::curve::Fr> =
            [encode(px), encode(py)].concat();

        let (pk, vk) = ir.keygen(&TestParams).await.unwrap();
        let preimage = ProofPreimage {
            binding_input: 42.into(),
            communications_commitment: None,
            inputs,
            private_transcript,
            public_transcript_inputs: vec![],
            public_transcript_outputs: vec![],
            key_location: KeyLocation(Cow::Borrowed("builtin")),
        };
        let (proof, _) = preimage
            .prove::<IrSource>(
                &mut ChaCha20Rng::from_seed([42; 32]),
                &TestParams,
                &TestResolver {
                    pk: pk.clone(),
                    vk: vk.clone(),
                    ir: ir.clone(),
                },
            )
            .await
            .unwrap();
        vk.verify(&PARAMS_VERIFIER, &proof, [42.into()].into_iter())
            .unwrap();
    }

    #[actix_rt::test]
    async fn test_curve25519_bytes32_proof() {
        // Curve25519 counterpart of test_bytes32_proof, exercising
        // into_bytes32 / from_bytes32 on Curve25519Base and Curve25519Scalar:
        //   1. Round-trips a typed value through into_bytes32 then
        //      from_bytes32 and checks it matches the original.
        //   2. Converts a fixed, non-canonical 32-byte string (all 0xff,
        //      which exceeds both fields' moduli) via from_bytes32 and checks
        //      the in-circuit result against the off-circuit reference
        //      implementation, exercising the modular-reduction behavior
        //      documented on the instruction.
        use midnight_zkir_v3::ir_instructions::from_bytes32::from_bytes32_offcircuit;
        use midnight_zkir_v3::ir_types::IrType;

        let ir_raw = r#"{
           "version": { "major": 3, "minor": 0 },
           "inputs": [
              { "name": "%c25519_base",   "type": "Base<Curve25519>"   },
              { "name": "%c25519_scalar", "type": "Scalar<Curve25519>" },
              { "name": "%raw",           "type": "Bytes<32>"          }
           ],
           "outputs": [],
           "do_communications_commitment": false,
           "instructions": [
               { "op": "into_bytes32", "input": "%c25519_base",   "output": "%base_bytes"   },
               { "op": "into_bytes32", "input": "%c25519_scalar", "output": "%scalar_bytes" },

               { "op": "from_bytes32", "bytes": "%base_bytes",   "type": "Base<Curve25519>",   "output": "%base_back"   },
               { "op": "from_bytes32", "bytes": "%scalar_bytes", "type": "Scalar<Curve25519>", "output": "%scalar_back" },

               { "op": "constrain_eq", "a": "%base_back",   "b": "%c25519_base"   },
               { "op": "constrain_eq", "a": "%scalar_back", "b": "%c25519_scalar" },

               { "op": "from_bytes32", "bytes": "%raw", "type": "Base<Curve25519>",   "output": "%raw_base"   },
               { "op": "from_bytes32", "bytes": "%raw", "type": "Scalar<Curve25519>", "output": "%raw_scalar" },

               { "op": "private_input", "type": "Base<Curve25519>",   "guard": null, "output": "%raw_base_exp"   },
               { "op": "private_input", "type": "Scalar<Curve25519>", "guard": null, "output": "%raw_scalar_exp" },

               { "op": "constrain_eq", "a": "%raw_base",   "b": "%raw_base_exp"   },
               { "op": "constrain_eq", "a": "%raw_scalar", "b": "%raw_scalar_exp" }
           ]
        }"#;
        let ir = IrSource::load(ir_raw.as_bytes()).unwrap();

        let base_val = curve25519::Fp::random(OsRng);
        let scalar_val = <curve25519::Scalar as Field>::random(OsRng);
        let raw_bytes = [0xffu8; 32];

        let encode = |v: IrValue| -> Vec<transient_crypto::curve::Fr> {
            encode_offcircuit(&v)
                .into_iter()
                .map(|x| x.try_into().unwrap())
                .collect()
        };

        let inputs: Vec<transient_crypto::curve::Fr> = [
            encode(IrValue::Curve25519Base(base_val)),
            encode(IrValue::Curve25519Scalar(scalar_val)),
            encode(IrValue::Bytes32(raw_bytes)),
        ]
        .concat();

        let raw_base_exp = from_bytes32_offcircuit(&IrType::Curve25519Base, &raw_bytes).unwrap();
        let raw_scalar_exp =
            from_bytes32_offcircuit(&IrType::Curve25519Scalar, &raw_bytes).unwrap();

        let private_transcript: Vec<transient_crypto::curve::Fr> =
            [encode(raw_base_exp), encode(raw_scalar_exp)].concat();

        let (pk, vk) = ir.keygen(&TestParams).await.unwrap();
        let preimage = ProofPreimage {
            binding_input: 42.into(),
            communications_commitment: None,
            inputs,
            private_transcript,
            public_transcript_inputs: vec![],
            public_transcript_outputs: vec![],
            key_location: KeyLocation(Cow::Borrowed("builtin")),
        };
        let (proof, _) = preimage
            .prove::<IrSource>(
                &mut ChaCha20Rng::from_seed([42; 32]),
                &TestParams,
                &TestResolver {
                    pk: pk.clone(),
                    vk: vk.clone(),
                    ir: ir.clone(),
                },
            )
            .await
            .unwrap();
        vk.verify(&PARAMS_VERIFIER, &proof, [42.into()].into_iter())
            .unwrap();
    }

    #[actix_rt::test]
    async fn test_curve25519_point_constrain_eq_fails_on_unequal() {
        let ir_raw = r#"{
           "version": { "major": 3, "minor": 0 },
           "inputs": [
              { "name": "%p0", "type": "Point<Curve25519>" },
              { "name": "%p1", "type": "Point<Curve25519>" }
           ],
           "outputs": [
           ],
           "do_communications_commitment": false,
           "instructions": [
               { "op": "constrain_eq", "a": "%p0", "b": "%p1" }
           ]
        }"#;
        let ir = IrSource::load(ir_raw.as_bytes()).unwrap();

        let (pk, vk) = ir.keygen(&TestParams).await.unwrap();

        // Different points: constrain_eq should fail
        let p = curve25519::Curve25519Subgroup::random(OsRng);
        let q = curve25519::Curve25519Subgroup::random(OsRng);
        assert_ne!(p, q);

        let encode = |v: IrValue| -> Vec<transient_crypto::curve::Fr> {
            encode_offcircuit(&v)
                .into_iter()
                .map(|x| x.try_into().unwrap())
                .collect()
        };

        let preimage_fail = ProofPreimage {
            binding_input: 42.into(),
            communications_commitment: None,
            inputs: [
                encode(IrValue::Curve25519Point(p)),
                encode(IrValue::Curve25519Point(q)),
            ]
            .concat(),
            private_transcript: vec![],
            public_transcript_inputs: vec![],
            public_transcript_outputs: vec![],
            key_location: KeyLocation(Cow::Borrowed("builtin")),
        };
        let result = preimage_fail
            .prove::<IrSource>(
                &mut ChaCha20Rng::from_seed([42; 32]),
                &TestParams,
                &TestResolver {
                    pk: pk.clone(),
                    vk: vk.clone(),
                    ir: ir.clone(),
                },
            )
            .await;
        assert!(
            result.is_err(),
            "constrain_eq on different Curve25519 points should fail"
        );
    }
}
