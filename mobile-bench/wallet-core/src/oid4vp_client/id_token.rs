//! SIOPv2 id_token JOSE header + payload types.
//!
//! Wire-format choice: per the normative guide
//! §"Self-issued ID Token claims", the self-asserted public key
//! belongs in the **payload** as the `sub_jwk` claim, not in
//! the JOSE header. The legacy wallet-core path (still alive in
//! `oid4vp_client::jws` for transitional compat — removed in
//! Task 9) put `jwk` in the header instead. This module ships
//! the normative shape.
//!
//! The JOSE header only carries the protection-relevant fields
//! `{ alg, typ, kid }`. `kid` is the full DID URL form
//! (`did:midnight:abc#key-auth`); verifiers resolve the DID
//! from the prefix and look up the verification method by full
//! URL.
//!
//! Reference:
//! `docs/superpowers/specs/2026-06-02-login-with-did-architecture.md`.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};

use crate::clock::Clock;
use crate::oid4vp_client::errors::LoginError;
use crate::oid4vp_client::ports::{DidAuthnDiscovery, DidSigner};
use crate::{DidId, PublicKeyJwk};

/// JOSE protected header. EdDSA + JWT typ + full-DID-URL kid is
/// the only Phase-1 shape; new fields (`crit`, `x5c`, etc.) are
/// not emitted.
#[derive(Debug, Serialize, Deserialize)]
pub struct JwsHeader<'a> {
    pub alg: &'a str,
    pub typ: &'a str,
    pub kid: &'a str,
}

/// SIOPv2 self-issued id_token payload.
///
/// `sub_jwk` is `Some` whenever the wallet wants the RP to be
/// able to verify the signature without doing its own DID
/// resolution — the Phase-1 issuer-mock takes this path. Once
/// the Phase-2 indexer-backed verifier lands on the issuer,
/// `sub_jwk` becomes redundant (the issuer resolves the DID
/// itself and reads the on-chain JWK); the wallet keeps emitting
/// it for backward compat. Verifier callers MUST NOT trust
/// `sub_jwk` over the DID document — it's a convenience, not a
/// trust anchor.
#[derive(Debug, Serialize, Deserialize)]
pub struct IdTokenPayload {
    pub iss: String,
    pub sub: String,
    pub aud: String,
    pub nonce: String,
    pub iat: u64,
    pub exp: u64,

    /// Per OID4VP / SIOPv2 §"Self-issued ID Token claims".
    /// `kty=OKP, crv=Ed25519, x=<base64url public key>` for the
    /// Phase-1 demo (the only key type Midnight DIDs currently
    /// enrol).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub_jwk: Option<PublicKeyJwk>,
}

/// Base64url-encode a JSON-serialisable value with no padding —
/// JWS / SIOP convention. Pure helper; used by the builder.
pub fn encode_segment<T: Serialize>(value: &T) -> Result<String, serde_json::Error> {
    Ok(URL_SAFE_NO_PAD.encode(serde_json::to_vec(value)?))
}

