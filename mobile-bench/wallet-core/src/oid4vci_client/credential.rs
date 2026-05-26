//! `/credential` endpoint client + atomic `vc_store` landing.
//!
//! Flow:
//! 1. Run `request_token` to get the access token + c_nonce.
//! 2. Build a DID-bound JWS proof over the c_nonce (reusing the
//!    SIOPv2 id_token shape from `oid4vp_client::jws`, with
//!    `aud = issuer` and `nonce = c_nonce`).
//! 3. POST `{format, proof: {proof_type: "jwt", jwt}}` to
//!    `{issuer}/credential` with Bearer auth.
//! 4. Decode the wire VC + openings, base64-decode the bodies,
//!    and land everything in `vc_store` via the single-write-txn
//!    `insert_vc_with_openings`.

use serde::{Deserialize, Serialize};

use crate::oid4vci_client::token::TokenResponse;
use crate::oid4vp_client::build_id_token;
use crate::secret_storage::SecretStorage;
use crate::vc_store::{StoredVc, VcOpening, VcStore};
use crate::wallet::Wallet;
use crate::DidId;

#[derive(Debug, Serialize)]
struct CredentialRequest<'a> {
    format: &'a str,
    proof: Proof<'a>,
}

#[derive(Debug, Serialize)]
struct Proof<'a> {
    proof_type: &'a str,
    jwt: &'a str,
}

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
    Http(String),
    #[error("non-2xx {status}: {body}")]
    Status { status: u16, body: String },
    #[error("decode: {0}")]
    Decode(#[from] serde_json::Error),
    #[error("base64: {0}")]
    Base64(#[from] base64::DecodeError),
    #[error("token error: {0}")]
    Token(#[from] crate::oid4vci_client::token::Oid4vciTokenError),
    #[error("proof JWS error: {0}")]
    Proof(#[from] crate::oid4vp_client::IdTokenError),
    #[error("vc_store: {0}")]
    Store(String),
}

/// Drive the full Pre-Authorized Code Flow end-to-end:
/// /token → /credential → land VC + openings in vc_store atomically.
pub async fn request_credential(
    issuer: &str,
    pre_authorized_code: &str,
    wallet: &Wallet,
    secret_store: &dyn SecretStorage,
    holder_did: &DidId,
    vc_store: &VcStore,
) -> Result<String, CredentialFlowError> {
    use base64::engine::general_purpose::STANDARD as B64;
    use base64::Engine;

    let token: TokenResponse =
        crate::oid4vci_client::token::request_token(issuer, pre_authorized_code).await?;

    // Build a DID-bound JWS over the c_nonce. The proof_type=jwt
    // path of OID4VCI just reuses the SIOPv2 id_token shape, with
    // `aud=issuer` and `nonce=c_nonce`.
    let proof_jwt = build_id_token(
        wallet,
        secret_store,
        holder_did,
        issuer,
        &token.c_nonce,
        300,
    )
    .await?;

    let body = CredentialRequest {
        format: "midnight-vc-compact",
        proof: Proof {
            proof_type: "jwt",
            jwt: &proof_jwt,
        },
    };
    let resp = reqwest::Client::new()
        .post(format!("{}/credential", issuer.trim_end_matches('/')))
        .bearer_auth(&token.access_token)
        .json(&body)
        .send()
        .await
        .map_err(|e| CredentialFlowError::Http(e.to_string()))?;
    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| CredentialFlowError::Http(e.to_string()))?;
    if !status.is_success() {
        return Err(CredentialFlowError::Status {
            status: status.as_u16(),
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
        issued_at_ms: now_ms(),
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

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{
        stub_secret_store_with_bootstrapped_did, stub_wallet_with_bootstrapped_did,
    };
    use base64::engine::general_purpose::STANDARD as B64;
    use base64::Engine;
    use tempfile::TempDir;
    use wiremock::{matchers::*, Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn request_credential_lands_vc_and_openings() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "AT", "c_nonce": "CN", "token_type": "Bearer", "expires_in": 600
            })))
            .mount(&mock)
            .await;
        let issued = serde_json::json!({
            "credential": {
                "vc_uri": "urn:uuid:birth-1",
                "issuer_did": "did:midnight:issuer",
                "holder_did": "did:midnight:alice",
                "body_b64": B64.encode(b"COMPACT_VC_BYTES")
            },
            "openings": [
                { "claim_path": "/credentialSubject/dateOfBirth",
                  "plaintext_b64": B64.encode(b"1985-01-01"),
                  "opening_b64":   B64.encode(b"rand") }
            ]
        });
        Mock::given(method("POST"))
            .and(path("/credential"))
            .respond_with(ResponseTemplate::new(200).set_body_json(issued))
            .mount(&mock)
            .await;

        let seed = [23u8; 32];
        let (wallet, did) = stub_wallet_with_bootstrapped_did(seed).await;
        let store = stub_secret_store_with_bootstrapped_did(seed).await;
        let dir = TempDir::new().unwrap();
        let vc_store = VcStore::open(dir.path().join("vc.redb")).unwrap();

        let vc_uri =
            request_credential(&mock.uri(), "CODE-1", &wallet, &store, &did, &vc_store)
                .await
                .expect("ok");
        assert_eq!(vc_uri, "urn:uuid:birth-1");
        let landed = vc_store.get_vc(&vc_uri).unwrap().expect("present");
        assert_eq!(landed.body, b"COMPACT_VC_BYTES");
        let op = vc_store
            .get_opening(&vc_uri, "/credentialSubject/dateOfBirth")
            .unwrap()
            .expect("op");
        assert_eq!(op.plaintext, b"1985-01-01");
    }
}
