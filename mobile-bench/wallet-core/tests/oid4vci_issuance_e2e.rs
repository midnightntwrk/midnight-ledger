//! End-to-end integration tests for the OID4VCI Pre-Authorized
//! Code Flow driven by the new
//! [`wallet_core::oid4vci_client::CredentialCoordinator`] +
//! [`wallet_core::oid4vci_client::ProofBuilder`] pipeline (audit
//! §5.B, commit `9a3e5e21`).
//!
//! Coverage mirrors the OID4VP `oid4vp_login_e2e.rs` matrix as
//! closely as the OID4VCI shape allows:
//!
//! | Scenario                                  | Test fn                                          |
//! |-------------------------------------------|--------------------------------------------------|
//! | Happy path (token + credential + landed)  | `happy_path_lands_vc_and_openings`               |
//! | /token returns 4xx                        | `token_400_surfaces_status`                      |
//! | /credential returns 4xx with the issuer's typed code | `credential_401_surfaces_status_*` family |
//! | Discovery short-circuits (no authn key)   | `discover_failure_short_circuits`                |
//! | Signer short-circuits (no local secret)   | `sign_failure_short_circuits`                    |
//! | Custom proof builder                      | `custom_proof_builder_replaces_default`          |
//!
//! Same MockHttpClient driver pattern: each test stages a
//! response queue, lets the orchestrator pull them, then asserts
//! on the recorded HTTP traffic + the final
//! `Result<vc_uri, IssuanceFlowError>`.

#![cfg(feature = "test-support")]

use std::sync::Arc;

use async_trait::async_trait;
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use serde_json::json;

use wallet_core::clock::{Clock, FixedClock};
use wallet_core::http::mock::MockHttpClient;
use wallet_core::oid4vci_client::{
    run_issuance, CredentialCoordinator, CredentialFlowError, IdTokenProofBuilder,
    IssuanceFlowError, ProofBuilder, ProofValue,
};
use wallet_core::oid4vp_client::{
    DidAuthnDiscovery, DidSigner, DiscoverError, LoginError, SignError,
};
use wallet_core::test_support::{
    stub_authn_discovery, stub_did_signer, stub_secret_store_with_bootstrapped_did,
    stub_wallet_with_bootstrapped_did,
};
use wallet_core::vc_store::InMemoryVcStore;
use wallet_core::DidId;

const SEED: [u8; 32] = [99u8; 32];
const ISSUER_URL: &str = "https://issuer.local";
const QR_URL_TPL: &str = "openid-credential-offer://issuer.local/?credential_offer=";

fn fixed_clock() -> Arc<dyn Clock> {
    Arc::new(FixedClock::new(1_700_000_000_000))
}

fn offer_url(pre_auth_code: &str) -> String {
    let offer = json!({
        "credential_issuer": ISSUER_URL,
        "credential_configuration_ids": ["birth"],
        "grants": {
            "urn:ietf:params:oauth:grant-type:pre-authorized_code": {
                "pre-authorized_code": pre_auth_code
            }
        }
    })
    .to_string();
    format!("{QR_URL_TPL}{}", urlencoding::encode(&offer))
}

fn token_ok() -> serde_json::Value {
    json!({
        "access_token": "AT",
        "c_nonce": "CN-1",
        "token_type": "Bearer",
        "expires_in": 600,
    })
}

fn credential_ok(vc_uri: &str) -> serde_json::Value {
    json!({
        "credential": {
            "vc_uri": vc_uri,
            "issuer_did": "did:midnight:issuer",
            "holder_did": "did:midnight:alice",
            "body_b64": B64.encode(b"COMPACT_VC_BYTES"),
        },
        "openings": [
            {
                "claim_path": "/credentialSubject/dateOfBirth",
                "plaintext_b64": B64.encode(b"1985-01-01"),
                "opening_b64":   B64.encode(b"rand"),
            }
        ]
    })
}

/// Construct a coordinator backed by real (stub) DID resolve +
/// in-memory secret-store signing. HTTP is the only swappable
/// surface in these tests — every flow goes through `MockHttpClient`.
async fn real_coordinator() -> (CredentialCoordinator, DidId) {
    let (wallet, did) = stub_wallet_with_bootstrapped_did(SEED).await;
    let store = stub_secret_store_with_bootstrapped_did(SEED).await;
    let coord = CredentialCoordinator::jwt(IdTokenProofBuilder::new(
        stub_authn_discovery(wallet),
        stub_did_signer(store),
        fixed_clock(),
        did.clone(),
    ));
    (coord, did)
}

