//! Phase-1 SIOPv2 id_token builder — runs through the
//! [`DidAuthnDiscovery`] + [`DidSigner`] ports.
//!
//! Compared to the legacy `oid4vp_client::jws::build_id_token`,
//! this builder:
//!
//! 1. Resolves the DID **once** (one indexer roundtrip) — the
//!    old probe+real sign-twice pattern is gone.
//! 2. Signs **once**.
//! 3. Places `sub_jwk` in the **payload**, not the JOSE header
//!    (normative SIOPv2 shape).
//! 4. Surfaces errors through the unified [`LoginError`] enum
//!    — the issuer-mock (Task 7) emits matching codes.

use std::sync::Arc;

use async_trait::async_trait;

use super::ResponseBuilder;
use crate::clock::Clock;
use crate::oid4vp_client::errors::LoginError;
use crate::oid4vp_client::id_token::sign_id_token_with_ports;
use crate::oid4vp_client::ports::{DidAuthnDiscovery, DidSigner};
use crate::oid4vp_client::request::AuthorizationRequest;
use crate::oid4vp_client::response::AuthorizationResponse;
use crate::DidId;

/// Phase-1 default id_token TTL — 5 minutes, matching OID4VP
/// convention. Callers can override via
/// [`IdTokenBuilder::lifetime_secs`].
pub(crate) const DEFAULT_LIFETIME_SECS: u64 = 300;

/// Builds the SIOPv2 self-issued id_token and writes it to
/// `resp.id_token`.
pub struct IdTokenBuilder {
    pub discovery: Arc<dyn DidAuthnDiscovery>,
    pub signer: Arc<dyn DidSigner>,
    pub clock: Arc<dyn Clock>,
    pub holder: DidId,
    /// Token validity window from `iat`. 300 by convention.
    pub lifetime_secs: u64,
}

impl IdTokenBuilder {
    pub fn new(
        discovery: Arc<dyn DidAuthnDiscovery>,
        signer: Arc<dyn DidSigner>,
        clock: Arc<dyn Clock>,
        holder: DidId,
    ) -> Self {
        Self {
            discovery,
            signer,
            clock,
            holder,
            lifetime_secs: DEFAULT_LIFETIME_SECS,
        }
    }
}

#[async_trait]
impl ResponseBuilder for IdTokenBuilder {
    async fn build(
        &self,
        req: &AuthorizationRequest,
        resp: &mut AuthorizationResponse,
    ) -> Result<(), LoginError> {
        // The single-resolve / single-sign discipline lives in
        // [`sign_id_token_with_ports`] — see its docstring for
        // the architectural rationale. The OID4VCI proof-of-
        // possession path calls the same helper, so both flows
        // emit the same wire shape and the verifier doesn't
        // have to branch on which one minted the JWS.
        //
        // The id_token's `aud` is the RP's `client_id` (the
        // verifier DID) and `nonce` is the request nonce — both
        // travel through `AuthorizationRequest`.
        let token = sign_id_token_with_ports(
            &*self.discovery,
            &*self.signer,
            &*self.clock,
            &self.holder,
            &req.client_id,
            &req.nonce,
            self.lifetime_secs,
        )
        .await?;
        resp.id_token = Some(token);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use async_trait::async_trait;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    use serde_json::Value;

    use super::*;
    use crate::clock::FixedClock;
    use crate::oid4vp_client::ports::{
        AuthnKey, DidAuthnDiscovery, DidSigner, DiscoverError, SignError,
    };
    use crate::oid4vp_client::request::{
        AuthorizationRequest, ResponseMode, ResponseType,
    };
    use crate::{CurveType, DidId, KeyType, PublicKeyJwk};

    /// Discovery mock that returns a canned [`AuthnKey`] and
    /// counts how many times `authn_key` is called. The count
    /// assertion is the whole point of Task 5 — proving Phase-1
    /// cut the indexer roundtrips from two to one.
    struct CountingDiscovery {
        key: AuthnKey,
        calls: AtomicUsize,
        fail_with: Option<DiscoverError>,
    }
    impl CountingDiscovery {
        fn ok(key: AuthnKey) -> Self {
            Self {
                key,
                calls: AtomicUsize::new(0),
                fail_with: None,
            }
        }
        fn fails(err: DiscoverError) -> Self {
            Self {
                key: dummy_key(),
                calls: AtomicUsize::new(0),
                fail_with: Some(err),
            }
        }
        fn calls(&self) -> usize {
            self.calls.load(Ordering::Relaxed)
        }
    }
    #[async_trait]
    impl DidAuthnDiscovery for CountingDiscovery {
        async fn authn_key(&self, _did: &DidId) -> Result<AuthnKey, DiscoverError> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            if let Some(e) = &self.fail_with {
                // Clone the variant for return without consuming
                // the test's fixture (so the count assertion can
                // still run).
                return Err(match e {
                    DiscoverError::Resolve(s) => DiscoverError::Resolve(s.clone()),
                    DiscoverError::NoAuthnKey(s) => DiscoverError::NoAuthnKey(s.clone()),
                });
            }
            Ok(self.key.clone())
        }
    }

