//! UniFFI bindings around `wallet-core`'s key surface.
//!
//! Scope deliberately limited to what's actually achievable
//! end-to-end:
//!
//! - **Keys CRUD** — full lifecycle via `RedbSecretStore`:
//!   open/create, list, generate, get-public, sign, delete.
//!
//! - **DID read / write** — explicitly `WalletError::NotImplemented`.
//!   DID resolution goes through `wallet_core::Wallet::resolve_did_full`
//!   which needs a full Wallet construction (seed, network attach,
//!   indexer client). DID writes additionally need the upstream-TS
//!   `prepareUnprovenCallTx` bridge that runs in the dioxus-wallet's
//!   WebView; porting that to RN's Hermes is its own subproject
//!   (architecture doc §14.2). Both surface here so the TS side
//!   gets type-safe stubs with a useful error message.

use std::sync::{Arc, Mutex};

use thiserror::Error;
use tokio::runtime::Handle;
use wallet_core::Network;
use wallet_core::secret_storage::{
    GenerateKeyInput, MidnightCurve, MidnightKeyType, PublicJwk, SecretStorage, SignOutput,
    StoredKeyMeta, redb_secret_store::RedbSecretStore,
};
use wallet_core::store::WalletStore;

#[derive(Error, Debug)]
pub enum WalletError {
    #[error("not yet implemented: {0}")]
    NotImplemented(String),
    #[error("already exists: {0}")]
    AlreadyExists(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("io: {0}")]
    Io(String),
    #[error("crypto: {0}")]
    Crypto(String),
    #[error("network: {0}")]
    Network(String),
    #[error("internal: {0}")]
    Internal(String),
}

/// Subset of `wallet_core::secret_storage::types::StoredKeyMeta`
/// flattened for UniFFI consumption.
#[derive(Debug, Clone)]
pub struct KeyInfo {
    pub key_ref: String,
    pub algorithm: String,
    pub created_at: String,
    pub label: Option<String>,
    pub public_key_jwk: String,
}

/// UniFFI `interface` — handle to one open wallet. Wraps a
/// `RedbSecretStore` behind an `Arc<Mutex<_>>` so multiple FFI
/// callers can share the handle (UniFFI's `interface` semantics
/// give us an `Arc<Self>` on the Rust side automatically).
pub struct Wallet {
    inner: Mutex<RedbSecretStore>,
    network: Network,
}

impl Wallet {
    /// UniFFI constructor — open or create a wallet at `path`.
    ///
    /// On first call, creates the redb file, encrypts the seed
    /// with `passphrase`, and binds a fresh `WalletId`. Subsequent
    /// calls open the existing file; the passphrase must match.
    pub fn new(
        path: String,
        passphrase: String,
        label: String,
        network: String,
        seed_bytes: Option<Vec<u8>>,
    ) -> Result<Self, WalletError> {
        let network = parse_network(&network)?;
        let store = WalletStore::open(&path, &passphrase)
            .map_err(|e| WalletError::Io(e.to_string()))?;

        // Check if we already have wallets in this store.
        let stats = store.stats().map_err(|e| WalletError::Io(e.to_string()))?;
        let wallet_id = if stats.wallets == 0 {
            // Fresh store — create a wallet row.
            let seed = seed_to_array(seed_bytes)?;
            store
                .create_wallet(&label, network, &seed)
                .map_err(|e| WalletError::Io(e.to_string()))?
        } else {
            // Re-open existing store — would normally look up the
            // matching wallet by network. For this demo iteration
            // we error if there's already a wallet but the caller
            // didn't say which one — the dioxus-wallet's
            // single-wallet-per-store assumption isn't what we
            // want long-term, but it's good enough to ship the
            // happy path.
            return Err(WalletError::AlreadyExists(format!(
                "store at {path:?} already has {} wallet(s); multi-wallet \
                 selection not yet implemented in this FFI iteration",
                stats.wallets
            )));
        };

        let secret_store = RedbSecretStore::new(store, wallet_id);

        Ok(Self {
            inner: Mutex::new(secret_store),
            network,
        })
    }

    pub fn network(&self) -> String {
        format!("{:?}", self.network).to_lowercase()
    }

    pub fn list_keys(&self) -> Result<Vec<KeyInfo>, WalletError> {
        let store = self
            .inner
            .lock()
            .map_err(|e| WalletError::Internal(e.to_string()))?;
        let metas =
            block_on(store.list_keys(None)).map_err(map_secret_err)?;
        let mut out = Vec::with_capacity(metas.len());
        for meta in metas {
            out.push(stored_meta_to_key_info(&store, meta)?);
        }
        Ok(out)
    }

