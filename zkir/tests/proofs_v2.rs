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

//! Tests for the v2 (zk-stdlib v2) proving and verification flow.

#[cfg(test)]
mod v2_proof_tests {
    use midnight_zkir::IrSource;
    use rand::SeedableRng;
    use rand_chacha::ChaCha20Rng;
    use std::borrow::Cow;
    use std::fs::File;
    use std::io::BufReader;
    use transient_crypto::proofs::{
        KeyLocation, PARAMS_VERIFIER, ParamsProver, ParamsProverProvider, ProofPreimage, Zkir,
    };

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
    async fn v2_prove_and_verify_minimal() {
        let ir_raw = r#"{
           "version": { "major": 2, "minor": 2 },
           "num_inputs": 1,
           "do_communications_commitment": false,
           "instructions": [
               { "op": "assert", "cond": 0 }
           ]
        }"#;
        let ir = IrSource::load(ir_raw.as_bytes()).unwrap();

        let (pk, vk) = ir.v2_keygen(&TestParams).await.unwrap();

        let preimage = ProofPreimage {
            binding_input: 42.into(),
            communications_commitment: None,
            inputs: vec![1.into()],
            private_transcript: vec![],
            public_transcript_inputs: vec![],
            public_transcript_outputs: vec![],
            key_location: KeyLocation(Cow::Borrowed("builtin")),
        };

        let (proof, pis, _skips) = ir
            .prove(
                &mut ChaCha20Rng::from_seed([42; 32]),
                &TestParams,
                pk,
                &preimage,
            )
            .await
            .unwrap();

        vk.verify(&PARAMS_VERIFIER, &proof, pis.into_iter())
            .unwrap();
    }

    #[actix_rt::test]
    async fn v2_prove_wrong_statement_fails() {
        let ir_raw = r#"{
           "version": { "major": 2, "minor": 2 },
           "num_inputs": 1,
           "do_communications_commitment": false,
           "instructions": [
               { "op": "assert", "cond": 0 }
           ]
        }"#;
        let ir = IrSource::load(ir_raw.as_bytes()).unwrap();
        let (pk, vk) = ir.v2_keygen(&TestParams).await.unwrap();

        let preimage = ProofPreimage {
            binding_input: 42.into(),
            communications_commitment: None,
            inputs: vec![1.into()],
            private_transcript: vec![],
            public_transcript_inputs: vec![],
            public_transcript_outputs: vec![],
            key_location: KeyLocation(Cow::Borrowed("builtin")),
        };

        let (proof, _pis, _skips) = ir
            .prove(
                &mut ChaCha20Rng::from_seed([42; 32]),
                &TestParams,
                pk,
                &preimage,
            )
            .await
            .unwrap();

        assert!(
            vk.verify(&PARAMS_VERIFIER, &proof, [43.into()].into_iter())
                .is_err()
        );
    }

    #[actix_rt::test]
    async fn v2_prove_hash_circuit() {
        use transient_crypto::hash::transient_hash;

        let ir_raw = r#"{
           "version": { "major": 2, "minor": 2 },
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

        let (pk, vk) = ir.v2_keygen(&TestParams).await.unwrap();

        let preimage = ProofPreimage {
            binding_input: 42.into(),
            communications_commitment: None,
            inputs: vec![1.into(), 2.into(), 3.into()],
            private_transcript: vec![],
            public_transcript_inputs: vec![x],
            public_transcript_outputs: vec![],
            key_location: KeyLocation(Cow::Borrowed("builtin")),
        };

        let (proof, pis, _skips) = ir
            .prove(
                &mut ChaCha20Rng::from_seed([42; 32]),
                &TestParams,
                pk,
                &preimage,
            )
            .await
            .unwrap();

        vk.verify(&PARAMS_VERIFIER, &proof, pis.into_iter())
            .unwrap();
    }

    #[actix_rt::test]
    async fn v2_prove_ec_circuit() {
        use transient_crypto::curve::EmbeddedGroupAffine;

        let ir_raw = r#"{
           "version": { "major": 2, "minor": 2 },
           "num_inputs": 4,
           "do_communications_commitment": false,
           "instructions": [
               { "op": "ec_mul", "a_x": 0, "a_y": 1, "scalar": 2 },
               { "op": "ec_mul_generator", "scalar": 3 },
               { "op": "ec_add", "a_x": 4, "a_y": 5, "b_x": 6, "b_y": 7 }
           ]
        }"#;
        let ir = IrSource::load(ir_raw.as_bytes()).unwrap();
        let (pk, vk) = ir.v2_keygen(&TestParams).await.unwrap();

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

        let (proof, pis, _skips) = ir
            .prove(
                &mut ChaCha20Rng::from_seed([42; 32]),
                &TestParams,
                pk,
                &preimage,
            )
            .await
            .unwrap();

        vk.verify(&PARAMS_VERIFIER, &proof, pis.into_iter())
            .unwrap();
    }
}
