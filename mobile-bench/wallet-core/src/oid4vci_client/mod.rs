//! Client side of OID4VCI Pre-Authorized Code Flow for the
//! Midnight `birth` credential family.
//!
//! Steps:
//! 1. `offer::parse_offer_url` extracts the offer object from
//!    the QR's `openid-credential-offer://` URL.
//! 2. `token::request_token` exchanges the pre-auth code for an
//!    access token + c_nonce.
//! 3. `credential::request_credential` mints a DID-bound JWS
//!    proof over the c_nonce, POSTs `{proof, format}` to
//!    the credential endpoint, parses the VC + openings, and
//!    hands them to `vc_store` atomically.

mod credential;
mod offer;
mod token;

pub use credential::{
    request_credential, CredentialBody, CredentialFlowError, IssuedVc, OpeningWire,
};
pub use offer::{parse_offer_url, CredentialOffer, Grants, Oid4vciParseError, PreAuthorized};
pub use token::{request_token, Oid4vciTokenError, TokenResponse};

/// Drive the full OID4VCI flow from a scanned QR URL.
pub async fn run_issuance(
    qr_url: &str,
    wallet: &crate::wallet::Wallet,
    secret_store: &dyn crate::secret_storage::SecretStorage,
    holder_did: &crate::DidId,
    vc_store: &crate::vc_store::VcStore,
) -> Result<String, IssuanceFlowError> {
    let offer = offer::parse_offer_url(qr_url)?;
    let code = offer.grants.pre_authorized.code.clone();
    let vc_uri = credential::request_credential(
        &offer.credential_issuer,
        &code,
        wallet,
        secret_store,
        holder_did,
        vc_store,
    )
    .await?;
    Ok(vc_uri)
}

#[derive(Debug, thiserror::Error)]
pub enum IssuanceFlowError {
    #[error(transparent)]
    Parse(#[from] offer::Oid4vciParseError),
    #[error(transparent)]
    Flow(#[from] credential::CredentialFlowError),
}

#[cfg(test)]
mod flow_tests {
    use super::*;
    use crate::test_support::{
        stub_secret_store_with_bootstrapped_did, stub_wallet_with_bootstrapped_did,
    };
    use base64::engine::general_purpose::STANDARD as B64;
    use base64::Engine;
    use tempfile::TempDir;
    use wiremock::{matchers::*, Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn run_issuance_happy_path() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "AT", "c_nonce": "CN", "token_type": "Bearer"
            })))
            .mount(&mock)
            .await;
        Mock::given(method("POST"))
            .and(path("/credential"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "credential": {
                    "vc_uri": "urn:uuid:flow-1",
                    "issuer_did": "did:midnight:i",
                    "holder_did": "did:midnight:h",
                    "body_b64": B64.encode(b"BODY")
                },
                "openings": []
            })))
            .mount(&mock)
            .await;

        let offer_json = serde_json::json!({
            "credential_issuer": mock.uri(),
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
        let (wallet, did) = stub_wallet_with_bootstrapped_did(seed).await;
        let store = stub_secret_store_with_bootstrapped_did(seed).await;
        let dir = TempDir::new().unwrap();
        let vc_store = crate::vc_store::VcStore::open(dir.path().join("v.redb")).unwrap();

        let uri = run_issuance(&qr, &wallet, &store, &did, &vc_store)
            .await
            .expect("ok");
        assert_eq!(uri, "urn:uuid:flow-1");
    }
}
