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

use hex::FromHex;
use js_sys::{BigInt, Function, JsString, Promise, Uint8Array};
use rand::rngs::OsRng;
use serialize::{tagged_deserialize, tagged_serialize};
use transient_crypto::{
    curve::Fr,
    proofs::{
        KeyLocation, ParamsProver, ParamsProverProvider, ProofPreimage, ProvingKeyMaterial,
        Resolver,
    },
};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use zkir::IrSource;

struct JsKeyProvider(JsValue);

fn try_to_string(jsv: JsValue) -> String {
    let res = js_sys::Reflect::get(&jsv, &"toString".into())
        .and_then(|f| f.dyn_into::<Function>())
        .and_then(|f| f.call0(&jsv))
        .and_then(|s| s.dyn_into::<JsString>());
    match res {
        Ok(s) => s.into(),
        Err(_) => "<failed to stringify>".into(),
    }
}

fn err(msg: impl Into<String>) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::Other, msg.into())
}

impl ParamsProverProvider for JsKeyProvider {
    async fn get_params(&self, k: u8) -> std::io::Result<ParamsProver> {
        let get_params = js_sys::Reflect::get(&self.0, &"getParams".into())
            .map_err(|_| err("could not get property 'getParams' on KeyMaterialProvider"))?
            .dyn_into::<Function>()
            .map_err(|_| err("property 'getParams' on KeyMaterialProvider is not a function"))?;
        let promise = get_params
            .call1(&self.0, &JsValue::from(k))
            .map_err(|e| err(format!("error calling getParams: {}", try_to_string(e))))?
            .dyn_into::<Promise>()
            .map_err(|_| err("result of getParams was not a promise"))?;
        let res = JsFuture::from(promise)
            .await
            .map_err(|e| {
                err(format!(
                    "getParams promise resolved to error: {}",
                    try_to_string(e)
                ))
            })?
            .dyn_into::<Uint8Array>()
            .map_err(|_| err("result of getParams was not a Uint8Array"))?
            .to_vec();
        ParamsProver::read(&res[..])
    }
}

impl Resolver for JsKeyProvider {
    async fn resolve_key(&self, key: KeyLocation) -> std::io::Result<Option<ProvingKeyMaterial>> {
        let lookup_key = js_sys::Reflect::get(&self.0, &"lookupKey".into())
            .map_err(|_| err("could not get property 'lookupKey' on KeyMaterialProvider"))?
            .dyn_into::<Function>()
            .map_err(|_| err("property 'lookupKey on KeyMaterialProvider is not a function"))?;
        let loc = JsValue::from(key.0.into_owned());
        let promise = lookup_key
            .call1(&self.0, &loc)
            .map_err(|e| err(format!("error calling lookupKey: {}", try_to_string(e))))?
            .dyn_into::<Promise>()
            .map_err(|_| err("result of lookupKey is not a promise"))?;
        let res = JsFuture::from(promise).await.map_err(|e| {
            err(format!(
                "lookupKey promise resolve to error: {}",
                try_to_string(e)
            ))
        })?;
        if res.is_undefined() || res.is_null() {
            return Ok(None);
        }
        let getprop = |prop: &str| {
            Ok::<_, std::io::Error>(
                js_sys::Reflect::get(&res, &prop.into())
                    .map_err(|_| {
                        err(format!(
                            "could not get property '{prop}' on ProvingKeyMaterial"
                        ))
                    })?
                    .dyn_into::<Uint8Array>()
                    .map_err(|_| {
                        err(format!(
                            "property '{prop}' on ProvingKeyMaterial is not a Uint8Array"
                        ))
                    })?
                    .to_vec(),
            )
        };
        let prover_key = getprop("proverKey")?;
        let verifier_key = getprop("verifierKey")?;
        let ir_source = getprop("ir")?;
        Ok(Some(ProvingKeyMaterial {
            prover_key,
            verifier_key,
            ir_source,
        }))
    }
}

fn fr_from_bigint(bigint: BigInt) -> Result<Fr, JsError> {
    let hex_str = String::from(
        bigint
            .to_string(16)
            .map_err(|err| JsError::new(&String::from(err.to_string())))?,
    );
    let padded_str = if hex_str.len() % 2 == 1 {
        "0".to_owned() + &hex_str
    } else {
        hex_str
    };
    let mut bytes = <Vec<u8>>::from_hex(padded_str.as_bytes())?;
    bytes.reverse();
    Fr::from_le_bytes(&bytes).ok_or_else(|| JsError::new("out of bounds for prime field"))
}

#[wasm_bindgen]
pub async fn prove(
    ser_preimage: Uint8Array,
    provider: JsValue,
    overwrite_binding_input: Option<BigInt>,
) -> Result<Uint8Array, JsError> {
    let mut preimage: ProofPreimage = tagged_deserialize(&mut &ser_preimage.to_vec()[..])?;
    if let Some(bi) = overwrite_binding_input {
        preimage.binding_input = fr_from_bigint(bi)?;
    }
    let provider = JsKeyProvider(provider);
    let proof = preimage
        .prove::<IrSource>(OsRng, &provider, &provider)
        .await
        .map_err(|e| JsError::new(&e.to_string()))?
        .0;
    let mut res = Vec::new();
    tagged_serialize(&proof, &mut res)?;
    Ok(Uint8Array::from(&res[..]))
}

#[wasm_bindgen]
pub async fn check(ser_preimage: Uint8Array, provider: JsValue) -> Result<Vec<JsValue>, JsError> {
    let preimage: ProofPreimage = tagged_deserialize(&mut &ser_preimage.to_vec()[..])?;
    let provider = JsKeyProvider(provider);
    let Some(data) = provider.resolve_key(preimage.key_location.clone()).await? else {
        return Err(JsError::new(&format!(
            "failed to resolve key at '{}'",
            &preimage.key_location.0
        )));
    };
    let ir: IrSource = tagged_deserialize(&mut &data.ir_source[..])?;
    let res = preimage
        .check(&ir)
        .map_err(|e| JsError::new(&e.to_string()))?;
    Ok(res
        .into_iter()
        .map(|val| match val {
            Some(val) => JsValue::from(BigInt::from(val)),
            None => JsValue::UNDEFINED,
        })
        .collect())
}

#[wasm_bindgen(js_name = "provingProvider")]
pub fn proving_provider(km_provider: JsValue) -> WrappedProvingProvider {
    WrappedProvingProvider { km_provider }
}

#[wasm_bindgen]
pub struct WrappedProvingProvider {
    km_provider: JsValue,
}

#[wasm_bindgen]
impl WrappedProvingProvider {
    pub async fn check(
        &self,
        ser_preimage: Uint8Array,
        _key_location: &str,
    ) -> Result<Vec<JsValue>, JsError> {
        check(ser_preimage, self.km_provider.clone()).await
    }
    pub async fn prove(
        &self,
        ser_preimage: Uint8Array,
        _key_location: &str,
        overwrite_binding_input: Option<BigInt>,
    ) -> Result<Uint8Array, JsError> {
        prove(
            ser_preimage,
            self.km_provider.clone(),
            overwrite_binding_input,
        )
        .await
    }
}

#[wasm_bindgen(js_name = "jsonIrToBinary")]
pub fn json_ir_to_binary(json: &str) -> Result<Uint8Array, JsError> {
    let ir: IrSource = IrSource::load(json.as_bytes())?;
    let mut buf = Vec::new();
    tagged_serialize(&ir, &mut buf)?;
    Ok(buf[..].into())
}
