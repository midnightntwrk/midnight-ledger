//! Client-side implementation of OID4VP / SIOPv2.
//!
//! Phase 1 only handles the "pure authentication" subset:
//! the request carries no presentation_definition; the wallet
//! responds with a signed id_token (no VP token). The flow:
//!
//! 1. User scans a QR carrying `openid4vp://...?request_uri=https://issuer/.../request/<id>`.
//! 2. `request::parse_request_url` extracts the request_uri.
//! 3. `request::fetch_request_object` GETs it, returning a typed
//!    [`AuthorizationRequest`].
//! 4. The [`LoginCoordinator`]'s `Vec<Box<dyn ResponseBuilder>>` is
//!    walked in declaration order; each builder mutates the shared
//!    [`AuthorizationResponse`]. Phase-1 Mode-A wires a single
//!    [`IdTokenBuilder`].
//! 5. `response::post_response` POSTs the response to the issuer's
//!    `response_uri`.
//!
//! The architecture is laid out in
//! `docs/superpowers/specs/2026-06-02-login-with-did-architecture.md`.
//!
//! ## Transitional duality
//!
//! Tasks 4-6 introduced the new typed shape alongside the legacy
//! `parser` / `jws` / `respond` modules. Task 8 swaps the
//! dioxus-wallet click site to the new coordinator-driven path;
//! Task 9 deletes the legacy modules + the `legacy_*` re-exports.

mod jws;
mod parser;
mod respond;
pub mod ports;

// Phase-1 normative shapes — typed AuthorizationRequest /
// AuthorizationResponse + parser/poster that match the OID4VP
// 1.0 + SIOPv2 wire format. The legacy `parser::AuthRequest`
// + `respond::PostResponseResult` are kept alongside until
// Task 8 (UI wire-in) and Task 9 (legacy purge) finish.
pub mod request;
pub mod response;

// Unified Phase-1 error taxonomy + JOSE-encoded id_token
// primitives + the chain-of-responsibility builder pattern.
// The IdTokenBuilder is the only builder in Phase 1; Phase 2
// adds VpTokenBuilder + PresentationSubmissionBuilder as
// sibling files.
pub mod errors;
pub mod id_token;
pub mod builders;

pub use jws::{build_id_token, IdTokenError};
pub use parser::{parse_request_url, fetch_request_object, AuthRequest, Oid4vpParseError};
pub use ports::{AuthnKey, DidAuthnDiscovery, DidSigner, DiscoverError, SignError};
pub use request::{
    AuthorizationRequest, PresentationDefinition, RequestParseError, ResponseMode,
    ResponseType,
};
pub use respond::{post_response, PostResponseError, PostResponseResult};
// `response::PostResponseError` shadows `respond::PostResponseError` and
// `response::PostResponseResult` shadows `respond::PostResponseResult` —
// they're the SAME type definitions, but live in two modules during the
// transition. Re-export the new module's names with prefixes so callers
// can pick the new types explicitly before Task 9 deletes the legacy.
pub use response::{
    AuthorizationResponse,
    PostResponseError as NewPostResponseError,
    PostResponseResult as NewPostResponseResult,
    post_response as new_post_response,
};
pub use builders::{IdTokenBuilder, ResponseBuilder};
pub use errors::LoginError;

// ── Legacy orchestrator (Tasks 4-6 transition; deleted in Task 9) ────

