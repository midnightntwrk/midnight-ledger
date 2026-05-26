//! Data model — direct port of `secret-storage/src/types.ts`.
//!
//! Field-by-field equivalent so an operator who knows the upstream
//! TypeScript API can move to Rust without re-learning the shape.
//! JSON serialisation uses camelCase to match what the upstream
//! file format would have written for interop with auxiliary
//! tooling.

use async_trait::async_trait;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

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

/// Handle the store hands out to refer to a stored key.
///
/// Carries two identifiers:
/// - [`SecretKeyRef::uuid`] — the opaque internal handle the store
///   uses to look the key up. UUID v4 today.
/// - [`SecretKeyRef::id`] — the caller-supplied "kid" tag (e.g.
///   `"ed25519/authentication"`). Mirrors [`StoredKeyMeta::id`].
///
/// On the wire / on disk the value is serialised as the bare UUID
/// string for backwards compatibility with the existing file and
/// redb formats. The `kid` is recovered from the surrounding
/// metadata's `id` field after deserialisation by the concrete
/// store. Loading a `SecretKeyRef` directly from a bare string
/// (e.g. a UUID handed in by a caller) leaves `kid` empty until the
/// store fills it in from metadata.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct SecretKeyRef {
    pub(crate) uuid: String,
    pub(crate) kid: String,
}

impl SecretKeyRef {
    /// Construct a key ref with both fields explicit. Used by
    /// store impls when materialising a meta row.
    pub fn new(uuid: impl Into<String>, kid: impl Into<String>) -> Self {
        Self {
            uuid: uuid.into(),
            kid: kid.into(),
        }
    }

    /// Construct from a bare UUID handle, leaving the kid empty.
    /// Stores patch the kid in from metadata after deserialisation.
    pub fn from_uuid(uuid: impl Into<String>) -> Self {
        Self {
            uuid: uuid.into(),
            kid: String::new(),
        }
    }

    /// The caller-supplied kid tag, e.g. `"ed25519/authentication"`.
    /// Empty for refs reconstructed from a bare UUID before the
    /// surrounding meta has been consulted.
    pub fn id(&self) -> &str {
        &self.kid
    }

    /// The internal opaque handle (UUID v4 today).
    pub fn uuid(&self) -> &str {
        &self.uuid
    }

}

impl AsRef<str> for SecretKeyRef {
    /// Returns the UUID handle so call sites that previously
    /// passed a `&SecretKeyRef` (when it was a `String` alias) to
    /// functions taking `&str` keep working.
    fn as_ref(&self) -> &str {
        &self.uuid
    }
}

impl std::fmt::Display for SecretKeyRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.uuid)
    }
}

impl Serialize for SecretKeyRef {
    /// Wire format is just the UUID — same shape as the legacy
    /// `pub type SecretKeyRef = String` alias produced. The kid
    /// lives separately in `StoredKeyMeta::id`.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.uuid)
    }
}

impl<'de> Deserialize<'de> for SecretKeyRef {
    /// Reads a bare UUID string; leaves `kid` empty. The owning
    /// `StoredKeyMeta` is expected to populate `kid` from its `id`
    /// field afterwards.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Ok(SecretKeyRef::from_uuid(s))
    }
}

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

    /// Import a 32-byte Ed25519 secret seed (RFC 8032) and tag the
    /// stored entry with `kid` (e.g. `"ed25519/authentication"`).
    /// The returned [`SecretKeyRef::id`] echoes `kid`; the internal
    /// UUID handle is available via [`SecretKeyRef::uuid`].
    ///
    /// Default impl delegates to [`SecretStorage::import_key`].
    /// Backends only need to override if they want to validate the
    /// seed differently.
    async fn import_ed25519(
        &mut self,
        secret: &[u8; 32],
        kid: &str,
    ) -> Result<SecretKeyRef, crate::secret_storage::SecretStoreError> {
        let (key_ref, _) = self
            .import_key(ImportKeyInput {
                id: kid.to_string(),
                private_key: secret.to_vec(),
                kty: MidnightKeyType::OKP,
                crv: MidnightCurve::Ed25519,
                did: None,
                purpose: None,
            })
            .await?;
        Ok(key_ref)
    }

    /// Find a stored key whose kid (the caller-supplied `id` tag,
    /// e.g. `"ed25519/authentication"` or a full DID URL with
    /// fragment) matches. Walks [`SecretStorage::list_keys`] so
    /// backends don't have to maintain a second index — not
    /// hot-path-critical (called at most once per outbound auth
    /// request). Default impl returns `None` on any underlying
    /// error so callers see a clean miss rather than having to
    /// disambiguate "not present" from "store errored".
    async fn find_by_kid(&self, kid: &str) -> Option<SecretKeyRef> {
        self.list_keys(None)
            .await
            .ok()?
            .into_iter()
            .find(|m| m.key_ref.id() == kid)
            .map(|m| m.key_ref)
    }

    /// Import a 32-byte Jubjub scalar and tag the stored entry with
    /// `kid` (e.g. `"jubjub/assertionMethod"`). See
    /// [`SecretStorage::import_ed25519`] for return semantics.
    async fn import_jubjub(
        &mut self,
        secret: &[u8; 32],
        kid: &str,
    ) -> Result<SecretKeyRef, crate::secret_storage::SecretStoreError> {
        let (key_ref, _) = self
            .import_key(ImportKeyInput {
                id: kid.to_string(),
                private_key: secret.to_vec(),
                kty: MidnightKeyType::EC,
                crv: MidnightCurve::Jubjub,
                did: None,
                purpose: None,
            })
            .await?;
        Ok(key_ref)
    }
}
