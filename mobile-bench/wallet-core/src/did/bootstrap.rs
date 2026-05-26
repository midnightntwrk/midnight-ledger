//! `bootstrap_did_with_keys` — atomically create a Midnight DID and
//! attach the two verification methods Phase 1 of the Identity
//! Centre relies on: Ed25519 in `authentication` (for SIOPv2
//! id-token signing) and Jubjub in `assertionMethod` (for VC/VP
//! signing).
//!
//! Deterministic from a 32-byte seed via HKDF-SHA256 with distinct
//! info strings so the same seed always derives the same DID on a
//! fresh standalone env. Matches the seed convention used by the
//! `midnight-did` integration tests so wallet and issuer DIDs are
//! reproducible across runs.

use hkdf::Hkdf;
use sha2::Sha256;

use crate::secret_storage::{PublicJwk, SecretKeyRef, SecretStorage};
use crate::wallet::Wallet;
use crate::DidId;

/// Result of a successful bootstrap.
#[derive(Debug, Clone)]
pub struct BootstrappedDid {
    pub did: DidId,
    pub ed25519_ref: SecretKeyRef,
    pub jubjub_ref: SecretKeyRef,
}

/// Errors callers may have to recover from.
#[derive(Debug, thiserror::Error)]
pub enum BootstrapError {
    #[error("create_did failed: {0}")]
    CreateDid(String),
    #[error("attach Ed25519 authn key failed: {0}")]
    AttachAuthn(String),
    #[error("attach Jubjub assertion key failed: {0}")]
    AttachAssertion(String),
    #[error("post-bootstrap resolution failed: {0}")]
    Resolve(String),
    #[error("post-bootstrap doc missing relation: {0}")]
    MissingRelation(&'static str),
}

/// Public so the `did-bootstrap` CLI can re-derive the secrets for the output keystore without widening the `SecretStorage` trait with an `export_secret` method.
pub fn derive_keys(seed: &[u8; 32]) -> ([u8; 32], [u8; 32]) {
    let h = Hkdf::<Sha256>::new(Some(b"midnight-identity-centre-v1"), seed);
    let mut ed = [0u8; 32];
    let mut jb = [0u8; 32];
    h.expand(b"ed25519/authentication", &mut ed)
        .expect("HKDF expand for ed25519");
    h.expand(b"jubjub/assertionMethod", &mut jb)
        .expect("HKDF expand for jubjub");
    (ed, jb)
}

/// Compose the JSON arg the `addVerificationMethod` Compact circuit
/// expects. Mirrors the shape `Wallet::add_verification_method`
/// forwards via `prepareUnprovenCallTx`:
///
/// ```json
/// {
///   "id": "<did>#<fragment>",
///   "type": "Ed25519VerificationKey2020" | "JubjubVerificationKey2026",
///   "controller": "<did>",
///   "publicKeyJwk": { "kty": ..., "crv": ..., "x": "...", "y": "..." }
/// }
/// ```
///
/// The JWK fields come straight from
/// [`SecretStorage::get_public_key`]: Ed25519 stores `x` as
/// base64url(pub_bytes); Jubjub stores the coordinates as decimal
/// bigints (`x`/`y`) per the upstream `secret-storage` contract.
///
/// **TODO (Phase 1, Tasks 5+):** the `type` strings here are
/// placeholders chosen to disambiguate at the stub layer. The real
/// `addVerificationMethod` circuit consumes the `VerificationMethod`
/// type tag through a different path (the JS harness re-derives it
/// from `(kty, crv)`); when self-verify lands we'll align both ends
/// on the canonical Midnight verification-method type names.
fn build_verification_method_json(
    did: &DidId,
    fragment: &str,
    jwk: &PublicJwk,
) -> serde_json::Value {
    use crate::secret_storage::MidnightCurve;
    let typ = match jwk.crv {
        MidnightCurve::Ed25519 => "Ed25519VerificationKey2020",
        MidnightCurve::Jubjub => "JubjubVerificationKey2026",
        MidnightCurve::P256 => "JsonWebKey2020",
    };
    let mut public_key_jwk = serde_json::json!({
        "kty": jwk.kty,
        "crv": jwk.crv,
        "x": jwk.x,
    });
    if let Some(y) = &jwk.y {
        public_key_jwk["y"] = serde_json::Value::String(y.clone());
    }
    serde_json::json!({
        "id": format!("{}#{}", did.to_did_string(), fragment),
        "type": typ,
        "controller": did.to_did_string(),
        "publicKeyJwk": public_key_jwk,
    })
}

/// Atomically create a DID and attach the two Phase 1 verification
/// methods: Ed25519 in `authentication`, Jubjub in `assertionMethod`.
///
/// Six-step flow:
/// 1. Import both HKDF-derived secrets into the secret store
///    (keys-first: surviving a later crash leaves keys recoverable
///    from disk/redb).
/// 2. Deploy the DID contract, capturing the freshly-minted
///    `controller_sk` the wallet committed at deploy time.
/// 3. Fetch the public-key JWK for each key.
/// 4. Attach Ed25519 → register VM + push relation `authentication`.
/// 5. Attach Jubjub  → register VM + push relation `assertionMethod`.
/// 6. Resolve the DID and assert both relation arrays are populated.
///
/// On any step's failure the wallet's secret store retains both
/// imported keys (step 1 is idempotent against the seed); the
/// on-chain DID, if step 2 succeeded, is left in whatever partial
/// state the failing step reached.
pub async fn bootstrap_did_with_keys(
    wallet: &Wallet,
    secret_store: &mut dyn SecretStorage,
    seed: &[u8; 32],
) -> Result<BootstrappedDid, BootstrapError> {
    let (ed_bytes, jb_bytes) = derive_keys(seed);

    // 1. Import keys into the secret store first.
    let ed25519_ref = secret_store
        .import_ed25519(&ed_bytes, "ed25519/authentication")
        .await
        .map_err(|e| BootstrapError::AttachAuthn(format!("import ed25519: {e}")))?;
    let jubjub_ref = secret_store
        .import_jubjub(&jb_bytes, "jubjub/assertionMethod")
        .await
        .map_err(|e| BootstrapError::AttachAssertion(format!("import jubjub: {e}")))?;

    // 2. Create the DID on chain.
    let (did, controller_sk) = wallet
        .create_did_awaitable_with_controller()
        .await
        .map_err(|e| BootstrapError::CreateDid(e.to_string()))?;

    // 3. Fetch the JWKs the secret store assembled from the imported
    //    private bytes. Encoding (base64url vs decimal bigint) is
    //    curve-specific and already baked into the JWK by
    //    `curve_support::from_private_bytes`.
    let ed_jwk = secret_store
        .get_public_key(ed25519_ref.uuid())
        .await
        .map_err(|e| BootstrapError::AttachAuthn(format!("get_public_key ed25519: {e}")))?;
    let jb_jwk = secret_store
        .get_public_key(jubjub_ref.uuid())
        .await
        .map_err(|e| {
            BootstrapError::AttachAssertion(format!("get_public_key jubjub: {e}"))
        })?;

    let ed_vm_json = build_verification_method_json(&did, "key-auth", &ed_jwk);
    let jb_vm_json = build_verification_method_json(&did, "key-assert", &jb_jwk);

    // 4. Attach Ed25519 → authentication.
    wallet
        .add_verification_method(&did, &ed25519_ref, ed_vm_json, controller_sk)
        .await
        .map_err(|e| BootstrapError::AttachAuthn(e.to_string()))?;
    wallet
        .add_verification_method_relation(
            &did,
            "key-auth",
            crate::did::VerificationMethodRelation::Authentication,
            controller_sk,
        )
        .await
        .map_err(|e| BootstrapError::AttachAuthn(e.to_string()))?;

    // 5. Attach Jubjub → assertionMethod.
    wallet
        .add_verification_method(&did, &jubjub_ref, jb_vm_json, controller_sk)
        .await
        .map_err(|e| BootstrapError::AttachAssertion(e.to_string()))?;
    wallet
        .add_verification_method_relation(
            &did,
            "key-assert",
            crate::did::VerificationMethodRelation::AssertionMethod,
            controller_sk,
        )
        .await
        .map_err(|e| BootstrapError::AttachAssertion(e.to_string()))?;

    // 6. Verify the resolved doc carries both relations.
    let doc = wallet
        .resolve_did(&did.to_did_string())
        .await
        .map_err(|e| BootstrapError::Resolve(e.to_string()))?;
    if doc.authentication.is_empty() {
        return Err(BootstrapError::MissingRelation("authentication"));
    }
    if doc.assertion_method.is_empty() {
        return Err(BootstrapError::MissingRelation("assertionMethod"));
    }

    Ok(BootstrappedDid {
        did,
        ed25519_ref,
        jubjub_ref,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_keys_is_deterministic() {
        let seed = [42u8; 32];
        let (a1, b1) = derive_keys(&seed);
        let (a2, b2) = derive_keys(&seed);
        assert_eq!(a1, a2);
        assert_eq!(b1, b2);
    }

    #[test]
    fn derive_keys_separates_ed_and_jubjub() {
        let seed = [42u8; 32];
        let (ed, jb) = derive_keys(&seed);
        assert_ne!(ed, jb, "info strings must produce distinct outputs");
    }

    #[test]
    fn derive_keys_changes_with_seed() {
        let (ed_a, _) = derive_keys(&[1u8; 32]);
        let (ed_b, _) = derive_keys(&[2u8; 32]);
        assert_ne!(ed_a, ed_b);
    }

    #[tokio::test]
    async fn bootstrap_populates_both_relations_in_returned_struct() {
        use crate::test_support::{stub_secret_store, stub_wallet};
        let wallet = stub_wallet();
        let mut store = stub_secret_store();
        let seed = [7u8; 32];

        let out = bootstrap_did_with_keys(&wallet, &mut store, &seed)
            .await
            .expect("bootstrap should succeed against stub");

        assert!(
            out.ed25519_ref.id().starts_with("ed25519/"),
            "ed25519 key ref must be tagged",
        );
        assert!(
            out.jubjub_ref.id().starts_with("jubjub/"),
            "jubjub key ref must be tagged",
        );
        assert!(
            out.did.to_did_string().starts_with("did:midnight:"),
            "DID must be in the midnight namespace",
        );
    }
}
