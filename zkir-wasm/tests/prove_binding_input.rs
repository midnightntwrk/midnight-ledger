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

//! Regression tests for https://github.com/midnightntwrk/midnight-ledger/issues/832
//!
//! `prove()` must honour `overwrite_binding_input` regardless of which
//! verifier-key version it dispatches on. Dust spends use a
//! `verifier-key[v6]` circuit and always pass an override, so if it is dropped
//! the proof is bound to the preimage's default (zero) binding input and the
//! node rejects it with `InvalidDustSpendProof`.
//!
//! Run with `wasm-pack test --node zkir-wasm` (or `cargo test --target
//! wasm32-unknown-unknown -p midnight-zkir-wasm` with `wasm-bindgen-test-runner`
//! configured), with `MIDNIGHT_PP` pointing at the public parameters.

#![cfg(target_arch = "wasm32")]

use js_sys::{BigInt, Object, Promise, Reflect, Uint8Array};
use midnight_zkir_wasm::prove;
use serialize::{tagged_deserialize, tagged_serialize};
use std::borrow::Cow;
use transient_crypto::curve::Fr;
use transient_crypto::proofs::{KeyLocation, ProofPreimage, Zkir as ZkirNew};
use transient_crypto_old::proofs::Zkir as ZkirOld;
use wasm_bindgen::prelude::*;
use wasm_bindgen_test::*;
use zkir::{IrMinorVersion, IrSource};

const MIDNIGHT_PP: &str = env!("MIDNIGHT_PP");
const OVERRIDE: u64 = 42;

#[wasm_bindgen(module = "fs")]
extern "C" {
    #[wasm_bindgen(js_name = readFileSync)]
    fn read_file_sync(path: &str) -> Uint8Array;
}

fn params_bytes(k: u8) -> Vec<u8> {
    read_file_sync(&format!("{MIDNIGHT_PP}/bls_midnight_2p{k}")).to_vec()
}

struct FsParams;

impl transient_crypto_old::proofs::ParamsProverProvider for FsParams {
    async fn get_params(
        &self,
        k: u8,
    ) -> std::io::Result<transient_crypto_old::proofs::ParamsProver> {
        transient_crypto_old::proofs::ParamsProver::read(&params_bytes(k)[..])
    }
}

impl transient_crypto::proofs::ParamsProverProvider for FsParams {
    async fn get_params(&self, k: u8) -> std::io::Result<transient_crypto::proofs::ParamsProver> {
        transient_crypto::proofs::ParamsProver::read(&params_bytes(k)[..])
    }
}

/// A `KeyMaterialProvider` JS object, as handed to `prove()` by the wallet.
fn key_material_provider(prover_key: Vec<u8>, verifier_key: Vec<u8>, ir: Vec<u8>) -> JsValue {
    let provider = Object::new();
    let lookup_key = Closure::<dyn Fn(JsValue) -> Promise>::new(move |_loc: JsValue| {
        let km = Object::new();
        for (name, bytes) in [
            ("proverKey", &prover_key),
            ("verifierKey", &verifier_key),
            ("ir", &ir),
        ] {
            Reflect::set(&km, &name.into(), &Uint8Array::from(&bytes[..])).unwrap();
        }
        Promise::resolve(&km)
    });
    let get_params = Closure::<dyn Fn(JsValue) -> Promise>::new(|k: JsValue| {
        let k = k.as_f64().expect("k is a number") as u8;
        Promise::resolve(&Uint8Array::from(&params_bytes(k)[..]))
    });
    Reflect::set(&provider, &"lookupKey".into(), lookup_key.as_ref()).unwrap();
    Reflect::set(&provider, &"getParams".into(), get_params.as_ref()).unwrap();
    lookup_key.forget();
    get_params.forget();
    provider.into()
}

/// A minimal circuit whose statement is just `[binding_input]`.
fn minimal_ir(minor: u8) -> IrSource {
    IrSource::load(
        format!(
            r#"{{
               "version": {{ "major": 2, "minor": {minor} }},
               "num_inputs": 1,
               "do_communications_commitment": false,
               "instructions": [ {{ "op": "assert", "cond": 0 }} ]
            }}"#
        )
        .as_bytes(),
    )
    .unwrap()
}

