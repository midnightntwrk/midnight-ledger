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
//! 5. `respond::post_response` POSTs `{id_token, state}` to redirect_uri.

mod jws;
mod parser;
mod respond;

pub use jws::{build_id_token, IdTokenError};
pub use parser::{parse_request_url, fetch_request_object, AuthRequest, Oid4vpParseError};
pub use respond::{post_response, PostResponseError, PostResponseResult};

/// Drive the entire OID4VP / SIOPv2 authentication flow:
/// parse the QR URL -> fetch the request object -> mint a
/// DID-bound id_token -> POST it back -> return the issuer's
/// session_id + status.
pub async fn run_authentication(
    http: &dyn crate::HttpClient,
    clock: &dyn crate::Clock,
    qr_url: &str,
    wallet: &crate::wallet::Wallet,
    secret_store: &dyn crate::secret_storage::SecretStorage,
    did: &crate::DidId,
) -> Result<respond::PostResponseResult, AuthFlowError> {
    let request_uri = parser::parse_request_url(qr_url)?;
    let req = parser::fetch_request_object(http, &request_uri).await?;
    let id_token = jws::build_id_token(
        wallet,
        secret_store,
        clock,
        did,
        &req.client_id,
        &req.nonce,
        300,
    )
    .await?;
    let result =
        respond::post_response(http, &req.redirect_uri, &id_token, req.state.as_deref()).await?;
    Ok(result)
}

#[derive(Debug, thiserror::Error)]
pub enum AuthFlowError {
    #[error(transparent)]
    Parse(#[from] parser::Oid4vpParseError),
    #[error(transparent)]
    Token(#[from] jws::IdTokenError),
    #[error(transparent)]
    Post(#[from] respond::PostResponseError),
}

#[cfg(test)]
mod flow_tests {
    use super::*;
    use crate::clock::FixedClock;
    use crate::http::mock::MockHttpClient;
    use crate::test_support::{
        stub_secret_store_with_bootstrapped_did, stub_wallet_with_bootstrapped_did,
    };
    use serde_json::json;

    #[tokio::test]
    async fn run_authentication_happy_path() {
        let http = MockHttpClient::default();
        // 1. GET /request/abc → AuthRequest JSON.
        http.push_json(
            200,
            &json!({
                "client_id": "demo-issuer",
                "nonce": "nonce-x",
                "state": "st-x",
                "redirect_uri": "https://issuer.local/authorize-response",
            }),
        );
        // 2. POST /authorize-response → session_id + status.
        http.push_json(
            200,
            &json!({
                "session_id": "S-42",
                "status": "authenticated",
            }),
        );

        let qr =
            "openid4vp://demo/?request_uri=https%3A%2F%2Fissuer.local%2Frequest%2Fabc";
        let seed = [21u8; 32];
        let (wallet, did) = stub_wallet_with_bootstrapped_did(seed).await;
        let store = stub_secret_store_with_bootstrapped_did(seed).await;

        let clock = FixedClock::new(1_700_000_000_000);
        let r = run_authentication(&http, &clock, qr, &wallet, &store, &did)
            .await
            .expect("ok");
        assert_eq!(r.session_id, "S-42");
        assert_eq!(r.status, "authenticated");

        let rec = http.recorded();
        assert_eq!(rec.len(), 2);
        assert_eq!(rec[0].method, "GET");
        assert_eq!(rec[0].url, "https://issuer.local/request/abc");
        assert_eq!(rec[1].method, "POST");
        assert_eq!(rec[1].url, "https://issuer.local/authorize-response");
        let posted = rec[1].body.as_ref().expect("post body recorded");
        assert_eq!(posted["state"], "st-x");
        assert!(posted["id_token"].is_string());
    }
}
