//! Test-support scaffolding for Identity Centre Task 2 and beyond.
//!
//! The `stub_wallet` factory produces a [`crate::Wallet`] that
//! short-circuits the four DID write/read methods Task 2's
//! `bootstrap_did_with_keys` orchestration drives:
//!
//! - [`crate::Wallet::create_did_awaitable`]
//! - [`crate::Wallet::add_verification_method`]
//! - [`crate::Wallet::add_verification_method_relation`]
//! - [`crate::Wallet::resolve_did`]
//!
//! The stub keeps an in-process `HashMap<DidId, DidDocument>` shared
//! between the wallet and its injected [`StubIndexerClient`]. Each
//! awaitable method mutates the map directly; `resolve_did` reads
//! from it. The full chain-op pipeline (JS bridge ->
//! prepareUnprovenCallTx -> balance -> prove -> submit) is NOT
//! exercised — that pipeline requires a JS Compact runtime, a real
//! dust state, live `LedgerParameters`, and halo2 proving, none of
//! which fits the <300-LOC budget for Task 1.5.D.
//!
//! Pipeline-level fidelity (Path 1 in the Task 1.5.D brief) is a
//! Phase 2 follow-up; for Task 2's "does the orchestration body
//! call the four wallet methods in the right order with the right
//! args?" question, the wallet-level bypass (Path 2) is sufficient.
//!
//! Gated behind `#[cfg(any(test, feature = "test-support"))]` so
//! none of this lands in release builds unless a downstream crate
//! opts in via the `test-support` feature.

#![cfg(any(test, feature = "test-support"))]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use crate::chain::{IndexerClient, NodeClient, Prover};
use crate::indexer::{ChainTipInfo, ContractStateInfo, IndexerError};
use crate::node::{NodeError, SubmitResult};
use crate::secret_storage::InMemorySecretStore;
use crate::tx::TxError;
use crate::tx::build::UnprovenTx;
use crate::tx::prove::ProvenTx;
use crate::{DidDocument, DidId, MidnightSigner, Network, Wallet};

/// Shared in-memory DID-document map. The stub wallet and its
/// injected [`StubIndexerClient`] hold the same `Arc<Mutex<_>>` so
/// mutations made via `Wallet::add_verification_method` /
/// `Wallet::add_verification_method_relation` (which go through the
/// stub-mode bypass on `Wallet`) are immediately visible to
/// `resolve_did` and to direct `IndexerClient::contract_state`
/// queries.
pub(crate) type StubDidMap = Arc<Mutex<HashMap<DidId, DidDocument>>>;

/// In-memory indexer stub.
///
/// `chain_tip` returns `None` (no chain — the stub doesn't simulate
/// blocks); `contract_state` returns `None` for every address (DID
/// state is served via the wallet-level bypass and `resolve_did`,
/// not through the indexer trait, because `ContractStateInfo`
/// requires a SCALE-encoded `state_hex` we don't synthesise).
#[derive(Clone, Debug)]
pub struct StubIndexerClient {
    state: StubDidMap,
}

impl StubIndexerClient {
    /// Build a stub indexer sharing `state` with the parent wallet.
    pub fn new(state: StubDidMap) -> Self {
        Self { state }
    }

    /// Direct accessor for the shared map. Tests use this to
    /// pre-seed a DID document or to assert on the final state
    /// without round-tripping through `Wallet::resolve_did`.
    pub fn state(&self) -> StubDidMap {
        Arc::clone(&self.state)
    }
}

#[async_trait]
impl IndexerClient for StubIndexerClient {
    async fn chain_tip(&self) -> Result<Option<ChainTipInfo>, IndexerError> {
        Ok(None)
    }

    async fn contract_state(
        &self,
        _address_hex: &str,
    ) -> Result<Option<ContractStateInfo>, IndexerError> {
        // The wallet-level stub bypass serves DID state directly
        // (see Wallet::resolve_did's stub-mode branch); the trait
        // method returns None so that any code that reaches here
        // bails cleanly rather than fabricating bogus state_hex.
        Ok(None)
    }
}

