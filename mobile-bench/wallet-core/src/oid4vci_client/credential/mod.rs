//! Credential sub-modules: birth (legacy Compact VC) and
//! digital_passport (passport-issuer Compact VC with midnight
//! extension).

pub mod birth;
pub mod digital_passport;

use crate::http::HttpError;

/// Errors shared by both credential flows.
#[derive(Debug, thiserror::Error)]
pub enum CredentialFlowError {
    #[error("http: {0}")]
    Http(#[from] HttpError),
    #[error("non-2xx {status}: {body}")]
    Status { status: u16, body: String },
    #[error("decode: {0}")]
    Decode(#[from] serde_json::Error),
    #[error("base64: {0}")]
    Base64(#[from] base64::DecodeError),
    #[error("token error: {0}")]
    Token(#[from] crate::oid4vci_client::token::Oid4vciTokenError),
    #[error("proof JWS error: {0}")]
    Proof(#[from] crate::oid4vp_client::LoginError),
    #[error("vc_store: {0}")]
    Store(String),
    #[error("js bridge: {0}")]
    JsBridge(String),
    #[error("secret store: {0}")]
    SecretStore(String),
}