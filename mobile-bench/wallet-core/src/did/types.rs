//! DID Core types restricted to Midnight's verification-method
//! shape (Ed25519 / P-256 / Jubjub).
//!
//! Mirrors `midnight-did-domain`:
//! - `did-document.ts` → [`DidDocument`], [`VerificationMethod`],
//!   [`Service`].
//! - `crypto-codecs.ts` → [`KeyType`] / [`CurveType`] / [`PublicKeyJwk`].
//!
//! Field naming follows Rust's snake_case; (de)serialisation uses
//! the JSON-LD camelCase the DID Core spec prescribes.

use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::did::id::DidId;

/// DID Core 1.0 document.
///
/// Field order is significant for the JSON-LD canonical form, but
/// serde does not preserve insertion order across all targets, so
/// downstream comparisons must use a canonical-JSON crate. For now
/// we serialize/deserialize round-trip on the wire and let the
/// caller worry about canonicalisation when signing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DidDocument {
    pub id: DidId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub controller: Option<DidId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub also_known_as: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub verification_method: Vec<VerificationMethod>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub authentication: Vec<VerificationMethodRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assertion_method: Vec<VerificationMethodRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub key_agreement: Vec<VerificationMethodRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capability_invocation: Vec<VerificationMethodRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capability_delegation: Vec<VerificationMethodRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub service: Vec<Service>,
    /// Out-of-spec metadata fields we still want to keep around.
    /// The DID Core spec treats these as `didDocumentMetadata`, not
    /// part of the document proper, but for a pure-Rust API it's
    /// simpler to keep them on one struct.
    #[serde(default, skip_serializing_if = "is_default_bool")]
    pub deactivated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<SystemTime>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated: Option<SystemTime>,
    #[serde(default, skip_serializing_if = "is_default_u64")]
    pub version: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationMethod {
    /// `<did>#<fragment>`.
    pub id: String,
    #[serde(rename = "type")]
    pub typ: VerificationMethodType,
    pub controller: DidId,
    pub public_key_jwk: PublicKeyJwk,
}

/// A verification-method ID *or* an inline `VerificationMethod`. DID
/// Core allows both forms in the relation arrays
/// (`authentication`, etc.). We model both via an untagged enum.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum VerificationMethodRef {
    Id(String),
    Inline(VerificationMethod),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VerificationMethodType {
    /// The single type Midnight emits today, per
    /// `midnight-did/api/src/lib.ts:assertMidnightKeyProfile`.
    JsonWebKey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VerificationMethodRelation {
    Authentication,
    AssertionMethod,
    KeyAgreement,
    CapabilityInvocation,
    CapabilityDelegation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeyType {
    /// Octet key pair (Ed25519).
    OKP,
    /// Elliptic curve (P-256 / Jubjub).
    EC,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CurveType {
    Ed25519,
    P256,
    Jubjub,
}

/// JWK constrained to Midnight's three accepted profiles. The
/// `x`/`y` fields are URL-safe base64 of the curve coordinate
/// bytes; for OKP/Ed25519 only `x` is set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicKeyJwk {
    pub kty: KeyType,
    pub crv: CurveType,
    /// Base64url-encoded x coordinate.
    pub x: String,
    /// Base64url-encoded y coordinate. None for OKP/Ed25519.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Service {
    pub id: String,
    #[serde(rename = "type")]
    pub typ: String,
    pub service_endpoint: ServiceEndpoint,
}

/// Service endpoint is either a single URL string or a structured
/// JSON object per DID Core. We keep it as `serde_json::Value` for
/// a faithful round-trip; consumers can pattern-match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ServiceEndpoint {
    Uri(String),
    Object(serde_json::Value),
}

fn is_default_bool(b: &bool) -> bool {
    !*b
}
fn is_default_u64(n: &u64) -> bool {
    *n == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Network;

    fn fixture_did() -> DidId {
        DidId::new(Network::PreProd, [0x42u8; 32])
    }

    #[test]
    fn round_trip_minimal_document() {
        let doc = DidDocument {
            id: fixture_did(),
            controller: None,
            also_known_as: vec![],
            verification_method: vec![],
            authentication: vec![],
            assertion_method: vec![],
            key_agreement: vec![],
            capability_invocation: vec![],
            capability_delegation: vec![],
            service: vec![],
            deactivated: false,
            created: None,
            updated: None,
            version: 0,
        };
        let json = serde_json::to_string(&doc).unwrap();
        let back: DidDocument = serde_json::from_str(&json).unwrap();
        assert_eq!(doc, back);
    }

    #[test]
    fn round_trip_with_jwk_and_service() {
        let did = fixture_did();
        let vm = VerificationMethod {
            id: format!("{}#key-1", did.to_did_string()),
            typ: VerificationMethodType::JsonWebKey,
            controller: did.clone(),
            public_key_jwk: PublicKeyJwk {
                kty: KeyType::EC,
                crv: CurveType::Jubjub,
                x: "abcd".into(),
                y: Some("efgh".into()),
            },
        };
        let svc = Service {
            id: format!("{}#service-1", did.to_did_string()),
            typ: "LinkedDomains".into(),
            service_endpoint: ServiceEndpoint::Uri(
                "https://example.test".into(),
            ),
        };
        let doc = DidDocument {
            id: did.clone(),
            controller: Some(did.clone()),
            also_known_as: vec!["did:web:example.test".into()],
            verification_method: vec![vm.clone()],
            authentication: vec![VerificationMethodRef::Id(vm.id.clone())],
            assertion_method: vec![VerificationMethodRef::Inline(vm.clone())],
            key_agreement: vec![],
            capability_invocation: vec![],
            capability_delegation: vec![],
            service: vec![svc],
            deactivated: false,
            created: None,
            updated: None,
            version: 1,
        };
        let json = serde_json::to_string(&doc).unwrap();
        let back: DidDocument = serde_json::from_str(&json).unwrap();
        assert_eq!(doc, back);
    }
}
