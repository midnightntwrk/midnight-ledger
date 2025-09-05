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

use crate::conversions::*;
use base_crypto::signatures::Signature;
use js_sys::Uint8Array;
use ledger::structure::{ProofKind, ProofMarker, ProofPreimageMarker};
use onchain_runtime_wasm::from_value_ser;
use serialize::tagged_serialize;
use storage::db::InMemoryDB;
use transient_crypto::commitment::{Pedersen, PedersenRandomness, PureGeneratorPedersen};
use wasm_bindgen::JsError;
use wasm_bindgen::prelude::*;

#[derive(Clone)]
#[wasm_bindgen]
pub struct SignatureEnabled(pub(crate) Signature);

try_ref_for_exported!(SignatureEnabled);

#[wasm_bindgen]
impl SignatureEnabled {
    #[wasm_bindgen(constructor)]
    pub fn new(signature: String) -> Result<SignatureEnabled, JsError> {
        Ok(SignatureEnabled(from_hex_ser(&signature)?))
    }

    pub fn serialize(&self) -> Result<Uint8Array, JsError> {
        let mut res = vec![];
        tagged_serialize(&self.0, &mut res)?;
        Ok(Uint8Array::from(&res[..]))
    }

    pub fn deserialize(raw: Uint8Array) -> Result<SignatureEnabled, JsError> {
        Ok(SignatureEnabled(from_value_ser(raw, "SignatureEnabled")?))
    }

    #[wasm_bindgen(js_name = "toString")]
    pub fn to_string(&self, compact: Option<bool>) -> String {
        if compact.unwrap_or(false) {
            format!("{:?}", &self.0)
        } else {
            format!("{:#?}", &self.0)
        }
    }

    #[wasm_bindgen(getter)]
    pub fn instance(&self) -> String {
        String::from("signature")
    }
}

#[wasm_bindgen]
pub struct SignatureErased();

try_ref_for_exported!(SignatureErased);

#[wasm_bindgen]
impl SignatureErased {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<SignatureErased, JsError> {
        Ok(SignatureErased())
    }

    #[wasm_bindgen(js_name = "toString")]
    pub fn to_string(&self, _compact: Option<bool>) -> String {
        String::from("SignatureErased()")
    }

    #[wasm_bindgen(getter)]
    pub fn instance(&self) -> String {
        String::from("signature-erased")
    }
}

#[derive(Clone)]
#[wasm_bindgen]
pub struct PreBinding(pub(crate) PedersenRandomness);

try_ref_for_exported!(PreBinding);

#[wasm_bindgen]
impl PreBinding {
    #[wasm_bindgen(constructor)]
    pub fn new(binding: String) -> Result<PreBinding, JsError> {
        Ok(PreBinding(from_hex_ser(&binding)?))
    }

    pub fn serialize(&self) -> Result<Uint8Array, JsError> {
        let mut res = vec![];
        tagged_serialize(&self.0, &mut res)?;
        Ok(Uint8Array::from(&res[..]))
    }

    pub fn deserialize(raw: Uint8Array) -> Result<PreBinding, JsError> {
        Ok(PreBinding(from_value_ser(raw, "PreBinding")?))
    }

    #[wasm_bindgen(js_name = "toString")]
    pub fn to_string(&self, compact: Option<bool>) -> String {
        if compact.unwrap_or(false) {
            format!("{:?}", &self.0)
        } else {
            format!("{:#?}", &self.0)
        }
    }

    #[wasm_bindgen(getter)]
    pub fn instance(&self) -> String {
        String::from("pre-binding")
    }
}

#[derive(Clone)]
#[wasm_bindgen]
pub struct Binding(pub(crate) PureGeneratorPedersen);

try_ref_for_exported!(Binding);

#[wasm_bindgen]
impl Binding {
    #[wasm_bindgen(constructor)]
    pub fn new(binding: String) -> Result<Binding, JsError> {
        Ok(Binding(from_hex_ser(&binding)?))
    }

    pub fn serialize(&self) -> Result<Uint8Array, JsError> {
        let mut res = vec![];
        tagged_serialize(&self.0, &mut res)?;
        Ok(Uint8Array::from(&res[..]))
    }

    pub fn deserialize(raw: Uint8Array) -> Result<Binding, JsError> {
        Ok(Binding(from_value_ser(raw, "Binding")?))
    }