/// Legacy entry point retained for the duration of the
/// architecture refactor. The dioxus-wallet click site still
/// calls this via the `oid4vp_run_authentication` alias in
/// `wallet-core::lib`. Task 8 routes the click through
/// [`run_authentication`] (new) + [`LoginCoordinator`]; Task 9
/// deletes this function along with `jws` / `parser` / `respond`.
pub async fn legacy_run_authentication(
    http: &dyn crate::HttpClient,
    clock: &dyn crate::Clock,
    qr_url: &str,
    wallet: &crate::wallet::Wallet,
    secret_store: &dyn crate::secret_storage::SecretStorage,
    did: &crate::DidId,
) -> Result<respond::PostResponseResult, LegacyAuthFlowError> {
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

/// Legacy `AuthFlowError`. Kept under its original `AuthFlowError`
/// alias in `wallet-core::lib` until Task 8 migrates the only
/// caller (dioxus-wallet's `run_oid4vp_authenticate`).
#[derive(Debug, thiserror::Error)]
pub enum LegacyAuthFlowError {
    #[error(transparent)]
    Parse(#[from] parser::Oid4vpParseError),
    #[error(transparent)]
    Token(#[from] jws::IdTokenError),
    #[error(transparent)]
    Post(#[from] respond::PostResponseError),
}

// ── New coordinator-driven orchestrator (Task 6) ─────────────────────

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
    /// Construct from an explicit builder list. Callers that want
    /// fine-grained control (e.g. adding a custom builder in
    /// between Phase-1 and Phase-2 ones for telemetry) use this.
    pub fn new(builders: Vec<Box<dyn ResponseBuilder>>) -> Self {
        Self { builders }
    }

    /// Mode-A convenience: id_token only.
    pub fn mode_a(builder: IdTokenBuilder) -> Self {
        Self::new(vec![Box::new(builder)])
    }
}

/// Failure modes [`run_authentication`] can surface. Each variant
/// maps 1:1 to a step in the pipeline so the UI can present
/// step-specific recovery affordances (e.g. "issuer unreachable",
/// "your DID's authentication key is missing").
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
) -> Result<response::PostResponseResult, AuthFlowError> {
    let request_uri = request::parse_request_url(qr_url)?;
    let req = request::fetch_request_object(http, &request_uri).await?;
    let mut resp = AuthorizationResponse::new(req.state.clone());
    for b in &coordinator.builders {
        b.build(&req, &mut resp).await?;
    }
    let result = response::post_response(http, &req.response_uri, &resp).await?;
    Ok(result)
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod legacy_flow_tests {
    //! Smoke test for the soon-to-be-removed
    //! [`legacy_run_authentication`]. Kept until Task 9 so a
    //! regression in the legacy path during the transition is
    //! caught by CI.

    use super::*;
    use crate::clock::FixedClock;
    use crate::http::mock::MockHttpClient;
    use crate::test_support::{
        stub_secret_store_with_bootstrapped_did, stub_wallet_with_bootstrapped_did,
    };
    use serde_json::json;

    #[tokio::test]
    async fn legacy_run_authentication_happy_path() {
        let http = MockHttpClient::default();
        http.push_json(
            200,
            &json!({
                "client_id": "demo-issuer",
                "nonce": "nonce-x",
                "state": "st-x",
                "redirect_uri": "https://issuer.local/authorize-response",
            }),
        );
        http.push_json(
            200,
            &json!({ "session_id": "S-42", "status": "authenticated" }),
        );

        let qr =
            "openid4vp://demo/?request_uri=https%3A%2F%2Fissuer.local%2Frequest%2Fabc";
        let seed = [21u8; 32];
        let (wallet, did) = stub_wallet_with_bootstrapped_did(seed).await;
        let store = stub_secret_store_with_bootstrapped_did(seed).await;

        let clock = FixedClock::new(1_700_000_000_000);
        let r = legacy_run_authentication(&http, &clock, qr, &wallet, &store, &did)
            .await
            .expect("ok");
        assert_eq!(r.session_id, "S-42");
        assert_eq!(r.status, "authenticated");
    }
}

#[cfg(test)]
mod flow_tests {
    //! Happy-path test for the new coordinator-driven
    //! [`run_authentication`]. Uses the same
    //! `stub_wallet_with_bootstrapped_did` fixture so the
    //! discovery / signing under the hood is the real wallet-core
    //! path (no port mocks); only HTTP is stubbed.

    use std::sync::Arc;

    use serde_json::json;

    use super::*;
    use crate::clock::FixedClock;
    use crate::http::mock::MockHttpClient;
    use crate::test_support::{
        stub_secret_store_with_bootstrapped_did, stub_wallet_with_bootstrapped_did,
    };

    /// Wallet-core inherent adapter — wires the `Wallet`'s
    /// `resolve_did` into [`DidAuthnDiscovery`]. Mirrors what
    /// dioxus-wallet's `CachedWalletAuthnDiscovery` does, minus
    /// the cache. Lives inside the test module because no other
    /// wallet-core consumer needs it — the cache version is the
    /// one production callers use.
    struct WalletDiscovery {
        wallet: crate::wallet::Wallet,
    }
    #[async_trait::async_trait]
    impl DidAuthnDiscovery for WalletDiscovery {
        async fn authn_key(
            &self,
            did: &crate::DidId,
        ) -> Result<AuthnKey, DiscoverError> {
            let doc = self
                .wallet
                .resolve_did(&did.to_did_string())
                .await
                .map_err(|e| DiscoverError::Resolve(e.to_string()))?;
            let (kid, public_jwk) = match doc
                .authentication
                .first()
                .ok_or_else(|| DiscoverError::NoAuthnKey(did.to_did_string()))?
            {
                crate::VerificationMethodRef::Inline(vm) => {
                    (vm.id.clone(), vm.public_key_jwk.clone())
                }
                crate::VerificationMethodRef::Id(id) => {
                    let vm = doc
                        .verification_method
                        .iter()
                        .find(|v| v.id == *id)
                        .ok_or_else(|| {
                            DiscoverError::Resolve(format!(
                                "authentication kid {id} not in verificationMethod[]"
                            ))
                        })?;
                    (vm.id.clone(), vm.public_key_jwk.clone())
                }
            };
            Ok(AuthnKey { kid, public_jwk })
        }
    }

    /// Wallet-core inherent adapter — wraps `InMemorySecretStore`
    /// in the [`DidSigner`] port. Mirrors dioxus-wallet's
    /// `RedbDidSigner`; lives in the test module so the new
    /// `run_authentication` can be tested without dragging in
    /// dioxus-wallet's redb dep.
    struct InMemorySigner {
        store: crate::secret_storage::InMemorySecretStore,
    }
    #[async_trait::async_trait]
    impl DidSigner for InMemorySigner {
        async fn sign(
            &self,
            kid: &str,
            payload: &[u8],
        ) -> Result<Vec<u8>, SignError> {
            use crate::secret_storage::SecretStorage;
            let key_ref = self
                .store
                .find_by_kid(kid)
                .await
                .ok_or_else(|| SignError::NoLocalSecret(kid.to_string()))?;
            let out = self
                .store
                .sign(key_ref.uuid(), payload)
                .await
                .map_err(|e| SignError::Sign(e.to_string()))?;
            Ok(out.signature)
        }
    }

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

        // Test wallet + matching secret store fixture.
        let seed = [99u8; 32];
        let (wallet, did) = stub_wallet_with_bootstrapped_did(seed).await;
        let store = stub_secret_store_with_bootstrapped_did(seed).await;

        let coordinator = LoginCoordinator::mode_a(IdTokenBuilder::new(
            Arc::new(WalletDiscovery { wallet }),
            Arc::new(InMemorySigner { store }),
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

        // The wire shape: state echoed back, id_token present,
        // no vp_token / presentation_submission fields.
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
        // The wallet should fail FAST when an issuer requests a
        // VP-bearing mode in Phase 1 — surfacing as a typed
        // RequestParseError::UnsupportedMode rather than running
        // halfway through and emitting a malformed response.
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
            Arc::new(WalletDiscovery { wallet }),
            Arc::new(InMemorySigner { store }),
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

        // POST never fired — the wallet bailed at parse.
        let rec = http.recorded();
        assert_eq!(rec.len(), 1, "only the GET happened");
        assert_eq!(rec[0].method, "GET");
    }
}
