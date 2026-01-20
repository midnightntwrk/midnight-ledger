// This file is part of midnight-ledger.
// Copyright (C) 2025 Midnight Foundation
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

#[cfg(test)]
mod proof_tests {
    use rand::SeedableRng;
    use rand_chacha::ChaCha20Rng;
    #[cfg(feature = "proptest")]
    use serialize::randomised_serialization_test;
    use serialize::{Deserializable, Serializable, tagged_serialize};
    use std::borrow::Cow;
    use std::collections::HashMap;
    use std::fs::File;
    use std::io::BufReader;
    use transient_crypto::curve::EmbeddedGroupAffine;
    use transient_crypto::proofs::Proof;
    #[cfg(feature = "proptest")]
    use transient_crypto::proofs::{
        KeyLocation, PARAMS_VERIFIER, ParamsProver, ParamsProverProvider, ProofPreimage,
        ProvingKeyMaterial, Resolver, VerifierKey, Zkir,
    };
    use zkir_v3::{Identifier, IrSource, Preprocessed};

    type ProverKey = transient_crypto::proofs::ProverKey<IrSource>;

    struct TestResolver {
        pk: ProverKey,
        vk: VerifierKey,
        ir: IrSource,
    }

    impl Resolver for TestResolver {
        async fn resolve_key(
            &self,
            _key: KeyLocation,
        ) -> std::io::Result<Option<ProvingKeyMaterial>> {
            let mut pk = Vec::new();
            tagged_serialize(&self.pk, &mut pk)?;
            let mut vk = Vec::new();
            tagged_serialize(&self.vk, &mut vk)?;
            let mut ir = Vec::new();
            tagged_serialize(&self.ir, &mut ir)?;
            Ok(Some(ProvingKeyMaterial {
                prover_key: pk,
                verifier_key: vk,
                ir_source: ir,
            }))
        }
    }

    struct TestParams;

    impl ParamsProverProvider for TestParams {
        async fn get_params(&self, k: u8) -> std::io::Result<ParamsProver> {
            const DIR: &str = env!("MIDNIGHT_PP");
            ParamsProver::read(BufReader::new(File::open(format!(
                "{DIR}/bls_midnight_2p{k}"
            ))?))
        }
    }

