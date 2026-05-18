//! Wire schema for the redb-backed wallet store.
//!
//! Every table is namespaced under `store/`. The structure
//! mirrors the design recommended in `mobile-bench/STORE_PLAN.md`:
//! one file, network as the sharding axis, row-level envelope
//! for secret-bearing values.
//!
//! Row structs are versioned by suffix (`WalletRowV1` etc.); a
//! schema change adds a new struct + a `migrate::v_to_v+1`
//! closure that walks the old rows and writes new ones. The
//! `SCHEMA_VERSION` constant is the only source of truth for
//! "what version is on-disk expected to be".

use redb::{MultimapTableDefinition, TableDefinition};
use serde::{Deserialize, Serialize};

use crate::Network;
use crate::secret_storage::{
    AlgorithmTag, MidnightCurve, MidnightKeyType, PublicJwk,
};
use crate::store::envelope::SecretEnvelope;

/// The on-disk schema this binary expects. Migration runs
/// `0..SCHEMA_VERSION` closures at `open()`.
pub const SCHEMA_VERSION: u32 = 2;

// ── Wallet identity ────────────────────────────────────────────

/// 16-byte UUID v4 carried as raw bytes. Stable across the
/// wallet's lifetime; does not leak any seed material since the
/// UUID is sampled independently from the seed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WalletId(pub [u8; 16]);

impl std::fmt::Display for WalletId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for b in self.0.iter() {
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}

// ── Network discriminator ──────────────────────────────────────

/// 1-byte network tag — the natural sharding axis for wallet
/// rows. Stable across releases (don't reorder), matches the
/// variant order in `Network::label()`. Values are deliberately
/// non-contiguous starting positions for future expansion.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NetworkTag(pub u8);

impl From<Network> for NetworkTag {
    fn from(n: Network) -> Self {
        NetworkTag(match n {
            Network::Mainnet => 1,
            Network::PreProd => 2,
            Network::Preview => 3,
            Network::QaNet => 4,
            Network::DevNet => 5,
            Network::Undeployed => 6,
        })
    }
}

impl TryFrom<u8> for NetworkTag {
    type Error = u8;
    fn try_from(b: u8) -> Result<Self, Self::Error> {
        if (1..=6).contains(&b) {
            Ok(NetworkTag(b))
        } else {
            Err(b)
        }
    }
}

// ── Tables ─────────────────────────────────────────────────────

/// Key-value scratch — `(name → bytes)`. Carries the on-disk
/// schema version + any future per-file constants (KDF params,
/// app salt, last-opened wallet id). Keys are bounded ASCII so
/// `&'static str` works as the column type.
pub(crate) const META: TableDefinition<&'static str, &'static [u8]> =
    TableDefinition::new("meta");
pub(crate) const META_SCHEMA_VERSION_KEY: &str = "schema_version";

/// Wallets — keyed by `WalletId` raw bytes; value is a
/// bincoded `WalletRowV1`. Seed is encrypted inside the row.
pub(crate) const WALLETS: TableDefinition<[u8; 16], &'static [u8]> =
    TableDefinition::new("wallets");

/// Controller secrets — keyed by `(network, did)`; value is a
/// bincoded `SecretEnvelope` wrapping 32 random bytes.
pub(crate) const CONTROLLER_SECRETS: TableDefinition<(u8, &'static str), &'static [u8]> =
    TableDefinition::new("controller_secrets");

/// Keys — keyed by `(wallet_id, key_ref)`. Value carries the
/// HD derivation parameters or, for imported keys, a wrapped
/// raw scalar. No raw scalars for HD-derived rows — the wallet
/// re-derives at sign time so a leaked DB without the seed is
/// just public-key metadata.
pub(crate) const KEYS: TableDefinition<([u8; 16], &'static str), &'static [u8]> =
    TableDefinition::new("keys");

/// Secondary index: `wallet_id → set of key_refs` so listing
/// keys (and filtering by DID via the row body) doesn't have
/// to scan the entire `keys` table.
pub(crate) const KEYS_BY_WALLET: MultimapTableDefinition<[u8; 16], &'static str> =
    MultimapTableDefinition::new("keys_by_wallet");

// ── Row types ─────────────────────────────────────────────────

/// Wallet row, version 1. A schema change creates `WalletRowV2`
/// and the migration registry walks the table.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct WalletRowV1 {
    /// Human-readable label — what the UI shows in the wallet
    /// picker. Free-form; not used as a key.
    pub label: String,
    /// Sharding axis. Same seed can spawn one wallet per
    /// network if a workflow ever wants that; today the demo
    /// wallets use one network per row.
    pub network: NetworkTag,
    /// Pre-rendered bech32m receive address. Optional — empty
    /// string means "not derived yet". Cached so the UI's
    /// wallet picker doesn't have to unlock the seed just to
    /// show the address.
    pub address_bech32: String,
    /// Created / last-modified timestamps in unix-ms.
    pub created_at: i64,
    pub updated_at: i64,
    /// AES-256-GCM envelope wrapping the 32-byte seed. Each
    /// row carries its own scrypt salt + IV; rotating the
    /// passphrase rewrites every wrapped row but leaves
    /// non-secret rows untouched.
    pub seed_envelope: SecretEnvelope,
}

// Required to use NetworkTag inside a Serialize derive without
// pulling in a serde-only helper crate.
impl Serialize for NetworkTag {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_u8(self.0)
    }
}

