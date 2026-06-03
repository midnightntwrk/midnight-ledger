//! Client side of OID4VCI Pre-Authorized Code Flow for the
//! Midnight credential family.
//!
//! Steps:
//! 1. `offer::parse_offer_url` extracts the offer object from
//!    the QR's `openid-credential-offer://` URL.
//! 2. `metadata::fetch_metadata` discovers the credential-issuer
//!    metadata from `/.well-known/openid-credential-issuer`,
//!    yielding endpoint URLs and credential configurations.
//! 3. `token::request_token` exchanges the pre-auth code for an
//!    access token + c_nonce.
//! 4. `credential::birth::request_credential` (birth format) or
//!    `credential::digital_passport::request_credential` (passport
//!    format) mints a DID-bound JWS proof, POSTs to the credential
//!    endpoint, parses the VC + openings, and lands in `vc_store`.

mod credential;
mod metadata;
mod offer;
pub mod proof;
mod token;

pub use credential::{
    birth, digital_passport, CredentialFlowError,
};
pub use metadata::{
    fetch_metadata, CredentialConfiguration, CredentialIssuerMetadata, MetadataError,
    ProofTypeMetadata,
};
pub use offer::{parse_offer_url, CredentialOffer, Grants, Oid4vciParseError, PreAuthorized};
pub use proof::{CredentialCoordinator, IdTokenProofBuilder, ProofBuilder, ProofValue};
pub use token::{request_token, Oid4vciTokenError, TokenResponse};

/// Determine the credential flow type based on the configuration ID.
///
/// Configuration IDs starting with `"digital_passport"` route to the
/// digital-passport flow (richer request/response with midnight
/// extension and compact-value-v1 encoding). All other IDs route to
/// the legacy birth flow (simple `{format, proof}` request).
fn is_passport_flow(config_id: &str) -> bool {
    config_id.starts_with("digital_passport")
}

/// Drive the full OID4VCI flow from a scanned QR URL.
///
/// 1. Parse the offer URL.
/// 2. Fetch credential-issuer metadata (every time — no caching).
/// 3. Look up the first credential configuration ID from the offer
///    in the metadata to get `format`, `token_endpoint`, and
///    `credential_endpoint`.
/// 4. Drive token → credential → store, dispatching to the birth
///    or digital-passport credential flow based on the configuration ID.
///
/// The coordinator owns the proof-of-possession step. Phase-1
/// callers wire `CredentialCoordinator::jwt(IdTokenProofBuilder)`
/// for the canonical JWT-typed proof — that uses the same
/// `Arc<dyn DidAuthnDiscovery>` + `Arc<dyn DidSigner>` pair the
/// OID4VP path (`LoginCoordinator::mode_a`) consumes, so a
/// session-scoped discovery cache covers both protocols.
///
/// `js_bridge` must be `Some` when the digital-passport flow is
/// taken (the bridge extracts `issuer_did` and verifies proofs).
/// For the birth (mock-issuer) flow `js_bridge` may be `None` — it
/// is not consulted.
pub async fn run_issuance(
    http: &dyn crate::HttpClient,
    clock: &dyn crate::Clock,
    js_bridge: Option<std::sync::Arc<dyn crate::js_bridge::JsBridge>>,
    qr_url: &str,
    coordinator: &CredentialCoordinator,
    wallet: &crate::Wallet,
    secret_store: &dyn crate::secret_storage::SecretStorage,
    holder_did: &crate::DidId,
    vc_store: &dyn crate::vc_store::VcStorage,
) -> Result<String, IssuanceFlowError> {
    let offer = offer::parse_offer_url(qr_url)?;
    let code = offer.grants.pre_authorized.code.clone();

    // Fetch metadata every time (no caching).
    let meta = metadata::fetch_metadata(http, &offer.credential_issuer).await?;

    // Look up the first configuration ID from the offer.
    let config_id = offer
        .credential_configuration_ids
        .first()
        .ok_or(IssuanceFlowError::MissingConfigurationId)?;
    let config = meta
        .credential_configurations_supported
        .get(config_id)
        .ok_or_else(|| IssuanceFlowError::UnknownConfigurationId(config_id.clone()))?;

    // Dispatch to the appropriate credential flow.
    let vc_uri = if is_passport_flow(config_id) {
        let bridge = js_bridge.ok_or_else(|| {
            IssuanceFlowError::JsBridgeUnavailable(
                "digital-passport flow requires the JS bridge but it was not available"
                    .into(),
            )
        })?;
        credential::digital_passport::request_credential(
            http,
            clock,
            &*bridge,
            &meta.credential_issuer,
            &meta.token_endpoint,
            &meta.credential_endpoint,
            &config.format,
            config_id,
            &code,
            coordinator,
            wallet,
            secret_store,
            holder_did,
            vc_store,
        )
        .await?
    } else {
        credential::birth::request_credential(
            http,
            clock,
            &meta.credential_issuer,
            &meta.token_endpoint,
            &meta.credential_endpoint,
            &config.format,
            &code,
            coordinator,
            vc_store,
        )
        .await?
    };
    Ok(vc_uri)
}

