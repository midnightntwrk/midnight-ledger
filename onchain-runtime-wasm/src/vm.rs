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

use crate::context::CostModel;
use crate::state::StateValue;
use crate::{ensure_ops_valid, from_value, to_value};
use onchain_runtime::ops::Op;
use onchain_runtime::result_mode::ResultModeGather;
use onchain_runtime::vm;
use onchain_runtime::vm_value::{ValueStrength, VmValue};
use storage::db::InMemoryDB;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct VmResults(vm::VmResults<ResultModeGather, InMemoryDB>);

#[wasm_bindgen]
impl VmResults {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<StateValue, JsError> {
        Err(JsError::new(
            "VmResults cannot be constructed directly through the WASM API.",
        ))
    }

    #[wasm_bindgen(getter)]
    pub fn stack(&self) -> VmStack {
        VmStack(self.0.stack.clone())
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
pub struct VmStack(pub(crate) Vec<VmValue<InMemoryDB>>);

#[wasm_bindgen]
impl VmStack {
    #[allow(clippy::new_without_default)]
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        VmStack(Vec::new())
    }

    pub fn push(&mut self, value: &StateValue, is_strong: bool) {
        self.0.push(VmValue {
            strength: if is_strong {
                ValueStrength::Strong
            } else {
                ValueStrength::Weak
            },
            value: value.0.clone(),
        })
    }

    #[wasm_bindgen(js_name = "removeLast")]
    pub fn remove_last(&mut self) {
        self.0.pop();
    }

    pub fn length(&self) -> usize {
        self.0.len()
    }

    pub fn get(&self, idx: usize) -> Option<StateValue> {
        self.0.get(idx).map(|v| StateValue(v.value.clone()))
    }

    #[wasm_bindgen(js_name = "isStrong")]
    pub fn is_strong(&self, idx: usize) -> Option<bool> {
        self.0.get(idx).map(|v| v.strength == ValueStrength::Strong)
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

#[wasm_bindgen(js_name = "runProgram")]
pub fn run_program(
    initial: &VmStack,
    ops: JsValue,
    cost_model: &CostModel,
    gas_limit: JsValue,
) -> Result<VmResults, JsError> {
    let ops: Vec<Op<ResultModeGather, InMemoryDB>> = from_value(ops)?;
    let gas_limit = if gas_limit.is_null() || gas_limit.is_undefined() {
        None
    } else {
        Some(from_value(gas_limit)?)
    };
    ensure_ops_valid(&ops)?;
    Ok(VmResults(vm::run_program(
        &initial.0,
        &ops,
        gas_limit,
        &cost_model.0,
    )?))
}
