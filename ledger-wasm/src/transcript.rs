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
use crate::tx::{Transaction, TransactionTypes};
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

/// Runs `$body` with `$tx` bound to the ledger transaction inside a
/// [`TransactionTypes`], whichever its markers.
macro_rules! with_ledger_tx {
    ($types:expr, |$tx:ident| $body:expr) => {{
        use TransactionTypes::*;
        match $types {
            UnprovenWithSignaturePreBinding($tx) => $body,
            UnprovenWithSignatureBinding($tx) => $body,
            UnprovenWithSignatureErasedPreBinding($tx) => $body,
            UnprovenWithSignatureErasedBinding($tx) => $body,
            ProvenWithSignaturePreBinding($tx) => $body,
            ProvenWithSignatureBinding($tx) => $body,
            ProvenWithSignatureErasedPreBinding($tx) => $body,
            ProvenWithSignatureErasedBinding($tx) => $body,
            ProofErasedWithSignatureNoBinding($tx) => $body,
            ProofErasedWithSignatureErasedNoBinding($tx) => $body,
        }
    }};
}

#[wasm_bindgen]
#[derive(Debug)]
pub struct TransactionShape(pub(crate) ledger::construct::TransactionShape);

try_ref_for_exported!(TransactionShape);

#[wasm_bindgen]
impl TransactionShape {
    #[wasm_bindgen(js_name = "estimateFromCalls")]
    pub fn estimate_from_calls(
        calls: Vec<JsValue>,
        params: &LedgerParameters,
    ) -> Result<TransactionShape, JsError> {
        let pre_transcripts = pre_transcripts_from_values(&calls)?;
        Ok(TransactionShape(
            ledger::construct::TransactionShape::estimate_from_calls(&pre_transcripts, params)?,
        ))
    }

    /// Kept for the callers of 9.1.0.2-rc.0; renamed to `estimateFromCalls`.
    #[wasm_bindgen(js_name = "forCalls")]
    pub fn for_calls(
        calls: Vec<JsValue>,
        params: &LedgerParameters,
    ) -> Result<TransactionShape, JsError> {
        TransactionShape::estimate_from_calls(calls, params)
    }

    pub fn of(tx: &Transaction) -> TransactionShape {
        TransactionShape(with_ledger_tx!(&tx.0, |tx| {
            ledger::construct::TransactionShape::of(tx)
        }))
    }

    /// Checks `tx`, the final transaction of calls partitioned for this shape,
    /// against the time to dismiss the node allows it; throws, naming the
    /// counts that differ, if it takes longer.
    #[wasm_bindgen(js_name = "checkTx")]
    pub fn check_tx(&self, tx: &Transaction, params: &LedgerParameters) -> Result<(), JsError> {
        with_ledger_tx!(&tx.0, |tx| self.0.check_tx(tx, params))?;
        Ok(())
    }

    pub fn merge(&self, other: &TransactionShape) -> TransactionShape {
        TransactionShape(self.0.merge(&other.0))
    }

    #[wasm_bindgen(getter)]
    pub fn intents(&self) -> usize {
        self.0.intents
    }

    #[wasm_bindgen(setter)]
    pub fn set_intents(&mut self, intents: f64) -> Result<(), JsError> {
        self.0.intents = count_from_js("intents", intents)?;
        Ok(())
    }

    #[wasm_bindgen(getter, js_name = "nightSpends")]
    pub fn night_spends(&self) -> usize {
        self.0.night_spends
    }

    #[wasm_bindgen(setter, js_name = "nightSpends")]
    pub fn set_night_spends(&mut self, night_spends: f64) -> Result<(), JsError> {
        self.0.night_spends = count_from_js("night_spends", night_spends)?;
        Ok(())
    }

    #[wasm_bindgen(getter, js_name = "nightOutputs")]
    pub fn night_outputs(&self) -> usize {
        self.0.night_outputs
    }

    #[wasm_bindgen(setter, js_name = "nightOutputs")]
    pub fn set_night_outputs(&mut self, night_outputs: f64) -> Result<(), JsError> {
        self.0.night_outputs = count_from_js("night_outputs", night_outputs)?;
        Ok(())
    }

    #[wasm_bindgen(getter, js_name = "nonNightUnshieldedSpends")]
    pub fn non_night_unshielded_spends(&self) -> usize {
        self.0.non_night_unshielded_spends
    }

    #[wasm_bindgen(setter, js_name = "nonNightUnshieldedSpends")]
    pub fn set_non_night_unshielded_spends(
        &mut self,
        non_night_unshielded_spends: f64,
    ) -> Result<(), JsError> {
        self.0.non_night_unshielded_spends =
            count_from_js("non_night_unshielded_spends", non_night_unshielded_spends)?;
        Ok(())
    }

    #[wasm_bindgen(getter, js_name = "nonNightUnshieldedOutputs")]
    pub fn non_night_unshielded_outputs(&self) -> usize {
        self.0.non_night_unshielded_outputs
    }

