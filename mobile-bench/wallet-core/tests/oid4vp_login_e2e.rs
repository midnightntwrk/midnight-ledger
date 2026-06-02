//! End-to-end integration tests for the Login-with-DID OID4VP
//! Mode-A pipeline.
//!
//! Each test drives the full
//! [`wallet_core::oid4vp_client::run_authentication`] entry
//! point against a [`wallet_core::http::mock::MockHttpClient`]
//! that simulates a specific issuer-side response (200 happy,
//! or 401 with a typed error code matching the
//! IssuerDIDIT-mock pipeline's `VerifierError.code`). Plus the
//! wallet-side failure modes where the issuer never gets
//! reached (parser rejections; discovery / sign errors).
//!
//! Mapping to the normative test matrix
//! (`~/Downloads/login_with_did_oid4vp_siop2_implementation_guide.md`
//! §"Minimum test matrix"):
//!
//! | Guide test                                | Test fn                                        | Where it lives semantically |
//! |-------------------------------------------|------------------------------------------------|------------------------------|
//! | Valid DID login with fresh nonce          | `happy_path_returns_authenticated`             | wallet ↔ issuer round-trip  |
//! | Replay (reused nonce)                     | `issuer_401_reused_nonce_surfaces_post_status` | issuer-side; wallet sees 401|
//! | Wrong nonce                               | `issuer_401_invalid_nonce_surfaces_post_status`| issuer-side; wallet sees 401|
//! | Missing nonce                             | (issuer enforces; wallet always emits nonce — N/A on wallet)|
//! | Wrong audience                            | `issuer_401_invalid_audience_surfaces_post_status` | issuer-side; wallet sees 401 |
//! | Expired ID Token                          | `issuer_401_expired_token_surfaces_post_status`| issuer-side; wallet sees 401|
//! | Signature by unknown key                  | `issuer_401_invalid_signature_surfaces_post_status` | issuer-side; wallet sees 401 |
//! | DID key not authorized for authentication | `issuer_401_vm_not_authorized_surfaces_post_status` | issuer-side; wallet sees 401 |
//! | DID resolution failure                    | `wallet_discover_failure_short_circuits`       | wallet-side               |
//!
//! Additional wallet-side coverage beyond the guide:
//!
//! - `request_parser_rejects_vp_mode` — Phase-1 must hard-fail
//!   on a `response_type` that asks for vp_token, before any
//!   work happens.
//! - `request_parser_rejects_missing_response_target` —
//!   defensively reject a request object that has neither
//!   `response_uri` nor `redirect_uri`.
//! - `request_parser_rejects_malformed_url` — bad scheme,
//!   missing query param.
//! - `wallet_sign_failure_short_circuits` — DidSigner error
//!   prevents the POST.

#![cfg(feature = "test-support")]

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use wallet_core::clock::FixedClock;
use wallet_core::http::mock::MockHttpClient;
use wallet_core::oid4vp_client::{
    AuthFlowError, AuthnKey, DidAuthnDiscovery, DidSigner, DiscoverError,
    IdTokenBuilder, LoginCoordinator, LoginError, RequestParseError, ResponseType,
    SignError, run_authentication,
};
use wallet_core::test_support::{
    stub_authn_discovery, stub_did_signer,
    stub_secret_store_with_bootstrapped_did, stub_wallet_with_bootstrapped_did,
};
use wallet_core::DidId;

const ISSUER_CLIENT_ID: &str = "did:midnight:issuer-mock";
const RESPONSE_URI: &str = "https://issuer.local/authorize-response";
/// QR URL the wallet scans; its `request_uri` query param points
/// at `https://issuer.local/request/abc` — the URL the
/// MockHttpClient's first response is keyed against.
const QR_URL: &str =
    "openid4vp://demo/?request_uri=https%3A%2F%2Fissuer.local%2Frequest%2Fabc";

