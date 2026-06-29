//! Headless wallet façade — the same port-typed use cases the
//! Dioxus shell drives, packaged as a session-scoped struct any
//! caller can use without a UI.
//!
//! ## Purpose
//!
//! The hex-architecture audit (§5.B + §9) identified the
//! orchestrator functions (`bootstrap_did_with_keys`,
//! `oid4vp_client::run_authentication`,
//! `oid4vci_client::run_issuance`, `self_verify_and_cache`) as
//! the application core's business logic. They take port-typed
//! arguments — no UI dependencies, no Dioxus runtime — so the
//! Dioxus shell isn't actually special; it's just one caller.
//!
//! `HeadlessWallet` is the *other* caller. It wires those same
//! orchestrators against real-deps adapters (`Wallet` over
//! `HttpIndexerClient + SubxtNodeClient + (HttpProver | LocalProver)`,
//! `ReqwestHttpClient`, `SystemClock`, `RedbVcStore`,
//! `InMemorySecretStore`) and exposes them as a session-scoped
//! façade — call `bootstrap`, then `login`, then
//! `request_credential`, then `verify`, all sharing the same
//! cryptographic state because the seed re-derives the same
//! keys.
//!
//! ## Use cases
//!
//! 1. **The `headless-wallet` CLI binary** — same flows the
//!    Dioxus app drives, scriptable from a shell.
//! 2. **Use-case integration tests** — bring up the standalone
//!    env, instantiate a `HeadlessWallet`, exercise each
//!    orchestrator end-to-end against the live chain + a live
//!    issuer-mock. See `wallet-core/tests/headless_*_e2e.rs`.
//! 3. **Onboarding** — a new contributor can `cargo run --bin
//!    headless-wallet -- bootstrap …` to confirm the toolchain
//!    works without touching the Dioxus build.
//!
//! ## Why the `Mutex` around `secret_store`
//!
//! `SecretStorage::generate_key` / `import_key` /
//! `derive_key_from_seed` all take `&mut self` (they may extend
//! an internal HashMap). Once bootstrap has populated the store,
//! signing uses `&self` only — but the `Arc<dyn DidSigner>` we
//! hand to OID4VP / OID4VCI needs the store to live somewhere
//! shareable. The simplest solution is a `tokio::sync::Mutex`:
//! `bootstrap` locks the mutex once and writes keys; subsequent
//! operations construct an `Arc<dyn DidSigner>` that locks
//! per-`sign` call. The locks are uncontended in single-session
//! use (the whole API is `&mut self` at the façade level), so
//! there's no perf cost in practice.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::clock::{Clock, SystemClock};
use crate::did::{bootstrap_did_with_keys, BootstrapError, BootstrappedDid};
use crate::http::{HttpClient, ReqwestHttpClient};
use crate::indexer::HttpIndexerClient;
use crate::node::SubxtNodeClient;
use crate::oid4vci_client::{
    run_issuance, CredentialCoordinator, IdTokenProofBuilder, IssuanceFlowError,
};
use crate::oid4vp_client::{
    run_authentication, AuthFlowError, AuthnKey, DidAuthnDiscovery, DidSigner,
    DiscoverError, IdTokenBuilder, LoginCoordinator, PostResponseResult, SignError,
};
use crate::secret_storage::{InMemorySecretStore, SecretStorage};
use crate::vc_self_verify::{self_verify_and_cache, SelfVerifyResult};
use crate::vc_store::{RedbVcStore, VcStoreError};
use crate::{
    chain::{HttpProver, LocalProver, Prover},
    DidId, IndexerClient, Network, NodeClient, VerificationMethodRef, Wallet,
};