    #[actix_rt::test]
    async fn test_extension_attack() {
        let ir_raw = r#"{
           "version": { "major": 3, "minor": 0 },
           "inputs": ["%v_0"],
           "do_communications_commitment": false,
           "arg_alignment": [],
           "return_val_alignment": [],
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
                    memory: HashMap::from([(Identifier("v0".to_string()), 1.into())]),
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
           "inputs": ["%v_0"],
           "do_communications_commitment": false,
           "arg_alignment": [],
           "return_val_alignment": [],
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
           "inputs": ["%v_0", "%v_1", "%v_2"],
           "do_communications_commitment": false,
           "arg_alignment": [],
           "return_val_alignment": [],
           "instructions": [
               { "op": "hash_to_curve", "inputs": ["%v_0", "%v_1", "%v_2"], "outputs": ["%v_3", "%v_4"] }
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

    // Test impact instruction with symbolic ops.
    // Uses simple ops (dup, lt) that exercise the symbolic op encoding without StateValue.
    #[actix_rt::test]
    async fn test_hash_proof() {
        let ir_raw = r#"{
           "version": { "major": 3, "minor": 0 },
           "inputs": ["%v_0", "%v_1", "%v_2"],
           "do_communications_commitment": false,
           "arg_alignment": [],
           "return_val_alignment": [],
           "instructions": [
               { "op": "transient_hash", "inputs": ["%v_0", "%v_1", "%v_2"], "output": "%v_3" },
               { "op": "impact", "guard": "0x01", "ops": [{"dup": {"n": 0}}, "lt"], "field_count": 2 }
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
        // The ops [dup(0), lt] encode to field elements:
        // - dup(0) -> 0x30 (opcode 0x30 | n where n=0)
        // - lt -> 0x01
        let preimage = ProofPreimage {
            binding_input: 42.into(),
            communications_commitment: None,
            inputs: vec![1.into(), 2.into(), 3.into()],
            private_transcript: vec![],
            public_transcript_inputs: vec![0x30.into(), 0x01.into()],
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
        // Verifier receives: binding_input (42) + public_transcript_inputs (0x30, 0x01)
        vk.verify(
            &PARAMS_VERIFIER,
            &proof,
            [42.into(), 0x30.into(), 0x01.into()].into_iter(),
        )
        .unwrap();
    }

    #[actix_rt::test]
    async fn test_persistent_hash_proof() {
        let ir_raw = r#"{
           "version": { "major": 3, "minor": 0 },
           "inputs": ["%v_0"],
           "do_communications_commitment": false,
           "arg_alignment": [],
           "return_val_alignment": [],
           "instructions": [
               { "op": "persistent_hash", "alignment": [ { "tag": "atom", "value": { "tag": "bytes", "length": 1 } } ], "inputs": ["%v_0"], "outputs": ["%v_1", "%v_2"] }
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
           "inputs": ["%v_0", "%v_1", "%v_2", "%v_3"],
           "do_communications_commitment": false,
           "arg_alignment": [],
           "return_val_alignment": [],
           "instructions": [
               { "op": "ec_mul", "a_x": "%v_0", "a_y": "%v_1", "scalar": "%v_2", "outputs": ["%v_4", "%v_5"] },
               { "op": "ec_mul_generator", "scalar": "%v_3", "outputs": ["%v_6", "%v_7"] },
               { "op": "ec_add", "a_x": "%v_4", "a_y": "%v_5", "b_x": "%v_6", "b_y": "%v_7", "outputs": ["%v_8", "%v_9"] }
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
        let preimage = ProofPreimage {
            binding_input: 42.into(),
            communications_commitment: None,
            inputs: vec![p.x().unwrap(), p.y().unwrap(), 42.into(), 63.into()],
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
    async fn test_divmod_proof() {
        let ir_raw = r#"{
           "version": { "major": 3, "minor": 0 },
           "inputs": ["%v_0"],
           "do_communications_commitment": false,
           "arg_alignment": [],
           "return_val_alignment": [],
           "instructions": [
               { "op": "div_mod_power_of_two", "val": "%v_0", "bits": 3, "outputs": ["%v_1", "%v_2"] },
               { "op": "private_input", "guard": null, "output": "%v_3" },
               { "op": "private_input", "guard": null, "output": "%v_4" },
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
           "inputs": ["%v_0"],
           "do_communications_commitment": false,
           "arg_alignment": [],
           "return_val_alignment": [],
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
           "inputs": ["%v_0", "%v_1"],
           "do_communications_commitment": false,
           "arg_alignment": [],
           "return_val_alignment": [],
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
           "inputs": ["%v_0", "%v_1"],
           "do_communications_commitment": false,
           "arg_alignment": [],
           "return_val_alignment": [],
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
           "inputs": ["%v_0"],
           "do_communications_commitment": false,
           "arg_alignment": [],
           "return_val_alignment": [],
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

    // Test impact instruction with symbolic ops and public inputs.
    // Uses dup and addi ops to exercise symbolic op encoding with an immediate value.
    #[actix_rt::test]
    async fn test_immediate_with_public_inputs() {
        let ir_raw = r#"{
           "version": { "major": 3, "minor": 0 },
           "inputs": ["%v_0", "%v_1"],
           "do_communications_commitment": false,
           "arg_alignment": [],
           "return_val_alignment": [],
           "instructions": [
               { "op": "constrain_bits", "val": "%v_0", "bits": 8 },
               { "op": "constrain_bits", "val": "%v_1", "bits": 248 },
               { "op": "cond_select", "bit": "%v_0", "a": "0x00", "b": "0x01", "output": "%v_2" },
               { "op": "assert", "cond": "%v_2" },
               { "op": "impact", "guard": "0x01", "ops": [{"dup": {"n": 0}}, {"addi": {"immediate": 5}}], "field_count": 3 }
           ]
        }"#;
        let ir = IrSource::load(ir_raw.as_bytes()).unwrap();

        let (pk, vk) = ir.keygen(&TestParams).await.unwrap();

        // The ops [dup(0), addi(5)] encode to field elements:
        // - dup(0) -> 0x30 (opcode 0x30 | n where n=0)
        // - addi(5) -> [0x0e, 5] (opcode 0x0e + immediate value)
        let preimage = ProofPreimage {
            binding_input: 48.into(),
            communications_commitment: None,
            inputs: vec![0.into(), 42.into()],
            private_transcript: vec![],
            public_transcript_inputs: vec![0x30.into(), 0x0e.into(), 5.into()],
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
        // Verifier receives: binding_input (48) + public_transcript_inputs (0x30, 0x0e, 5)
        vk.verify(
            &PARAMS_VERIFIER,
            &proof,
            [48.into(), 0x30.into(), 0x0e.into(), 5.into()].into_iter(),
        )
        .unwrap();
    }

    #[actix_rt::test]
    async fn test_immediate_little_endian_encoding() {
        let ir_raw = r#"{
           "version": { "major": 3, "minor": 0 },
           "inputs": ["%v_0"],
           "do_communications_commitment": false,
           "arg_alignment": [],
           "return_val_alignment": [],
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
           "inputs": ["%v_0"],
           "do_communications_commitment": false,
           "arg_alignment": [],
           "return_val_alignment": [],
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

    #[test]
    fn test_invalid_operand_no_percent_prefix() {
        // Variables without '%' prefix should fail to deserialize
        let ir_raw = r#"{
           "version": { "major": 3, "minor": 0 },
           "inputs": ["v_0"],
           "do_communications_commitment": false,
           "arg_alignment": [],
           "return_val_alignment": [],
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
           "inputs": ["%v_0"],
           "do_communications_commitment": false,
           "arg_alignment": [],
           "return_val_alignment": [],
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
           "inputs": ["foo"],
           "do_communications_commitment": false,
           "arg_alignment": [],
           "return_val_alignment": [],
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
}
