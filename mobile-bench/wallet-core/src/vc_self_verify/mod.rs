//! `vc_self_verify` — re-resolve a VC's issuer DID and check the
//! VC body's proof signature against the published
//! `assertionMethod` verification method.
//!
//! Three-state result:
//! - [`SelfVerifyResult::Valid`] — proof signature checks against
//!   the freshly-resolved issuer key.
//! - [`SelfVerifyResult::Invalid`] — body parsed, but the proof
//!   couldn't be checked. Carries an [`InvalidReason`] tag.
//! - [`SelfVerifyResult::Error`] — could not even attempt the
//!   check (DID resolve failed, body wasn't CBOR, etc).
//!
//! The verification path goes through [`SecretStorage::verify`]
//! with the resolved VM's `publicKeyJwk` populating the
//! `VerifyInput::public_jwk` field — the wallet itself has no
//! detached-verify primitive in this crate. (The plan's signature
//! took only `&Wallet`; we add a `&dyn SecretStorage` parameter
//! so the verify call has somewhere to land.)
//!
//! Phase 1 VC body shape (placeholder, not the upstream Compact
//! binary encoding — that lands in Phase 2):
//!
//! ```cbor
//! {
//!   "credentialSubject": <bytes>,
//!   "proof": {
//!     "verificationMethod": "<did>#<fragment>",
//!     "signature": "<base64 std signature bytes>"
//!   }
//! }
//! ```
//!
//! The canonical payload signed is the same CBOR map with the
//! `proof` entry removed. `test_support::stub_sign_birth_vc`
//! produces the matching shape for unit tests.

use std::collections::BTreeMap;

use base64::{Engine, engine::general_purpose::STANDARD as B64};

use crate::secret_storage::{
    MidnightCurve, MidnightKeyType, PublicJwk, SecretStorage, VerifyInput,
};
use crate::vc_store::StoredVc;
use crate::wallet::Wallet;
use crate::{
    CurveType, DidId, KeyType, PublicKeyJwk, VerificationMethod, VerificationMethodRef,
};

/// Outcome of [`self_verify`] / [`self_verify_and_cache`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelfVerifyResult {
    /// Proof signature checked against the resolved issuer key.
    Valid {
        /// Full DID URL of the verification method that signed
        /// (e.g. `did:midnight:preprod:abc#key-assert`).
        vm_id: String,
    },
    /// VC body parsed but the proof could not be checked.
    Invalid(InvalidReason),
    /// Could not attempt the check at all — fatal error.
    Error(String),
}

/// Reason a VC was rejected as `Invalid`. Fine-grained so the UI
/// can render a precise message instead of a generic failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvalidReason {
    /// Body wasn't CBOR or didn't decode to a map at the top
    /// level.
    UnparseableBody,
    /// The `proof` field was missing or malformed (not a map, or
    /// missing required `verificationMethod` / `signature` keys).
    MissingProof,
    /// The signature field couldn't be base64-decoded.
    UnparseableSignature,
    /// `proof.verificationMethod` didn't match any
    /// `assertionMethod`-relation VM on the resolved issuer DID.
    UnknownVerificationMethod(String),
    /// The verification method's `publicKeyJwk` couldn't be
    /// converted to the [`PublicJwk`] shape `SecretStorage::verify`
    /// requires (e.g. unsupported curve).
    UnsupportedKey(String),
    /// The signature didn't verify against the canonical
    /// (proof-stripped) body bytes.
    SignatureMismatch,
}

