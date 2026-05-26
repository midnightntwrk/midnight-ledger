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
use crate::{DidId, VerificationMethodRef};

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

/// `Ok((kid, signature_bytes))` on success. `kid` is the full
/// DID URL form (`did:midnight:net:addr#fragment`). The signature
/// is raw bytes — JWS construction is the caller's concern.
pub async fn sign_for_authentication(
    wallet: &Wallet,
    secret_store: &dyn SecretStorage,
    did: &DidId,
    payload: &[u8],
) -> Result<(String, Vec<u8>), DidAuthError> {
    let doc = wallet
        .resolve_did(&did.to_did_string())
        .await
        .map_err(|e| DidAuthError::Resolve(e.to_string()))?;

    // First authentication-relation VM. Both `Id(s)` and `Inline(vm)`
    // forms carry a string id; we coerce to a single kid string.
    let kid = doc
        .authentication
        .first()
        .map(|r| match r {
            VerificationMethodRef::Id(s) => s.clone(),
            VerificationMethodRef::Inline(vm) => vm.id.clone(),
        })
        .ok_or_else(|| DidAuthError::NoAuthnKey(did.to_did_string()))?;

    let key_ref = secret_store
        .find_by_kid(&kid)
        .await
        .ok_or_else(|| DidAuthError::NoLocalSecret(kid.clone()))?;
    let out = secret_store
        .sign(key_ref.uuid(), payload)
        .await
        .map_err(|e| DidAuthError::Sign(e.to_string()))?;

    Ok((kid, out.signature))
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
        let (kid, sig) = sign_for_authentication(&wallet, &store, &did, payload)
            .await
            .expect("sign");
        assert!(kid.starts_with("did:midnight:"));
        assert!(kid.contains("#key-auth"));
        assert!(!sig.is_empty());
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
