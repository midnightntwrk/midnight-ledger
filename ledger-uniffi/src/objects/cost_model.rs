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
}

#[uniffi::export]
pub fn transaction_cost_model_dummy() -> Result<Arc<TransactionCostModel>, FfiError> {
    Ok(Arc::new(TransactionCostModel { inner: Arc::new(ledger::structure::INITIAL_TRANSACTION_COST_MODEL) }))
}

#[uniffi::export]
pub fn transaction_cost_model_deserialize(raw: Vec<u8>) -> Result<Arc<TransactionCostModel>, FfiError> {
    let cursor = Cursor::new(raw);
    let val: ledger::structure::TransactionCostModel = tagged_deserialize(cursor)?;
    Ok(Arc::new(TransactionCostModel { inner: Arc::new(val) }))
}

impl TransactionCostModel {
    pub(crate) fn from_inner(inner: ledger::structure::TransactionCostModel) -> Self {
        Self { inner: Arc::new(inner) }
    }
    pub fn inner(&self) -> &ledger::structure::TransactionCostModel { &self.inner }
}
