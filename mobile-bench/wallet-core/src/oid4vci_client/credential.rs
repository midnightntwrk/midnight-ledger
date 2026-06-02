//! `/credential` endpoint client + atomic `vc_store` landing.
//!
//! Flow:
//! 1. Run `request_token` to get the access token + c_nonce.
//! 2. Build a DID-bound JWS proof over the c_nonce via the shared
//!    [`sign_id_token_with_ports`] helper — same single-resolve /
//!    single-sign discipline OID4VP's `IdTokenBuilder` uses, just
//!    parameterised with `aud = issuer` and `nonce = c_nonce`.
//! 3. POST `{format, proof: {proof_type: "jwt", jwt}}` to
//!    `{issuer}/credential` with Bearer auth.
//! 4. Decode the wire VC + openings, base64-decode the bodies,
//!    and land everything in `vc_store` via the single-write-txn
//!    `insert_vc_with_openings`.
//!
//! Port-typed surface: the caller owns the
//! [`DidAuthnDiscovery`] + [`DidSigner`] adapters (the same pair
//! OID4VP's `LoginCoordinator` consumes), so a single adapter
//! cache benefits both protocols within one wallet session.

use std::sync::Arc;

use serde::Deserialize;

use crate::clock::Clock;
use crate::http::{HttpClient, HttpError};
use crate::oid4vci_client::token::TokenResponse;
use crate::oid4vp_client::id_token::sign_id_token_with_ports;
use crate::oid4vp_client::{DidAuthnDiscovery, DidSigner, LoginError};
use crate::vc_store::{StoredVc, VcOpening, VcStorage};
use crate::DidId;