    struct CountingSigner {
        calls: AtomicUsize,
        canned_sig: Vec<u8>,
        fail_with: Option<SignError>,
        last_kid: std::sync::Mutex<Option<String>>,
        last_payload: std::sync::Mutex<Option<Vec<u8>>>,
    }
    impl CountingSigner {
        fn ok() -> Self {
            Self {
                calls: AtomicUsize::new(0),
                canned_sig: vec![0xAA; 64],
                fail_with: None,
                last_kid: std::sync::Mutex::new(None),
                last_payload: std::sync::Mutex::new(None),
            }
        }
        fn fails(err: SignError) -> Self {
            Self {
                calls: AtomicUsize::new(0),
                canned_sig: vec![],
                fail_with: Some(err),
                last_kid: std::sync::Mutex::new(None),
                last_payload: std::sync::Mutex::new(None),
            }
        }
        fn calls(&self) -> usize {
            self.calls.load(Ordering::Relaxed)
        }
    }
    #[async_trait]
    impl DidSigner for CountingSigner {
        async fn sign(&self, kid: &str, payload: &[u8]) -> Result<Vec<u8>, SignError> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            *self.last_kid.lock().unwrap() = Some(kid.to_string());
            *self.last_payload.lock().unwrap() = Some(payload.to_vec());
            if let Some(e) = &self.fail_with {
                return Err(match e {
                    SignError::NoLocalSecret(s) => SignError::NoLocalSecret(s.clone()),
                    SignError::Sign(s) => SignError::Sign(s.clone()),
                });
            }
            Ok(self.canned_sig.clone())
        }
    }

    /// 32-byte address as a hex string — matches the DID URL
    /// the live wallet emits. Anything shorter / odd-length
    /// fails `DidId::parse`'s hex decoder.
    const FIXTURE_ADDR_HEX: &str =
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn dummy_key() -> AuthnKey {
        AuthnKey {
            kid: format!("did:midnight:undeployed:{FIXTURE_ADDR_HEX}#key-auth"),
            public_jwk: PublicKeyJwk {
                kty: KeyType::OKP,
                crv: CurveType::Ed25519,
                x: "AAAA".into(),
                y: None,
            },
        }
    }

    fn req_mode_a() -> AuthorizationRequest {
        AuthorizationRequest {
            client_id: "did:midnight:issuer-mock".into(),
            response_type: ResponseType::IdToken,
            response_mode: ResponseMode::DirectPost,
            response_uri: "https://issuer/cb".into(),
            scope: "openid".into(),
            nonce: "n-test".into(),
            state: Some("st-test".into()),
            presentation_definition: None,
        }
    }

    fn holder_did() -> DidId {
        DidId::parse(&format!("did:midnight:undeployed:{FIXTURE_ADDR_HEX}"))
            .expect("parse")
    }

    #[tokio::test]
    async fn build_emits_id_token_with_sub_jwk_in_payload() {
        let disc = Arc::new(CountingDiscovery::ok(dummy_key()));
        let signer = Arc::new(CountingSigner::ok());
        let clock = Arc::new(FixedClock::new(1_700_000_000_000));
        let builder = IdTokenBuilder::new(
            disc.clone() as Arc<dyn DidAuthnDiscovery>,
            signer.clone() as Arc<dyn DidSigner>,
            clock,
            holder_did(),
        );

        let mut resp = AuthorizationResponse::new(Some("st-test".into()));
        builder.build(&req_mode_a(), &mut resp).await.expect("build");

        let id_token = resp.id_token.expect("id_token");

        // Three b64 segments.
        let parts: Vec<&str> = id_token.split('.').collect();
        assert_eq!(parts.len(), 3, "JWS = header.payload.sig");

        // Decode header — no `jwk` field.
        let header_bytes = URL_SAFE_NO_PAD.decode(parts[0]).unwrap();
        let header: Value = serde_json::from_slice(&header_bytes).unwrap();
        assert_eq!(header["alg"], "EdDSA");
        assert_eq!(header["typ"], "JWT");
        assert_eq!(
            header["kid"],
            format!("did:midnight:undeployed:{FIXTURE_ADDR_HEX}#key-auth")
        );
        assert!(
            header.as_object().unwrap().get("jwk").is_none(),
            "JOSE header MUST NOT carry `jwk` — sub_jwk lives in payload",
        );

        // Decode payload — `sub_jwk` IS here.
        let payload_bytes = URL_SAFE_NO_PAD.decode(parts[1]).unwrap();
        let payload: Value = serde_json::from_slice(&payload_bytes).unwrap();
        let expected_did = format!("did:midnight:undeployed:{FIXTURE_ADDR_HEX}");
        assert_eq!(payload["iss"], expected_did);
        assert_eq!(payload["sub"], expected_did);
        assert_eq!(payload["aud"], "did:midnight:issuer-mock");
        assert_eq!(payload["nonce"], "n-test");
        assert_eq!(payload["iat"], 1_700_000_000);
        assert_eq!(payload["exp"], 1_700_000_000 + 300);
        assert_eq!(payload["sub_jwk"]["kty"], "OKP");
        assert_eq!(payload["sub_jwk"]["crv"], "Ed25519");
        assert_eq!(payload["sub_jwk"]["x"], "AAAA");
    }

    /// The architectural payoff of Task 5: exactly **one**
    /// discovery + **one** sign per id_token. The legacy
    /// `oid4vp_client::jws::build_id_token` did 2 + 2.
    #[tokio::test]
    async fn build_calls_each_port_exactly_once() {
        let disc = Arc::new(CountingDiscovery::ok(dummy_key()));
        let signer = Arc::new(CountingSigner::ok());
        let clock = Arc::new(FixedClock::new(1_700_000_000_000));

        let builder = IdTokenBuilder::new(
            disc.clone() as Arc<dyn DidAuthnDiscovery>,
            signer.clone() as Arc<dyn DidSigner>,
            clock,
            holder_did(),
        );
        let mut resp = AuthorizationResponse::new(None);
        builder.build(&req_mode_a(), &mut resp).await.expect("build");

        assert_eq!(disc.calls(), 1, "DidAuthnDiscovery called once");
        assert_eq!(signer.calls(), 1, "DidSigner called once");

        // And the signer signed under the kid discovery returned.
        let signed_kid = signer.last_kid.lock().unwrap().clone();
        assert_eq!(
            signed_kid,
            Some(format!("did:midnight:undeployed:{FIXTURE_ADDR_HEX}#key-auth")),
        );
    }

    #[tokio::test]
    async fn build_signs_header_dot_payload() {
        let disc = Arc::new(CountingDiscovery::ok(dummy_key()));
        let signer = Arc::new(CountingSigner::ok());
        let clock = Arc::new(FixedClock::new(1_700_000_000_000));
        let builder = IdTokenBuilder::new(
            disc as Arc<dyn DidAuthnDiscovery>,
            signer.clone() as Arc<dyn DidSigner>,
            clock,
            holder_did(),
        );

        let mut resp = AuthorizationResponse::new(None);
        builder.build(&req_mode_a(), &mut resp).await.expect("build");

        let id_token = resp.id_token.unwrap();
        let parts: Vec<&str> = id_token.split('.').collect();
        let signed_input = format!("{}.{}", parts[0], parts[1]);
        let observed = signer.last_payload.lock().unwrap().clone().unwrap();
        assert_eq!(observed, signed_input.as_bytes());
    }

    #[tokio::test]
    async fn discover_error_maps_to_login_error() {
        let disc = Arc::new(CountingDiscovery::fails(DiscoverError::NoAuthnKey(
            format!("did:midnight:undeployed:{FIXTURE_ADDR_HEX}"),
        )));
        let signer = Arc::new(CountingSigner::ok());
        let clock = Arc::new(FixedClock::new(1_700_000_000_000));
        let builder = IdTokenBuilder::new(
            disc.clone() as Arc<dyn DidAuthnDiscovery>,
            signer.clone() as Arc<dyn DidSigner>,
            clock,
            holder_did(),
        );

        let mut resp = AuthorizationResponse::new(None);
        let err = builder
            .build(&req_mode_a(), &mut resp)
            .await
            .expect_err("must fail");
        assert!(matches!(err, LoginError::DiscoverFailed(ref m) if m.contains("authentication-relation")));
        // Signer never reached.
        assert_eq!(signer.calls(), 0);
        // And no id_token written.
        assert!(resp.id_token.is_none());
    }

    #[tokio::test]
    async fn sign_error_maps_to_login_error() {
        let disc = Arc::new(CountingDiscovery::ok(dummy_key()));
        let signer = Arc::new(CountingSigner::fails(SignError::NoLocalSecret(
            format!("did:midnight:undeployed:{FIXTURE_ADDR_HEX}#key-auth"),
        )));
        let clock = Arc::new(FixedClock::new(1_700_000_000_000));
        let builder = IdTokenBuilder::new(
            disc as Arc<dyn DidAuthnDiscovery>,
            signer as Arc<dyn DidSigner>,
            clock,
            holder_did(),
        );

        let mut resp = AuthorizationResponse::new(None);
        let err = builder
            .build(&req_mode_a(), &mut resp)
            .await
            .expect_err("must fail");
        assert!(matches!(err, LoginError::SignFailed(ref m) if m.contains("no local secret")));
        assert!(resp.id_token.is_none());
    }
}
