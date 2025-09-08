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
    use std::fs::File;
    use std::io::BufReader;
    use transient_crypto::curve::EmbeddedGroupAffine;
    use transient_crypto::hash::transient_hash;
    #[cfg(feature = "proptest")]
    use transient_crypto::proofs::Proof;
    use transient_crypto::proofs::{
        KeyLocation, PARAMS_VERIFIER, ParamsProver, ParamsProverProvider, ProofPreimage,
        ProvingKeyMaterial, Resolver, VerifierKey, Zkir,
    };
    use zkir::{IrSource, Preprocessed};

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
                "{DIR}/bls_filecoin_2p{k}"
            ))?))
        }
    }

    #[actix_rt::test]
    async fn test_extension_attack() {
        let ir_raw = r#"{
           "version": { "major": 2, "minor": 0 },
           "num_inputs": 1,
           "do_communications_commitment": false,
           "instructions": [
               { "op": "assert", "cond": 0 }
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
                    memory: vec![1.into()],
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
           "version": { "major": 2, "minor": 0 },
           "num_inputs": 1,
           "do_communications_commitment": false,
           "instructions": [
               { "op": "assert", "cond": 0 }
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
           "version": { "major": 2, "minor": 0 },
           "num_inputs": 3,
           "do_communications_commitment": false,
           "instructions": [
               { "op": "hash_to_curve", "inputs": [0, 1, 2] }
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

    #[actix_rt::test]
    async fn test_hash_proof() {
        let ir_raw = r#"{
           "version": { "major": 2, "minor": 0 },
           "num_inputs": 3,
           "do_communications_commitment": false,
           "instructions": [
               { "op": "transient_hash", "inputs": [0, 1, 2] },
               { "op": "declare_pub_input", "var": 3 },
               { "op": "pi_skip", "guard": null, "count": 1}
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
    async fn test_persistent_hash_proof() {
        let ir_raw = r#"{
           "version": { "major": 2, "minor": 0 },
           "num_inputs": 1,
           "do_communications_commitment": false,
           "instructions": [
               { "op": "persistent_hash", "alignment": [ { "tag": "atom", "value": { "tag": "bytes", "length": 1 } } ], "inputs": [0] }
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
           "version": { "major": 2, "minor": 0 },
           "num_inputs": 4,
           "do_communications_commitment": false,
           "instructions": [
               { "op": "ec_mul", "a_x": 0, "a_y": 1, "scalar": 2 },
               { "op": "ec_mul_generator", "scalar": 3 },
               { "op": "ec_add", "a_x": 4, "a_y": 5, "b_x": 6, "b_y": 7 }
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
           "version": { "major": 2, "minor": 0 },
           "num_inputs": 1,
           "do_communications_commitment": false,
           "instructions": [
               { "op": "div_mod_power_of_two", "var": 0, "bits": 3 },
               { "op": "private_input", "guard": null },
               { "op": "private_input", "guard": null },
               { "op": "constrain_eq", "a": 1, "b": 3 },
               { "op": "constrain_eq", "a": 2, "b": 4 },
               { "op": "reconstitute_field", "divisor": 1, "modulus": 2, "bits": 3 },
               { "op": "constrain_eq", "a": 5, "b": 0 }
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
           "version": { "major": 2, "minor": 0 },
           "num_inputs": 1,
           "do_communications_commitment": false,
           "instructions": [
               { "op": "assert", "cond": 0 }
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
}
