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

use std::sync::Arc;

use base_crypto::fab::{AlignedValue, Alignment};
use base_crypto::hash::HashOutput;
use coin_structure::contract::ContractAddress;
use hex::FromHex;
use js_sys::{Array, BigInt, Function, JsString, Promise, Reflect};
use onchain_runtime::cost_model::INITIAL_COST_MODEL;
use onchain_runtime_state::state::{
    ChargedState as RtChargedState, EntryPointBuf, StateValue as RtStateValue,
};
use onchain_runtime_wasm::context::QueryContext;
use onchain_runtime_wasm::state::StateValue;
use rand::rngs::OsRng;
use storage::db::InMemoryDB;
use transient_crypto::curve::Fr;
use transient_crypto::fab::{AlignedValueExt, AlignmentExt};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use zkir_v3::ir_execute::{
    Call as RsCall, CallRole as RsCallRole, ExecutionContext as RsExecutionContext, ExecutionError,
    ExecutionResult, ZkirProvider,
};

use crate::Zkir;

struct JsExecutionProvider {
    inner: JsValue,
}

fn try_to_string(jsv: JsValue) -> String {
    let res = Reflect::get(&jsv, &"toString".into())
        .and_then(|f| f.dyn_into::<Function>())
        .and_then(|f| f.call0(&jsv))
        .and_then(|s| s.dyn_into::<JsString>());
    match res {
        Ok(s) => s.into(),
        Err(_) => "<failed to stringify>".into(),
    }
}

fn provider_err(msg: impl Into<String>) -> ExecutionError {
    ExecutionError::ProviderError(msg.into())
}

fn get_method(obj: &JsValue, prop: &str) -> Result<Function, ExecutionError> {
    Reflect::get(obj, &prop.into())
        .map_err(|_| {
            provider_err(format!(
                "could not get property '{prop}' on ExecutionProvider"
            ))
        })?
        .dyn_into::<Function>()
        .map_err(|_| provider_err(format!("'{prop}' on ExecutionProvider is not a function")))
}

impl ZkirProvider<InMemoryDB> for JsExecutionProvider {
    async fn fetch_contract(
        &self,
        address: ContractAddress,
        entry_point: &[u8],
    ) -> Result<(zkir_v3::IrSource, RtChargedState<InMemoryDB>), ExecutionError> {
        let method = get_method(&self.inner, "getContract")?;
        let addr_str = JsValue::from_str(&hex::encode(address.0.0));
        let ep_str = std::str::from_utf8(entry_point)
            .map_err(|e| provider_err(format!("entry_point is not valid UTF-8: {e}")))?;
        let ep_js = JsValue::from_str(ep_str);
        let promise = method
            .call2(&self.inner, &addr_str, &ep_js)
            .map_err(|e| {
                provider_err(format!(
                    "error calling getContract: {}",
                    try_to_string(e)
                ))
            })?
            .dyn_into::<Promise>()
            .map_err(|_| provider_err("result of getContract was not a Promise"))?;
        let res = JsFuture::from(promise).await.map_err(|e| {
            provider_err(format!(
                "getContract rejected: {}",
                try_to_string(e)
            ))
        })?;
        if res.is_undefined() || res.is_null() {
            return Err(provider_err(format!(
                "getContract returned undefined for ({address:?}, entry_point={ep_str:?})"
            )));
        }
        // The JS side returns `{ zkir: Zkir, state: StateValue }` as a
        // single object so the caller can guarantee both fields were
        // observed at the same logical instant.
        let zkir_field = Reflect::get(&res, &"zkir".into())
            .map_err(|_| provider_err("getContract result missing 'zkir' field"))?;
        let state_field = Reflect::get(&res, &"state".into())
            .map_err(|_| provider_err("getContract result missing 'state' field"))?;
        let zkir = zkir_field
            .dyn_into::<Zkir>()
            .map_err(|_| provider_err("getContract.zkir is not a Zkir instance"))?;
        let state = state_field
            .dyn_into::<StateValue>()
            .map_err(|_| provider_err("getContract.state is not a StateValue instance"))?;
        let raw_state: RtStateValue<InMemoryDB> = state.into();
        Ok((zkir.0.clone(), RtChargedState::new(raw_state)))
    }
}

