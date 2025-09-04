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

use crate::state::ChargedState;
use crate::vm::VmStack;
use crate::{ensure_ops_valid, from_value, from_value_hex_ser, to_value, to_value_hex_ser};
use base_crypto::fab;
use onchain_runtime::context;
use onchain_runtime::cost_model::INITIAL_COST_MODEL;
use onchain_runtime::ops::Op;
use onchain_runtime::result_mode::ResultModeGather;
use onchain_runtime::transcript::Transcript;
use std::collections::HashMap;
use storage::db::InMemoryDB;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct QueryResults(context::QueryResults<ResultModeGather, InMemoryDB>);

#[wasm_bindgen]
impl QueryResults {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<QueryResults, JsError> {
        Err(JsError::new(
            "QueryResults cannot be constructed directly through the WASM API.",
        ))
    }

    #[wasm_bindgen(getter)]
    pub fn context(&self) -> QueryContext {
        QueryContext(self.0.context.clone())
    }

    #[wasm_bindgen(getter)]
    pub fn events(&self) -> Result<JsValue, JsError> {
        Ok(to_value(&self.0.events)?)
    }

    #[wasm_bindgen(getter = gasCost)]
    pub fn gas_cost(&self) -> Result<JsValue, JsError> {
        Ok(to_value(&self.0.gas_cost)?)
    }

    #[wasm_bindgen(js_name = "toString")]
    pub fn to_string(&self, compact: Option<bool>) -> String {
        if compact.unwrap_or(false) {
            format!("{:?}", &self.0)
        } else {
            format!("{:#?}", &self.0)
        }
    }
}

#[wasm_bindgen]
#[derive(Clone)]
pub struct QueryContext(context::QueryContext<InMemoryDB>);

impl From<context::QueryContext<InMemoryDB>> for QueryContext {
    fn from(ctxt: context::QueryContext<InMemoryDB>) -> QueryContext {
        QueryContext(ctxt)
    }
}

impl From<QueryContext> for context::QueryContext<InMemoryDB> {
    fn from(ctxt: QueryContext) -> context::QueryContext<InMemoryDB> {
        ctxt.0
    }
}

#[wasm_bindgen]
#[derive(Clone)]
pub struct CostModel(pub(crate) onchain_runtime::cost_model::CostModel);

impl From<onchain_runtime::cost_model::CostModel> for CostModel {
    fn from(model: onchain_runtime::cost_model::CostModel) -> CostModel {
        CostModel(model)
    }
}

impl From<CostModel> for onchain_runtime::cost_model::CostModel {
    fn from(model: CostModel) -> onchain_runtime::cost_model::CostModel {
        model.0
    }
}

#[wasm_bindgen]
impl CostModel {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<CostModel, JsError> {
        Err(JsError::new(
            "CostModel cannot be constructed directly through the WASM API.",
        ))
    }

    #[wasm_bindgen(js_name = "initialCostModel")]
    pub fn initial_cost_model() -> CostModel {
        CostModel(INITIAL_COST_MODEL)
    }

    #[wasm_bindgen(js_name = "toString")]
    pub fn to_string(&self, compact: Option<bool>) -> String {
        if compact.unwrap_or(false) {
            format!("{:?}", &self.0)
        } else {
            format!("{:#?}", &self.0)
        }
    }
}

#[wasm_bindgen]
impl QueryContext {
    #[wasm_bindgen(constructor)]
    pub fn new(state: &ChargedState, address: &str) -> Result<QueryContext, JsError> {
        Ok(QueryContext(context::QueryContext::new(
            state.0.clone(),
            from_value_hex_ser(address)?,
        )))
    }

    #[wasm_bindgen(getter)]
    pub fn state(&self) -> ChargedState {
        ChargedState(self.0.state.clone())
    }

    #[wasm_bindgen(getter)]
    pub fn address(&self) -> Result<String, JsError> {
        to_value_hex_ser(&self.0.address)
    }

    #[wasm_bindgen(getter)]
    pub fn effects(&self) -> Result<JsValue, JsError> {
        Ok(to_value(&self.0.effects)?)
    }

    #[wasm_bindgen(setter = effects)]
    pub fn set_effects(&mut self, effects: JsValue) -> Result<(), JsError> {
        self.0.effects = from_value(effects)?;
        Ok(())
    }

    #[wasm_bindgen(getter)]
    pub fn block(&self) -> Result<JsValue, JsError> {
        Ok(to_value(&self.0.call_context)?)
    }

    #[wasm_bindgen(setter = block)]
    pub fn set_block(&mut self, block: JsValue) -> Result<(), JsError> {
        self.0.call_context = from_value(block)?;
        Ok(())
    }

    #[wasm_bindgen(getter = comIndices)]
    pub fn com_indices(&self) -> Result<JsValue, JsError> {
        let indices: HashMap<String, u64> = self
            .0
            .call_context
            .com_indices
            .iter()
            .map(|(k, v)| Ok((to_value_hex_ser(&k)?, *v)))
            .collect::<Result<_, JsError>>()?;
        Ok(to_value(&indices)?)
    }

    #[wasm_bindgen(js_name = "insertCommitment")]
    pub fn insert_commitment(&self, comm: &str, index: u64) -> Result<QueryContext, JsError> {
        Ok(QueryContext(context::QueryContext {
            call_context: context::CallContext {
                com_indices: self
                    .0
                    .call_context
                    .com_indices
                    .insert(from_value_hex_ser(comm)?, index),
                ..self.0.call_context.clone()
            },
            ..self.0.clone()
        }))
    }

    pub fn qualify(&self, coin: JsValue) -> Result<JsValue, JsError> {
        let coin: fab::Value = from_value(coin)?;
        match self.0.qualify(&(&*coin).try_into()?) {
            Some(qci) => Ok(to_value(&fab::Value::try_from(qci)?)?),
            None => Ok(JsValue::UNDEFINED),
        }
    }

    #[wasm_bindgen(js_name = "runTranscript")]
    pub fn run_transcript(
        &self,
        transcript: JsValue,
        cost_model: &CostModel,
    ) -> Result<QueryContext, JsError> {
        let transcript: Transcript<InMemoryDB> = from_value(transcript)?;
        Ok(QueryContext(
            self.0.run_transcript(&transcript, &cost_model.0)?.context,
        ))
    }

    // query(ty: QueryType, args: Value): [QueryContext, AlignedValue]
    pub fn query(
        &self,
        ops: JsValue,
        cost_model: &CostModel,
        gas_limit: JsValue,
    ) -> Result<QueryResults, JsError> {
        let ops: Vec<Op<ResultModeGather, InMemoryDB>> = from_value(ops)?;
        ensure_ops_valid(&ops)?;
        let gas_limit = if gas_limit.is_null() || gas_limit.is_undefined() {
            None
        } else {
            Some(from_value(gas_limit)?)
        };
        Ok(QueryResults(self.0.query(
            &ops,
            gas_limit,
            &cost_model.0,
        )?))
    }

    #[wasm_bindgen(js_name = "toVmStack")]
    pub fn to_vm_stack(&self) -> VmStack {
        VmStack(self.0.to_vm_stack().clone())
    }

    #[wasm_bindgen(js_name = "toString")]
    pub fn to_string(&self, compact: Option<bool>) -> String {
        if compact.unwrap_or(false) {
            format!("{:?}", &self.0)
        } else {
            format!("{:#?}", &self.0)
        }
    }
}