impl<'de> Deserialize<'de> for NetworkTag {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        let b = u8::deserialize(de)?;
        Ok(NetworkTag(b))
    }
}

/// How the wallet recovers a key's secret bytes. Stored in
/// `KeyRowV1.derivation`; the sign / get-private path branches
/// on this to either re-derive from the wallet seed (HD case)
/// or unwrap the row's envelope (imported case).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeyDerivation {
    /// BIP32 + HKDF derivation under `Role::Metadata`. The
    /// canonical path the upstream `secret-storage`'s
    /// `deriveKeyFromSeed` uses. `candidate` is the retry slot
    /// for curve-specific validity failures (P-256 zero scalar
    /// etc.) — almost always 0.
    Hkdf {
        account: u32,
        index: u32,
        candidate: u32,
    },
    /// Raw scalar bytes wrapped under the store passphrase.
    /// Used by `importKey` (the caller hands us a 32-byte
    /// secret with no derivation path).
    Direct {
        envelope: SecretEnvelope,
    },
}

/// Storage-side mirror of `PublicJwk`. The upstream
/// `PublicJwk` uses `#[serde(skip_serializing_if = "Option::is_none")]`
/// on `y` for JSON compactness; bincode honours that attribute
/// too, which makes encode + decode asymmetric and trips
/// "unexpected end of file" on every OKP key load. The
/// store-side struct drops the attribute so the on-disk
/// representation always carries the `Option` tag byte. The
/// `From` conversions on either side keep this private.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PublicJwkStored {
    pub kty: MidnightKeyType,
    pub crv: MidnightCurve,
    pub x: String,
    pub y: Option<String>,
}

impl From<PublicJwk> for PublicJwkStored {
    fn from(p: PublicJwk) -> Self {
        Self {
            kty: p.kty,
            crv: p.crv,
            x: p.x,
            y: p.y,
        }
    }
}

impl From<PublicJwkStored> for PublicJwk {
    fn from(p: PublicJwkStored) -> Self {
        Self {
            kty: p.kty,
            crv: p.crv,
            x: p.x,
            y: p.y,
        }
    }
}

/// Key row, version 1. Carries metadata + derivation; never
/// raw scalars (those are either re-derived from the wallet
/// seed or unwrapped from `KeyDerivation::Direct::envelope`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct KeyRowV1 {
    /// Caller-supplied label — `StoredKeyMeta.id`.
    pub label: String,
    /// DID the key is bound to, if any. The list-with-filter
    /// surface uses this to narrow results without a second
    /// table.
    pub did: Option<String>,
    /// Free-form purpose tag (`"authentication"`,
    /// `"assertionMethod"`, …). Not parsed by the store.
    pub purpose: Option<String>,
    pub kty: MidnightKeyType,
    pub crv: MidnightCurve,
    /// Pre-computed JWK so listing + display doesn't need to
    /// re-derive the public key. Cached at write time;
    /// invariant against derivation params (any drift means
    /// the row is corrupt).
    pub public_jwk: PublicJwkStored,
    pub algorithm: AlgorithmTag,
    pub derivation: KeyDerivation,
    pub created_at: i64,
    pub updated_at: i64,
}