#[wasm_bindgen]
pub struct Call(RsCall<InMemoryDB>);

#[wasm_bindgen]
impl Call {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<Call, JsError> {
        Err(JsError::new(
            "Call cannot be constructed directly through the WASM API.",
        ))
    }

    #[wasm_bindgen(getter)]
    pub fn address(&self) -> String {
        hex::encode(self.0.address.0.0)
    }

    #[wasm_bindgen(getter)]
    pub fn circuit(&self) -> String {
        String::from_utf8_lossy(&self.0.entry_point.0).into_owned()
    }

    #[wasm_bindgen(getter)]
    pub fn input(&self) -> Result<JsValue, JsError> {
        Ok(to_value(&wrap_frs_av(
            &self.0.input,
            &self.0.input_alignment,
        )?)?)
    }

    #[wasm_bindgen(getter)]
    pub fn output(&self) -> Result<JsValue, JsError> {
        Ok(to_value(&wrap_frs_av(
            &self.0.output,
            &self.0.output_alignment,
        )?)?)
    }

    #[wasm_bindgen(getter)]
    pub fn program(&self) -> Result<JsValue, JsError> {
        Ok(to_value(&self.0.program)?)
    }

    #[wasm_bindgen(getter)]
    pub fn context(&self) -> QueryContext {
        QueryContext::from(self.0.context.clone())
    }

    #[wasm_bindgen(getter)]
    pub fn parent(&self) -> JsValue {
        match self.0.parent {
            Some(idx) => JsValue::from_f64(idx as f64),
            None => JsValue::NULL,
        }
    }

    #[wasm_bindgen(getter)]
    pub fn role(&self) -> Result<JsValue, JsError> {
        let obj = js_sys::Object::new();
        match &self.0.role {
            RsCallRole::Root => {
                Reflect::set(&obj, &"kind".into(), &"root".into()).map_err(jsval_to_jserror)?;
            }
            RsCallRole::Sub {
                comm_comm,
                comm_comm_rand,
            } => {
                Reflect::set(&obj, &"kind".into(), &"sub".into()).map_err(jsval_to_jserror)?;
                Reflect::set(&obj, &"commComm".into(), &fr_to_bigint(*comm_comm)?.into())
                    .map_err(jsval_to_jserror)?;
                Reflect::set(
                    &obj,
                    &"commCommRand".into(),
                    &fr_to_bigint(*comm_comm_rand)?.into(),
                )
                .map_err(jsval_to_jserror)?;
            }
        }
        Ok(obj.into())
    }

    #[wasm_bindgen(getter, js_name = "privateTranscriptOutputs")]
    pub fn private_transcript_outputs(&self) -> Result<JsValue, JsError> {
        Ok(to_value(&self.0.private_transcript_outputs)?)
    }
}

