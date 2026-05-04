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
    use group::Group;
    use midnight_curves::JubjubSubgroup;
    use midnight_zkir_v3::{Identifier, IrSource, Preprocessed, ir_types::IrValue};
    use rand::{SeedableRng, rngs::OsRng};
    use rand_chacha::ChaCha20Rng;
    #[cfg(feature = "proptest")]
    use serialize::randomised_serialization_test;
    use serialize::{Deserializable, Serializable, tagged_serialize};
    use std::borrow::Cow;
    use std::collections::HashMap;
    use std::fs::File;
    use std::io::BufReader;
    use transient_crypto::curve::EmbeddedGroupAffine;
    use transient_crypto::hash::transient_hash;
    use transient_crypto::proofs::Proof;
    #[cfg(feature = "proptest")]
    use transient_crypto::proofs::{
        KeyLocation, PARAMS_VERIFIER, ParamsProver, ParamsProverProvider, ProofPreimage,
        ProvingKeyMaterial, Resolver, VerifierKey, Zkir,
    };

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
                    contract_call_comm_rands: vec![],
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

    /// Hashes three inputs and binds the result to the public-input stream
    /// via a structured `Impact { Push }` block. The Push materializes a
    /// Field-aligned `Cell` whose operand is `%v_3` (the hash output); the
    /// circuit pushes the canonical `[opcode, Cell-tag, alignment, value]`
    /// Fr sequence to the verifier's PI list.
    #[actix_rt::test]
    async fn test_hash_proof() {
        use transient_crypto::curve::Fr;

        // Hashes %v_0..%v_2 into %v_3, then binds %v_3 to the public-input
        // stream via a structured `Impact { Push }` whose operand is the
        // variable `%v_3`. See `test_immediate_with_public_inputs` for the
        // serde shapes of `Alignment` / `AlignmentSegment` / `AlignmentAtom`.
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
               {
                   "op": "impact",
                   "guard": "0x01",
                   "ops": [
                       {
                           "push": {
                               "storage": false,
                               "value": {
                                   "storage": false,
                                   "alignment": [
                                       { "tag": "atom", "value": { "tag": "field" } }
                                   ],
                                   "operands": ["%v_3"]
                               }
                           }
                       }
                   ],
                   "read_results": []
               }
           ]
        }"#;
        let ir = IrSource::load(ir_raw.as_bytes()).unwrap();

        let x = transient_hash(&[1.into(), 2.into(), 3.into()]);

        // Field-element contribution of the Impact's `Push` of a
        // `Cell([Field], %v_3)`:
        //   0x10 — opcode: `Push { storage: false }`
        //   0x01 — `StateValue::Cell` tag
        //   0x01 — alignment: 1 segment
        //   -2   — alignment atom: `Field` tag
        //   x    — value (= %v_3, resolved at proof time)
        // See `zkir_mode.rs` and `onchain-vm/src/ops.rs::impl FieldRepr for Op`
        // for the canonical encoding; cross-encoder agreement is pinned by
        // `zkir_mode::tests::push_runtime_matches_zkir`.
        let impact_pis: Vec<Fr> = vec![
            Fr::from(0x10u64),
            Fr::from(1u64),
            Fr::from(1u64),
            -Fr::from(2u64),
            x,
        ];

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
            public_transcript_inputs: impact_pis.clone(),
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

        let mut verifier_pis = vec![Fr::from(42u64)];
        verifier_pis.extend(impact_pis);
        vk.verify(&PARAMS_VERIFIER, &proof, verifier_pis.into_iter())
            .unwrap();
    }

    #[actix_rt::test]
    async fn test_persistent_hash_proof() {
        let ir_raw = r#"{
           "version": { "major": 3, "minor": 0 },
           "inputs": [
              { "name": "%v_0", "type": "Scalar<BLS12-381>" }
           ],
           "outputs": [],
           "do_communications_commitment": false,
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
           "inputs": [
              { "name": "%p0", "type": "Point<Jubjub>" },
              { "name": "%s0", "type": "Scalar<BLS12-381>" },
              { "name": "%s1", "type": "Scalar<BLS12-381>" }
           ],
           "outputs": [],
           "do_communications_commitment": false,
           "instructions": [
               { "op": "ec_mul", "a": "%p0", "scalar": "%s0", "output": "%p1" },
               { "op": "ec_mul_generator", "scalar": "%s1", "output": "%p2" },
               { "op": "add", "a": "%p1", "b": "%p2", "output": "%p3" },
               { "op": "private_input", "type": "Point<Jubjub>", "guard": null, "output": "%p4" },
               { "op": "ec_mul", "a": "%p4", "scalar": "%s0", "output": "%p5" }
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
            inputs: vec![p.x().unwrap(), p.y().unwrap(), 42.into(), 63.into()],
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

    /// Drives a circuit that bit-constrains its inputs, performs a
    /// cond_select asserting the picked value is `1`, and then binds the
    /// constant `0x30` (= 48) to the public-input stream via a structured
    /// `Impact { Push }` block whose operand is the immediate.
    #[actix_rt::test]
    async fn test_immediate_with_public_inputs() {
        use transient_crypto::curve::Fr;

        // The Impact contains a single `Push` of a `Cell` whose alignment
        // is one `Field` atom and whose operand is the immediate `0x30`.
        // The serialized shape of the nested `Alignment` /
        // `AlignmentSegment` / `AlignmentAtom` types is:
        //   Alignment        — `#[serde(transparent)]` → `[..segments..]`
        //   AlignmentSegment — `#[serde(tag,content)]` → `{"tag":"atom","value":{..atom..}}`
        //   AlignmentAtom    — `#[serde(tag)]`         → `{"tag":"field"}`
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
               {
                   "op": "impact",
                   "guard": "0x01",
                   "ops": [
                       {
                           "push": {
                               "storage": false,
                               "value": {
                                   "storage": false,
                                   "alignment": [
                                       { "tag": "atom", "value": { "tag": "field" } }
                                   ],
                                   "operands": ["0x30"]
                               }
                           }
                       }
                   ],
                   "read_results": []
               }
           ]
        }"#;
        let ir = IrSource::load(ir_raw.as_bytes()).unwrap();

        // Field-element contribution of the Impact's `Push` of a
        // `Cell([Field], 0x30)`:
        //   0x10 — opcode: `Push { storage: false }`
        //   0x01 — `StateValue::Cell` tag
        //   0x01 — alignment: 1 segment
        //   -2   — alignment atom: `Field` tag
        //   0x30 — value (the immediate)
        // See `zkir_mode.rs` and `onchain-vm/src/ops.rs::impl FieldRepr for Op`
        // for the canonical encoding; cross-encoder agreement is pinned by
        // `zkir_mode::tests::push_runtime_matches_zkir`.
        let impact_pis: Vec<Fr> = vec![
            Fr::from(0x10u64),
            Fr::from(1u64),
            Fr::from(1u64),
            -Fr::from(2u64),
            Fr::from(0x30u64),
        ];

        let (pk, vk) = ir.keygen(&TestParams).await.unwrap();

        let preimage = ProofPreimage {
            binding_input: 48.into(),
            communications_commitment: None,
            inputs: vec![0.into(), 42.into()],
            private_transcript: vec![],
            public_transcript_inputs: impact_pis.clone(),
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

        let mut verifier_pis = vec![Fr::from(48u64)];
        verifier_pis.extend(impact_pis);
        vk.verify(&PARAMS_VERIFIER, &proof, verifier_pis.into_iter())
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

    /// Proves a circuit with an inactive-guard Impact (guard=0).
    ///
    /// The Impact contains a Dup op (opcode 0x30) which would produce a
    /// non-zero field element if active. With guard=0, the circuit must
    /// push zeros via select(0, val, 0), and the preprocessor must also
    /// push zeros to pis. If either side pushes real values instead of
    /// zeros, the pi_push cross-check fires during synthesis.
    ///
    /// The verifier sees a Noop{1} where the Dup would have been.
    #[actix_rt::test]
    async fn test_inactive_guard_impact() {
        use midnight_zkir_v3::{Instruction, Operand, TypedIdentifier, ir_types::IrType};
        use onchain_vm::ops::Op;
        use std::sync::Arc;

        let ir = IrSource {
            inputs: vec![TypedIdentifier::new(
                Identifier("%v_0".to_string()),
                IrType::Native,
            )],
            outputs: vec![],
            do_communications_commitment: false,
            instructions: Arc::new(vec![
                // Inactive Impact: guard=0, so ops don't execute.
                // Dup{0} encodes as [0x30] — one non-zero field element.
                Instruction::Impact {
                    guard: Operand::Immediate(0.into()),
                    ops: vec![Op::Dup { n: 0 }],
                    read_results: vec![],
                },
                // Assert that input is 1 (so the circuit has something to do).
                Instruction::Assert {
                    cond: Operand::Variable(Identifier("%v_0".to_string())),
                },
            ]),
        };

        let (pk, vk) = ir.keygen(&TestParams).await.unwrap();

        // The inactive Impact produces 1 field element (Dup opcode),
        // which appears as a Noop{1} (one zero) in the transcript.
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

        // Verifier PIs: [binding_input, 0 (Noop{1})]
        vk.verify(&PARAMS_VERIFIER, &proof, [42.into(), 0.into()].into_iter())
            .unwrap();
    }
}