    #[wasm_bindgen(setter, js_name = "nonNightUnshieldedOutputs")]
    pub fn set_non_night_unshielded_outputs(
        &mut self,
        non_night_unshielded_outputs: f64,
    ) -> Result<(), JsError> {
        self.0.non_night_unshielded_outputs =
            count_from_js("non_night_unshielded_outputs", non_night_unshielded_outputs)?;
        Ok(())
    }

    #[wasm_bindgen(getter, js_name = "dustSpends")]
    pub fn dust_spends(&self) -> usize {
        self.0.dust_spends
    }

    #[wasm_bindgen(setter, js_name = "dustSpends")]
    pub fn set_dust_spends(&mut self, dust_spends: f64) -> Result<(), JsError> {
        self.0.dust_spends = count_from_js("dust_spends", dust_spends)?;
        Ok(())
    }

    #[wasm_bindgen(getter, js_name = "zswapInputs")]
    pub fn zswap_inputs(&self) -> usize {
        self.0.zswap_inputs
    }

    #[wasm_bindgen(setter, js_name = "zswapInputs")]
    pub fn set_zswap_inputs(&mut self, zswap_inputs: f64) -> Result<(), JsError> {
        self.0.zswap_inputs = count_from_js("zswap_inputs", zswap_inputs)?;
        Ok(())
    }

    #[wasm_bindgen(getter, js_name = "zswapOutputs")]
    pub fn zswap_outputs(&self) -> usize {
        self.0.zswap_outputs
    }

    #[wasm_bindgen(setter, js_name = "zswapOutputs")]
    pub fn set_zswap_outputs(&mut self, zswap_outputs: f64) -> Result<(), JsError> {
        self.0.zswap_outputs = count_from_js("zswap_outputs", zswap_outputs)?;
        Ok(())
    }

    #[wasm_bindgen(getter, js_name = "zswapTransients")]
    pub fn zswap_transients(&self) -> usize {
        self.0.zswap_transients
    }

    #[wasm_bindgen(setter, js_name = "zswapTransients")]
    pub fn set_zswap_transients(&mut self, zswap_transients: f64) -> Result<(), JsError> {
        self.0.zswap_transients = count_from_js("zswap_transients", zswap_transients)?;
        Ok(())
    }

    #[wasm_bindgen(getter, js_name = "dustRegistrations")]
    pub fn dust_registrations(&self) -> usize {
        self.0.dust_registrations
    }

    #[wasm_bindgen(setter, js_name = "dustRegistrations")]
    pub fn set_dust_registrations(&mut self, dust_registrations: f64) -> Result<(), JsError> {
        self.0.dust_registrations = count_from_js("dust_registrations", dust_registrations)?;
        Ok(())
    }

    #[wasm_bindgen(getter, js_name = "dustDeregistrations")]
    pub fn dust_deregistrations(&self) -> usize {
        self.0.dust_deregistrations
    }

    #[wasm_bindgen(setter, js_name = "dustDeregistrations")]
    pub fn set_dust_deregistrations(&mut self, dust_deregistrations: f64) -> Result<(), JsError> {
        self.0.dust_deregistrations = count_from_js("dust_deregistrations", dust_deregistrations)?;
        Ok(())
    }

    #[wasm_bindgen(getter, js_name = "fallibleNightSpends")]
    pub fn fallible_night_spends(&self) -> usize {
        self.0.fallible_night_spends
    }

    #[wasm_bindgen(setter, js_name = "fallibleNightSpends")]
    pub fn set_fallible_night_spends(&mut self, fallible_night_spends: f64) -> Result<(), JsError> {
        self.0.fallible_night_spends =
            count_from_js("fallible_night_spends", fallible_night_spends)?;
        Ok(())
    }

    #[wasm_bindgen(getter, js_name = "fallibleNightOutputs")]
    pub fn fallible_night_outputs(&self) -> usize {
        self.0.fallible_night_outputs
    }

    #[wasm_bindgen(setter, js_name = "fallibleNightOutputs")]
    pub fn set_fallible_night_outputs(
        &mut self,
        fallible_night_outputs: f64,
    ) -> Result<(), JsError> {
        self.0.fallible_night_outputs =
            count_from_js("fallible_night_outputs", fallible_night_outputs)?;
        Ok(())
    }

    #[wasm_bindgen(getter, js_name = "fallibleNonNightUnshieldedSpends")]
    pub fn fallible_non_night_unshielded_spends(&self) -> usize {
        self.0.fallible_non_night_unshielded_spends
    }

    #[wasm_bindgen(setter, js_name = "fallibleNonNightUnshieldedSpends")]
    pub fn set_fallible_non_night_unshielded_spends(
        &mut self,
        fallible_non_night_unshielded_spends: f64,
    ) -> Result<(), JsError> {
        self.0.fallible_non_night_unshielded_spends = count_from_js(
            "fallible_non_night_unshielded_spends",
            fallible_non_night_unshielded_spends,
        )?;
        Ok(())
    }

