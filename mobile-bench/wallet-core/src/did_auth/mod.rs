//! Bridge between "I have a DID" and "I have a SecretKeyRef I
//! can sign with". Looks up the DID document, picks the first
//! verification method in the `authentication` relation, finds
//! the matching local secret by kid, signs the payload, and
//! returns `(kid, signature_bytes)`.
//!
//! The `kid` is the full DID URL with the verification-method
//! fragment (`did:midnight:abc#key-auth`) — that's what JWS
//! headers need next in the oid4vp_client path.

use crate::secret_storage::SecretStorage;
use crate::wallet::Wallet;
use crate::{DidId, PublicKeyJwk, VerificationMethodRef};

#[derive(Debug, thiserror::Error)]
pub enum DidAuthError {
    #[error("resolve failed: {0}")]
    Resolve(String),
    #[error("no authentication-relation verification method on {0}")]
    NoAuthnKey(String),
    #[error("local secret for kid {0} not in this wallet's store")]
    NoLocalSecret(String),
    #[error("sign failed: {0}")]
    Sign(String),
}

/// `Ok((kid, public_jwk, signature_bytes))` on success.
///
/// - `kid` is the full DID URL form (`did:midnight:net:addr#fragment`).
/// - `public_jwk` is the verification method's `publicKeyJwk` —
///   surfaces so JWS-header builders (OID4VP id_token, OID4VCI
///   credential proof) can embed it as the self-asserted `jwk`
///   header parameter the Phase-1 issuer verifier requires.
/// - The signature is raw bytes — JWS construction is the
///   caller's concern.
pub async fn sign_for_authentication(
    wallet: &Wallet,
    secret_store: &dyn SecretStorage,
    did: &DidId,
    payload: &[u8],
) -> Result<(String, PublicKeyJwk, Vec<u8>), DidAuthError> {
    let doc = wallet
        .resolve_did(&did.to_did_string())
        .await
        .map_err(|e| DidAuthError::Resolve(e.to_string()))?;

    // First authentication-relation VM. Both `Id(s)` and `Inline(vm)`
    // forms carry a string id; we coerce to a single kid string.
    // When the relation entry is `Inline`, the publicKeyJwk is
    // right there; for `Id(s)` we look it up in
    // `doc.verification_method` by id.
    let (kid, public_jwk) = match doc
        .authentication
        .first()
        .ok_or_else(|| DidAuthError::NoAuthnKey(did.to_did_string()))?
    {
        VerificationMethodRef::Inline(vm) => (vm.id.clone(), vm.public_key_jwk.clone()),
        VerificationMethodRef::Id(id) => {
            let vm = doc
                .verification_method
                .iter()
                .find(|v| v.id == *id)
                .ok_or_else(|| {
                    DidAuthError::Resolve(format!(
                        "authentication kid {id} not present in verificationMethod[]"
                    ))
                })?;
            (vm.id.clone(), vm.public_key_jwk.clone())
        }
    };

    let key_ref = secret_store
        .find_by_kid(&kid)
        .await
        .ok_or_else(|| DidAuthError::NoLocalSecret(kid.clone()))?;
    let out = secret_store
        .sign(key_ref.uuid(), payload)
        .await
        .map_err(|e| DidAuthError::Sign(e.to_string()))?;

    Ok((kid, public_jwk, out.signature))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{
        stub_secret_store_with_bootstrapped_did, stub_wallet_with_bootstrapped_did,
        stub_wallet_with_empty_did,
    };

    #[tokio::test]
    async fn sign_for_authentication_returns_kid_and_sig() {
        let (wallet, did) = stub_wallet_with_bootstrapped_did([5u8; 32]).await;
        let store = stub_secret_store_with_bootstrapped_did([5u8; 32]).await;

        let payload = b"hello-nonce";
        let (kid, jwk, sig) = sign_for_authentication(&wallet, &store, &did, payload)
            .await
            .expect("sign");
        assert!(kid.starts_with("did:midnight:"));
        assert!(kid.contains("#key-auth"));
        assert!(!sig.is_empty());
        // The Ed25519 auth key is `kty=OKP`. The `x` coordinate
        // is whatever the stub fixture put on chain — for live
        // wallets it's the base64url-encoded raw Ed25519 public
        // key bytes; for the stub it may be empty (the fixture
        // doesn't mint real keys). The semantic check is just
        // that the structure carries an OKP-shaped JWK.
        assert_eq!(format!("{:?}", jwk.kty), format!("{:?}", crate::KeyType::OKP));
    }

    #[tokio::test]
    async fn no_authn_key_returns_specific_error() {
        let (wallet, did) = stub_wallet_with_empty_did().await;
        let store = crate::secret_storage::InMemorySecretStore::default();
        let err = sign_for_authentication(&wallet, &store, &did, b"x")
            .await
            .expect_err("must fail");
        assert!(
            matches!(err, DidAuthError::NoAuthnKey(_)),
            "got {err:?}"
        );
    }
}