/// Re-resolve the VC's issuer DID on chain and check the body's
/// proof signature against the published `assertionMethod` key.
///
/// `secret_store` is used purely as a verify primitive — its key
/// store is NOT consulted (the verification method's public JWK
/// goes into `VerifyInput::public_jwk` directly).
pub async fn self_verify(
    vc: &StoredVc,
    wallet: &Wallet,
    secret_store: &dyn SecretStorage,
) -> SelfVerifyResult {
    // 1. Parse the issuer DID string.
    let issuer = match DidId::parse(&vc.issuer_did) {
        Ok(d) => d,
        Err(e) => return SelfVerifyResult::Error(format!("issuer did parse: {e}")),
    };

    // 2. Re-resolve on chain (or via stub map under tests).
    let doc = match wallet.resolve_did(&issuer.to_did_string()).await {
        Ok(d) => d,
        Err(e) => return SelfVerifyResult::Error(format!("resolve did: {e}")),
    };

    // 3. Decode the CBOR body to a Value, then locate the proof
    //    sub-map.
    let body_value: serde_cbor::Value = match serde_cbor::from_slice(&vc.body) {
        Ok(v) => v,
        Err(_) => return SelfVerifyResult::Invalid(InvalidReason::UnparseableBody),
    };
    let entries = match body_value {
        serde_cbor::Value::Map(m) => m,
        _ => return SelfVerifyResult::Invalid(InvalidReason::UnparseableBody),
    };

    // Re-stage as a BTreeMap so we can both pull the proof out
    // and re-emit the remaining entries in deterministic order
    // identical to what the signer used.
    let mut staged: BTreeMap<String, serde_cbor::Value> = BTreeMap::new();
    for (k, v) in entries {
        let key = match k {
            serde_cbor::Value::Text(s) => s,
            _ => return SelfVerifyResult::Invalid(InvalidReason::UnparseableBody),
        };
        staged.insert(key, v);
    }
    let proof_value = match staged.remove("proof") {
        Some(p) => p,
        None => return SelfVerifyResult::Invalid(InvalidReason::MissingProof),
    };

    // 4. Pull verificationMethod + signature out of proof map.
    let proof_entries = match proof_value {
        serde_cbor::Value::Map(m) => m,
        _ => return SelfVerifyResult::Invalid(InvalidReason::MissingProof),
    };
    let mut vm_id_opt: Option<String> = None;
    let mut sig_b64_opt: Option<String> = None;
    for (k, v) in proof_entries {
        let key = match k {
            serde_cbor::Value::Text(s) => s,
            _ => continue,
        };
        match (key.as_str(), v) {
            ("verificationMethod", serde_cbor::Value::Text(s)) => vm_id_opt = Some(s),
            ("signature", serde_cbor::Value::Text(s)) => sig_b64_opt = Some(s),
            _ => {}
        }
    }
    let (vm_id, sig_b64) = match (vm_id_opt, sig_b64_opt) {
        (Some(v), Some(s)) => (v, s),
        _ => return SelfVerifyResult::Invalid(InvalidReason::MissingProof),
    };
    let signature = match B64.decode(sig_b64.as_bytes()) {
        Ok(b) => b,
        Err(_) => return SelfVerifyResult::Invalid(InvalidReason::UnparseableSignature),
    };

    // 5. Find the assertionMethod VM whose id matches.
    let vm = match find_assertion_vm(&doc.assertion_method, &doc.verification_method, &vm_id) {
        Some(vm) => vm,
        None => {
            return SelfVerifyResult::Invalid(InvalidReason::UnknownVerificationMethod(vm_id));
        }
    };

    // 6. Convert DID-Core PublicKeyJwk to secret-storage PublicJwk.
    let pjwk = match convert_jwk(&vm.public_key_jwk) {
        Ok(j) => j,
        Err(reason) => return SelfVerifyResult::Invalid(InvalidReason::UnsupportedKey(reason)),
    };

    // 7. Re-emit the proof-stripped body bytes via the same
    //    BTreeMap-ordered path the signer used. `staged` already
    //    has the proof removed.
    let canonical_value = serde_cbor::Value::Map(
        staged
            .into_iter()
            .map(|(k, v)| (serde_cbor::Value::Text(k), v))
            .collect(),
    );
    let canonical_bytes = match serde_cbor::to_vec(&canonical_value) {
        Ok(b) => b,
        Err(_) => return SelfVerifyResult::Invalid(InvalidReason::UnparseableBody),
    };

    // 8. Verify via SecretStorage's public_jwk path.
    let ok = secret_store
        .verify(VerifyInput {
            key_ref: None,
            public_jwk: Some(pjwk),
            payload: canonical_bytes,
            signature,
        })
        .await;
    match ok {
        Ok(true) => SelfVerifyResult::Valid { vm_id },
        Ok(false) => SelfVerifyResult::Invalid(InvalidReason::SignatureMismatch),
        Err(e) => SelfVerifyResult::Error(format!("verify: {e}")),
    }
}

