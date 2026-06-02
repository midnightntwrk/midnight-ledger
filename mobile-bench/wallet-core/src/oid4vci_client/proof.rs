//! Proof-of-possession builders for the OID4VCI `/credential`
//! request.
//!
//! ## What lives here
//!
//! The OID4VCI spec carries a `proof` object in every `/credential`
//! request, signed by the holder, that the issuer's verifier uses to
//! re-bind the freshly-minted VC to the holder's key. Phase 1 ships
//! exactly one proof type — the SIOPv2-shaped JWS bound to the
//! issuer's `c_nonce` (`proof_type = "jwt"`), produced via the same
//! `sign_id_token_with_ports` helper OID4VP's `IdTokenBuilder` uses.
//!
//! ## Why a trait + coordinator
//!
//! OID4VP's `ResponseBuilder` + `LoginCoordinator` (Phase-1 today,
//! Phase-2 modes B/C registered as additional builders) is the
//! canonical composability seam in this codebase. The OID4VCI side
//! used to be a flat function — the `request_credential` body inlined
//! the proof construction. That meant adding another proof type
//! (`ldp_vp`, `mso_mdoc`, EBSI proofs) required editing the
//! orchestrator. Lifting the proof-construction step into
//! [`ProofBuilder`] + [`CredentialCoordinator`] makes the OID4VCI
//! flow extensible the same way OID4VP's response is — register a
//! new builder, don't edit `request_credential`.
//!
//! Spec rationale: `docs/superpowers/specs/2026-06-03-hex-architecture-audit.md`
//! §5.B.

use std::sync::Arc;

use async_trait::async_trait;

use crate::clock::Clock;
use crate::oid4vp_client::id_token::sign_id_token_with_ports;
use crate::oid4vp_client::{DidAuthnDiscovery, DidSigner};
use crate::DidId;

use super::credential::CredentialFlowError;

/// Phase-1 default proof TTL — 5 minutes, matching the OID4VP
/// id_token convention so c_nonce-bound replay windows have the
/// same upper bound across both flows.
pub(crate) const DEFAULT_PROOF_LIFETIME_SECS: u64 = 300;

/// The minted proof — what gets embedded in the `/credential`
/// request body under the `proof` field.
///
/// `proof_type` is the OID4VCI-spec identifier for the proof
/// shape (`"jwt"`, `"ldp_vp"`, `"mso_mdoc"`, …). `payload` is
/// the wire bytes for that shape — for `jwt`, it's the compact
/// JWS string the issuer's JWS verifier parses.
///
/// The current Phase-1 builder ([`IdTokenProofBuilder`]) emits
/// `proof_type = "jwt"`. Phase-2 builders for other proof types
/// will set their own `proof_type` and serialise differently —
/// the wire envelope `{ proof_type, jwt }` is preserved as long
/// as the issuer's parser stays JWT-shaped; richer envelopes
/// will land alongside their builders.
#[derive(Debug, Clone)]
pub struct ProofValue {
    pub proof_type: String,
    /// For `proof_type = "jwt"`, this is the compact JWS
    /// (`header.payload.signature`). For other proof types,
    /// it's whatever wire payload that type defines.
    pub payload: String,
}

/// Trait surface for OID4VCI proof-of-possession construction.
///
/// Implementations are async because every realistic
/// proof-of-possession involves a remote DID resolution (to
/// discover the kid + key) and a signing primitive — both of
/// which the wallet exposes via the
/// [`DidAuthnDiscovery`] + [`DidSigner`] port pair.
#[async_trait]
pub trait ProofBuilder: Send + Sync {
    /// Mint a proof bound to the issuer's `c_nonce`. `issuer` is
    /// the OID4VCI issuer's base URL — placed in the `aud` claim
    /// of the resulting JWS so the verifier can confirm the
    /// proof was minted FOR THIS issuer, not replayed from
    /// another.
    async fn build(
        &self,
        issuer: &str,
        c_nonce: &str,
    ) -> Result<ProofValue, CredentialFlowError>;
}

