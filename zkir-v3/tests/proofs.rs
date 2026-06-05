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
    use midnight_curves::{JubjubSubgroup, secp256k1};
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
        let pk_fmt = format!("{:#?}", &pk);
        let vk_fmt = format!("{:#?}", &vk);
        let pk: ProverKey = Deserializable::deserialize(&mut &pk_data[..], 0).unwrap();
        let vk: VerifierKey = Deserializable::deserialize(&mut &vk_data[..], 0).unwrap();
        pk.init().unwrap();
        vk.init().unwrap();
        dbg!(pk_fmt == format!("{:#?}", &pk));
        dbg!(vk_fmt == format!("{:#?}", &vk));
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
        let pk_fmt = format!("{:#?}", &pk);
        let pk: ProverKey = Deserializable::deserialize(&mut &pk_data[..], 0).unwrap();
        pk.init().unwrap();
        dbg!(pk_fmt == format!("{:#?}", &pk));
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
        let pk_fmt = format!("{:#?}", &pk);
        let pk: ProverKey = Deserializable::deserialize(&mut &pk_data[..], 0).unwrap();
        pk.init().unwrap();
        dbg!(pk_fmt == format!("{:#?}", &pk));
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
               { "op": "persistent_hash", "alignment": [ { "tag": "atom", "value": { "tag": "bytes", "length": 1 } } ], "inputs": ["%v_0"], "outputs": ["%v_1", "%v_2"] },
               { "op": "keccak256", "alignment": [ { "tag": "atom", "value": { "tag": "bytes", "length": 1 } } ], "inputs": ["%v_0"], "outputs": ["%v_3", "%v_4"] }
           ]
        }"#;
        let ir = IrSource::load(ir_raw.as_bytes()).unwrap();

        let (pk, vk) = ir.keygen(&TestParams).await.unwrap();
        let mut pk_data = Vec::new();
        let mut vk_data = Vec::new();
        Serializable::serialize(&pk, &mut pk_data).unwrap();
        Serializable::serialize(&vk, &mut vk_data).unwrap();
        let pk_fmt = format!("{:#?}", &pk);
        let pk: ProverKey = Deserializable::deserialize(&mut &pk_data[..], 0).unwrap();
        pk.init().unwrap();
        dbg!(pk_fmt == format!("{:#?}", &pk));
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
        let pk_fmt = format!("{:#?}", &pk);
        let pk: ProverKey = Deserializable::deserialize(&mut &pk_data[..], 0).unwrap();
        pk.init().unwrap();
        dbg!(pk_fmt == format!("{:#?}", &pk));
        let mut pk_data = Vec::new();
        let mut vk_data = Vec::new();
        Serializable::serialize(&pk, &mut pk_data).unwrap();
        Serializable::serialize(&vk, &mut vk_data).unwrap();
        let pk_fmt = format!("{:#?}", &pk);
        let pk: ProverKey = Deserializable::deserialize(&mut &pk_data[..], 0).unwrap();
        pk.init().unwrap();
        dbg!(pk_fmt == format!("{:#?}", &pk));
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
        let pk_fmt = format!("{:#?}", &pk);
        let vk_fmt = format!("{:#?}", &vk);
        let pk: ProverKey = Deserializable::deserialize(&mut &pk_data[..], 0).unwrap();
        let vk: VerifierKey = Deserializable::deserialize(&mut &vk_data[..], 0).unwrap();
        pk.init().unwrap();
        vk.init().unwrap();
        dbg!(pk_fmt == format!("{:#?}", &pk));
        dbg!(vk_fmt == format!("{:#?}", &vk));
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
        // Base and Scalar values are decoded from native limbs, added, then
        // encoded back; a round-trip decode verifies encode is its inverse.
        // Points are typed inputs; their sum is checked via a private input.
        let ir_raw = r#"{
           "version": { "major": 3, "minor": 0 },
           "inputs": [
              { "name": "%id",   "type": "Point<Secp256k1>"  },
              { "name": "%p0",   "type": "Point<Secp256k1>"  },
              { "name": "%p1",   "type": "Point<Secp256k1>"  },
              { "name": "%b0_0", "type": "Scalar<BLS12-381>" },
              { "name": "%b0_1", "type": "Scalar<BLS12-381>" },
              { "name": "%b0_2", "type": "Scalar<BLS12-381>" },
              { "name": "%b0_3", "type": "Scalar<BLS12-381>" },
              { "name": "%b1_0", "type": "Scalar<BLS12-381>" },
              { "name": "%b1_1", "type": "Scalar<BLS12-381>" },
              { "name": "%b1_2", "type": "Scalar<BLS12-381>" },
              { "name": "%b1_3", "type": "Scalar<BLS12-381>" },
              { "name": "%s0_0", "type": "Scalar<BLS12-381>" },
              { "name": "%s0_1", "type": "Scalar<BLS12-381>" },
              { "name": "%s0_2", "type": "Scalar<BLS12-381>" },
              { "name": "%s0_3", "type": "Scalar<BLS12-381>" },
              { "name": "%s1_0", "type": "Scalar<BLS12-381>" },
              { "name": "%s1_1", "type": "Scalar<BLS12-381>" },
              { "name": "%s1_2", "type": "Scalar<BLS12-381>" },
              { "name": "%s1_3", "type": "Scalar<BLS12-381>" }
           ],
           "outputs": [],
           "do_communications_commitment": false,
           "instructions": [
               { "op": "decode", "type": "Base<Secp256k1>",   "inputs": ["%b0_0","%b0_1","%b0_2","%b0_3"], "output": "%b0" },
               { "op": "decode", "type": "Base<Secp256k1>",   "inputs": ["%b1_0","%b1_1","%b1_2","%b1_3"], "output": "%b1" },
               { "op": "decode", "type": "Scalar<Secp256k1>", "inputs": ["%s0_0","%s0_1","%s0_2","%s0_3"], "output": "%s0" },
               { "op": "decode", "type": "Scalar<Secp256k1>", "inputs": ["%s1_0","%s1_1","%s1_2","%s1_3"], "output": "%s1" },
               { "op": "add", "a": "%p0", "b": "%p1", "output": "%p2" },
               { "op": "add", "a": "%b0", "b": "%b1", "output": "%b2" },
               { "op": "add", "a": "%s0", "b": "%s1", "output": "%s2" },
               { "op": "mul", "a": "%b0", "b": "%b1", "output": "%b_prod" },
               { "op": "mul", "a": "%s0", "b": "%s1", "output": "%s_prod" },
               { "op": "encode", "input": "%b2", "outputs": ["%b2_0","%b2_1","%b2_2","%b2_3"] },
               { "op": "encode", "input": "%s2", "outputs": ["%s2_0","%s2_1","%s2_2","%s2_3"] },
               { "op": "decode", "type": "Base<Secp256k1>",   "inputs": ["%b2_0","%b2_1","%b2_2","%b2_3"], "output": "%b2_rt" },
               { "op": "decode", "type": "Scalar<Secp256k1>", "inputs": ["%s2_0","%s2_1","%s2_2","%s2_3"], "output": "%s2_rt" },
               { "op": "neg", "a": "%p0",  "output": "%p0_neg" },
               { "op": "neg", "a": "%b0",  "output": "%b0_neg" },
               { "op": "neg", "a": "%s0",  "output": "%s0_neg" },
               { "op": "private_input", "type": "Point<Secp256k1>",  "guard": null, "output": "%p2_priv"    },
               { "op": "private_input", "type": "Base<Secp256k1>",   "guard": null, "output": "%b_prod_priv" },
               { "op": "private_input", "type": "Scalar<Secp256k1>", "guard": null, "output": "%s_prod_priv" },
               { "op": "private_input", "type": "Point<Secp256k1>",  "guard": null, "output": "%p0_neg_priv" },
               { "op": "private_input", "type": "Base<Secp256k1>",   "guard": null, "output": "%b0_neg_priv" },
               { "op": "private_input", "type": "Scalar<Secp256k1>", "guard": null, "output": "%s0_neg_priv" },
               { "op": "constrain_eq", "a": "%p2",     "b": "%p2_priv"     },
               { "op": "constrain_eq", "a": "%b2",     "b": "%b2_rt"       },
               { "op": "constrain_eq", "a": "%b_prod", "b": "%b_prod_priv" },
               { "op": "constrain_eq", "a": "%p0_neg", "b": "%p0_neg_priv" },
               { "op": "constrain_eq", "a": "%b0_neg", "b": "%b0_neg_priv" },
               { "op": "test_eq",      "a": "%s2",     "b": "%s2_rt",    "output": "%s_eq"   },
               { "op": "assert",       "cond": "%s_eq" },
               { "op": "test_eq",      "a": "%s_prod", "b": "%s_prod_priv", "output": "%sp_eq" },
               { "op": "assert",       "cond": "%sp_eq" },
               { "op": "test_eq",      "a": "%s0_neg", "b": "%s0_neg_priv", "output": "%sn_eq" },
               { "op": "assert",       "cond": "%sn_eq" }
           ]
        }"#;
        let ir = IrSource::load(ir_raw.as_bytes()).unwrap();

        let id = secp256k1::Secp256k1::identity();
        let p0 = secp256k1::Secp256k1::random(OsRng);
        let p1 = secp256k1::Secp256k1::random(OsRng);
        let b0 = secp256k1::Fp::random(OsRng);
        let b1 = secp256k1::Fp::random(OsRng);
        let s0 = secp256k1::Fq::random(OsRng);
        let s1 = secp256k1::Fq::random(OsRng);

        let encode = |v: IrValue| -> Vec<transient_crypto::curve::Fr> {
            encode_offcircuit(&v)
                .into_iter()
                .map(|x| x.try_into().unwrap())
                .collect()
        };

        // p0, p1 are typed Point<Secp256k1> inputs (8 limbs each);
        // b0, b1, s0, s1 are passed as raw native limbs (4 each) for decode.
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
}