// ── Happy path ───────────────────────────────────────────────────

#[tokio::test]
async fn happy_path_lands_vc_and_openings() {
    let http = MockHttpClient::default();
    http.push_json(200, &token_ok());
    http.push_json(200, &credential_ok("urn:uuid:flow-1"));

    let (coord, _did) = real_coordinator().await;
    let vc_store = InMemoryVcStore::default();
    let clock = fixed_clock();

    let vc_uri = run_issuance(&http, &clock, &offer_url("CODE-1"), &coord, &vc_store)
        .await
        .expect("issuance ok");
    assert_eq!(vc_uri, "urn:uuid:flow-1");

    // Two requests, ordered: /token then /credential.
    let rec = http.recorded();
    assert_eq!(rec.len(), 2);
    assert_eq!(rec[0].url, format!("{ISSUER_URL}/token"));
    assert!(rec[0].bearer.is_none(), "/token has no Bearer");
    assert_eq!(rec[1].url, format!("{ISSUER_URL}/credential"));
    assert_eq!(rec[1].bearer.as_deref(), Some("AT"));

    // The proof envelope went through with the coordinator's default
    // proof_type (`jwt`). Phase-2 builders would swap this.
    let posted = rec[1].body.as_ref().expect("credential body");
    assert_eq!(posted["format"], "midnight-vc-compact");
    assert_eq!(posted["proof"]["proof_type"], "jwt");
    assert!(posted["proof"]["jwt"].is_string());
}

// ── Issuer-side rejections (4xx with typed body) ──────────────────

/// `/token` 400 maps to `IssuanceFlowError::Flow(Token(Status …))`.
#[tokio::test]
async fn token_400_surfaces_status() {
    let http = MockHttpClient::default();
    http.push_json(
        400,
        &json!({ "error": "invalid_grant", "error_description": "code expired" }),
    );

    let (coord, _did) = real_coordinator().await;
    let vc_store = InMemoryVcStore::default();
    let clock = fixed_clock();

    let err = run_issuance(&http, &clock, &offer_url("CODE-X"), &coord, &vc_store)
        .await
        .expect_err("must fail");
    assert!(
        matches!(
            err,
            IssuanceFlowError::Flow(CredentialFlowError::Token(_)),
        ),
        "got {err:?}",
    );
}

/// Helper: run the flow with /token OK and /credential returning a
/// typed 401 body. The wallet surfaces the raw response so the UI
/// can show the issuer's `error_description`.
async fn run_against_credential_401(
    error_code: &str,
    message: &str,
) -> IssuanceFlowError {
    let http = MockHttpClient::default();
    http.push_json(200, &token_ok());
    http.push_json(
        401,
        &json!({ "error": error_code, "error_description": message }),
    );

    let (coord, _did) = real_coordinator().await;
    let vc_store = InMemoryVcStore::default();
    let clock = fixed_clock();
    run_issuance(&http, &clock, &offer_url("CODE-1"), &coord, &vc_store)
        .await
        .expect_err("must fail")
}

#[tokio::test]
async fn credential_401_surfaces_status_invalid_proof() {
    let err = run_against_credential_401(
        "invalid_proof",
        "JWS signature did not verify",
    )
    .await;
    let IssuanceFlowError::Flow(CredentialFlowError::Status { status, body }) = err
    else {
        panic!("expected Status, got {err:?}");
    };
    assert_eq!(status, 401);
    assert!(body.contains("invalid_proof"), "body: {body}");
}

#[tokio::test]
async fn credential_401_surfaces_status_invalid_nonce() {
    let err = run_against_credential_401(
        "invalid_or_missing_proof",
        "c_nonce does not match the issued one",
    )
    .await;
    assert!(matches!(
        err,
        IssuanceFlowError::Flow(CredentialFlowError::Status { status: 401, .. })
    ));
}

#[tokio::test]
async fn credential_401_surfaces_status_unknown_credential() {
    let err = run_against_credential_401(
        "unsupported_credential_format",
        "midnight-vc-compact not in credential_configurations_supported",
    )
    .await;
    assert!(matches!(
        err,
        IssuanceFlowError::Flow(CredentialFlowError::Status { status: 401, .. })
    ));
}

// ── Wallet-side short-circuits (never reach the issuer's /credential) ─