// Real-path discovery + signer come from
// `wallet_core::test_support::{stub_authn_discovery, stub_did_signer}`
// — shared with the OID4VP unit tests in `oid4vp_client::mod` and
// the OID4VCI tests in `oid4vci_client::credential`. That keeps the
// adapter wiring documented once, instead of duplicated across
// three test sites.

// ── Test-only discovery / signer that fail on demand, for the
//    wallet-short-circuit cases. ─────────────────────────────

struct FailingDiscovery(DiscoverError);
#[async_trait]
impl DidAuthnDiscovery for FailingDiscovery {
    async fn authn_key(&self, _did: &DidId) -> Result<AuthnKey, DiscoverError> {
        Err(match &self.0 {
            DiscoverError::Resolve(s) => DiscoverError::Resolve(s.clone()),
            DiscoverError::NoAuthnKey(s) => DiscoverError::NoAuthnKey(s.clone()),
        })
    }
}

struct FailingSigner(SignError);
#[async_trait]
impl DidSigner for FailingSigner {
    async fn sign(&self, _kid: &str, _payload: &[u8]) -> Result<Vec<u8>, SignError> {
        Err(match &self.0 {
            SignError::NoLocalSecret(s) => SignError::NoLocalSecret(s.clone()),
            SignError::Sign(s) => SignError::Sign(s.clone()),
        })
    }
}

// ── Builders ──────────────────────────────────────────────────

/// A coordinator whose discovery + signer + clock are all
/// real (driven by the `stub_wallet_with_bootstrapped_did`
/// fixture). HTTP is the only swappable surface — the
/// callers push the responses they want.
async fn real_coordinator() -> LoginCoordinator {
    let seed = [99u8; 32];
    let (wallet, did) = stub_wallet_with_bootstrapped_did(seed).await;
    let store = stub_secret_store_with_bootstrapped_did(seed).await;
    LoginCoordinator::mode_a(IdTokenBuilder::new(
        stub_authn_discovery(wallet),
        stub_did_signer(store),
        Arc::new(FixedClock::new(1_700_000_000_000)),
        did,
    ))
}

/// A request-object body in the normative shape — varies only
/// in the issuer's nonce + state per call.
fn request_body_normative(nonce: &str, state: &str) -> serde_json::Value {
    json!({
        "client_id": ISSUER_CLIENT_ID,
        "response_type": "id_token",
        "response_mode": "direct_post",
        "response_uri": RESPONSE_URI,
        "scope": "openid",
        "nonce": nonce,
        "state": state,
    })
}

// ── Happy path ────────────────────────────────────────────────

#[tokio::test]
async fn happy_path_returns_authenticated() {
    let http = MockHttpClient::default();
    http.push_json(200, &request_body_normative("nonce-1", "st-1"));
    http.push_json(
        200,
        &json!({ "session_id": "S-OK", "status": "authenticated" }),
    );

    let coordinator = real_coordinator().await;
    let r = run_authentication(&http, &coordinator, QR_URL)
        .await
        .expect("happy path");
    assert_eq!(r.session_id, "S-OK");
    assert_eq!(r.status, "authenticated");

    // Wire shape sanity: state echoed; id_token present;
    // no vp_token / presentation_submission.
    let rec = http.recorded();
    assert_eq!(rec.len(), 2);
    let post_body = rec[1].body.as_ref().expect("post body");
    assert!(post_body["id_token"].is_string());
    assert_eq!(post_body["state"], "st-1");
    assert!(post_body.as_object().unwrap().get("vp_token").is_none());
    assert!(
        post_body
            .as_object()
            .unwrap()
            .get("presentation_submission")
            .is_none()
    );
}

// ── Issuer-side 401 simulations ───────────────────────────────
//
// The wallet's view of every issuer rejection is the same shape:
// the POST returns 401 with a typed error code in the body. The
// wallet doesn't parse the code; it surfaces the whole
// `Status { status, body }` to the UI. These tests assert the
// flow propagates 401s correctly without confusion across the
// specific codes the IssuerDIDIT-mock pipeline emits.

