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
use crate::zswap_wasm::LedgerParameters;
use js_sys::{Array, JsString};
use onchain_runtime_wasm::context::QueryContext;
use storage::db::InMemoryDB;
use transient_crypto::curve::Fr;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
#[derive(Debug)]
pub struct PreTranscript {
    context: onchain_runtime::context::QueryContext<InMemoryDB>,
    program:
        Vec<onchain_runtime::ops::Op<onchain_runtime::result_mode::ResultModeVerify, InMemoryDB>>,
    comm_comm: Option<Fr>,
}

try_ref_for_exported!(PreTranscript);

#[wasm_bindgen]
impl PreTranscript {
    #[wasm_bindgen(constructor)]
    pub fn new(
        context: &QueryContext,
        program: JsValue,
        comm_comm: JsValue,
    ) -> Result<PreTranscript, JsError> {
        let comm_comm = if comm_comm.is_null() {
            None
        } else if comm_comm.is_undefined() {
            None
        } else if comm_comm.is_string() {
            let comm_comm_str = String::from(JsString::from(comm_comm));
            Some(from_hex_ser(&comm_comm_str)?)
        } else {
            return Err(JsError::new("expected string | undefined"));
        };
        Ok(PreTranscript {
            context: context.clone().into(),
            program: from_value(program)?,
            comm_comm,
        })
    }

    #[wasm_bindgen(js_name = "toString")]
    pub fn to_string(&self, compact: Option<bool>) -> String {
        if compact.unwrap_or(false) {
            format!("{:?}", &self)
        } else {
            format!("{:#?}", &self)
        }
    }
}

#[wasm_bindgen(js_name = "partitionTranscripts")]
pub fn partition_transcripts(
    calls: Vec<JsValue>,
    params: &LedgerParameters,
) -> Result<Array, JsError> {
    let owned_pre_transcripts = calls
        .iter()
        .map(|val| {
            PreTranscript::try_ref(val)
                .transpose()
                .ok_or_else(|| JsError::new("Expected PreTranscript"))
                .and_then(|x| x)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let borrowed_pre_transcripts = owned_pre_transcripts
        .iter()
        .map(|pt| ledger::construct::PreTranscript {
            context: &pt.context,
            program: &pt.program,
            comm_comm: pt.comm_comm,
        })
        .collect::<Vec<_>>();
    let transcripts = ledger::construct::partition_transcripts(&borrowed_pre_transcripts, params)?;
    let res = Array::new();
    for (guaranteed, fallible) in transcripts.into_iter() {
        let pair = Array::new();
        pair.push(&to_value(&guaranteed)?);
        pair.push(&to_value(&fallible)?);
        res.push(&pair);
    }
    Ok(res)
}