/// Wraps [`self_verify`] and writes the outcome onto the VC's
/// metadata row so a carousel can show "last verified at HH:MM:SS"
/// without re-running the chain query.
///
/// Writes `last_verified_ms` (epoch millis, now) and
/// `last_verify_outcome` (a stable display string) regardless of
/// the outcome — even `Error(_)` lands as
/// `"Error: <message>"` so the carousel can surface it.
pub async fn self_verify_and_cache(
    vc: &StoredVc,
    wallet: &Wallet,
    secret_store: &dyn SecretStorage,
    vc_store: &crate::VcStore,
) -> SelfVerifyResult {
    let result = self_verify(vc, wallet, secret_store).await;
    let outcome = match &result {
        SelfVerifyResult::Valid { .. } => "Valid".to_string(),
        SelfVerifyResult::Invalid(reason) => format!("Invalid: {reason:?}"),
        SelfVerifyResult::Error(msg) => format!("Error: {msg}"),
    };
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    // Best-effort metadata write — verification result is the
    // primary return; a metadata write failure shouldn't mask the
    // verification outcome itself.
    let _ = vc_store.update_metadata(&vc.vc_uri, |m| {
        m.last_verified_ms = Some(now_ms);
        m.last_verify_outcome = Some(outcome);
    });
    result
}

/// Locate a `VerificationMethod` whose `id` matches `vm_id` among
/// the entries the issuer DID document lists under `assertion_method`.
/// Accepts both `VerificationMethodRef::Id(...)` (with a lookup
/// into the doc's `verification_method` array) and the inline
/// `Inline(VerificationMethod)` form.
fn find_assertion_vm<'a>(
    assertion_refs: &'a [VerificationMethodRef],
    verification_methods: &'a [VerificationMethod],
    vm_id: &str,
) -> Option<&'a VerificationMethod> {
    for r in assertion_refs {
        match r {
            VerificationMethodRef::Inline(vm) if vm.id == vm_id => return Some(vm),
            VerificationMethodRef::Id(s) if s == vm_id => {
                return verification_methods.iter().find(|vm| vm.id == vm_id);
            }
            _ => {}
        }
    }
    None
}