    #[wasm_bindgen(getter, js_name = "fallibleNonNightUnshieldedOutputs")]
    pub fn fallible_non_night_unshielded_outputs(&self) -> usize {
        self.0.fallible_non_night_unshielded_outputs
    }

    #[wasm_bindgen(setter, js_name = "fallibleNonNightUnshieldedOutputs")]
    pub fn set_fallible_non_night_unshielded_outputs(
        &mut self,
        fallible_non_night_unshielded_outputs: f64,
    ) -> Result<(), JsError> {
        self.0.fallible_non_night_unshielded_outputs = count_from_js(
            "fallible_non_night_unshielded_outputs",
            fallible_non_night_unshielded_outputs,
        )?;
        Ok(())
    }

    #[wasm_bindgen(getter, js_name = "fallibleZswapInputs")]
    pub fn fallible_zswap_inputs(&self) -> usize {
        self.0.fallible_zswap_inputs
    }

    #[wasm_bindgen(setter, js_name = "fallibleZswapInputs")]
    pub fn set_fallible_zswap_inputs(&mut self, fallible_zswap_inputs: f64) -> Result<(), JsError> {
        self.0.fallible_zswap_inputs =
            count_from_js("fallible_zswap_inputs", fallible_zswap_inputs)?;
        Ok(())
    }

    #[wasm_bindgen(getter, js_name = "fallibleZswapOutputs")]
    pub fn fallible_zswap_outputs(&self) -> usize {
        self.0.fallible_zswap_outputs
    }

    #[wasm_bindgen(setter, js_name = "fallibleZswapOutputs")]
    pub fn set_fallible_zswap_outputs(
        &mut self,
        fallible_zswap_outputs: f64,
    ) -> Result<(), JsError> {
        self.0.fallible_zswap_outputs =
            count_from_js("fallible_zswap_outputs", fallible_zswap_outputs)?;
        Ok(())
    }

    #[wasm_bindgen(getter, js_name = "fallibleZswapTransients")]
    pub fn fallible_zswap_transients(&self) -> usize {
        self.0.fallible_zswap_transients
    }

    #[wasm_bindgen(setter, js_name = "fallibleZswapTransients")]
    pub fn set_fallible_zswap_transients(
        &mut self,
        fallible_zswap_transients: f64,
    ) -> Result<(), JsError> {
        self.0.fallible_zswap_transients =
            count_from_js("fallible_zswap_transients", fallible_zswap_transients)?;
        Ok(())
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

/// A count assigned from JavaScript, which must be a non-negative integer:
/// wasm-bindgen would wrap a negative number to a huge count and truncate a
/// fraction.
fn count_from_js(name: &str, value: f64) -> Result<usize, JsError> {
    if value.is_finite() && value >= 0.0 && value.fract() == 0.0 && value <= u32::MAX as f64 {
        Ok(value as usize)
    } else {
        Err(JsError::new(&format!(
            "TransactionShape count {name} must be a non-negative integer, got {value}"
        )))
    }
}

/// Reads an optional expected shape argument: a [`TransactionShape`], or a
/// pair of them giving the range between; `undefined` and `null` give `None`.
pub(crate) fn expected_tx_shape_from_value(
    value: &JsValue,
) -> Result<Option<ledger::construct::TransactionShapeRange>, JsError> {
    if value.is_undefined() || value.is_null() {
        return Ok(None);
    }
    let shape = |value: &JsValue| {
        TransactionShape::try_ref(value)?
            .map(|tx_shape| tx_shape.0.clone())
            .ok_or_else(|| {
                JsError::new("Expected TransactionShape or [TransactionShape, TransactionShape]")
            })
    };
    if Array::is_array(value) {
        let pair = Array::from(value);
        if pair.length() != 2 {
            return Err(JsError::new(
                "Expected a range of exactly two TransactionShapes",
            ));
        }
        return Ok(Some(ledger::construct::TransactionShapeRange::between(
            &shape(&pair.get(0))?,
            &shape(&pair.get(1))?,
        )));
    }
    Ok(Some(shape(value)?.into()))
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
    tx: JsValue,
) -> Result<Array, JsError> {
    let pre_transcripts = pre_transcripts_from_values(&calls)?;
    let expected_tx_shape = expected_tx_shape_from_value(&expected_tx_shape)?;
    let transcripts = if tx.is_undefined() || tx.is_null() {
        match expected_tx_shape {
            Some(expected_tx_shapes) => {
                ledger::construct::partition_transcripts_with_tx_shape_range(
                    &pre_transcripts,
                    params,
                    &expected_tx_shapes,
                )?
            }
            None => ledger::construct::partition_transcripts(&pre_transcripts, params)?,
        }
    } else {
        let tx = Transaction::try_ref(&tx)?.ok_or_else(|| JsError::new("Expected Transaction"))?;
        with_ledger_tx!(&tx.0, |tx| {
            ledger::construct::partition_transcripts_for_tx(
                &pre_transcripts,
                params,
                tx,
                expected_tx_shape.as_ref(),
            )
        })?
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
