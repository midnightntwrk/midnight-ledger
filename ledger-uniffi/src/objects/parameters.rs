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
// limitations under the License

use std::io::Cursor;
use std::sync::Arc;

use serialize::{tagged_deserialize, tagged_serialize};

use crate::FfiError;

#[derive(uniffi::Object)]
pub struct LedgerParameters {
    inner: Arc<ledger::structure::LedgerParameters>,
}

#[uniffi::export]
impl LedgerParameters {
    pub fn serialize(&self) -> Result<Vec<u8>, FfiError> {
        let mut buf = Vec::new();
        tagged_serialize(&*self.inner, &mut buf).map_err(|e| FfiError::DeserializeError {
            details: e.to_string(),
        })?;
        Ok(buf)
    }

    pub fn to_string(&self, compact: Option<bool>) -> String {
        if compact.unwrap_or(false) {
            format!("{:?}", &*self.inner)
        } else {
            format!("{:#?}", &*self.inner)
        }
    }

    // Getter to maintain parity with WASM API
    pub fn transaction_cost_model(&self) -> Arc<crate::objects::cost_model::TransactionCostModel> {
        Arc::new(
            crate::objects::cost_model::TransactionCostModel::from_inner(
                self.inner.cost_model.clone(),
            ),
        )
    }

    // Additional getter to mirror WASM API: parameters.dust
    pub fn dust(&self) -> Arc<crate::objects::dust::DustParameters> {
        Arc::new(crate::objects::dust::DustParameters::from_inner(
            self.inner.dust,
        ))
    }
}

#[uniffi::export]
pub fn ledger_parameters_dummy_parameters() -> Result<Arc<LedgerParameters>, FfiError> {
    Ok(Arc::new(LedgerParameters {
        inner: Arc::new(ledger::structure::INITIAL_PARAMETERS),
    }))
}

#[uniffi::export]
pub fn ledger_parameters_deserialize(raw: Vec<u8>) -> Result<Arc<LedgerParameters>, FfiError> {
    let cursor = Cursor::new(raw);
    let val: ledger::structure::LedgerParameters = tagged_deserialize(cursor)?;
    Ok(Arc::new(LedgerParameters {
        inner: Arc::new(val),
    }))
}

impl LedgerParameters {
    #[allow(dead_code)]
    pub(crate) fn from_inner(inner: ledger::structure::LedgerParameters) -> Self {
        Self {
            inner: Arc::new(inner),
        }
    }
    #[allow(dead_code)]
    pub fn inner(&self) -> &ledger::structure::LedgerParameters {
        &self.inner
    }
}