/// Phase-1 proof builder: SIOPv2-shape JWS proof
/// (`proof_type = "jwt"`).
///
/// Goes through [`sign_id_token_with_ports`] so the OID4VP and
/// OID4VCI flows emit byte-for-byte identical JWS shapes — same
/// header `{alg=EdDSA, typ=JWT, kid}`, same payload schema with
/// `sub_jwk` in the payload, same EdDSA signature. The only
/// differences are the claim values: OID4VCI sets `aud = issuer
/// URL` and `nonce = c_nonce`, while OID4VP sets `aud = RP
/// client_id` and `nonce = request.nonce`.
pub struct IdTokenProofBuilder {
    pub discovery: Arc<dyn DidAuthnDiscovery>,
    pub signer: Arc<dyn DidSigner>,
    pub clock: Arc<dyn Clock>,
    pub holder: DidId,
    /// JWS TTL from `iat`. 300 by convention; override for
    /// tests that need to drive an expiry.
    pub lifetime_secs: u64,
}

impl IdTokenProofBuilder {
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
            lifetime_secs: DEFAULT_PROOF_LIFETIME_SECS,
        }
    }
}

#[async_trait]
impl ProofBuilder for IdTokenProofBuilder {
    async fn build(
        &self,
        issuer: &str,
        c_nonce: &str,
    ) -> Result<ProofValue, CredentialFlowError> {
        let jwt = sign_id_token_with_ports(
            &*self.discovery,
            &*self.signer,
            &*self.clock,
            &self.holder,
            issuer,
            c_nonce,
            self.lifetime_secs,
        )
        .await?;
        Ok(ProofValue {
            proof_type: "jwt".into(),
            payload: jwt,
        })
    }
}

/// Composes the OID4VCI `/credential`-request pipeline. Owns a
/// single [`ProofBuilder`] today; Phase 2 may grow this into a
/// chain (multiple proof types if the issuer's
/// `credential_configurations_supported` lists more than one)
/// — at which point this becomes
/// `Vec<Box<dyn ProofBuilder>>` and we walk it the way
/// [`LoginCoordinator`] walks `Vec<Box<dyn ResponseBuilder>>`.
///
/// [`LoginCoordinator`]: crate::oid4vp_client::LoginCoordinator
pub struct CredentialCoordinator {
    pub(crate) proof_builder: Box<dyn ProofBuilder>,
}

impl CredentialCoordinator {
    /// Construct from any [`ProofBuilder`]. Use this when the
    /// caller wants a custom proof type (Phase 2+).
    pub fn new(proof_builder: Box<dyn ProofBuilder>) -> Self {
        Self { proof_builder }
    }

    /// Phase-1 convenience: a JWT-shaped proof bound to the
    /// holder's `key-auth`. Mirrors `LoginCoordinator::mode_a`
    /// for OID4VP.
    pub fn jwt(builder: IdTokenProofBuilder) -> Self {
        Self::new(Box::new(builder))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::clock::FixedClock;
    use crate::oid4vp_client::{AuthnKey, DiscoverError, SignError};
    use crate::{CurveType, KeyType, PublicKeyJwk};

    /// 32-byte hex address — matches the DID URL the live
    /// wallet emits. `DidId::parse`'s hex decoder rejects
    /// anything shorter or odd-length.
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

    struct CountingDiscovery {
        key: AuthnKey,
        calls: AtomicUsize,
    }
    #[async_trait]
    impl DidAuthnDiscovery for CountingDiscovery {
        async fn authn_key(&self, _did: &DidId) -> Result<AuthnKey, DiscoverError> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            Ok(self.key.clone())
        }
    }