/// A preimage built the way `DustLocalState::spend` builds one: with a
/// default (zero) binding input, to be overridden at proving time.
fn serialized_preimage() -> Uint8Array {
    let preimage = ProofPreimage {
        inputs: vec![1u64.into()],
        private_transcript: vec![],
        public_transcript_inputs: vec![],
        public_transcript_outputs: vec![],
        binding_input: Default::default(),
        communications_commitment: None,
        key_location: KeyLocation(Cow::Borrowed("test/minimal")),
    };
    let mut ser = Vec::new();
    tagged_serialize(&preimage, &mut ser).unwrap();
    Uint8Array::from(&ser[..])
}

fn ir_bytes(ir: &IrSource) -> Vec<u8> {
    let mut ser = Vec::new();
    ir.serialize_to_tagged(&mut ser).unwrap();
    ser
}

#[wasm_bindgen_test]
async fn prove_applies_binding_input_override_for_verifier_key_v6() {
    let ir = minimal_ir(0);
    let (pk, vk) = ZkirOld::keygen(&ir, &FsParams).await.unwrap();
    let mut pk_ser = Vec::new();
    IrSource::serialize_stdlib_v1_prover_key_to_tagged(IrMinorVersion::V0, &pk, &mut pk_ser)
        .unwrap();
    let mut vk_ser = Vec::new();
    tagged_serialize(&vk, &mut vk_ser).unwrap();
    assert!(vk_ser.starts_with(b"midnight:verifier-key[v6]:"));

    let proof = prove(
        serialized_preimage(),
        key_material_provider(pk_ser, vk_ser, ir_bytes(&ir)),
        Some(BigInt::from(OVERRIDE)),
    )
    .await
    .unwrap_or_else(|e| panic!("prove failed: {:?}", JsValue::from(e)));
    let proof: transient_crypto_old::proofs::Proof =
        tagged_deserialize(&mut &proof.to_vec()[..]).unwrap();

    let verify = |bi: u64| {
        vk.verify(
            &transient_crypto_old::proofs::PARAMS_VERIFIER,
            &proof,
            [transient_crypto_old::curve::Fr::from(bi)].into_iter(),
        )
    };
    assert!(
        verify(OVERRIDE).is_ok(),
        "verifier-key[v6] proof does not verify against the overridden binding input {OVERRIDE} \
         (verifies against binding input 0: {})",
        verify(0).is_ok(),
    );
    assert!(verify(0).is_err());
}

// Control: the verifier-key[v7] branch already applies the override.
#[wasm_bindgen_test]
async fn prove_applies_binding_input_override_for_verifier_key_v7() {
    let ir = minimal_ir(2);
    let (pk, vk) = ZkirNew::keygen(&ir, &FsParams).await.unwrap();
    let mut pk_ser = Vec::new();
    IrSource::serialize_prover_key_to_tagged(IrMinorVersion::V2, &pk, &mut pk_ser).unwrap();
    let mut vk_ser = Vec::new();
    tagged_serialize(&vk, &mut vk_ser).unwrap();
    assert!(vk_ser.starts_with(b"midnight:verifier-key[v7]:"));

    let proof = prove(
        serialized_preimage(),
        key_material_provider(pk_ser, vk_ser, ir_bytes(&ir)),
        Some(BigInt::from(OVERRIDE)),
    )
    .await
    .unwrap_or_else(|e| panic!("prove failed: {:?}", JsValue::from(e)));
    let proof: transient_crypto::proofs::Proof =
        tagged_deserialize(&mut &proof.to_vec()[..]).unwrap();

    let verify = |bi: u64| {
        vk.verify(
            &transient_crypto::proofs::PARAMS_VERIFIER,
            &proof,
            [Fr::from(bi)].into_iter(),
        )
    };
    assert!(verify(OVERRIDE).is_ok());
    assert!(verify(0).is_err());
}
