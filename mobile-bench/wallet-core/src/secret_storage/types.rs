//! Data model — direct port of `secret-storage/src/types.ts`.
//!
//! Field-by-field equivalent so an operator who knows the upstream
//! TypeScript API can move to Rust without re-learning the shape.
//! JSON serialisation uses camelCase to match what the upstream
//! file format would have written for interop with auxiliary
//! tooling.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Three curves the Midnight DID protocol allows for verification
/// methods. Maps 1:1 to the upstream `MidnightCurve`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MidnightCurve {
    Ed25519,
    Jubjub,
    #[serde(rename = "P-256")]
    P256,
}

/// JWK `kty` values the protocol permits. `OKP` pairs with
/// Ed25519, `EC` pairs with P-256 / Jubjub.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MidnightKeyType {
    OKP,
    EC,
}

/// Opaque handle the store hands out to refer to a stored key.
/// Treat as an opaque string; today it's a UUID v4.
pub type SecretKeyRef = String;

/// Public-key JWK in the subset the protocol uses. `y` is
/// `Some(_)` for EC curves and `None` for OKP (Ed25519 has only
/// `x`). String form mirrors the upstream — base64url for byte
/// strings, decimal for bigint-aligned Jubjub coordinates.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicJwk {
    pub kty: MidnightKeyType,
    pub crv: MidnightCurve,
    pub x: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub y: Option<String>,
}

/// Metadata envelope for a stored secret. Mirrors
/// `StoredKeyMeta` in the upstream — every field present, same
/// semantics. Timestamps are RFC 3339 strings (upstream's
/// `new Date().toISOString()` format).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredKeyMeta {
    /// Caller-supplied label; e.g. "issuer-key-2026".
    pub id: String,
    /// Opaque store handle the caller uses to reference this key.
    pub key_ref: SecretKeyRef,
    /// DID the key is bound to. Optional — keys can exist before a
    /// DID is created.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub did: Option<String>,
    /// Free-form purpose tag — e.g. "authentication",
    /// "assertionMethod". Not parsed by the store.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purpose: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub algorithm: AlgorithmTag,
}

/// `algorithm` sub-object on [`StoredKeyMeta`] — narrow record
/// of `(kty, crv)` so callers can switch on curve without
/// dereferencing the whole JWK.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlgorithmTag {
    pub kty: MidnightKeyType,
    pub crv: MidnightCurve,
}

/// Args for [`SecretStorage::generate_key`].
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateKeyInput {
    pub id: String,
    pub kty: MidnightKeyType,
    pub crv: MidnightCurve,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub did: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purpose: Option<String>,
}

/// Args for [`SecretStorage::import_key`].
#[derive(Clone, Debug)]
pub struct ImportKeyInput {
    pub id: String,
    /// Raw secret bytes. Ed25519: 32-byte seed. P-256: 32-byte
    /// scalar. Jubjub: 32-byte scalar.
    pub private_key: Vec<u8>,
    pub kty: MidnightKeyType,
    pub crv: MidnightCurve,
    pub did: Option<String>,
    pub purpose: Option<String>,
}

/// Args for [`SecretStorage::derive_key_from_seed`].
#[derive(Clone, Debug)]
pub struct DeriveKeyFromSeedInput {
    pub id: String,
    /// 64-char hex string — 32 bytes of BIP32 seed material.
    pub seed_hex: String,
    pub kty: MidnightKeyType,
    pub crv: MidnightCurve,
    /// BIP32 account index. Defaults to 0.
    pub account: Option<u32>,
    /// Per-curve key index. Defaults to 0.
    pub index: Option<u32>,
    pub did: Option<String>,
    pub purpose: Option<String>,
}

/// Args for [`SecretStorage::verify`]. Either `key_ref` (look the
/// pk up from the store) or `public_jwk` (caller supplies an
/// external pk) is required — pure-detached verification.
#[derive(Clone, Debug)]
pub struct VerifyInput {
    pub key_ref: Option<SecretKeyRef>,
    pub public_jwk: Option<PublicJwk>,
    pub payload: Vec<u8>,
    pub signature: Vec<u8>,
}

/// Output of [`SecretStorage::sign`]. `format` matches the
/// upstream's narrow `"raw"` literal — concatenated byte form, no
/// envelope (DER / IEEE-P1363 / etc).
#[derive(Clone, Debug)]
pub struct SignOutput {
    pub signature: Vec<u8>,
    pub format: SignatureFormat,
}

/// Wire format of a signature. Today the store only emits `Raw`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SignatureFormat {
    Raw,
}

/// Async trait every secret-store backend implements. Mirrors the
/// upstream's `SecretStorage` interface; one Rust impl in this
/// crate today ([`crate::secret_storage::file_secret_store::FileSecretStore`]
/// — once it lands).
#[async_trait]
pub trait SecretStorage: Send + Sync {
    async fn initialize(
        &mut self,
        location: &std::path::Path,
        passphrase: Option<&str>,
    ) -> Result<(), crate::secret_storage::SecretStoreError>;

    async fn list_keys(
        &self,
        did_filter: Option<&str>,
    ) -> Result<Vec<StoredKeyMeta>, crate::secret_storage::SecretStoreError>;

    async fn generate_key(
        &mut self,
        params: GenerateKeyInput,
    ) -> Result<(SecretKeyRef, PublicJwk), crate::secret_storage::SecretStoreError>;

    async fn import_key(
        &mut self,
        params: ImportKeyInput,
    ) -> Result<(SecretKeyRef, PublicJwk), crate::secret_storage::SecretStoreError>;

    async fn derive_key_from_seed(
        &mut self,
        params: DeriveKeyFromSeedInput,
    ) -> Result<(SecretKeyRef, PublicJwk), crate::secret_storage::SecretStoreError>;

    async fn get_public_key(
        &self,
        key_ref: &str,
    ) -> Result<PublicJwk, crate::secret_storage::SecretStoreError>;

    async fn sign(
        &self,
        key_ref: &str,
        payload: &[u8],
    ) -> Result<SignOutput, crate::secret_storage::SecretStoreError>;

    async fn verify(
        &self,
        input: VerifyInput,
    ) -> Result<bool, crate::secret_storage::SecretStoreError>;

    async fn delete_key(
        &mut self,
        key_ref: &str,
    ) -> Result<(), crate::secret_storage::SecretStoreError>;
}