/// Configuration for [`HeadlessWallet`]. Built up via the
/// `Builder` pattern so optional knobs (proof-server URL
/// override, custom HTTP client) don't blow up the call signature.
#[derive(Debug, Clone)]
pub struct HeadlessConfig {
    pub network: Network,
    /// 32-byte master seed. The DID + every key tied to it
    /// derive deterministically from this — re-deriving in a
    /// fresh session reproduces the same identity.
    pub seed: [u8; 32],
    /// Where to persist the VC store. Created on first use if
    /// missing. Use a `tempfile`-backed path for ephemeral tests.
    pub vc_store_path: PathBuf,
    /// Override the proof-server URL. `None` falls through to
    /// the network's configured default. `Some("")` opts into
    /// the in-process [`LocalProver`] (slow, no proof-server
    /// dependency).
    pub proof_server_url: Option<String>,
}

/// Errors the headless façade surfaces. Each variant wraps an
/// orchestrator's typed error — no `String`-payload widening.
#[derive(Debug, thiserror::Error)]
pub enum HeadlessError {
    #[error("build wallet deps: {0}")]
    Deps(String),
    #[error("open vc store: {0}")]
    OpenVcStore(#[from] VcStoreError),
    #[error("bootstrap: {0}")]
    Bootstrap(#[from] BootstrapError),
    #[error("login: {0}")]
    Login(#[from] AuthFlowError),
    #[error("issuance: {0}")]
    Issuance(#[from] IssuanceFlowError),
    #[error("verify: vc not found in store: {0}")]
    VcNotFound(String),
    /// Wraps the `String`-typed errors that `Wallet`'s vault methods
    /// surface — they bubble up `format!(...)` messages from the
    /// indexer / JS bridge / submission pipeline. Carry them verbatim
    /// so the dispatcher can echo them into the JSON `error.message`.
    #[error("vault: {0}")]
    Vault(String),
}

/// Result of [`HeadlessWallet::bootstrap`] — the freshly minted
/// DID + its 32-byte controller secret. The DID is what every
/// later call passes back; the controller secret is what's used
/// to drive write circuits (e.g. MaintenanceUpdate) against the
/// same DID — round-trip it back via [`HeadlessWallet::remember_controller_secret`].
#[derive(Debug)]
pub struct BootstrapOutcome {
    pub did: DidId,
    pub controller_sk: [u8; 32],
}

impl From<BootstrappedDid> for BootstrapOutcome {
    fn from(b: BootstrappedDid) -> Self {
        Self {
            did: b.did,
            controller_sk: b.controller_sk,
        }
    }
}

/// Session-scoped headless wallet. Owns its `Wallet`, secret
/// store, and VC store; the `Arc<dyn _>` ports are shared between
/// orchestrator calls so the per-DID resolve cache (when
/// callers wire a cached `DidAuthnDiscovery`) is reused.
pub struct HeadlessWallet {
    network: Network,
    /// Arc-wrapped so the per-flow `WalletDiscovery` adapters can
    /// share the same handle without consuming it. `Wallet` itself
    /// isn't `Clone` — the indexer/node/prover handles inside it
    /// don't all support cheap cloning — but `Arc<Wallet>` is
    /// trivially shareable and the trait calls go through `&self`.
    wallet: Arc<Wallet>,
    secret_store: Arc<Mutex<InMemorySecretStore>>,
    http: Arc<dyn HttpClient>,
    clock: Arc<dyn Clock>,
    vc_store: RedbVcStore,
}

impl HeadlessWallet {
    /// Connect to the chain endpoints + open / create the VC
    /// store, but **do not** bootstrap any DID. Call
    /// [`Self::bootstrap`] next, then `login` / `request_credential`
    /// / `verify` in any order.
    pub async fn connect(config: HeadlessConfig) -> Result<Self, HeadlessError> {
        let network = config.network;
        let indexer: Arc<dyn IndexerClient> = Arc::new(
            HttpIndexerClient::new(network)
                .map_err(|e| HeadlessError::Deps(format!("indexer: {e}")))?,
        );
        let node: Arc<dyn NodeClient> = Arc::new(
            SubxtNodeClient::connect(network)
                .await
                .map_err(|e| HeadlessError::Deps(format!("node: {e}")))?,
        );
        let prover = build_prover(network, config.proof_server_url.as_deref());
        let wallet = Arc::new(Wallet::with_deps(config.seed, network, indexer, node, prover));

        let http: Arc<dyn HttpClient> = Arc::new(ReqwestHttpClient::default());
        let clock: Arc<dyn Clock> = Arc::new(SystemClock);
        let vc_store = RedbVcStore::open(&config.vc_store_path)?;
        let secret_store = Arc::new(Mutex::new(InMemorySecretStore::default()));

        Ok(Self {
            network,
            wallet,
            secret_store,
            http,
            clock,
            vc_store,
        })
    }

    /// Bootstrap a fresh DID — runs `bootstrap_did_with_keys`
    /// (HKDF → import keys → on-chain `create_did` +
    /// `addVerificationMethod` ×2) against the wallet's
    /// configured network. Idempotent only across runs with the
    /// same `seed`; calling twice in one session creates a
    /// second DID.
    pub async fn bootstrap(&self, seed: [u8; 32]) -> Result<BootstrapOutcome, HeadlessError> {
        let mut guard = self.secret_store.lock().await;
        let out = bootstrap_did_with_keys(&*self.wallet, &mut *guard, &seed).await?;
        Ok(out.into())
    }

    /// Drive an OID4VP / SIOPv2 login. Resolves the holder DID,
    /// signs an id_token with the `key-auth` VM, POSTs it back
    /// to the RP. Returns the issuer's `session_id` + `status`.
    pub async fn login(
        &self,
        holder: DidId,
        qr_url: &str,
    ) -> Result<PostResponseResult, HeadlessError> {
        let discovery: Arc<dyn DidAuthnDiscovery> =
            Arc::new(WalletDiscovery::new(Arc::clone(&self.wallet)));
        let signer: Arc<dyn DidSigner> =
            Arc::new(MutexBackedSigner::new(self.secret_store.clone()));
        let coordinator = LoginCoordinator::mode_a(IdTokenBuilder::new(
            discovery,
            signer,
            self.clock.clone(),
            holder,
        ));
        Ok(run_authentication(&*self.http, &coordinator, qr_url).await?)
    }

    /// Drive an OID4VCI Pre-Authorized Code Flow. Acquires an
    /// access token + c_nonce, mints a c_nonce-bound proof JWS,
    /// POSTs `/credential`, lands the resulting VC + openings
    /// into the wallet's `vc_store`. Returns the freshly-issued
    /// `vc_uri`.
    pub async fn request_credential(
        &self,
        holder: DidId,
        qr_url: &str,
    ) -> Result<String, HeadlessError> {
        let discovery: Arc<dyn DidAuthnDiscovery> =
            Arc::new(WalletDiscovery::new(Arc::clone(&self.wallet)));
        let signer: Arc<dyn DidSigner> =
            Arc::new(MutexBackedSigner::new(self.secret_store.clone()));
        let coordinator = CredentialCoordinator::jwt(IdTokenProofBuilder::new(
            discovery,
            signer,
            self.clock.clone(),
            holder.clone(),
        ));
        // The b753e399 merge added 4 more positional parameters to
        // `run_issuance` so the inner `credential::digital_passport`
        // dispatch arm can pull holder material directly. Headless
        // doesn't drive the digital-passport flow today (no JS
        // bridge in the headless harness), so we pass `None` for
        // `js_bridge` — the birth-credential dispatch arm doesn't
        // consult it and the digital-passport arm would surface
        // `JsBridgeUnavailable` (which is the right behaviour for
        // a JS-bridge-less harness anyway).
        let guard = self.secret_store.lock().await;
        let secret_store: &dyn SecretStorage = &*guard;
        Ok(run_issuance(
            &*self.http,
            &*self.clock,
            None,
            qr_url,
            &coordinator,
            &*self.wallet,
            secret_store,
            &holder,
            &self.vc_store,
        )
        .await?)
    }

    /// Verify a previously-issued VC against its issuer DID
    /// (resolved on-chain). The VC must already live in the
    /// session's `vc_store` — typically because `request_credential`
    /// landed it there earlier in the same session.
    pub async fn verify(&self, vc_uri: &str) -> Result<SelfVerifyResult, HeadlessError> {
        let stored = self
            .vc_store
            .get_vc(vc_uri)
            .map_err(HeadlessError::OpenVcStore)?
            .ok_or_else(|| HeadlessError::VcNotFound(vc_uri.to_string()))?;
        let guard = self.secret_store.lock().await;
        Ok(self_verify_and_cache(
            &stored,
            &*self.wallet,
            &*guard,
            &self.vc_store,
            &*self.clock,
        )
        .await)
    }

    /// Borrow the network identifier for logging / display.
    pub fn network(&self) -> Network {
        self.network
    }

    // ─── Vault verbs ──────────────────────────────────────────────
    //
    // Thin delegators to [`Wallet`]'s vault methods. The Rust path
    // signs the funding spend itself, sidestepping the JS SDK's
    // `1010 InputsSignaturesLengthMismatch` (per `wallet.rs:2014`).
    // The verifier (dApp / CLI) pins which vault to act on by
    // passing `contract_address_hex` to every verb; there's no
    // wallet-side default.

    /// Read the vault's currently-locked NIGHT total (base units).
    /// Read-only: no seed, dust, proving, or submission involved.
    pub async fn vault_total_locked(
        &self,
        contract_address_hex: String,
    ) -> Result<u128, HeadlessError> {
        self.wallet
            .vault_total_locked(contract_address_hex)
            .await
            .map_err(HeadlessError::Vault)
    }

    /// Enumerate the vault's locks (id, policy, per-lock pool) plus
    /// the global `lockCount`. Returns the raw `readVaultLocks` JSON.
    pub async fn vault_list_locks(
        &self,
        contract_address_hex: String,
    ) -> Result<serde_json::Value, HeadlessError> {
        self.wallet
            .list_locks(contract_address_hex)
            .await
            .map_err(HeadlessError::Vault)
    }

    /// Enumerate this wallet's stored digital-passport credentials.
    /// Reads from the session's `vc_store` — no vault contract is
    /// involved (the parameter list is therefore empty).
    pub fn vault_list_credentials(&self) -> Result<Vec<StoredVcSummary>, HeadlessError> {
        let vcs = self
            .vc_store
            .list_ordered()
            .map_err(HeadlessError::OpenVcStore)?;
        Ok(vcs
            .into_iter()
            .map(|stored| StoredVcSummary {
                vc_uri: stored.vc_uri,
                issuer_did: stored.issuer_did,
                holder_did: stored.holder_did,
                format: stored.format,
                issued_at_ms: stored.issued_at_ms,
            })
            .collect())
    }

    /// Create a new lock with `policy` and an optional initial
    /// deposit of `initial_amount` base units. Returns the submitted
    /// tx hash plus the assigned lock id (pre-increment lockCount).
    pub async fn vault_create_lock(
        &self,
        contract_address_hex: String,
        policy: crate::VaultLockPolicy,
        initial_amount: u128,
    ) -> Result<crate::wallet::VaultCreateLockOutcome, HeadlessError> {
        self.wallet
            .create_lock(contract_address_hex, policy, initial_amount)
            .await
            .map_err(HeadlessError::Vault)
    }

    /// Top up an existing lock's pool with `amount_base_units` of
    /// native UNSHIELDED NIGHT. Returns the submitted tx hash. Only
    /// the lock's creator may deposit (enforced on-chain).
    pub async fn vault_deposit(
        &self,
        contract_address_hex: String,
        lock_id: u64,
        amount_base_units: u128,
    ) -> Result<String, HeadlessError> {
        self.wallet
            .deposit_to_lock(contract_address_hex, lock_id, amount_base_units)
            .await
            .map_err(HeadlessError::Vault)
    }

    /// Claim `amount_base_units` from `lock_id` against a stored
    /// credential's `bundle`. Returns the submitted tx hash.
    pub async fn vault_claim(
        &self,
        contract_address_hex: String,
        lock_id: u64,
        amount_base_units: u128,
        bundle: serde_json::Value,
        current_day: Option<u64>,
    ) -> Result<String, HeadlessError> {
        self.wallet
            .claim_from_lock(
                contract_address_hex,
                lock_id,
                amount_base_units,
                bundle,
                current_day,
            )
            .await
            .map_err(HeadlessError::Vault)
    }
}

/// Display-only summary of a credential held in the session's
/// `vc_store`. Mirrors the dApp connector's `VaultCredential` shape
/// — enough to populate the credential picker, no PII or
/// signature material.
#[derive(Debug, Clone, serde::Serialize)]
pub struct StoredVcSummary {
    pub vc_uri: String,
    pub issuer_did: String,
    pub holder_did: String,
    pub format: String,
    pub issued_at_ms: u64,
}

// ─── Internal adapters ─────────────────────────────────────────

/// Wallet-backed [`DidAuthnDiscovery`] — the same pattern
/// `test_support::stub_authn_discovery` uses, kept here because
/// the headless wallet is real-deps (not test-cfg) and the
/// `test_support` module gates on
/// `#[cfg(any(test, feature = "test-support"))]`.
struct WalletDiscovery {
    wallet: Arc<Wallet>,
}

impl WalletDiscovery {
    fn new(wallet: Arc<Wallet>) -> Self {
        Self { wallet }
    }
}

#[async_trait]
impl DidAuthnDiscovery for WalletDiscovery {
    async fn authn_key(&self, did: &DidId) -> Result<AuthnKey, DiscoverError> {
        let doc = self
            .wallet
            .resolve_did(&did.to_did_string())
            .await
            .map_err(|e| DiscoverError::Resolve(e.to_string()))?;
        let (kid, public_jwk) = match doc
            .authentication
            .first()
            .ok_or_else(|| DiscoverError::NoAuthnKey(did.to_did_string()))?
        {
            VerificationMethodRef::Inline(vm) => (vm.id.clone(), vm.public_key_jwk.clone()),
            VerificationMethodRef::Id(id) => {
                let vm = doc
                    .verification_method
                    .iter()
                    .find(|v| v.id == *id)
                    .ok_or_else(|| {
                        DiscoverError::Resolve(format!(
                            "authentication kid {id} not in verificationMethod[]"
                        ))
                    })?;
                (vm.id.clone(), vm.public_key_jwk.clone())
            }
        };
        Ok(AuthnKey { kid, public_jwk })
    }
}

/// [`DidSigner`] that holds an `Arc<Mutex<InMemorySecretStore>>`
/// and locks per-sign call. The mutex is uncontended in
/// single-session use (the façade API is `&self`-style but the
/// outermost mutation happens once via `bootstrap`).
struct MutexBackedSigner {
    store: Arc<Mutex<InMemorySecretStore>>,
}

impl MutexBackedSigner {
    fn new(store: Arc<Mutex<InMemorySecretStore>>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl DidSigner for MutexBackedSigner {
    async fn sign(&self, kid: &str, payload: &[u8]) -> Result<Vec<u8>, SignError> {
        let guard = self.store.lock().await;
        let key_ref = guard
            .find_by_kid(kid)
            .await
            .ok_or_else(|| SignError::NoLocalSecret(kid.to_string()))?;
        let out = guard
            .sign(key_ref.uuid(), payload)
            .await
            .map_err(|e| SignError::Sign(e.to_string()))?;
        Ok(out.signature)
    }
}

fn build_prover(network: Network, override_url: Option<&str>) -> Arc<dyn Prover> {
    let url: Option<String> = match override_url {
        // Empty string means: explicitly fall back to the
        // in-process LocalProver.
        Some("") => None,
        Some(u) => Some(u.to_owned()),
        None => Some(network.config().proving_server_url.to_owned()),
    };
    match url {
        Some(u) => Arc::new(HttpProver::new(u)),
        None => Arc::new(LocalProver),
    }
}

