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

use crate::conversions::*;
use crate::zswap_wasm::LedgerParameters;
use js_sys::{Array, JsString};
use onchain_runtime_wasm::context::QueryContext;
use storage::db::InMemoryDB;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
#[derive(Debug)]
pub struct PreTranscript(pub(crate) ledger::construct::PreTranscript<InMemoryDB>);

try_ref_for_exported!(PreTranscript);

#[wasm_bindgen]
impl PreTranscript {
    #[wasm_bindgen(constructor)]
    pub fn new(
        context: &QueryContext,
        program: JsValue,
        comm_comm: JsValue,
    ) -> Result<PreTranscript, JsError> {
        let comm_comm = if comm_comm.is_null() || comm_comm.is_undefined() {
            None
        } else if comm_comm.is_string() {
            let comm_comm_str = String::from(JsString::from(comm_comm));
            Some(from_hex_ser(&comm_comm_str)?)
        } else {
            return Err(JsError::new("expected string | undefined"));
        };
        Ok(PreTranscript(ledger::construct::PreTranscript {
            context: context.clone().into(),
            program: from_value(program)?,
            comm_comm,
        }))
    }

    #[wasm_bindgen(js_name = "toString")]
    pub fn to_string(&self, compact: Option<bool>) -> String {
        if compact.unwrap_or(false) {
            format!("{:?}", self)
        } else {
            format!("{:#?}", self)
        }
    }
}

#[wasm_bindgen]
#[derive(Debug)]
pub struct TransactionShape(pub(crate) ledger::construct::TransactionShape);

try_ref_for_exported!(TransactionShape);

#[wasm_bindgen]
impl TransactionShape {
    #[wasm_bindgen(js_name = "forCalls")]
    pub fn for_calls(
        calls: Vec<JsValue>,
        params: &LedgerParameters,
    ) -> Result<TransactionShape, JsError> {
        let pre_transcripts = pre_transcripts_from_values(&calls)?;
        Ok(TransactionShape(
            ledger::construct::TransactionShape::for_calls(&pre_transcripts, params)?,
        ))
    }

    #[wasm_bindgen(getter)]
    pub fn intents(&self) -> usize {
        self.0.intents
    }

    #[wasm_bindgen(setter)]
    pub fn set_intents(&mut self, intents: usize) {
        self.0.intents = intents;
    }

    #[wasm_bindgen(getter, js_name = "nightSpends")]
    pub fn night_spends(&self) -> usize {
        self.0.night_spends
    }

    #[wasm_bindgen(setter, js_name = "nightSpends")]
    pub fn set_night_spends(&mut self, night_spends: usize) {
        self.0.night_spends = night_spends;
    }

    #[wasm_bindgen(getter, js_name = "nightOutputs")]
    pub fn night_outputs(&self) -> usize {
        self.0.night_outputs
    }

    #[wasm_bindgen(setter, js_name = "nightOutputs")]
    pub fn set_night_outputs(&mut self, night_outputs: usize) {
        self.0.night_outputs = night_outputs;
    }

    #[wasm_bindgen(getter, js_name = "nonNightUnshieldedSpends")]
    pub fn non_night_unshielded_spends(&self) -> usize {
        self.0.non_night_unshielded_spends
    }

    #[wasm_bindgen(setter, js_name = "nonNightUnshieldedSpends")]
    pub fn set_non_night_unshielded_spends(&mut self, non_night_unshielded_spends: usize) {
        self.0.non_night_unshielded_spends = non_night_unshielded_spends;
    }

    #[wasm_bindgen(getter, js_name = "nonNightUnshieldedOutputs")]
    pub fn non_night_unshielded_outputs(&self) -> usize {
        self.0.non_night_unshielded_outputs
    }

    #[wasm_bindgen(setter, js_name = "nonNightUnshieldedOutputs")]
    pub fn set_non_night_unshielded_outputs(&mut self, non_night_unshielded_outputs: usize) {
        self.0.non_night_unshielded_outputs = non_night_unshielded_outputs;
    }

    #[wasm_bindgen(getter, js_name = "dustSpends")]
    pub fn dust_spends(&self) -> usize {
        self.0.dust_spends
    }

    #[wasm_bindgen(setter, js_name = "dustSpends")]
    pub fn set_dust_spends(&mut self, dust_spends: usize) {
        self.0.dust_spends = dust_spends;
    }

    #[wasm_bindgen(getter, js_name = "zswapInputs")]
    pub fn zswap_inputs(&self) -> usize {
        self.0.zswap_inputs
    }

    #[wasm_bindgen(setter, js_name = "zswapInputs")]
    pub fn set_zswap_inputs(&mut self, zswap_inputs: usize) {
        self.0.zswap_inputs = zswap_inputs;
    }

    #[wasm_bindgen(getter, js_name = "zswapOutputs")]
    pub fn zswap_outputs(&self) -> usize {
        self.0.zswap_outputs
    }

    #[wasm_bindgen(setter, js_name = "zswapOutputs")]
    pub fn set_zswap_outputs(&mut self, zswap_outputs: usize) {
        self.0.zswap_outputs = zswap_outputs;
    }

    #[wasm_bindgen(js_name = "toString")]
    pub fn to_string(&self, compact: Option<bool>) -> String {
        if compact.unwrap_or(false) {
            format!("{:?}", self)
        } else {
            format!("{:#?}", self)
        }
    }
}

/// Reads an optional [`TransactionShape`] argument; `undefined` and `null`
/// give `None`.
pub(crate) fn expected_tx_shape_from_value(
    value: &JsValue,
) -> Result<Option<ledger::construct::TransactionShape>, JsError> {
    if value.is_undefined() || value.is_null() {
        return Ok(None);
    }
    let tx_shape = TransactionShape::try_ref(value)?
        .ok_or_else(|| JsError::new("Expected TransactionShape"))?;
    Ok(Some(tx_shape.0.clone()))
}

fn pre_transcripts_from_values(
    calls: &[JsValue],
) -> Result<Vec<ledger::construct::PreTranscript<InMemoryDB>>, JsError> {
    calls
        .iter()
        .map(|val| {
            PreTranscript::try_ref(val)
                .transpose()
                .ok_or_else(|| JsError::new("Expected PreTranscript"))
                .and_then(|x| x)
                .map(|pt| pt.0.clone())
        })
        .collect()
}

#[wasm_bindgen(js_name = "partitionTranscripts")]
pub fn partition_transcripts(
    calls: Vec<JsValue>,
    params: &LedgerParameters,
    expected_tx_shape: JsValue,
) -> Result<Array, JsError> {
    let pre_transcripts = pre_transcripts_from_values(&calls)?;
    let transcripts = match expected_tx_shape_from_value(&expected_tx_shape)? {
        Some(expected_tx_shape) => ledger::construct::partition_transcripts_with_tx_shape(
            &pre_transcripts,
            params,
            &expected_tx_shape,
        )?,
        None => ledger::construct::partition_transcripts(&pre_transcripts, params)?,
    };
    let res = Array::new();
    for (guaranteed, fallible) in transcripts.into_iter() {
        let pair = Array::new();
        pair.push(&to_value(&guaranteed)?);
        pair.push(&to_value(&fallible)?);
        res.push(&pair);
    }
    Ok(res)
}