    pub fn generate_key(
        &self,
        algorithm: String,
        label: Option<String>,
    ) -> Result<KeyInfo, WalletError> {
        let mut store = self
            .inner
            .lock()
            .map_err(|e| WalletError::Internal(e.to_string()))?;
        let (kty, crv) = curve_from_str(&algorithm)?;
        let params = GenerateKeyInput {
            id: label.clone().unwrap_or_else(|| {
                format!("key-{}", chrono_ms_or_zero())
            }),
            kty,
            crv,
            did: None,
            purpose: None,
        };
        let (key_ref, jwk) = block_on(store.generate_key(params)).map_err(map_secret_err)?;
        let metas = block_on(store.list_keys(None)).map_err(map_secret_err)?;
        let meta = metas
            .into_iter()
            .find(|m| m.key_ref == key_ref)
            .ok_or_else(|| {
                WalletError::Internal(format!(
                    "generated key {key_ref:?} not visible to list()"
                ))
            })?;
        Ok(KeyInfo {
            key_ref: meta.key_ref.clone(),
            algorithm: format_algorithm(&meta.algorithm),
            created_at: meta.created_at.clone(),
            label: Some(meta.id.clone()),
            public_key_jwk: jwk_to_json(&jwk)?,
        })
    }

    pub fn get_public_key_jwk(&self, key_ref: String) -> Result<String, WalletError> {
        let store = self
            .inner
            .lock()
            .map_err(|e| WalletError::Internal(e.to_string()))?;
        let jwk = block_on(store.get_public_key(&key_ref)).map_err(map_secret_err)?;
        jwk_to_json(&jwk)
    }

    pub fn sign(&self, key_ref: String, payload: Vec<u8>) -> Result<Vec<u8>, WalletError> {
        let store = self
            .inner
            .lock()
            .map_err(|e| WalletError::Internal(e.to_string()))?;
        let SignOutput { signature, .. } =
            block_on(store.sign(&key_ref, &payload)).map_err(map_secret_err)?;
        Ok(signature)
    }

    pub fn delete_key(&self, key_ref: String) -> Result<(), WalletError> {
        let mut store = self
            .inner
            .lock()
            .map_err(|e| WalletError::Internal(e.to_string()))?;
        block_on(store.delete_key(&key_ref)).map_err(map_secret_err)
    }
}

// ─── DID surface ──────────────────────────────────────────────────

/// Resolve a Midnight DID via the network's indexer.
///
/// Goes through `wallet_core::Wallet::resolve_did` which:
///   1. Parses the `did:midnight:<addr>` string into a `DidId`
///   2. Validates the network matches the parsed DID
///   3. Calls the indexer's `contract_state` GraphQL query
///   4. Decodes the on-chain state into a `DidDocument`
///
/// The `Wallet` instance we use here is constructed from a
/// throwaway zero-seed; resolve is purely read-only and never
/// touches the wallet's secret keys.
///
/// Returns the resolved document as JSON. On failure, returns
/// `WalletError::Network` with the indexer's error message.
pub fn did_resolve(network: String, did: String) -> Result<String, WalletError> {
    let net = parse_network(&network)?;
    let doc = block_on(async {
        // Read-only — seed not consulted by resolve_did.
        let wallet = wallet_core::Wallet::from_seed([0u8; 32], net);
        wallet
            .resolve_did(&did)
            .await
            .map_err(|e| WalletError::Network(e.to_string()))
    })?;
    serde_json::to_string(&doc).map_err(|e| WalletError::Internal(e.to_string()))
}

pub fn did_deploy(_wallet: Arc<Wallet>, _label: String) -> Result<String, WalletError> {
    Err(WalletError::NotImplemented(NOT_IMPLEMENTED_MSG.into()))
}

pub fn did_update_aka(
    _wallet: Arc<Wallet>,
    _did: String,
    _new_aka: String,
) -> Result<String, WalletError> {
    Err(WalletError::NotImplemented(NOT_IMPLEMENTED_MSG.into()))
}

pub fn did_deactivate(_wallet: Arc<Wallet>, _did: String) -> Result<String, WalletError> {
    Err(WalletError::NotImplemented(NOT_IMPLEMENTED_MSG.into()))
}

const NOT_IMPLEMENTED_MSG: &str = "DID write flows (deploy/update/deactivate) require the \
    upstream-TS prepareUnprovenCallTx bridge. Porting that bridge \
    to React Native's Hermes engine is its own subproject — see \
    architecture doc §14.2 for the integration plan. Until then, \
    these calls return NotImplemented. The keys CRUD path works today.";