/// In-memory node stub. `submit_deploy` records each call's byte
/// length in `submitted_lens` and returns a canned `SubmitResult`
/// with zero hashes. The wallet-level bypass means this is never
/// reached during Task 2's unit-test flow, but the impl exists so
/// `Wallet::with_deps` accepts a valid trait object.
#[derive(Clone, Debug, Default)]
pub struct StubNodeClient {
    submitted_lens: Arc<Mutex<Vec<usize>>>,
}

impl StubNodeClient {
    pub fn new() -> Self {
        Self::default()
    }

    /// Byte lengths of every `submit_deploy` call. Test-only
    /// observability hook.
    pub fn submitted_lens(&self) -> Vec<usize> {
        self.submitted_lens.lock().expect("poisoned").clone()
    }
}

#[async_trait]
impl NodeClient for StubNodeClient {
    async fn submit_deploy(
        &self,
        bytes: Vec<u8>,
        _signer: &MidnightSigner,
    ) -> Result<SubmitResult, NodeError> {
        self.submitted_lens
            .lock()
            .expect("poisoned")
            .push(bytes.len());
        Ok(SubmitResult { tx_hash: [0u8; 32], block_hash: [0u8; 32] })
    }
}

/// Pass-through prover stub. Returns `TxError::Prove` because we
/// can't synthesise a `ProvenTx` out of thin air — every halo2
/// proof is curve-specific data — and the wallet-level bypass means
/// no test reaches the prove stage anyway. Exists so
/// `Wallet::with_deps` accepts a valid trait object.
#[derive(Clone, Debug, Default)]
pub struct StubProver;

#[async_trait]
impl Prover for StubProver {
    async fn prove(&self, _tx: UnprovenTx) -> Result<ProvenTx, TxError> {
        Err(TxError::Prove(
            "stub prover; Task 2 unit tests use the wallet-level bypass and \
             do not reach the prove stage".into(),
        ))
    }
}

/// Default test secret store — re-exports
/// [`InMemorySecretStore::default`] under the name the brief and
/// Task 2 reference. Trivial wrapper kept so the factory namespace
/// is self-contained.
pub fn stub_secret_store() -> InMemorySecretStore {
    InMemorySecretStore::default()
}

/// Deterministic 32-byte seed used by [`stub_wallet`]. The exact
/// value doesn't matter; we pick a non-zero pattern so any
/// downstream code that checks "did the test forget to set a seed?"
/// can spot it.
const STUB_SEED: [u8; 32] = [0x5Au8; 32];

/// Factory: a [`Wallet`] wired with in-memory indexer + node +
/// prover stubs **and** stub-mode flag enabled. The four DID
/// write/read methods short-circuit to the shared
/// `Arc<Mutex<HashMap<DidId, DidDocument>>>` rather than driving
/// the live chain-op pipeline.
///
/// Used by Task 2's `bootstrap_did_with_keys` unit tests; can also
/// be used by any future test that needs a "wallet I can call the
/// four awaitable DID methods on, deterministically, without a
/// live indexer / node / proof-server".
pub fn stub_wallet() -> Wallet {
    let state: StubDidMap = Arc::new(Mutex::new(HashMap::new()));
    let indexer = Arc::new(StubIndexerClient::new(Arc::clone(&state)));
    let node = Arc::new(StubNodeClient::new());
    let prover = Arc::new(StubProver);
    Wallet::with_deps(STUB_SEED, Network::Undeployed, indexer, node, prover)
        .with_stub_did_state(state)
}