#[wasm_bindgen]
pub async fn execute(provider: JsValue, context: JsValue) -> Result<Vec<Call>, JsError> {
    let input_js = Reflect::get(&context, &"input".into())
        .map_err(|_| JsError::new("context.input missing"))?;
    let input_av: AlignedValue = from_value(input_js)?;
    let mut input: Vec<Fr> = Vec::new();
    input_av.value_only_field_repr(&mut input);

    let address_js = Reflect::get(&context, &"address".into())
        .map_err(|_| JsError::new("context.address missing"))?;
    let address_str = address_js
        .as_string()
        .ok_or_else(|| JsError::new("context.address must be a string"))?;
    let address = address_from_hex(&address_str)?;

    let circuit_js = Reflect::get(&context, &"circuit".into())
        .map_err(|_| JsError::new("context.circuit missing"))?;
    let circuit = circuit_js
        .as_string()
        .ok_or_else(|| JsError::new("context.circuit must be a string"))?;
    let entry_point = EntryPointBuf(circuit.into_bytes());

    let max_depth_js = Reflect::get(&context, &"maxCallDepth".into())
        .map_err(|_| JsError::new("context.maxCallDepth missing"))?;
    let max_call_depth = max_depth_js
        .as_f64()
        .ok_or_else(|| JsError::new("context.maxCallDepth must be a number"))?
        as u32;

    let js_provider = Arc::new(JsExecutionProvider { inner: provider });

    let (top_ir, top_state) = js_provider
        .fetch_contract(address, entry_point.0.as_slice())
        .await
        .map_err(|e| JsError::new(&e.to_string()))?;

    let rs_context = RsExecutionContext {
        ledger_state: top_state,
        address,
        entry_point,
        zkir_provider: js_provider,
        witness_provider: None,
        call_depth: 0,
        max_call_depth,
        cost_model: INITIAL_COST_MODEL,
    };

    let mut rng = OsRng;
    let result: ExecutionResult<InMemoryDB> = top_ir
        .execute(input, rs_context, &mut rng)
        .await
        .map_err(|e| JsError::new(&e.to_string()))?;

    Ok(result.into_iter().map(Call).collect())
}

#[wasm_bindgen(js_name = "rootOf")]
pub fn root_of(result: Array) -> Result<JsValue, JsError> {
    if result.length() == 0 {
        return Err(JsError::new("execution result has no root call"));
    }
    Ok(result.get(0))
}

#[wasm_bindgen(js_name = "subCallsOf")]
pub fn sub_calls_of(result: Array, parent_index: u32) -> Result<Array, JsError> {
    let out = Array::new();
    let target = parent_index as f64;
    for i in 0..result.length() {
        let elem = result.get(i);
        let parent = Reflect::get(&elem, &"parent".into()).map_err(jsval_to_jserror)?;
        if parent.as_f64() == Some(target) {
            out.push(&elem);
        }
    }
    Ok(out)
}

fn address_from_hex(s: &str) -> Result<ContractAddress, JsError> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    let bytes = <[u8; 32]>::from_hex(s)
        .map_err(|_| JsError::new("address must be 32 hex bytes (with or without 0x prefix)"))?;
    Ok(ContractAddress(HashOutput(bytes)))
}

fn fr_to_bigint(fr: Fr) -> Result<BigInt, JsError> {
    let mut bytes = fr.0.to_bytes_le();
    bytes.reverse();
    let hex_str = format!("0x{}", hex::encode(&bytes));
    BigInt::new(&JsValue::from_str(&hex_str))
        .map_err(|e| JsError::new(&format!("BigInt::new failed for Fr: {:?}", e)))
}

/// Wrap a slice of `Fr` as an `AlignedValue` with the supplied alignment.
/// Defers to `Alignment::parse_field_repr`, which correctly distributes
/// field elements across atoms regardless of whether they're `Field`,
/// `Bytes<N>`, etc. — so this works uniformly for any present or future
/// `IrType`.
fn wrap_frs_av(frs: &[Fr], alignment: &Alignment) -> Result<AlignedValue, JsError> {
    alignment.parse_field_repr(frs).ok_or_else(|| {
        JsError::new(&format!(
            "could not parse {} field elements as alignment {:?}",
            frs.len(),
            alignment
        ))
    })
}

fn jsval_to_jserror(v: JsValue) -> JsError {
    JsError::new(&format!("{:?}", v))
}

pub(crate) fn to_value<T: serde::Serialize + ?Sized>(value: &T) -> Result<JsValue, JsError> {
    Ok(value.serialize(
        &serde_wasm_bindgen::Serializer::new().serialize_large_number_types_as_bigints(true),
    )?)
}

pub(crate) fn from_value<T: for<'de> serde::Deserialize<'de>>(
    value: JsValue,
) -> Result<T, JsError> {
    Ok(serde_wasm_bindgen::from_value(value)?)
}