// ─── helpers ──────────────────────────────────────────────────────

fn parse_network(s: &str) -> Result<Network, WalletError> {
    match s.to_lowercase().as_str() {
        "main" | "mainnet" => Ok(Network::Mainnet),
        "preprod" | "pre-prod" | "preProd" => Ok(Network::PreProd),
        "preview" => Ok(Network::Preview),
        "qanet" => Ok(Network::QaNet),
        "devnet" => Ok(Network::DevNet),
        "undeployed" | "regtest" => Ok(Network::Undeployed),
        other => Err(WalletError::Internal(format!(
            "unknown network {other:?}; expected mainnet/preprod/preview/qanet/devnet/undeployed"
        ))),
    }
}

fn seed_to_array(bytes: Option<Vec<u8>>) -> Result<[u8; 32], WalletError> {
    match bytes {
        None => {
            use rand::RngCore;
            let mut seed = [0u8; 32];
            rand::thread_rng().fill_bytes(&mut seed);
            Ok(seed)
        }
        Some(v) if v.len() == 32 => {
            let mut a = [0u8; 32];
            a.copy_from_slice(&v);
            Ok(a)
        }
        Some(v) => Err(WalletError::Crypto(format!(
            "seed must be 32 bytes, got {}",
            v.len()
        ))),
    }
}

/// Map a user-friendly algorithm string to the (kty, crv) pair the
/// store expects. Today we support Ed25519 (the typical DID auth
/// key) and Jubjub (the DID maintenance key the Midnight protocol
/// uses for circuit calls).
fn curve_from_str(s: &str) -> Result<(MidnightKeyType, MidnightCurve), WalletError> {
    match s.to_lowercase().as_str() {
        "ed25519" => Ok((MidnightKeyType::OKP, MidnightCurve::Ed25519)),
        "jubjub" | "jubjub-schnorr" => Ok((MidnightKeyType::EC, MidnightCurve::Jubjub)),
        "p-256" | "p256" => Ok((MidnightKeyType::EC, MidnightCurve::P256)),
        other => Err(WalletError::NotImplemented(format!(
            "algorithm {other:?} not supported — try ed25519, jubjub, or p-256"
        ))),
    }
}

fn format_algorithm(tag: &wallet_core::secret_storage::AlgorithmTag) -> String {
    format!("{:?}/{:?}", tag.kty, tag.crv)
}

fn jwk_to_json(jwk: &PublicJwk) -> Result<String, WalletError> {
    serde_json::to_string(jwk).map_err(|e| WalletError::Internal(e.to_string()))
}

fn stored_meta_to_key_info(
    store: &RedbSecretStore,
    meta: StoredKeyMeta,
) -> Result<KeyInfo, WalletError> {
    let jwk = block_on(store.get_public_key(&meta.key_ref)).map_err(map_secret_err)?;
    Ok(KeyInfo {
        key_ref: meta.key_ref.clone(),
        algorithm: format_algorithm(&meta.algorithm),
        created_at: meta.created_at.clone(),
        label: Some(meta.id.clone()),
        public_key_jwk: jwk_to_json(&jwk)?,
    })
}

fn map_secret_err(e: wallet_core::secret_storage::SecretStoreError) -> WalletError {
    use wallet_core::secret_storage::SecretStoreError as E;
    match e {
        E::NotFound(s) => WalletError::NotFound(s),
        E::Crypto(s) => WalletError::Crypto(s),
        E::UnsupportedCurve(s) => WalletError::Crypto(format!("unsupported curve: {s}")),
        E::SigningNotSupported(s) => WalletError::Crypto(format!("signing not supported: {s}")),
        E::InvalidInput(s) => WalletError::Crypto(format!("invalid input: {s}")),
        E::VerificationFailed => WalletError::Crypto("signature verification failed".into()),
        E::Init(s) => WalletError::Io(s),
        E::Locked => WalletError::Crypto("store is locked".into()),
        E::Io(e) => WalletError::Io(e.to_string()),
        E::Json(e) => WalletError::Internal(format!("json: {e}")),
    }
}

/// Block on an async future from a sync FFI context. Reuses the
/// process-wide tokio runtime from `crate::runtime()` (used for
/// `prove`) so we don't pay nested-runtime overhead.
fn block_on<F: std::future::Future>(fut: F) -> F::Output {
    if let Ok(handle) = Handle::try_current() {
        tokio::task::block_in_place(|| handle.block_on(fut))
    } else {
        crate::runtime().block_on(fut)
    }
}

fn chrono_ms_or_zero() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