/// Test helper: stub wallet + bootstrapped DID with both authn
/// + assertion VMs attached. Used by `did_auth` tests.
///
/// The wallet runs in stub mode so `bootstrap_did_with_keys`
/// short-circuits through the in-memory DID-document map. The
/// secret store is created fresh inside this call; pair with
/// [`stub_secret_store_with_bootstrapped_did`] using the SAME
/// seed if you need the matching store handed back independently.
pub async fn stub_wallet_with_bootstrapped_did(seed: [u8; 32]) -> (Wallet, crate::DidId) {
    let wallet = stub_wallet();
    let mut store = InMemorySecretStore::default();
    let out = crate::bootstrap_did_with_keys(&wallet, &mut store, &seed)
        .await
        .expect("stub bootstrap should not fail");
    (wallet, out.did)
}

/// Test helper: SecretStorage seeded by a fresh
/// `bootstrap_did_with_keys` run, deterministic per `seed`. Pair
/// with [`stub_wallet_with_bootstrapped_did`] using the SAME seed
/// when tests need both halves independently.
///
/// Both helpers re-run `bootstrap_did_with_keys` against a fresh
/// stub wallet so the resulting `SecretStorage` carries the same
/// (kid, key) pairs the paired wallet's DID document references.
pub async fn stub_secret_store_with_bootstrapped_did(
    seed: [u8; 32],
) -> InMemorySecretStore {
    let wallet = stub_wallet();
    let mut store = InMemorySecretStore::default();
    let _ = crate::bootstrap_did_with_keys(&wallet, &mut store, &seed)
        .await
        .expect("stub bootstrap should not fail");
    store
}

/// Test helper: build a wallet-backed [`DidAuthnDiscovery`] for
/// tests. Mirrors what `dioxus-wallet`'s
/// `CachedWalletAuthnDiscovery` does in production, minus the
/// cache (tests want every call observable).
///
/// Both OID4VP `run_authentication` tests and OID4VCI
/// `request_credential` tests use this — keeps the adapter
/// pattern in one place instead of duplicated across two test
/// modules.
pub fn stub_authn_discovery(
    wallet: Wallet,
) -> Arc<dyn crate::oid4vp_client::DidAuthnDiscovery> {
    Arc::new(WalletDiscovery { wallet })
}

/// Test helper: wrap [`InMemorySecretStore`] in a [`DidSigner`].
/// Mirrors `dioxus-wallet`'s `RedbDidSigner`.
pub fn stub_did_signer(
    store: InMemorySecretStore,
) -> Arc<dyn crate::oid4vp_client::DidSigner> {
    Arc::new(InMemorySigner { store })
}

