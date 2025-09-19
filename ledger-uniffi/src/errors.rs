use thiserror::Error;

#[derive(uniffi::Error, Debug, Error)]
pub enum FfiError {
    #[error("Invalid input: {details}")]
    InvalidInput { details: String },
    #[error("Deserialize error: {details}")]
    DeserializeError { details: String },
    #[error("Unsupported variant: {details}")]
    UnsupportedVariant { details: String },
    #[error("Segment mismatch: {details}")]
    SegmentMismatch { details: String },
    #[error("Already proof-erased")] 
    AlreadyProofErased,
    #[error("Internal error: {details}")]
    Internal { details: String },
}

impl From<std::io::Error> for FfiError {
    fn from(e: std::io::Error) -> Self { Self::DeserializeError { details: e.to_string() } }
}

impl From<serde_json::Error> for FfiError {
    fn from(e: serde_json::Error) -> Self { Self::DeserializeError { details: e.to_string() } }
}

impl From<anyhow::Error> for FfiError {
    fn from(e: anyhow::Error) -> Self { Self::Internal { details: e.to_string() } }
}

impl From<String> for FfiError {
    fn from(e: String) -> Self { Self::InvalidInput { details: e } }
}