/// Compose + sign a SIOPv2-shape id_token via the new ports.
///
/// Single-resolve / single-sign: one [`DidAuthnDiscovery::authn_key`]
/// call to discover the kid + jwk, one [`DidSigner::sign`] call
/// over `b64(header) || "." || b64(payload)`. Shared by:
///
/// - `oid4vp_client::builders::IdTokenBuilder` (the OID4VP /
///   SIOPv2 id_token sent to the RP's /authorize-response).
/// - `oid4vci_client::credential::request_credential` (the
///   OID4VCI proof-of-possession JWS posted to /credential —
///   `aud = issuer URL`, `nonce = c_nonce` from /token).
///
/// Both consumers produce the **same wire shape** (`{alg,
/// typ, kid}` header + `{iss, sub, aud, nonce, iat, exp,
/// sub_jwk}` payload + EdDSA signature) so the verifier
/// pipeline doesn't have to branch on the flow that minted
/// the JWS.
pub async fn sign_id_token_with_ports(
    discovery: &dyn DidAuthnDiscovery,
    signer: &dyn DidSigner,
    clock: &dyn Clock,
    holder: &DidId,
    aud: &str,
    nonce: &str,
    lifetime_secs: u64,
) -> Result<String, LoginError> {
    // ① Discover the kid + jwk — one DID resolution (the
    //    adapter caches if it wants to).
    let key = discovery
        .authn_key(holder)
        .await
        .map_err(|e| LoginError::DiscoverFailed(e.to_string()))?;

    // ② Compose JOSE header + claims. `sub_jwk` lives in the
    //    payload per the SIOPv2 normative shape.
    let iat = clock.now_ms() / 1_000;
    let header = JwsHeader {
        alg: "EdDSA",
        typ: "JWT",
        kid: &key.kid,
    };
    let payload = IdTokenPayload {
        iss: holder.to_did_string(),
        sub: holder.to_did_string(),
        aud: aud.to_string(),
        nonce: nonce.to_string(),
        iat,
        exp: iat + lifetime_secs,
        sub_jwk: Some(key.public_jwk),
    };
    let header_b64 = encode_segment(&header)
        .map_err(|e| LoginError::Internal(format!("encode header: {e}")))?;
    let payload_b64 = encode_segment(&payload)
        .map_err(|e| LoginError::Internal(format!("encode payload: {e}")))?;
    let sign_input = format!("{header_b64}.{payload_b64}");

    // ③ Sign the canonical b64(header).b64(payload) input.
    let sig = signer
        .sign(&key.kid, sign_input.as_bytes())
        .await
        .map_err(|e| LoginError::SignFailed(e.to_string()))?;
    let sig_b64 = URL_SAFE_NO_PAD.encode(&sig);
    Ok(format!("{sign_input}.{sig_b64}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CurveType, KeyType};

    #[test]
    fn header_serializes_three_fields() {
        let h = JwsHeader { alg: "EdDSA", typ: "JWT", kid: "did:midnight:abc#key-auth" };
        let s = serde_json::to_string(&h).unwrap();
        // No `jwk` in the header — normative shape.
        assert!(!s.contains("jwk"));
        assert!(s.contains("\"alg\":\"EdDSA\""));
        assert!(s.contains("\"typ\":\"JWT\""));
        assert!(s.contains("\"kid\":\"did:midnight:abc#key-auth\""));
    }

    #[test]
    fn payload_sub_jwk_in_payload_not_header() {
        let p = IdTokenPayload {
            iss: "did:midnight:abc".into(),
            sub: "did:midnight:abc".into(),
            aud: "did:midnight:issuer".into(),
            nonce: "n-1".into(),
            iat: 1_700_000_000,
            exp: 1_700_000_300,
            sub_jwk: Some(PublicKeyJwk {
                kty: KeyType::OKP,
                crv: CurveType::Ed25519,
                x: "AAAA".into(),
                y: None,
            }),
        };
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(v["iss"], "did:midnight:abc");
        assert!(v["sub_jwk"].is_object());
        assert_eq!(v["sub_jwk"]["kty"], "OKP");
        assert_eq!(v["sub_jwk"]["crv"], "Ed25519");
        assert_eq!(v["sub_jwk"]["x"], "AAAA");
        assert!(v["sub_jwk"].get("y").is_none());
    }

    #[test]
    fn payload_omits_sub_jwk_when_absent() {
        let p = IdTokenPayload {
            iss: "did:midnight:abc".into(),
            sub: "did:midnight:abc".into(),
            aud: "did:midnight:issuer".into(),
            nonce: "n-1".into(),
            iat: 1_700_000_000,
            exp: 1_700_000_300,
            sub_jwk: None,
        };
        let v = serde_json::to_value(&p).unwrap();
        assert!(v.as_object().unwrap().get("sub_jwk").is_none());
    }

    #[test]
    fn encode_segment_is_b64_url_no_pad() {
        let h = JwsHeader { alg: "EdDSA", typ: "JWT", kid: "k" };
        let s = encode_segment(&h).unwrap();
        assert!(!s.contains('='), "no padding");
        assert!(!s.contains('+'), "url-safe alphabet");
        assert!(!s.contains('/'));
    }
}