/// Convert a DID-Core [`PublicKeyJwk`] (Ed25519/P256/Jubjub) to
/// the secret-storage [`PublicJwk`] shape `SecretStorage::verify`
/// requires. Mostly trivial — the two are structurally identical
/// but use distinct enum types; the only on-wire difference is
/// P-256 (`"P256"` vs `"P-256"`), which we map here.
fn convert_jwk(jwk: &PublicKeyJwk) -> Result<PublicJwk, String> {
    let kty = match jwk.kty {
        KeyType::OKP => MidnightKeyType::OKP,
        KeyType::EC => MidnightKeyType::EC,
    };
    let crv = match jwk.crv {
        CurveType::Ed25519 => MidnightCurve::Ed25519,
        CurveType::Jubjub => MidnightCurve::Jubjub,
        CurveType::P256 => MidnightCurve::P256,
    };
    Ok(PublicJwk {
        kty,
        crv,
        x: jwk.x.clone(),
        y: jwk.y.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{
        stub_secret_store_with_bootstrapped_did, stub_sign_birth_vc,
        stub_wallet_with_bootstrapped_did,
    };

    fn make_vc(issuer: &DidId, body: Vec<u8>) -> StoredVc {
        StoredVc {
            vc_uri: "urn:uuid:test-vc".to_string(),
            issuer_did: issuer.to_did_string(),
            holder_did: "did:midnight:undeployed:00".to_string(),
            format: "midnight-vc-cbor-phase1".to_string(),
            body,
            issued_at_ms: 0,
        }
    }

    #[tokio::test]
    async fn self_verify_valid_round_trip() {
        let seed = [55u8; 32];
        let (wallet, did) = stub_wallet_with_bootstrapped_did(seed).await;
        let store = stub_secret_store_with_bootstrapped_did(seed).await;
        let body = stub_sign_birth_vc(&wallet, &store, &did, b"BIRTH-FIXTURE").await;
        let vc = make_vc(&did, body);

        let result = self_verify(&vc, &wallet, &store).await;
        match result {
            SelfVerifyResult::Valid { vm_id } => {
                assert!(vm_id.starts_with("did:midnight:"), "vm_id: {vm_id}");
                assert!(vm_id.contains("#"), "vm_id should be DID URL form: {vm_id}");
            }
            other => panic!("expected Valid, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn self_verify_tampered_body_is_invalid() {
        let seed = [56u8; 32];
        let (wallet, did) = stub_wallet_with_bootstrapped_did(seed).await;
        let store = stub_secret_store_with_bootstrapped_did(seed).await;
        let body = stub_sign_birth_vc(&wallet, &store, &did, b"BIRTH-FIXTURE").await;

        // Tamper: flip one byte in the body. To make the body
        // still decode as CBOR (we want to land in the
        // SignatureMismatch path, not UnparseableBody), we flip a
        // byte well inside the credentialSubject payload — the
        // ASCII bytes near the end of "BIRTH-FIXTURE", which
        // live unambiguously inside a CBOR bytes blob.
        let mut tampered = body.clone();
        let needle = b"BIRTH-FIXTURE";
        let pos = tampered
            .windows(needle.len())
            .position(|w| w == needle)
            .expect("payload should appear in CBOR-encoded body");
        // Flip a high bit so the CBOR length headers remain valid.
        tampered[pos] ^= 0x01;
        let vc = make_vc(&did, tampered);

        let result = self_verify(&vc, &wallet, &store).await;
        assert_eq!(
            result,
            SelfVerifyResult::Invalid(InvalidReason::SignatureMismatch),
            "expected SignatureMismatch, got {result:?}"
        );
    }

    #[tokio::test]
    async fn self_verify_and_cache_writes_metadata() {
        let seed = [57u8; 32];
        let (wallet, did) = stub_wallet_with_bootstrapped_did(seed).await;
        let store = stub_secret_store_with_bootstrapped_did(seed).await;
        let body = stub_sign_birth_vc(&wallet, &store, &did, b"BIRTH-FIXTURE").await;
        let vc = make_vc(&did, body);

        let dir = tempfile::TempDir::new().expect("tempdir");
        let vc_store = crate::VcStore::open(dir.path().join("vcs.redb")).expect("open vc store");
        vc_store.insert_vc(&vc).expect("insert vc");

        let result = self_verify_and_cache(&vc, &wallet, &store, &vc_store).await;
        assert!(
            matches!(result, SelfVerifyResult::Valid { .. }),
            "expected Valid, got {result:?}"
        );
        let md = vc_store
            .get_metadata(&vc.vc_uri)
            .expect("metadata read ok")
            .expect("metadata present");
        assert_eq!(md.last_verify_outcome, Some("Valid".to_string()));
        assert!(
            md.last_verified_ms.unwrap_or(0) > 0,
            "last_verified_ms should be set: {:?}",
            md.last_verified_ms
        );
    }
}
