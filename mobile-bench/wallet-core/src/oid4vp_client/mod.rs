//! Client-side implementation of OID4VP / SIOPv2.
//!
//! Phase 1 only handles the "pure authentication" subset:
//! the request carries no `presentation_definition`; the wallet
//! responds with a signed id_token (no VP token). The flow:
//!
//! 1. User scans a QR carrying `openid4vp://...?request_uri=https://issuer/.../request/<id>`.
//! 2. [`request::parse_request_url`] extracts the request_uri.
//! 3. [`request::fetch_request_object`] GETs it, returning a typed
//!    [`AuthorizationRequest`].
//! 4. The [`LoginCoordinator`]'s `Vec<Box<dyn ResponseBuilder>>` is
//!    walked in declaration order; each builder mutates the shared
//!    [`AuthorizationResponse`]. Phase-1 Mode-A wires a single
//!    [`IdTokenBuilder`].
//! 5. [`response::post_response`] POSTs the response to the
//!    issuer's `response_uri`.
//!
//! Architecture spec: `docs/superpowers/specs/2026-06-02-login-with-did-architecture.md`.
//!
//! ## JWS construction
//!
//! Both OID4VP id_token signing and OID4VCI proof-of-possession
//! signing go through [`id_token::sign_id_token_with_ports`].
//! The legacy `jws::build_id_token` (sign-twice probe pattern,
//! JOSE-header self-asserted JWK) was deleted once OID4VCI was
//! migrated to the new ports — there's no longer any caller that
//! needs the legacy shape.

pub mod ports;
pub mod request;
pub mod response;
pub mod errors;
pub mod id_token;
pub mod builders;

pub use ports::{AuthnKey, DidAuthnDiscovery, DidSigner, DiscoverError, SignError};
pub use request::{
    AuthorizationRequest, PresentationDefinition, RequestParseError, ResponseMode,
    ResponseType,
};
pub use response::{
    AuthorizationResponse, PostResponseError, PostResponseResult, post_response,
};
pub use builders::{IdTokenBuilder, ResponseBuilder};
pub use errors::LoginError;

/// Chain-of-responsibility orchestrator for an OID4VP / SIOPv2
/// authorization response. Walks its `builders` vector in order;
/// each builder may read or mutate the shared
/// [`AuthorizationResponse`] before passing it on.
///
/// ## Modes
///
/// The same coordinator + [`run_authentication`] entry point
/// handle every protocol mode by varying the registered
/// builders:
///
/// | Mode | builders                                                                  |
/// |------|---------------------------------------------------------------------------|
/// | A    | `[IdTokenBuilder]` — Phase 1.                                             |
/// | B    | `[IdTokenBuilder, VpTokenBuilder, PresentationSubmissionBuilder]` — Phase 2.|
/// | C    | `[VpTokenBuilder, PresentationSubmissionBuilder]` — Phase 2.              |
///
/// Phase 1 ships [`LoginCoordinator::mode_a`] as a convenience;
/// Phase 2 will add `mode_b` / `mode_c` constructors without
/// changing the surface here.
pub struct LoginCoordinator {
    builders: Vec<Box<dyn ResponseBuilder>>,
}

impl LoginCoordinator {
    /// Construct from an explicit builder list. Callers that
    /// want fine-grained control (e.g. adding a custom builder
    /// in between Phase-1 and Phase-2 ones for telemetry) use
    /// this.
    pub fn new(builders: Vec<Box<dyn ResponseBuilder>>) -> Self {
        Self { builders }
    }

    /// Mode-A convenience: id_token only.
    pub fn mode_a(builder: IdTokenBuilder) -> Self {
        Self::new(vec![Box::new(builder)])
    }
}