    struct CountingSigner {
        calls: AtomicUsize,
        canned_sig: Vec<u8>,
        last_aud: std::sync::Mutex<Option<String>>,
        last_nonce: std::sync::Mutex<Option<String>>,
    }
    #[async_trait]
    impl DidSigner for CountingSigner {
        async fn sign(
            &self,
            _kid: &str,
            payload: &[u8],
        ) -> Result<Vec<u8>, SignError> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            // Decode the JWS payload segment to confirm what got
            // signed (the architectural payoff check below).
            let s = std::str::from_utf8(payload).unwrap();
            let parts: Vec<&str> = s.split('.').collect();
            let payload_b64 = parts[1];
            use base64::engine::general_purpose::URL_SAFE_NO_PAD;
            use base64::Engine;
            let payload_bytes = URL_SAFE_NO_PAD.decode(payload_b64).unwrap();
            let payload_json: serde_json::Value =
                serde_json::from_slice(&payload_bytes).unwrap();
            *self.last_aud.lock().unwrap() =
                Some(payload_json["aud"].as_str().unwrap().to_string());
            *self.last_nonce.lock().unwrap() =
                Some(payload_json["nonce"].as_str().unwrap().to_string());
            Ok(self.canned_sig.clone())
        }
    }

    fn holder_did() -> DidId {
        DidId::parse(&format!("did:midnight:undeployed:{FIXTURE_ADDR_HEX}"))
            .expect("parse")
    }

    #[tokio::test]
    async fn id_token_proof_builder_signs_with_c_nonce_and_issuer_aud() {
        let disc = Arc::new(CountingDiscovery {
            key: dummy_key(),
            calls: AtomicUsize::new(0),
        });
        let signer = Arc::new(CountingSigner {
            calls: AtomicUsize::new(0),
            canned_sig: vec![0xAA; 64],
            last_aud: std::sync::Mutex::new(None),
            last_nonce: std::sync::Mutex::new(None),
        });
        let clock = Arc::new(FixedClock::new(1_700_000_000_000));
        let builder = IdTokenProofBuilder::new(
            disc.clone() as Arc<dyn DidAuthnDiscovery>,
            signer.clone() as Arc<dyn DidSigner>,
            clock,
            holder_did(),
        );

        let proof = builder
            .build("https://issuer.local", "C-NONCE-1")
            .await
            .expect("ok");

        // Wire shape: JWT-typed proof with three-segment JWS.
        assert_eq!(proof.proof_type, "jwt");
        let parts: Vec<&str> = proof.payload.split('.').collect();
        assert_eq!(parts.len(), 3, "JWS = header.payload.sig");

        // Audience + nonce flow through to the signed payload —
        // the issuer's verifier looks them up there. Wrong aud →
        // 401; wrong nonce → 401.
        assert_eq!(
            signer.last_aud.lock().unwrap().as_deref(),
            Some("https://issuer.local"),
        );
        assert_eq!(
            signer.last_nonce.lock().unwrap().as_deref(),
            Some("C-NONCE-1"),
        );

        // Single resolve + single sign — same architectural
        // payoff `IdTokenBuilder::build_calls_each_port_exactly_once`
        // checks for the OID4VP path.
        assert_eq!(disc.calls.load(Ordering::Relaxed), 1);
        assert_eq!(signer.calls.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn coordinator_jwt_constructor_wires_builder() {
        let disc = Arc::new(CountingDiscovery {
            key: dummy_key(),
            calls: AtomicUsize::new(0),
        });
        let signer = Arc::new(CountingSigner {
            calls: AtomicUsize::new(0),
            canned_sig: vec![0xAA; 64],
            last_aud: std::sync::Mutex::new(None),
            last_nonce: std::sync::Mutex::new(None),
        });
        let clock = Arc::new(FixedClock::new(1_700_000_000_000));
        let coordinator = CredentialCoordinator::jwt(IdTokenProofBuilder::new(
            disc as Arc<dyn DidAuthnDiscovery>,
            signer as Arc<dyn DidSigner>,
            clock,
            holder_did(),
        ));

        let proof = coordinator
            .proof_builder
            .build("https://issuer.local", "n")
            .await
            .expect("ok");
        assert_eq!(proof.proof_type, "jwt");
    }
}