#[derive(Debug, Clone, Deserialize)]
pub struct IssuedVc {
    pub credential: CredentialBody,
    pub openings: Vec<OpeningWire>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CredentialBody {
    pub vc_uri: String,
    pub issuer_did: String,
    pub holder_did: String,
    pub body_b64: String, // base64-encoded Compact-serialized VC
}

#[derive(Debug, Clone, Deserialize)]
pub struct OpeningWire {
    pub claim_path: String,
    pub plaintext_b64: String,
    pub opening_b64: String,
}

#[derive(Debug, thiserror::Error)]
pub enum CredentialFlowError {
    #[error("http: {0}")]
    Http(#[from] HttpError),
    #[error("non-2xx {status}: {body}")]
    Status { status: u16, body: String },
    #[error("decode: {0}")]
    Decode(#[from] serde_json::Error),
    #[error("base64: {0}")]
    Base64(#[from] base64::DecodeError),
    #[error("token error: {0}")]
    Token(#[from] crate::oid4vci_client::token::Oid4vciTokenError),
    /// Proof-of-possession JWS construction failed. Reuses
    /// OID4VP's `LoginError` because the two flows share the
    /// underlying [`sign_id_token_with_ports`] helper — same
    /// failure modes (discovery / sign), same caller-visible
    /// error text.
    #[error("proof JWS error: {0}")]
    Proof(#[from] LoginError),
    #[error("vc_store: {0}")]
    Store(String),
}

/// OID4VCI proof-of-possession JWS TTL. 5 minutes matches the
/// OID4VP id_token convention and gives c_nonce-bound replay
/// windows the same upper bound across both flows.
const PROOF_LIFETIME_SECS: u64 = 300;

/// Drive the full Pre-Authorized Code Flow end-to-end:
/// /token → /credential → land VC + openings in vc_store atomically.
///
/// Port-typed: `discovery` resolves the holder DID to the
/// authentication-relation kid + JWK, `signer` produces the
/// EdDSA signature for the c_nonce-bound JWS. Both are the same
/// `Arc<dyn _>` instances the wallet hands `LoginCoordinator`
/// for OID4VP — keeping the adapter pair shared means one
/// indexer-cache lookup covers both protocols within a session.
pub async fn request_credential(
    http: &dyn HttpClient,
    clock: &Arc<dyn Clock>,
    issuer: &str,
    pre_authorized_code: &str,
    discovery: &Arc<dyn DidAuthnDiscovery>,
    signer: &Arc<dyn DidSigner>,
    holder_did: &DidId,
    vc_store: &dyn VcStorage,
) -> Result<String, CredentialFlowError> {
    use base64::engine::general_purpose::STANDARD as B64;
    use base64::Engine;

    let token: TokenResponse =
        crate::oid4vci_client::token::request_token(http, issuer, pre_authorized_code).await?;

    // Build a DID-bound JWS over the c_nonce. The proof_type=jwt
    // path of OID4VCI reuses the SIOPv2 id_token shape — same
    // header (alg=EdDSA, typ=JWT, kid), same payload schema
    // (iss/sub/aud/nonce/iat/exp/sub_jwk) — just with
    // `aud = issuer URL` and `nonce = c_nonce`. The shared
    // helper guarantees both flows emit byte-for-byte identical
    // wire shapes so the issuer's verifier doesn't have to
    // branch on which flow minted the JWS.
    let proof_jwt = sign_id_token_with_ports(
        discovery,
        signer,
        clock,
        holder_did,
        issuer,
        &token.c_nonce,
        PROOF_LIFETIME_SECS,
    )
    .await?;

    let body = serde_json::json!({
        "format": "midnight-vc-compact",
        "proof": {
            "proof_type": "jwt",
            "jwt": proof_jwt,
        },
    });
    let url = format!("{}/credential", issuer.trim_end_matches('/'));
    let resp = http
        .post_json(&url, &body, Some(&token.access_token))
        .await?;
    let text = resp.body_text()?.to_string();
    if !resp.is_success() {
        return Err(CredentialFlowError::Status {
            status: resp.status,
            body: text,
        });
    }
    let issued: IssuedVc = serde_json::from_str(&text)?;

    let vc = StoredVc {
        vc_uri: issued.credential.vc_uri.clone(),
        issuer_did: issued.credential.issuer_did,
        holder_did: issued.credential.holder_did,
        format: "midnight-vc-compact".into(),
        body: B64.decode(&issued.credential.body_b64)?,
        issued_at_ms: clock.now_ms(),
    };
    let openings: Vec<VcOpening> = issued
        .openings
        .into_iter()
        .map(|o| {
            Ok(VcOpening {
                vc_uri: vc.vc_uri.clone(),
                claim_path: o.claim_path,
                plaintext: B64.decode(&o.plaintext_b64)?,
                opening: B64.decode(&o.opening_b64)?,
            })
        })
        .collect::<Result<_, base64::DecodeError>>()?;
    vc_store
        .insert_vc_with_openings(&vc, &openings)
        .map_err(|e| CredentialFlowError::Store(e.to_string()))?;
    Ok(vc.vc_uri)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::FixedClock;
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
    async fn request_credential_lands_vc_and_openings() {
        let http = MockHttpClient::default();
        // 1. /token
        http.push_json(
            200,
            &json!({
                "access_token": "AT",
                "c_nonce": "CN",
                "token_type": "Bearer",
                "expires_in": 600,
            }),
        );
        // 2. /credential
        http.push_json(
            200,
            &json!({
                "credential": {
                    "vc_uri": "urn:uuid:birth-1",
                    "issuer_did": "did:midnight:issuer",
                    "holder_did": "did:midnight:alice",
                    "body_b64": B64.encode(b"COMPACT_VC_BYTES")
                },
                "openings": [
                    {
                        "claim_path": "/credentialSubject/dateOfBirth",
                        "plaintext_b64": B64.encode(b"1985-01-01"),
                        "opening_b64":   B64.encode(b"rand")
                    }
                ]
            }),
        );

        let seed = [23u8; 32];
        let (wallet, did) = stub_wallet_with_bootstrapped_did(seed).await;
        let store = stub_secret_store_with_bootstrapped_did(seed).await;
        // Wrap the wallet + secret store in the same port pair
        // production callers pass in (matching the OID4VP path).
        let discovery = stub_authn_discovery(wallet);
        let signer = stub_did_signer(store);
        let vc_store = InMemoryVcStore::default();
        let clock: Arc<dyn Clock> = Arc::new(FixedClock::new(1_700_000_000_000));

        let vc_uri = request_credential(
            &http,
            &clock,
            "https://issuer.local",
            "CODE-1",
            &discovery,
            &signer,
            &did,
            &vc_store,
        )
        .await
        .expect("ok");
        assert_eq!(vc_uri, "urn:uuid:birth-1");

        let landed = vc_store.get_vc(&vc_uri).unwrap().expect("present");
        assert_eq!(landed.body, b"COMPACT_VC_BYTES");
        assert_eq!(
            landed.issued_at_ms, 1_700_000_000_000,
            "issued_at_ms should come from the injected clock"
        );
        let op = vc_store
            .get_opening(&vc_uri, "/credentialSubject/dateOfBirth")
            .unwrap()
            .expect("op");
        assert_eq!(op.plaintext, b"1985-01-01");

        // Request shape: token call has no bearer; credential call carries Bearer AT.
        let rec = http.recorded();
        assert_eq!(rec.len(), 2);
        assert_eq!(rec[0].url, "https://issuer.local/token");
        assert!(rec[0].bearer.is_none());
        assert_eq!(rec[1].url, "https://issuer.local/credential");
        assert_eq!(rec[1].bearer.as_deref(), Some("AT"));
        let posted = rec[1].body.as_ref().expect("credential body");
        assert_eq!(posted["format"], "midnight-vc-compact");
        assert_eq!(posted["proof"]["proof_type"], "jwt");
        assert!(posted["proof"]["jwt"].is_string());
    }
}