#[derive(Debug, thiserror::Error)]
pub enum IssuanceFlowError {
    #[error(transparent)]
    Parse(#[from] offer::Oid4vciParseError),
    #[error(transparent)]
    Metadata(#[from] metadata::MetadataError),
    #[error("offer has no credential_configuration_ids")]
    MissingConfigurationId,
    #[error("unknown credential configuration id: {0}")]
    UnknownConfigurationId(String),
    #[error("{0}")]
    JsBridgeUnavailable(String),
    #[error(transparent)]
    Flow(#[from] credential::CredentialFlowError),
}

#[cfg(test)]
mod flow_tests {
    use std::sync::Arc;

    use super::*;
    use crate::clock::{Clock, FixedClock};
    use crate::http::mock::MockHttpClient;
    use crate::test_support::{
        stub_authn_discovery, stub_did_signer,
        stub_secret_store_with_bootstrapped_did, stub_wallet_with_bootstrapped_did,
    };
    use crate::vc_store::InMemoryVcStore;
    use base64::engine::general_purpose::STANDARD as B64;
    use base64::Engine;
    use serde_json::json;

    #[tokio::test]
    async fn run_issuance_birth_happy_path() {
        let http = MockHttpClient::default();
        // 0. metadata discovery
        http.push_json(
            200,
            &json!({
                "credential_issuer": "https://issuer.local",
                "token_endpoint": "https://issuer.local/token",
                "credential_endpoint": "https://issuer.local/credential",
                "credential_configurations_supported": {
                    "birth": {
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
        // 1. /token
        http.push_json(
            200,
            &json!({
                "access_token": "AT",
                "c_nonce": "CN",
                "token_type": "Bearer",
            }),
        );
        // 2. /credential
        http.push_json(
            200,
            &json!({
                "credential": {
                    "vc_uri": "urn:uuid:flow-1",
                    "issuer_did": "did:midnight:i",
                    "holder_did": "did:midnight:h",
                    "body_b64": B64.encode(b"BODY")
                },
                "openings": []
            }),
        );

        let offer_json = json!({
            "credential_issuer": "https://issuer.local",
            "credential_configuration_ids": ["birth"],
            "grants": {
                "urn:ietf:params:oauth:grant-type:pre-authorized_code": {
                    "pre-authorized_code": "CODE-FLOW"
                }
            }
        })
        .to_string();
        let qr = format!(
            "openid-credential-offer://x/?credential_offer={}",
            urlencoding::encode(&offer_json),
        );

        let seed = [24u8; 32];
        // Two bootstrapped wallet/store pairs — one consumed by the
        // stub-port chain (`stub_authn_discovery`/`stub_did_signer`
        // take their argument by value), one held back for the
        // `run_issuance` reference args. Deterministic: the same
        // seed produces the same on-chain DID id from both calls.
        let (wallet, did) = stub_wallet_with_bootstrapped_did(seed).await;
        let store = stub_secret_store_with_bootstrapped_did(seed).await;
        let (w_for_stubs, _) = stub_wallet_with_bootstrapped_did(seed).await;
        let s_for_stubs = stub_secret_store_with_bootstrapped_did(seed).await;
        let discovery = stub_authn_discovery(w_for_stubs);
        let signer = stub_did_signer(s_for_stubs);
        let vc_store = InMemoryVcStore::default();
        let clock: Arc<dyn Clock> = Arc::new(FixedClock::new(1_700_000_001_000));
        let coordinator = CredentialCoordinator::jwt(IdTokenProofBuilder::new(
            discovery,
            signer,
            clock.clone(),
            did.clone(),
        ));

        // Birth flow does not need the JS bridge — pass None.
        let uri = run_issuance(&http, &*clock, None, &qr, &coordinator, &wallet, &store, &did, &vc_store)
            .await
            .expect("ok");
        assert_eq!(uri, "urn:uuid:flow-1");

        let rec = http.recorded();
        assert_eq!(rec.len(), 3);
        // 0: GET metadata
        assert_eq!(rec[0].method, "GET");
        assert_eq!(
            rec[0].url,
            "https://issuer.local/.well-known/openid-credential-issuer"
        );
        // 1: POST token
        assert_eq!(rec[1].url, "https://issuer.local/token");
        // 2: POST credential
        assert_eq!(rec[2].url, "https://issuer.local/credential");
    }

    #[tokio::test]
    async fn run_issuance_rejects_unknown_configuration_id() {
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

        let offer_json = json!({
            "credential_issuer": "https://issuer.local",
            "credential_configuration_ids": ["nonexistent"],
            "grants": {
                "urn:ietf:params:oauth:grant-type:pre-authorized_code": {
                    "pre-authorized_code": "CODE"
                }
            }
        })
        .to_string();
        let qr = format!(
            "openid-credential-offer://x/?credential_offer={}",
            urlencoding::encode(&offer_json),
        );

        let seed = [24u8; 32];
        let (wallet, did) = stub_wallet_with_bootstrapped_did(seed).await;
        let store = stub_secret_store_with_bootstrapped_did(seed).await;
        let (w_for_stubs, _) = stub_wallet_with_bootstrapped_did(seed).await;
        let s_for_stubs = stub_secret_store_with_bootstrapped_did(seed).await;
        let discovery = stub_authn_discovery(w_for_stubs);
        let signer = stub_did_signer(s_for_stubs);
        let vc_store = InMemoryVcStore::default();
        let clock: Arc<dyn Clock> = Arc::new(FixedClock::new(1_700_000_001_000));
        let coordinator = CredentialCoordinator::jwt(IdTokenProofBuilder::new(
            discovery,
            signer,
            clock.clone(),
            did.clone(),
        ));

        // Unknown config ID doesn't need a JS bridge either.
        let err = run_issuance(&http, &*clock, None, &qr, &coordinator, &wallet, &store, &did, &vc_store)
            .await
            .expect_err("should fail");
        match err {
            IssuanceFlowError::UnknownConfigurationId(id) => {
                assert_eq!(id, "nonexistent");
            }
            other => panic!("expected UnknownConfigurationId, got {other:?}"),
        }
    }

    #[test]
    fn is_passport_flow_detects_digital_passport_prefix() {
        assert!(is_passport_flow("digital_passport_v1"));
        assert!(is_passport_flow("digital_passport"));
        assert!(!is_passport_flow("birth"));
        assert!(!is_passport_flow("something_else"));
    }

    #[tokio::test]
    async fn run_issuance_passport_requires_js_bridge() {
        let http = MockHttpClient::default();
        // metadata for a passport-issuer config
        http.push_json(
            200,
            &json!({
                "credential_issuer": "https://passport-issuer.local",
                "token_endpoint": "https://passport-issuer.local/token",
                "credential_endpoint": "https://passport-issuer.local/credential",
                "credential_configurations_supported": {
                    "digital_passport_v1": {
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

        let offer_json = json!({
            "credential_issuer": "https://passport-issuer.local",
            "credential_configuration_ids": ["digital_passport_v1"],
            "grants": {
                "urn:ietf:params:oauth:grant-type:pre-authorized_code": {
                    "pre-authorized_code": "CODE"
                }
            }
        })
        .to_string();
        let qr = format!(
            "openid-credential-offer://x/?credential_offer={}",
            urlencoding::encode(&offer_json),
        );

        let seed = [77u8; 32];
        let (wallet, did) = stub_wallet_with_bootstrapped_did(seed).await;
        let store = stub_secret_store_with_bootstrapped_did(seed).await;
        let (w_for_stubs, _) = stub_wallet_with_bootstrapped_did(seed).await;
        let s_for_stubs = stub_secret_store_with_bootstrapped_did(seed).await;
        let discovery = stub_authn_discovery(w_for_stubs);
        let signer = stub_did_signer(s_for_stubs);
        let vc_store = InMemoryVcStore::default();
        let clock: Arc<dyn Clock> = Arc::new(FixedClock::new(1_700_000_001_000));
        let coordinator = CredentialCoordinator::jwt(IdTokenProofBuilder::new(
            discovery,
            signer,
            clock.clone(),
            did.clone(),
        ));

        let err = run_issuance(
            &http, &*clock, None, &qr, &coordinator, &wallet, &store, &did, &vc_store,
        )
        .await
        .expect_err("passport flow without JS bridge should fail");
        match err {
            IssuanceFlowError::JsBridgeUnavailable(msg) => {
                assert!(
                    msg.contains("JS bridge") || msg.contains("digital-passport"),
                    "error message should mention JS bridge or digital-passport: {msg}"
                );
            }
            other => panic!("expected JsBridgeUnavailable, got {other:?}"),
        }
    }
}