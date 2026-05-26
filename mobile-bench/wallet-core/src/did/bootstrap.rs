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

use crate::secret_storage::{SecretKeyRef, SecretStorage};
use crate::wallet::Wallet;
use crate::DidId;

/// Result of a successful bootstrap.
#[allow(dead_code)] // Fields consumed by Task 2 orchestration.
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

pub(crate) fn derive_keys(seed: &[u8; 32]) -> ([u8; 32], [u8; 32]) {
    let h = Hkdf::<Sha256>::new(Some(b"midnight-identity-centre-v1"), seed);
    let mut ed = [0u8; 32];
    let mut jb = [0u8; 32];
    h.expand(b"ed25519/authentication", &mut ed)
        .expect("HKDF expand for ed25519");
    h.expand(b"jubjub/assertionMethod", &mut jb)
        .expect("HKDF expand for jubjub");
    (ed, jb)
}

#[allow(dead_code)] // Full body lands in Task 2.
pub async fn bootstrap_did_with_keys(
    _wallet: &Wallet,
    _secret_store: &mut dyn SecretStorage,
    seed: &[u8; 32],
) -> Result<BootstrappedDid, BootstrapError> {
    let (_ed, _jb) = derive_keys(seed);
    // Filled in across Task 2.
    Err(BootstrapError::CreateDid("not implemented yet".into()))
}

#[cfg(test)]
mod tests {
    use super::derive_keys;

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
}
