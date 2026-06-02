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
pub mod proof;
mod token;

pub use credential::{
    request_credential, CredentialBody, CredentialFlowError, IssuedVc, OpeningWire,
};
pub use offer::{parse_offer_url, CredentialOffer, Grants, Oid4vciParseError, PreAuthorized};
pub use proof::{CredentialCoordinator, IdTokenProofBuilder, ProofBuilder, ProofValue};
pub use token::{request_token, Oid4vciTokenError, TokenResponse};

/// Drive the full OID4VCI flow from a scanned QR URL.
///
/// The coordinator owns the proof-of-possession step. Phase-1
/// callers wire `CredentialCoordinator::jwt(IdTokenProofBuilder)`
/// for the canonical JWT-typed proof — that uses the same
/// `Arc<dyn DidAuthnDiscovery>` + `Arc<dyn DidSigner>` pair the
/// OID4VP path (`LoginCoordinator::mode_a`) consumes, so a
/// session-scoped discovery cache covers both protocols.
pub async fn run_issuance(
    http: &dyn crate::HttpClient,
    clock: &std::sync::Arc<dyn crate::Clock>,
    qr_url: &str,
    coordinator: &CredentialCoordinator,
    vc_store: &dyn crate::vc_store::VcStorage,
) -> Result<String, IssuanceFlowError> {
    let offer = offer::parse_offer_url(qr_url)?;
    let code = offer.grants.pre_authorized.code.clone();
    let vc_uri = credential::request_credential(
        http,
        clock,
        &offer.credential_issuer,
        &code,
        coordinator,
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
    async fn run_issuance_happy_path() {
        let http = MockHttpClient::default();
        http.push_json(
            200,
            &json!({
                "access_token": "AT",
                "c_nonce": "CN",
                "token_type": "Bearer",
            }),
        );
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
        let (wallet, did) = stub_wallet_with_bootstrapped_did(seed).await;
        let store = stub_secret_store_with_bootstrapped_did(seed).await;
        let discovery = stub_authn_discovery(wallet);
        let signer = stub_did_signer(store);
        let vc_store = InMemoryVcStore::default();
        let clock: Arc<dyn Clock> = Arc::new(FixedClock::new(1_700_000_001_000));
        let coordinator = CredentialCoordinator::jwt(IdTokenProofBuilder::new(
            discovery,
            signer,
            clock.clone(),
            did,
        ));

        let uri = run_issuance(&http, &clock, &qr, &coordinator, &vc_store)
            .await
            .expect("ok");
        assert_eq!(uri, "urn:uuid:flow-1");

        let rec = http.recorded();
        assert_eq!(rec.len(), 2);
        assert_eq!(rec[0].url, "https://issuer.local/token");
        assert_eq!(rec[1].url, "https://issuer.local/credential");
    }
}
