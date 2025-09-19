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
        tagged_serialize(&*self.inner, &mut buf).map_err(|e| FfiError::DeserializeError { details: e.to_string() })?;
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
        Arc::new(crate::objects::cost_model::TransactionCostModel::from_inner(self.inner.cost_model.clone()))
    }

    // Additional getter to mirror WASM API: parameters.dust
    pub fn dust(&self) -> Arc<crate::objects::dust::DustParameters> {
        Arc::new(crate::objects::dust::DustParameters::from_inner(self.inner.dust))
    }
}

#[uniffi::export]
pub fn ledger_parameters_dummy_parameters() -> Result<Arc<LedgerParameters>, FfiError> {
    Ok(Arc::new(LedgerParameters { inner: Arc::new(ledger::structure::INITIAL_PARAMETERS) }))
}

#[uniffi::export]
pub fn ledger_parameters_deserialize(raw: Vec<u8>) -> Result<Arc<LedgerParameters>, FfiError> {
    let cursor = Cursor::new(raw);
    let val: ledger::structure::LedgerParameters = tagged_deserialize(cursor)?;
    Ok(Arc::new(LedgerParameters { inner: Arc::new(val) }))
}

impl LedgerParameters {
    #[allow(dead_code)]
    pub(crate) fn from_inner(inner: ledger::structure::LedgerParameters) -> Self {
        Self { inner: Arc::new(inner) }
    }
    #[allow(dead_code)]
    pub fn inner(&self) -> &ledger::structure::LedgerParameters { &self.inner }
}