struct FailingDiscovery(DiscoverError);
#[async_trait]
impl DidAuthnDiscovery for FailingDiscovery {
    async fn authn_key(
        &self,
        _did: &DidId,
    ) -> Result<wallet_core::oid4vp_client::AuthnKey, DiscoverError> {
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

#[tokio::test]
async fn discover_failure_short_circuits() {
    let http = MockHttpClient::default();
    http.push_json(200, &token_ok());
    // /credential should never be reached → don't queue it.

    let (_, did) = stub_wallet_with_bootstrapped_did(SEED).await;
    let coord = CredentialCoordinator::jwt(IdTokenProofBuilder::new(
        Arc::new(FailingDiscovery(DiscoverError::NoAuthnKey(
            did.to_did_string(),
        ))) as Arc<dyn DidAuthnDiscovery>,
        stub_did_signer(stub_secret_store_with_bootstrapped_did(SEED).await),
        fixed_clock(),
        did,
    ));
    let vc_store = InMemoryVcStore::default();
    let clock = fixed_clock();

    let err = run_issuance(&http, &clock, &offer_url("CODE-1"), &coord, &vc_store)
        .await
        .expect_err("must fail");
    let IssuanceFlowError::Flow(CredentialFlowError::Proof(LoginError::DiscoverFailed(_))) =
        err
    else {
        panic!("expected DiscoverFailed, got {err:?}");
    };

    // Only /token went out; /credential was never reached.
    let rec = http.recorded();
    assert_eq!(rec.len(), 1, "wallet short-circuited before /credential");
    assert_eq!(rec[0].url, format!("{ISSUER_URL}/token"));
}

#[tokio::test]
async fn sign_failure_short_circuits() {
    let http = MockHttpClient::default();
    http.push_json(200, &token_ok());

    let (wallet, did) = stub_wallet_with_bootstrapped_did(SEED).await;
    let coord = CredentialCoordinator::jwt(IdTokenProofBuilder::new(
        stub_authn_discovery(wallet),
        Arc::new(FailingSigner(SignError::NoLocalSecret(
            format!("{}#key-auth", did.to_did_string()),
        ))) as Arc<dyn DidSigner>,
        fixed_clock(),
        did,
    ));
    let vc_store = InMemoryVcStore::default();
    let clock = fixed_clock();

    let err = run_issuance(&http, &clock, &offer_url("CODE-1"), &coord, &vc_store)
        .await
        .expect_err("must fail");
    let IssuanceFlowError::Flow(CredentialFlowError::Proof(LoginError::SignFailed(_))) =
        err
    else {
        panic!("expected SignFailed, got {err:?}");
    };
    assert_eq!(http.recorded().len(), 1, "no /credential POST");
}

// ── Composability: custom proof builder ──────────────────────────

/// Confirms the coordinator delegates to whatever `ProofBuilder`
/// the caller supplied — the architectural payoff of audit §5.B.
/// A custom builder produces a sentinel JWS string + a non-JWT
/// `proof_type`; both surface untouched in the /credential body.
struct SentinelProofBuilder;
#[async_trait]
impl ProofBuilder for SentinelProofBuilder {
    async fn build(
        &self,
        _issuer: &str,
        _c_nonce: &str,
    ) -> Result<ProofValue, CredentialFlowError> {
        Ok(ProofValue {
            proof_type: "test-sentinel-v1".into(),
            payload: "PAYLOAD-MARKER".into(),
        })
    }
}

#[tokio::test]
async fn custom_proof_builder_replaces_default() {
    let http = MockHttpClient::default();
    http.push_json(200, &token_ok());
    http.push_json(200, &credential_ok("urn:uuid:sentinel"));

    let coord = CredentialCoordinator::new(Box::new(SentinelProofBuilder));
    let vc_store = InMemoryVcStore::default();
    let clock = fixed_clock();

    let vc_uri = run_issuance(&http, &clock, &offer_url("CODE-1"), &coord, &vc_store)
        .await
        .expect("ok");
    assert_eq!(vc_uri, "urn:uuid:sentinel");

    // The custom builder's proof_type + payload flow through to
    // the /credential request body unchanged.
    let rec = http.recorded();
    let posted = rec[1].body.as_ref().expect("body");
    assert_eq!(posted["proof"]["proof_type"], "test-sentinel-v1");
    assert_eq!(posted["proof"]["jwt"], "PAYLOAD-MARKER");
}
