//! OID4VCI credential-issuer metadata discovery.
//!
//! Fetches and parses `/.well-known/openid-credential-issuer` from the
//! `credential_issuer` origin. The metadata provides endpoint URLs
//! (`token_endpoint`, `credential_endpoint`) and credential configurations
//! (`credential_configurations_supported`) that the wallet uses to
//! determine the correct issuance flow.

use std::collections::HashMap;

use serde::Deserialize;

use crate::http::{HttpError, HttpClient};

// ---------------------------------------------------------------------------
// Structs
// ---------------------------------------------------------------------------

/// Parsed response from
/// `GET {credential_issuer}/.well-known/openid-credential-issuer`.
#[derive(Debug, Clone, Deserialize)]
pub struct CredentialIssuerMetadata {
    pub credential_issuer: String,
    pub token_endpoint: String,
    pub credential_endpoint: String,
    #[serde(default)]
    pub credential_configurations_supported: HashMap<String, CredentialConfiguration>,
}

/// A single credential configuration entry inside
/// `credential_configurations_supported`.
#[derive(Debug, Clone, Deserialize)]
pub struct CredentialConfiguration {
    pub format: String,
    #[serde(default)]
    pub cryptographic_binding_methods_supported: Vec<String>,
    #[serde(default)]
    pub proof_types_supported: HashMap<String, ProofTypeMetadata>,
}

/// Extra metadata carried under each proof type key.
/// The OID4VCI spec allows arbitrary key-value pairs here; we keep
/// `proof_signing_alg_values_supported` as the only field we need.
#[derive(Debug, Clone, Deserialize)]
pub struct ProofTypeMetadata {
    #[serde(default)]
    pub proof_signing_alg_values_supported: Vec<String>,
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum MetadataError {
    #[error("http: {0}")]
    Http(String),
    #[error("non-2xx {status}: {body}")]
    Status { status: u16, body: String },
    #[error("decode: {0}")]
    Decode(#[from] serde_json::Error),
}

impl From<HttpError> for MetadataError {
    fn from(e: HttpError) -> Self {
        MetadataError::Http(e.to_string())
    }
}

// ---------------------------------------------------------------------------
// Fetch
// ---------------------------------------------------------------------------

/// Fetch and parse the credential-issuer metadata document.
///
/// Performs a GET to `{credential_issuer}/.well-known/openid-credential-issuer`
/// every time (no caching).
pub async fn fetch_metadata(
    http: &dyn HttpClient,
    credential_issuer: &str,
) -> Result<CredentialIssuerMetadata, MetadataError> {
    let url = format!(
        "{}/.well-known/openid-credential-issuer",
        credential_issuer.trim_end_matches('/')
    );
    let resp = http.get(&url).await?;
    let text = resp
        .body_text()
        .map_err(|e| MetadataError::Http(e.to_string()))?
        .to_string();
    if !resp.is_success() {
        return Err(MetadataError::Status {
            status: resp.status,
            body: text,
        });
    }
    Ok(serde_json::from_str(&text)?)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::mock::MockHttpClient;
    use serde_json::json;

    /// Mock-issuer metadata shape using `midnight-vc-compact` format.
    #[tokio::test]
    async fn fetch_metadata_parses_mock_issuer() {
        let http = MockHttpClient::default();
        http.push_json(
            200,
            &json!({
                "credential_issuer": "https://mock-issuer.local",
                "token_endpoint": "https://mock-issuer.local/oauth/token",
                "credential_endpoint": "https://mock-issuer.local/oauth/credential",
                "credential_configurations_supported": {
                    "birth": {
                        "format": "midnight-vc-compact",
                        "cryptographic_binding_methods_supported": ["did"],
                        "proof_types_supported": {
                            "jwt": {
                                "proof_signing_alg_values_supported": ["ES256"]
                            }
                        }
                    }
                }
            }),
        );

        let meta = fetch_metadata(&http, "https://mock-issuer.local")
            .await
            .expect("ok");

        assert_eq!(meta.credential_issuer, "https://mock-issuer.local");
        assert_eq!(meta.token_endpoint, "https://mock-issuer.local/oauth/token");
        assert_eq!(
            meta.credential_endpoint,
            "https://mock-issuer.local/oauth/credential"
        );

        let cfg = meta
            .credential_configurations_supported
            .get("birth")
            .expect("birth config");
        assert_eq!(cfg.format, "midnight-vc-compact");
        assert_eq!(cfg.cryptographic_binding_methods_supported, vec!["did"]);
        assert!(cfg.proof_types_supported.contains_key("jwt"));

        // Verify the GET hit the well-known path.
        let rec = http.recorded();
        assert_eq!(rec.len(), 1);
        assert_eq!(
            rec[0].url,
            "https://mock-issuer.local/.well-known/openid-credential-issuer"
        );
        assert_eq!(rec[0].method, "GET");
    }

    /// Passport-issuer metadata shape using `midnight_compact_vc` format.
    #[tokio::test]
    async fn fetch_metadata_parses_passport_issuer() {
        let http = MockHttpClient::default();
        http.push_json(
            200,
            &json!({
                "credential_issuer": "https://passport-issuer.local",
                "token_endpoint": "https://passport-issuer.local/token",
                "credential_endpoint": "https://passport-issuer.local/credential",
                "credential_configurations_supported": {
                    "passport": {
                        "format": "midnight_compact_vc",
                        "cryptographic_binding_methods_supported": ["did:midnight"],
                        "proof_types_supported": {
                            "jwt": {
                                "proof_signing_alg_values_supported": ["EdDSA"]
                            }
                        }
                    }
                }
            }),
        );

        let meta = fetch_metadata(&http, "https://passport-issuer.local")
            .await
            .expect("ok");

        assert_eq!(meta.credential_issuer, "https://passport-issuer.local");
        assert_eq!(meta.token_endpoint, "https://passport-issuer.local/token");
        assert_eq!(
            meta.credential_endpoint,
            "https://passport-issuer.local/credential"
        );

        let cfg = meta
            .credential_configurations_supported
            .get("passport")
            .expect("passport config");
        assert_eq!(cfg.format, "midnight_compact_vc");

        let rec = http.recorded();
        assert_eq!(rec.len(), 1);
        assert_eq!(
            rec[0].url,
            "https://passport-issuer.local/.well-known/openid-credential-issuer"
        );
    }

    #[tokio::test]
    async fn fetch_metadata_surfaces_non_2xx() {
        let http = MockHttpClient::default();
        http.push_status_body(404, b"not found");

        let err = fetch_metadata(&http, "https://issuer.local")
            .await
            .expect_err("should fail");
        match err {
            MetadataError::Status { status: 404, body } => assert_eq!(body, "not found"),
            other => panic!("expected Status error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn fetch_metadata_handles_trailing_slash_on_issuer() {
        let http = MockHttpClient::default();
        http.push_json(
            200,
            &json!({
                "credential_issuer": "https://issuer.local",
                "token_endpoint": "https://issuer.local/token",
                "credential_endpoint": "https://issuer.local/credential",
                "credential_configurations_supported": {}
            }),
        );

        let _ = fetch_metadata(&http, "https://issuer.local/")
            .await
            .expect("ok");

        // The URL should strip the trailing slash before appending the well-known path.
        let rec = http.recorded();
        assert_eq!(
            rec[0].url,
            "https://issuer.local/.well-known/openid-credential-issuer"
        );
    }
}