async fn run_against_post_401(error_code: &str, message: &str) -> AuthFlowError {
    let http = MockHttpClient::default();
    http.push_json(200, &request_body_normative("nonce-x", "st-x"));
    http.push_json(401, &json!({ "error": error_code, "message": message }));
    let coordinator = real_coordinator().await;
    run_authentication(&http, &coordinator, QR_URL)
        .await
        .expect_err("must surface POST failure")
}

#[tokio::test]
async fn issuer_401_reused_nonce_surfaces_post_status() {
    let err = run_against_post_401("reused_nonce", "nonce already consumed").await;
    match err {
        AuthFlowError::Post(wallet_core::oid4vp_client::PostResponseError::Status {
            status,
            ref body,
        }) => {
            assert_eq!(status, 401);
            assert!(body.contains("reused_nonce"));
        }
        other => panic!("expected Post(Status 401), got {other:?}"),
    }
}

#[tokio::test]
async fn issuer_401_invalid_nonce_surfaces_post_status() {
    let err = run_against_post_401("invalid_nonce", "payload.nonce missing").await;
    assert!(matches!(
        err,
        AuthFlowError::Post(wallet_core::oid4vp_client::PostResponseError::Status {
            status: 401,
            ..
        })
    ));
}

#[tokio::test]
async fn issuer_401_invalid_audience_surfaces_post_status() {
    let err = run_against_post_401("invalid_audience", "aud != client_id").await;
    assert!(matches!(
        err,
        AuthFlowError::Post(wallet_core::oid4vp_client::PostResponseError::Status {
            status: 401,
            ..
        })
    ));
}

#[tokio::test]
async fn issuer_401_expired_token_surfaces_post_status() {
    let err = run_against_post_401("expired_token", "iat too old").await;
    assert!(matches!(
        err,
        AuthFlowError::Post(wallet_core::oid4vp_client::PostResponseError::Status {
            status: 401,
            ..
        })
    ));
}

#[tokio::test]
async fn issuer_401_invalid_signature_surfaces_post_status() {
    let err = run_against_post_401("invalid_signature", "JWS verify failed").await;
    assert!(matches!(
        err,
        AuthFlowError::Post(wallet_core::oid4vp_client::PostResponseError::Status {
            status: 401,
            ..
        })
    ));
}

#[tokio::test]
async fn issuer_401_vm_not_authorized_surfaces_post_status() {
    let err = run_against_post_401(
        "verification_method_not_authorized",
        "kid not in authentication relation",
    )
    .await;
    assert!(matches!(
        err,
        AuthFlowError::Post(wallet_core::oid4vp_client::PostResponseError::Status {
            status: 401,
            ..
        })
    ));
}

// ── Wallet-side short-circuits (never reach POST) ─────────────

#[tokio::test]
async fn wallet_discover_failure_short_circuits() {
    let http = MockHttpClient::default();
    http.push_json(200, &request_body_normative("n", "s"));
    // No POST response queued — the test asserts the wallet
    // never makes it that far.

    let seed = [42u8; 32];
    let (_wallet, did) = stub_wallet_with_bootstrapped_did(seed).await;
    let coordinator = LoginCoordinator::mode_a(IdTokenBuilder::new(
        Arc::new(FailingDiscovery(DiscoverError::Resolve(
            "indexer unreachable".into(),
        ))),
        Arc::new(FailingSigner(SignError::Sign("never reached".into()))),
        Arc::new(FixedClock::new(1_700_000_000_000)),
        did,
    ));

    let err = run_authentication(&http, &coordinator, QR_URL)
        .await
        .expect_err("must short-circuit");
    assert!(matches!(
        err,
        AuthFlowError::Build(LoginError::DiscoverFailed(ref m))
            if m.contains("indexer unreachable"),
    ));

    // Wallet stopped after GET; never POSTed.
    let rec = http.recorded();
    assert_eq!(rec.len(), 1);
    assert_eq!(rec[0].method, "GET");
}