/// Failure modes [`run_authentication`] can surface. Each
/// variant maps 1:1 to a step in the pipeline so the UI can
/// present step-specific recovery affordances (e.g. "issuer
/// unreachable", "your DID's authentication key is missing").
#[derive(Debug, thiserror::Error)]
pub enum AuthFlowError {
    #[error(transparent)]
    Request(#[from] RequestParseError),
    #[error(transparent)]
    Build(#[from] LoginError),
    #[error(transparent)]
    Post(#[from] response::PostResponseError),
}

/// Drive the OID4VP / SIOPv2 authorization flow end-to-end:
///
/// 1. Parse the QR `openid4vp://` URL → extract `request_uri`.
/// 2. GET the request object → typed [`AuthorizationRequest`].
/// 3. Walk `coordinator.builders` to populate
///    [`AuthorizationResponse`] (id_token in Phase 1; vp_token +
///    presentation_submission added in Phase 2).
/// 4. POST the response to the issuer's `response_uri`.
///
/// Returns the issuer's session id + status on success.
pub async fn run_authentication(
    http: &dyn crate::HttpClient,
    coordinator: &LoginCoordinator,
    qr_url: &str,
) -> Result<PostResponseResult, AuthFlowError> {
    let request_uri = request::parse_request_url(qr_url)?;
    let req = request::fetch_request_object(http, &request_uri).await?;
    let mut resp = AuthorizationResponse::new(req.state.clone());
    for b in &coordinator.builders {
        b.build(&req, &mut resp).await?;
    }
    let result = response::post_response(http, &req.response_uri, &resp).await?;
    Ok(result)
}

#[cfg(test)]
mod flow_tests {
    //! Happy-path test for the new coordinator-driven
    //! [`run_authentication`]. Uses the same
    //! `stub_wallet_with_bootstrapped_did` fixture so the
    //! discovery / signing under the hood is the real
    //! wallet-core path (no port mocks); only HTTP is stubbed.
    //!
    //! The full negative matrix lives in
    //! `wallet-core/tests/oid4vp_login_e2e.rs` — added by Task
    //! 10 of the Login-with-DID plan.

    use std::sync::Arc;

    use serde_json::json;

    use super::*;
    use crate::clock::FixedClock;
    use crate::http::mock::MockHttpClient;
    use crate::test_support::{
        stub_authn_discovery, stub_did_signer,
        stub_secret_store_with_bootstrapped_did, stub_wallet_with_bootstrapped_did,
    };

    #[tokio::test]
    async fn run_authentication_happy_path_via_coordinator() {
        let http = MockHttpClient::default();
        // 1. GET /request/abc → normative-shape AuthorizationRequest.
        http.push_json(
            200,
            &json!({
                "client_id": "did:midnight:issuer-mock",
                "response_type": "id_token",
                "response_mode": "direct_post",
                "response_uri": "https://issuer.local/authorize-response",
                "scope": "openid",
                "nonce": "nonce-y",
                "state": "st-y",
            }),
        );
        // 2. POST /authorize-response → session_id + status.
        http.push_json(
            200,
            &json!({ "session_id": "S-7", "status": "authenticated" }),
        );

        let seed = [99u8; 32];
        let (wallet, did) = stub_wallet_with_bootstrapped_did(seed).await;
        let store = stub_secret_store_with_bootstrapped_did(seed).await;

        let coordinator = LoginCoordinator::mode_a(IdTokenBuilder::new(
            stub_authn_discovery(wallet),
            stub_did_signer(store),
            Arc::new(FixedClock::new(1_700_000_000_000)),
            did,
        ));

        let qr =
            "openid4vp://demo/?request_uri=https%3A%2F%2Fissuer.local%2Frequest%2Fabc";
        let r = run_authentication(&http, &coordinator, qr)
            .await
            .expect("ok");
        assert_eq!(r.session_id, "S-7");
        assert_eq!(r.status, "authenticated");

        let rec = http.recorded();
        assert_eq!(rec.len(), 2);
        assert_eq!(rec[0].method, "GET");
        assert_eq!(rec[0].url, "https://issuer.local/request/abc");
        assert_eq!(rec[1].method, "POST");
        assert_eq!(rec[1].url, "https://issuer.local/authorize-response");

        let body = rec[1].body.as_ref().expect("body recorded");
        assert!(body["id_token"].is_string());
        assert_eq!(body["state"], "st-y");
        assert!(
            body.as_object().unwrap().get("vp_token").is_none(),
            "Phase 1 must not emit vp_token on the wire",
        );
        assert!(
            body.as_object()
                .unwrap()
                .get("presentation_submission")
                .is_none(),
            "Phase 1 must not emit presentation_submission on the wire",
        );
    }

    #[tokio::test]
    async fn run_authentication_rejects_vp_mode_at_request_parse() {
        let http = MockHttpClient::default();
        http.push_json(
            200,
            &json!({
                "client_id": "did:midnight:issuer-mock",
                "response_type": "vp_token id_token",
                "response_uri": "https://issuer.local/authorize-response",
                "nonce": "n",
            }),
        );

        let seed = [99u8; 32];
        let (wallet, did) = stub_wallet_with_bootstrapped_did(seed).await;
        let store = stub_secret_store_with_bootstrapped_did(seed).await;

        let coordinator = LoginCoordinator::mode_a(IdTokenBuilder::new(
            stub_authn_discovery(wallet),
            stub_did_signer(store),
            Arc::new(FixedClock::new(1_700_000_000_000)),
            did,
        ));

        let qr =
            "openid4vp://demo/?request_uri=https%3A%2F%2Fissuer.local%2Frequest%2Fabc";
        let err = run_authentication(&http, &coordinator, qr)
            .await
            .expect_err("must reject");
        assert!(matches!(
            err,
            AuthFlowError::Request(RequestParseError::UnsupportedMode(
                ResponseType::VpTokenIdToken
            ))
        ));

        let rec = http.recorded();
        assert_eq!(rec.len(), 1, "only the GET happened");
        assert_eq!(rec[0].method, "GET");
    }
}
