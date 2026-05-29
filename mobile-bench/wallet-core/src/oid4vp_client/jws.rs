//! SIOPv2 id-token builder.
//!
//! Header: `{ alg: "EdDSA", typ: "JWT", kid: <did>#<fragment> }`
//! Payload: `{ iss: <did>, sub: <did>, aud: client_id, nonce, iat, exp }`
//! Signature: EdDSA over `base64url(header) || "." || base64url(payload)`.
//!
//! `sign_for_authentication` is the only path that knows the kid the
//! authentication VM uses, so this builder does a two-call dance:
//! one "probe" call with a throwaway payload to discover the kid,
//! then a real sign call with the kid baked into the header. The
//! probe signature is discarded.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};

use crate::clock::Clock;
use crate::did_auth::{sign_for_authentication, DidAuthError};
use crate::secret_storage::SecretStorage;
use crate::wallet::Wallet;
use crate::{DidId, PublicKeyJwk};

#[derive(Debug, thiserror::Error)]
pub enum IdTokenError {
    #[error("did_auth error: {0}")]
    DidAuth(#[from] DidAuthError),
    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),
}

#[derive(Debug, Serialize)]
struct JwsHeader<'a> {
    alg: &'a str,
    typ: &'a str,
    kid: &'a str,
    /// Self-asserted public key (Phase-1 posture). The
    /// IssuerDIDIT-mock verifier (`oid4vpVerifier.ts`) requires
    /// this — it builds the holder document from the embedded
    /// `kid` + `jwk` and uses that jwk to verify the signature.
    /// Without it the issuer 401s with
    /// "missing self-asserted jwk in JWS header (Phase 1 posture)".
    jwk: &'a PublicKeyJwk,
}

#[derive(Debug, Serialize, Deserialize)]
struct IdTokenPayload {
    iss: String,
    sub: String,
    aud: String,
    nonce: String,
    iat: u64,
    exp: u64,
}

/// Build a signed SIOPv2 id_token.
///
/// `lifetime_secs` is how long the id_token remains valid; 5 minutes
/// (300) matches OID4VP convention.
pub async fn build_id_token(
    wallet: &Wallet,
    secret_store: &dyn SecretStorage,
    clock: &dyn Clock,
    holder: &DidId,
    client_id: &str,
    nonce: &str,
    lifetime_secs: u64,
) -> Result<String, IdTokenError> {
    // 1. Compose payload. `iat` is epoch-seconds — the clock
    //    port returns epoch-ms, divide.
    let iat = clock.now_ms() / 1_000;
    let payload = IdTokenPayload {
        iss: holder.to_did_string(),
        sub: holder.to_did_string(),
        aud: client_id.into(),
        nonce: nonce.into(),
        iat,
        exp: iat + lifetime_secs,
    };
    let payload_b64 = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload)?);

    // 2. Probe to discover the kid + publicKeyJwk the
    //    authentication VM uses. The signature here is thrown
    //    away — we need the kid (for the header) and the jwk
    //    (for the self-asserted `jwk` header parameter).
    let (kid, jwk, _probe_sig) =
        sign_for_authentication(wallet, secret_store, holder, b"oid4vp-kid-probe").await?;

    // 3. Build the final header with the real kid + jwk.
    let header_final = JwsHeader {
        alg: "EdDSA",
        typ: "JWT",
        kid: &kid,
        jwk: &jwk,
    };
    let header_b64 = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header_final)?);

    // 4. Sign the real `header.payload` input.
    let sign_input = format!("{header_b64}.{payload_b64}");
    let (_kid2, _jwk2, sig) =
        sign_for_authentication(wallet, secret_store, holder, sign_input.as_bytes()).await?;
    let sig_b64 = URL_SAFE_NO_PAD.encode(&sig);

    Ok(format!("{sign_input}.{sig_b64}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::FixedClock;
    use crate::test_support::{
        stub_secret_store_with_bootstrapped_did, stub_wallet_with_bootstrapped_did,
    };

    #[tokio::test]
    async fn build_id_token_is_three_dot_separated() {
        let seed = [6u8; 32];
        let (wallet, did) = stub_wallet_with_bootstrapped_did(seed).await;
        let store = stub_secret_store_with_bootstrapped_did(seed).await;
        let clock = FixedClock::new(1_700_000_000_000);
        let jwt =
            build_id_token(&wallet, &store, &clock, &did, "client-x", "nonce-y", 300)
                .await
                .expect("build");
        let parts: Vec<&str> = jwt.split('.').collect();
        assert_eq!(parts.len(), 3, "jws has three b64 segments");
        for p in &parts {
            assert!(!p.is_empty());
        }
    }

    #[tokio::test]
    async fn id_token_header_contains_real_kid() {
        let seed = [7u8; 32];
        let (wallet, did) = stub_wallet_with_bootstrapped_did(seed).await;
        let store = stub_secret_store_with_bootstrapped_did(seed).await;
        let clock = FixedClock::new(1_700_000_000_000);
        let jwt = build_id_token(&wallet, &store, &clock, &did, "c", "n", 60)
            .await
            .expect("build");
        let header_b64 = jwt.split('.').next().unwrap();
        let header_bytes = URL_SAFE_NO_PAD.decode(header_b64).expect("b64");
        let header: serde_json::Value = serde_json::from_slice(&header_bytes).expect("json");
        let kid = header["kid"].as_str().expect("kid present");
        assert!(kid.starts_with("did:midnight:"));
        assert!(kid.contains("#key-auth"));
        assert_eq!(header["alg"], "EdDSA");
    }

    #[tokio::test]
    async fn id_token_payload_contains_required_claims() {
        let seed = [8u8; 32];
        let (wallet, did) = stub_wallet_with_bootstrapped_did(seed).await;
        let store = stub_secret_store_with_bootstrapped_did(seed).await;
        let clock = FixedClock::new(1_700_000_000_000);
        let jwt = build_id_token(&wallet, &store, &clock, &did, "c", "n", 60)
            .await
            .expect("build");
        let payload_b64 = jwt.split('.').nth(1).unwrap();
        let payload_bytes = URL_SAFE_NO_PAD.decode(payload_b64).expect("b64");
        let payload: IdTokenPayload =
            serde_json::from_slice(&payload_bytes).expect("json");
        assert_eq!(payload.iss, did.to_did_string());
        assert_eq!(payload.sub, did.to_did_string());
        assert_eq!(payload.aud, "c");
        assert_eq!(payload.nonce, "n");
        assert!(payload.exp > payload.iat);
        // Deterministic timestamps via FixedClock.
        assert_eq!(payload.iat, 1_700_000_000);
        assert_eq!(payload.exp, 1_700_000_060);
    }
}
