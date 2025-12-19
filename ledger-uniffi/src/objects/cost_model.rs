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
pub struct TransactionCostModel {
    inner: Arc<ledger::structure::TransactionCostModel>,
}

#[uniffi::export]
impl TransactionCostModel {
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
}

#[uniffi::export]
pub fn transaction_cost_model_dummy() -> Result<Arc<TransactionCostModel>, FfiError> {
    Ok(Arc::new(TransactionCostModel {
        inner: Arc::new(ledger::structure::INITIAL_TRANSACTION_COST_MODEL),
    }))
}

#[uniffi::export]
pub fn transaction_cost_model_deserialize(
    raw: Vec<u8>,
) -> Result<Arc<TransactionCostModel>, FfiError> {
    let cursor = Cursor::new(raw);
    let val: ledger::structure::TransactionCostModel = tagged_deserialize(cursor)?;
    Ok(Arc::new(TransactionCostModel {
        inner: Arc::new(val),
    }))
}

impl TransactionCostModel {
    pub(crate) fn from_inner(inner: ledger::structure::TransactionCostModel) -> Self {
        Self {
            inner: Arc::new(inner),
        }
    }
    pub fn inner(&self) -> &ledger::structure::TransactionCostModel {
        &self.inner
    }
}
