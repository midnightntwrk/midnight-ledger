//! Client-side implementation of OID4VP / SIOPv2.
//!
//! Phase 1 only handles the "pure authentication" subset:
//! the request carries no presentation_definition; the wallet
//! responds with a signed id_token (no VP token). The flow:
//!
//! 1. User scans a QR carrying `openid4vp://...?request_uri=https://issuer/.../request/<id>`.
//! 2. `parser::parse_request_url` extracts the request_uri.
//! 3. `parser::fetch_request_object` GETs it, returning a typed AuthRequest.
//! 4. `jws::build_id_token` constructs the SIOPv2 id_token JWS.
//! 5. `http::post_response` POSTs `{id_token, state}` to redirect_uri.

mod http;
mod jws;
mod parser;

pub use http::{post_response, PostResponseError, PostResponseResult};
pub use jws::{build_id_token, IdTokenError};
pub use parser::{parse_request_url, fetch_request_object, AuthRequest, Oid4vpParseError};

/// Drive the entire OID4VP / SIOPv2 authentication flow:
/// parse the QR URL -> fetch the request object -> mint a
/// DID-bound id_token -> POST it back -> return the issuer's
/// session_id + status.
pub async fn run_authentication(
    qr_url: &str,
    wallet: &crate::wallet::Wallet,
    secret_store: &dyn crate::secret_storage::SecretStorage,
    did: &crate::DidId,
) -> Result<http::PostResponseResult, AuthFlowError> {
    let request_uri = parser::parse_request_url(qr_url)?;
    let req = parser::fetch_request_object(&request_uri).await?;
    let id_token = jws::build_id_token(
        wallet,
        secret_store,
        did,
        &req.client_id,
        &req.nonce,
        300,
    )
    .await?;
    let result = http::post_response(&req.redirect_uri, &id_token, req.state.as_deref()).await?;
    Ok(result)
}

#[derive(Debug, thiserror::Error)]
pub enum AuthFlowError {
    #[error(transparent)]
    Parse(#[from] parser::Oid4vpParseError),
    #[error(transparent)]
    Token(#[from] jws::IdTokenError),
    #[error(transparent)]
    Post(#[from] http::PostResponseError),
}

#[cfg(test)]
mod flow_tests {
    use super::*;
    use crate::test_support::{
        stub_secret_store_with_bootstrapped_did, stub_wallet_with_bootstrapped_did,
    };
    use wiremock::{matchers::*, Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn run_authentication_happy_path() {
        let mock = MockServer::start().await;
        // 1. /request/abc returns the AuthRequest JSON.
        Mock::given(method("GET"))
            .and(path("/request/abc"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "client_id": "demo-issuer",
                "nonce": "nonce-x",
                "state": "st-x",
                "redirect_uri": format!("{}/authorize-response", mock.uri()),
            })))
            .mount(&mock)
            .await;
        // 2. /authorize-response accepts the POST.
        Mock::given(method("POST"))
            .and(path("/authorize-response"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "session_id": "S-42",
                "status": "authenticated"
            })))
            .mount(&mock)
            .await;

        let qr = format!(
            "openid4vp://demo/?request_uri={}/request/abc",
            urlencoding::encode(&mock.uri())
        );
        let seed = [21u8; 32];
        let (wallet, did) = stub_wallet_with_bootstrapped_did(seed).await;
        let store = stub_secret_store_with_bootstrapped_did(seed).await;

        let r = run_authentication(&qr, &wallet, &store, &did)
            .await
            .expect("ok");
        assert_eq!(r.session_id, "S-42");
        assert_eq!(r.status, "authenticated");
    }
}