struct WalletDiscovery {
    wallet: Wallet,
}
#[async_trait]
impl crate::oid4vp_client::DidAuthnDiscovery for WalletDiscovery {
    async fn authn_key(
        &self,
        did: &crate::DidId,
    ) -> Result<
        crate::oid4vp_client::AuthnKey,
        crate::oid4vp_client::DiscoverError,
    > {
        use crate::oid4vp_client::{AuthnKey, DiscoverError};
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
            crate::VerificationMethodRef::Inline(vm) => {
                (vm.id.clone(), vm.public_key_jwk.clone())
            }
            crate::VerificationMethodRef::Id(id) => {
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

struct InMemorySigner {
    store: InMemorySecretStore,
}
#[async_trait]
impl crate::oid4vp_client::DidSigner for InMemorySigner {
    async fn sign(
        &self,
        kid: &str,
        payload: &[u8],
    ) -> Result<Vec<u8>, crate::oid4vp_client::SignError> {
        use crate::oid4vp_client::SignError;
        use crate::secret_storage::SecretStorage;
        let key_ref = self
            .store
            .find_by_kid(kid)
            .await
            .ok_or_else(|| SignError::NoLocalSecret(kid.to_string()))?;
        let out = self
            .store
            .sign(key_ref.uuid(), payload)
            .await
            .map_err(|e| SignError::Sign(e.to_string()))?;
        Ok(out.signature)
    }
}

/// Test helper: stub wallet + a freshly-created DID with no
/// verification methods attached. For testing the "no authn key"
/// error path in `did_auth`.
pub async fn stub_wallet_with_empty_did() -> (Wallet, crate::DidId) {
    let wallet = stub_wallet();
    let (did, _controller_sk) = wallet
        .create_did_awaitable_with_controller()
        .await
        .expect("stub create_did should not fail");
    (wallet, did)
}

/// Test helper: build a Phase-1 placeholder VC body (CBOR map) signed by
/// the issuer's `assertionMethod`-relation key.
///
/// Shape:
/// ```cbor
/// {
///   "credentialSubject": <payload bytes>,
///   "proof": {
///     "verificationMethod": "<did>#key-assert",
///     "signature": "<base64-std signature bytes>"
///   }
/// }
/// ```
///
/// The canonical (proof-stripped) bytes are what gets signed.
/// `vc_self_verify::self_verify` re-derives the same canonical
/// bytes by removing the `proof` entry before calling
/// `SecretStorage::verify` with the issuer's public JWK.
///
/// To keep the canonical bytes stable across sign + verify, we
/// stage the map as a `BTreeMap<&str, ...>` first (BTreeMap iter
/// is ordered by key); serde_cbor then preserves that order when
/// serializing the Value::Map.
pub async fn stub_sign_birth_vc(
    wallet: &Wallet,
    secret_store: &dyn crate::secret_storage::SecretStorage,
    issuer_did: &crate::DidId,
    payload: &[u8],
) -> Vec<u8> {
    use base64::{engine::general_purpose::STANDARD as B64, Engine};
    use std::collections::BTreeMap;

    // Re-resolve the issuer DID to find its assertionMethod VM kid.
    let doc = wallet
        .resolve_did(&issuer_did.to_did_string())
        .await
        .expect("resolve issuer");
    let assertion_ref = doc
        .assertion_method
        .first()
        .expect("issuer must have an assertionMethod VM");
    let kid = match assertion_ref {
        crate::VerificationMethodRef::Id(s) => s.clone(),
        crate::VerificationMethodRef::Inline(vm) => vm.id.clone(),
    };

    // Find the secret matching that kid. Seeded by
    // stub_secret_store_with_bootstrapped_did.
    let key_ref = secret_store
        .find_by_kid(&kid)
        .await
        .expect("issuer's assertionMethod secret in local store");

    // The stub `add_verification_method` path falls back to an
    // Ed25519 placeholder JWK whenever the upstream
    // `VerificationMethod` JSON fails to parse (notably: when the
    // `type` field is something like `"JubjubVerificationKey2026"`
    // — anything other than the lone `JsonWebKey` enum variant
    // serde knows about). For self-verify tests we need the
    // resolved doc to carry the real Jubjub JWK so the
    // `SecretStorage::verify` path can run. Patch the stub-mode
    // DID-doc map in place: look up the assertion VM whose `id`
    // matches `kid`, then overwrite its `public_key_jwk` with the
    // public-key half of the secret-store entry we just found.
    if let Some(state) = wallet.stub_did_state() {
        let real_jwk = secret_store
            .get_public_key(key_ref.uuid())
            .await
            .expect("issuer assertion pubkey readable");
        let did_jwk = crate::PublicKeyJwk {
            kty: match real_jwk.kty {
                crate::secret_storage::MidnightKeyType::OKP => crate::KeyType::OKP,
                crate::secret_storage::MidnightKeyType::EC => crate::KeyType::EC,
            },
            crv: match real_jwk.crv {
                crate::secret_storage::MidnightCurve::Ed25519 => crate::CurveType::Ed25519,
                crate::secret_storage::MidnightCurve::Jubjub => crate::CurveType::Jubjub,
                crate::secret_storage::MidnightCurve::P256 => crate::CurveType::P256,
            },
            x: real_jwk.x.clone(),
            y: real_jwk.y.clone(),
        };
        let mut guard = state.lock().expect("stub did state poisoned");
        if let Some(doc) = guard.get_mut(issuer_did) {
            for vm in doc.verification_method.iter_mut() {
                if vm.id == kid {
                    vm.public_key_jwk = did_jwk.clone();
                }
            }
        }
    }

    // Stage the (canonical, proof-stripped) map in a BTreeMap so
    // entry order is deterministic by key. serde_cbor::Value::Map
    // is a Vec of pairs; we collect from the BTreeMap to preserve
    // sorted-by-key order.
    let mut canonical_btree: BTreeMap<String, serde_cbor::Value> = BTreeMap::new();
    canonical_btree.insert(
        "credentialSubject".to_string(),
        serde_cbor::Value::Bytes(payload.to_vec()),
    );
    let canonical_value = serde_cbor::Value::Map(
        canonical_btree
            .clone()
            .into_iter()
            .map(|(k, v)| (serde_cbor::Value::Text(k), v))
            .collect(),
    );
    let canonical_bytes =
        serde_cbor::to_vec(&canonical_value).expect("encode canonical cbor");

    // Sign the canonical bytes with the assertion key.
    let sig = secret_store
        .sign(key_ref.uuid(), &canonical_bytes)
        .await
        .expect("sign assertion key");

    // Re-emit the full body with `proof` appended. BTreeMap key
    // ordering puts "credentialSubject" before "proof" (alphabetic),
    // which is the same order the verifier produces when it strips
    // "proof" — guaranteeing canonical bytes match on both sides.
    let mut proof_btree: BTreeMap<String, serde_cbor::Value> = BTreeMap::new();
    proof_btree.insert(
        "signature".to_string(),
        serde_cbor::Value::Text(B64.encode(&sig.signature)),
    );
    proof_btree.insert(
        "verificationMethod".to_string(),
        serde_cbor::Value::Text(kid),
    );
    let proof_value = serde_cbor::Value::Map(
        proof_btree
            .into_iter()
            .map(|(k, v)| (serde_cbor::Value::Text(k), v))
            .collect(),
    );

    let mut full_btree = canonical_btree;
    full_btree.insert("proof".to_string(), proof_value);
    let full_value = serde_cbor::Value::Map(
        full_btree
            .into_iter()
            .map(|(k, v)| (serde_cbor::Value::Text(k), v))
            .collect(),
    );
    serde_cbor::to_vec(&full_value).expect("encode full cbor body")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secret_storage::SecretKeyRef;
    use crate::VerificationMethodRelation;

    #[tokio::test]
    async fn stub_wallet_create_did_awaitable_returns_did() {
        let wallet = stub_wallet();
        let did = wallet.create_did_awaitable().await.expect("ok");
        assert!(did.to_did_string().starts_with("did:midnight:"));
    }

    #[tokio::test]
    async fn stub_wallet_add_vm_then_resolve_shows_it() {
        let wallet = stub_wallet();
        let did = wallet.create_did_awaitable().await.expect("did");

        let jwk_json = serde_json::json!({
            "id": format!("{}#key-auth", did.to_did_string()),
            "type": "Ed25519VerificationKey2020",
            "controller": did.to_did_string(),
            "publicKeyJwk": { "kty": "OKP", "crv": "Ed25519", "x": "ABCD" }
        });
        let key_ref = SecretKeyRef::new("stub-uuid", "ed25519/authentication");
        wallet
            .add_verification_method(&did, &key_ref, jwk_json, [0u8; 32])
            .await
            .expect("ok");
        wallet
            .add_verification_method_relation(
                &did,
                "key-auth",
                VerificationMethodRelation::Authentication,
                [0u8; 32],
            )
            .await
            .expect("ok");

        let doc = wallet
            .resolve_did(&did.to_did_string())
            .await
            .expect("resolve");
        assert!(!doc.authentication.is_empty());
        assert!(!doc.verification_method.is_empty());
    }
}