    #[wasm_bindgen(js_name = "toString")]
    pub fn to_string(&self, compact: Option<bool>) -> String {
        if compact.unwrap_or(false) {
            format!("{:?}", &self.0)
        } else {
            format!("{:#?}", &self.0)
        }
    }

    #[wasm_bindgen(getter)]
    pub fn instance(&self) -> String {
        String::from("binding")
    }
}

#[derive(Clone)]
#[wasm_bindgen]
pub struct NoBinding(pub(crate) Pedersen);

try_ref_for_exported!(NoBinding);

#[wasm_bindgen]
impl NoBinding {
    #[wasm_bindgen(constructor)]
    pub fn new(binding: String) -> Result<NoBinding, JsError> {
        Ok(NoBinding(from_hex_ser(&binding)?))
    }

    pub fn serialize(&self) -> Result<Uint8Array, JsError> {
        let mut res = vec![];
        tagged_serialize(&self.0, &mut res)?;
        Ok(Uint8Array::from(&res[..]))
    }

    pub fn deserialize(raw: Uint8Array) -> Result<NoBinding, JsError> {
        Ok(NoBinding(from_value_ser(raw, "NoBinding")?))
    }

    #[wasm_bindgen(js_name = "toString")]
    pub fn to_string(&self, compact: Option<bool>) -> String {
        if compact.unwrap_or(false) {
            format!("{:?}", &self.0)
        } else {
            format!("{:#?}", &self.0)
        }
    }

    #[wasm_bindgen(getter)]
    pub fn instance(&self) -> String {
        String::from("no-binding")
    }
}

#[derive(Clone)]
#[wasm_bindgen]
pub struct PreProof(pub(crate) <ProofPreimageMarker as ProofKind<InMemoryDB>>::Proof);

try_ref_for_exported!(PreProof);

#[wasm_bindgen]
impl PreProof {
    #[wasm_bindgen(constructor)]
    pub fn new(data: String) -> Result<PreProof, JsError> {
        Ok(PreProof(from_hex_ser(&data)?))
    }

    pub fn serialize(&self) -> Result<Uint8Array, JsError> {
        let mut res = vec![];
        tagged_serialize(&self.0, &mut res)?;
        Ok(Uint8Array::from(&res[..]))
    }

    pub fn deserialize(raw: Uint8Array) -> Result<PreProof, JsError> {
        Ok(PreProof(from_value_ser(raw, "PreProof")?))
    }

    #[wasm_bindgen(js_name = "toString")]
    pub fn to_string(&self, compact: Option<bool>) -> String {
        if compact.unwrap_or(false) {
            format!("{:?}", &self.0)
        } else {
            format!("{:#?}", &self.0)
        }
    }

    #[wasm_bindgen(getter)]
    pub fn instance(&self) -> String {
        String::from("pre-proof")
    }
}

#[derive(Clone)]
#[wasm_bindgen]
pub struct Proof(pub(crate) <ProofMarker as ProofKind<InMemoryDB>>::Proof);

try_ref_for_exported!(Proof);

#[wasm_bindgen]
impl Proof {
    #[wasm_bindgen(constructor)]
    pub fn new(data: String) -> Result<Proof, JsError> {
        Ok(Proof(from_hex_ser(&data)?))
    }

    pub fn serialize(&self) -> Result<Uint8Array, JsError> {
        let mut res = vec![];
        tagged_serialize(&self.0, &mut res)?;
        Ok(Uint8Array::from(&res[..]))
    }

    pub fn deserialize(raw: Uint8Array) -> Result<Proof, JsError> {
        Ok(Proof(from_value_ser(raw, "Proof")?))
    }

    #[wasm_bindgen(js_name = "toString")]
    pub fn to_string(&self, compact: Option<bool>) -> String {
        if compact.unwrap_or(false) {
            format!("{:?}", &self.0)
        } else {
            format!("{:#?}", &self.0)
        }
    }

    #[wasm_bindgen(getter)]
    pub fn instance(&self) -> String {
        String::from("proof")
    }
}

#[wasm_bindgen]
pub struct NoProof();

#[wasm_bindgen]
impl NoProof {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<NoProof, JsError> {
        Ok(NoProof())
    }

    #[wasm_bindgen(js_name = "toString")]
    pub fn to_string(&self, _compact: Option<bool>) -> String {
        String::from("NoProof()")
    }

    #[wasm_bindgen(getter)]
    pub fn instance(&self) -> String {
        String::from("no-proof")
    }
}