#[tokio::test]
async fn wallet_sign_failure_short_circuits() {
    let http = MockHttpClient::default();
    http.push_json(200, &request_body_normative("n", "s"));

    let seed = [42u8; 32];
    let (wallet, did) = stub_wallet_with_bootstrapped_did(seed).await;
    let coordinator = LoginCoordinator::mode_a(IdTokenBuilder::new(
        // Real discovery (returns OK); failing signer.
        stub_authn_discovery(wallet),
        Arc::new(FailingSigner(SignError::NoLocalSecret(
            "did:midnight:undeployed:abc#key-auth".into(),
        ))),
        Arc::new(FixedClock::new(1_700_000_000_000)),
        did,
    ));

    let err = run_authentication(&http, &coordinator, QR_URL)
        .await
        .expect_err("must short-circuit");
    assert!(matches!(
        err,
        AuthFlowError::Build(LoginError::SignFailed(ref m))
            if m.contains("no local secret"),
    ));

    let rec = http.recorded();
    assert_eq!(rec.len(), 1);
    assert_eq!(rec[0].method, "GET");
}

// ── Wallet-side request-object parser failures ────────────────

#[tokio::test]
async fn request_parser_rejects_vp_mode() {
    let http = MockHttpClient::default();
    http.push_json(
        200,
        &json!({
            "client_id": ISSUER_CLIENT_ID,
            "response_type": "vp_token id_token",
            "response_uri": RESPONSE_URI,
            "nonce": "n",
        }),
    );
    let coordinator = real_coordinator().await;
    let err = run_authentication(&http, &coordinator, QR_URL)
        .await
        .expect_err("must reject");
    assert!(matches!(
        err,
        AuthFlowError::Request(RequestParseError::UnsupportedMode(
            ResponseType::VpTokenIdToken
        )),
    ));
}

#[tokio::test]
async fn request_parser_rejects_missing_response_target() {
    let http = MockHttpClient::default();
    http.push_json(
        200,
        &json!({
            "client_id": ISSUER_CLIENT_ID,
            "nonce": "n",
            // Neither response_uri nor redirect_uri.
        }),
    );
    let coordinator = real_coordinator().await;
    let err = run_authentication(&http, &coordinator, QR_URL)
        .await
        .expect_err("must reject");
    assert!(matches!(
        err,
        AuthFlowError::Request(RequestParseError::MissingResponseTarget),
    ));
}

#[tokio::test]
async fn request_parser_rejects_malformed_url() {
    let http = MockHttpClient::default();
    // Empty response queue — wallet should fail before any HTTP
    // call thanks to the URL-level parse.
    let coordinator = real_coordinator().await;
    let err = run_authentication(&http, &coordinator, "not-a-url")
        .await
        .expect_err("must reject");
    match err {
        AuthFlowError::Request(_) => { /* ok — any RequestParseError */ }
        other => panic!("expected Request(_), got {other:?}"),
    }
    assert!(http.recorded().is_empty(), "wallet must not hit network");
}

#[tokio::test]
async fn request_parser_rejects_wrong_scheme() {
    let http = MockHttpClient::default();
    let coordinator = real_coordinator().await;
    let err = run_authentication(
        &http,
        &coordinator,
        "https://issuer/?request_uri=x",
    )
    .await
    .expect_err("must reject");
    assert!(matches!(
        err,
        AuthFlowError::Request(RequestParseError::BadScheme(_)),
    ));
}

#[tokio::test]
async fn request_parser_rejects_missing_request_uri_param() {
    let http = MockHttpClient::default();
    let coordinator = real_coordinator().await;
    let err =
        run_authentication(&http, &coordinator, "openid4vp://issuer.local/")
            .await
            .expect_err("must reject");
    assert!(matches!(
        err,
        AuthFlowError::Request(RequestParseError::MissingParam("request_uri")),
    ));
}
