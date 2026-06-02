//! Birth-credential (legacy Compact VC) request/response flow.
//!
//! Drives `/token` → `/credential` with the simple `{format, proof}`
//! request body and the legacy response shape
//! `{credential: {vc_uri, issuer_did, holder_did, body_b64}, openings}`.
//! The issuer assigns the `vc_uri`; all fields are plain JSON strings.
//!
//! All endpoint URLs, the credential `format`, and the
//! `credential_issuer` come from the credential-issuer metadata
//! document; callers extract them from `CredentialIssuerMetadata`
//! before calling this function.
//!
//! Composability: the proof-construction step lives behind the
//! [`ProofBuilder`] trait (`oid4vci_client::proof`). Phase-1
//! ships the JWT-typed builder; Phase-2 proof types
//! (`ldp_vp`, `mso_mdoc`, EBSI) plug in by registering a new
//! builder, not by editing this function. Spec rationale:
//! `docs/superpowers/specs/2026-06-03-hex-architecture-audit.md`
//! §5.B.
//!
//! [`ProofBuilder`]: super::super::proof::ProofBuilder

use serde::Deserialize;

use crate::http::HttpClient;
use crate::oid4vci_client::proof::CredentialCoordinator;
use crate::oid4vci_client::token::TokenResponse;
use crate::oid4vci_client::credential::CredentialFlowError;
use crate::vc_store::{StoredVc, VcOpening, VcStorage};

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

/// Drive the full Pre-Authorized Code Flow end-to-end for a
/// birth-format credential:
/// /token → /credential → land VC + openings in vc_store atomically.
///
/// All endpoint URLs, the credential `format`, and the
/// `credential_issuer` come from the credential-issuer metadata
/// document; callers extract them from `CredentialIssuerMetadata`
/// before calling this function.
///
/// The coordinator owns the proof-of-possession step. Phase-1
/// callers wire `CredentialCoordinator::jwt(IdTokenProofBuilder)`
/// for the canonical JWT-typed proof; Phase-2 proof types plug
/// in by passing a different `ProofBuilder` to
/// `CredentialCoordinator::new`.
pub async fn request_credential(
    http: &dyn HttpClient,
    clock: &dyn crate::Clock,
    credential_issuer: &str,
    token_endpoint: &str,
    credential_endpoint: &str,
    format: &str,
    pre_authorized_code: &str,
    coordinator: &CredentialCoordinator,
    vc_store: &dyn VcStorage,
) -> Result<String, CredentialFlowError> {
    use base64::engine::general_purpose::STANDARD as B64;
    use base64::Engine;

    let token: TokenResponse =
        crate::oid4vci_client::token::request_token(http, token_endpoint, pre_authorized_code)
            .await?;

    // Mint the proof-of-possession through the coordinator's
    // builder. Phase-1: `IdTokenProofBuilder` produces a JWS
    // matching OID4VP's id_token shape, with `aud = issuer URL`
    // and `nonce = c_nonce`. Phase-2 builders may emit other
    // proof types (`ldp_vp`, `mso_mdoc`, etc.) — see
    // `oid4vci_client::proof` for the trait surface.
    let proof = coordinator
        .proof_builder
        .build(credential_issuer, &token.c_nonce)
        .await?;

    let body = serde_json::json!({
        "format": format,
        "proof": {
            "proof_type": proof.proof_type,
            // Field name is `jwt` for back-compat with the
            // Phase-1 issuer-mock wire shape; for proof types
            // beyond JWT this becomes a more general
            // payload-bearing field (handled when those proof
            // types land).
            "jwt": proof.payload,
        },
    });
    let url = credential_endpoint.to_string();
    let resp = http
        .post_json(&url, &body, Some(&token.access_token))
        .await?;
    let text = resp
        .body_text()
        .map_err(|e| CredentialFlowError::Http(e))?
        .to_string();
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
        format: format.to_string(),
        body: B64.decode(&issued.credential.body_b64)?,
        proof: vec![],
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
    use crate::clock::{Clock, FixedClock};
    use crate::http::mock::MockHttpClient;
    use crate::oid4vci_client::proof::{CredentialCoordinator, IdTokenProofBuilder};
    use crate::test_support::{
        stub_authn_discovery, stub_did_signer,
        stub_secret_store_with_bootstrapped_did, stub_wallet_with_bootstrapped_did,
    };
    use crate::vc_store::InMemoryVcStore;
    use base64::engine::general_purpose::STANDARD as B64;
    use base64::Engine;
    use serde_json::json;
    use std::sync::Arc;

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
        // Wrap the wallet + secret store in the port pair the
        // coordinator's JWT proof builder consumes — matches the
        // OID4VP path.
        let discovery = stub_authn_discovery(wallet);
        let signer = stub_did_signer(store);
        let vc_store = InMemoryVcStore::default();
        let clock: Arc<dyn Clock> = Arc::new(FixedClock::new(1_700_000_000_000));
        let coordinator = CredentialCoordinator::jwt(IdTokenProofBuilder::new(
            discovery,
            signer,
            clock.clone(),
            did,
        ));

        let vc_uri = request_credential(
            &http,
            &clock,
            "https://issuer.local",           // credential_issuer (aud)
            "https://issuer.local/token",       // token_endpoint
            "https://issuer.local/credential",  // credential_endpoint
            "midnight_compact_vc",
            "CODE-1",
            &coordinator,
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
        assert_eq!(posted["format"], "midnight_compact_vc");
        assert_eq!(posted["proof"]["proof_type"], "jwt");
        assert!(posted["proof"]["jwt"].is_string());
    }
}