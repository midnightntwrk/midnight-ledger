use std::sync::Arc;
use std::io::Cursor;

use base_crypto::time::Duration;
use serialize::{tagged_deserialize, tagged_serialize};

use crate::FfiError;

#[derive(uniffi::Object)]
pub struct DustParameters {
    inner: Arc<ledger::dust::DustParameters>,
}

#[uniffi::export]
impl DustParameters {
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

    // Field getters to align with WASM API surface (setters omitted due to UniFFI mutability constraints)
    pub fn night_dust_ratio(&self) -> u64 { self.inner.night_dust_ratio }
    pub fn generation_decay_rate(&self) -> u32 { self.inner.generation_decay_rate }
    pub fn dust_grace_period_seconds(&self) -> i64 { self.inner.dust_grace_period.as_seconds() as i64 }
    pub fn time_to_cap_seconds(&self) -> i64 { self.inner.time_to_cap().as_seconds() as i64 }
}

#[uniffi::export]
pub fn dust_parameters_new(
    night_dust_ratio: u64,
    generation_decay_rate: u32,
    dust_grace_period_seconds: i64,
) -> Result<Arc<DustParameters>, FfiError> {
    let params = ledger::dust::DustParameters {
        night_dust_ratio,
        generation_decay_rate,
        dust_grace_period: Duration::from_secs(dust_grace_period_seconds as i128),
    };
    Ok(Arc::new(DustParameters { inner: Arc::new(params) }))
}

#[uniffi::export]
pub fn dust_parameters_deserialize(raw: Vec<u8>) -> Result<Arc<DustParameters>, FfiError> {
    let cursor = Cursor::new(raw);
    let val: ledger::dust::DustParameters = tagged_deserialize(cursor)?;
    Ok(Arc::new(DustParameters { inner: Arc::new(val) }))
}

impl DustParameters {
    pub(crate) fn from_inner(inner: ledger::dust::DustParameters) -> Self {
        Self { inner: Arc::new(inner) }
    }
    pub fn inner(&self) -> &ledger::dust::DustParameters { &self.inner }
}
