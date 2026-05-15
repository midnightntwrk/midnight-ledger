//! Substrate `Signer` impl over the wallet's secp256k1 secret.
//!
//! The wallet derives a 32-byte secret scalar at BIP32 path
//! `m/44'/2400'/0'/0/0` (role `NightExternal`). That same scalar
//! serves two signature schemes that share the secp256k1 curve:
//!
//! - **BIP340 schnorr** — used in-circuit by the DID contract for
//!   `controllerPublicKey` checks. Surface:
//!   [`crate::Wallet::did_controller_signing_key`].
//! - **ECDSA** — required by substrate's stock
//!   `sp_runtime::MultiSignature(Ecdsa(_))` to authenticate the tx
//!   envelope. Surface: this module.
//!
//! The on-chain signature scheme of midnight-node 0.22.3 is plain
//! `MultiSignature` (Ed25519 / Sr25519 / Ecdsa) — confirmed via
//! `MODE=offline cargo run -p wallet-core --example probe_metadata`.
//! No Midnight-custom variant; ECDSA is enough for envelope auth.
//!
//! Substrate `AccountId32` for ECDSA = `blake2_256(compressed_pubkey)`.
//! That's the canonical pallet-balances key; we expose it via
//! `MidnightSigner::account_id_bytes()` so callers don't have to pull
//! `sp_runtime` for one constant.

use blake2::{Blake2b, Digest, digest::consts::U32};
use k256::ecdsa::{Signature as EcdsaSignature, SigningKey, VerifyingKey};
use subxt::{Config, SubstrateConfig};
use subxt::tx::Signer as SubxtSigner;
use subxt::utils::{AccountId32, MultiAddress, MultiSignature};

#[derive(Debug, thiserror::Error)]
pub enum SignerError {
    #[error("invalid secret scalar: {0}")]
    InvalidSecret(String),
}

/// Wraps the secp256k1 secret + derived ECDSA verifying key. Cheap
/// to clone (32-byte secret + cached pubkey).
#[derive(Clone)]
pub struct MidnightSigner {
    signing_key: SigningKey,
    /// Pre-computed substrate AccountId bytes
    /// (`blake2_256(compressed_pubkey)`).
    account_id_bytes: [u8; 32],
}

impl MidnightSigner {
    /// Build from a 32-byte secp256k1 secret scalar — typically the
    /// output of [`crate::hd::derive_child_priv`] at the
    /// NightExternal role.
    pub fn from_secret_bytes(secret: &[u8; 32]) -> Result<Self, SignerError> {
        let signing_key = SigningKey::from_bytes(secret.into())
            .map_err(|e| SignerError::InvalidSecret(e.to_string()))?;
        let verifying_key = *signing_key.verifying_key();
        let account_id_bytes = ecdsa_account_id(&verifying_key);
        Ok(Self { signing_key, account_id_bytes })
    }

    /// 32-byte substrate `AccountId32` corresponding to this signer.
    /// Used as the `from` field on the tx envelope and as the lookup
    /// key for the wallet's on-chain balance.
    pub fn account_id_bytes(&self) -> [u8; 32] {
        self.account_id_bytes
    }

    /// 65-byte ECDSA signature in substrate's `(r, s, recovery_id)`
    /// layout. The signer-payload comes from subxt: it's either the
    /// raw bytes (≤256) or `blake2_256(raw_bytes)` (>256).
    /// Substrate's `MultiSignature::Ecdsa` verification recovers
    /// the pubkey against `blake2_256(payload_we_received)`, so we
    /// MUST sign that exact digest — `k256::sign_recoverable()`
    /// would silently use SHA-256 internally, which substrate
    /// rejects (the recovered pubkey doesn't match the AccountId,
    /// → "Invalid Transaction (1010)").
    pub fn sign_envelope(&self, payload: &[u8]) -> [u8; 65] {
        let digest = blake2_256(payload);
        let (sig, recovery_id): (EcdsaSignature, _) = self
            .signing_key
            .sign_prehash_recoverable(&digest)
            .expect("ECDSA sign over a 32-byte prehash cannot fail");
        let mut out = [0u8; 65];
        out[..64].copy_from_slice(&sig.to_bytes());
        out[64] = recovery_id.to_byte();
        out
    }

    /// 33-byte compressed verifying key. Useful as a stable
    /// fingerprint for the signer (e.g. UI display, logs).
    pub fn compressed_pubkey(&self) -> [u8; 33] {
        let bytes = self.signing_key.verifying_key().to_encoded_point(true);
        let slice = bytes.as_bytes();
        let mut out = [0u8; 33];
        out.copy_from_slice(slice);
        out
    }
}

impl SubxtSigner<SubstrateConfig> for MidnightSigner {
    fn account_id(&self) -> <SubstrateConfig as Config>::AccountId {
        AccountId32(self.account_id_bytes)
    }

    fn sign(&self, payload: &[u8]) -> <SubstrateConfig as Config>::Signature {
        let sig_bytes = self.sign_envelope(payload);
        MultiSignature::Ecdsa(sig_bytes)
    }
}

/// `Into<MultiAddress<AccountId32, u32>>` helper for callers that
/// want the substrate address (not the raw AccountId) — e.g. log
/// formatting.
impl From<&MidnightSigner> for MultiAddress<AccountId32, u32> {
    fn from(s: &MidnightSigner) -> Self {
        MultiAddress::Id(AccountId32(s.account_id_bytes))
    }
}

/// Substrate's blake2-256 hash. Wallet-core only needs it on this
/// codepath, so we keep the helper local rather than pulling in
/// `sp-crypto-hashing`.
fn blake2_256(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Blake2b::<U32>::new();
    hasher.update(bytes);
    hasher.finalize().into()
}

/// Stock substrate ECDSA `AccountId` derivation:
/// `blake2_256(compressed_pubkey)`.
fn ecdsa_account_id(vk: &VerifyingKey) -> [u8; 32] {
    let compressed = vk.to_encoded_point(true);
    blake2_256(compressed.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Network;
    use crate::Wallet;

    fn signer_from_demo(network: Network) -> MidnightSigner {
        let w = Wallet::demo(network);
        let secret = crate::hd::derive_child_priv(
            &w.seed_bytes(),
            0,
            crate::hd::Role::NightExternal,
            0,
        )
        .expect("hd derive");
        MidnightSigner::from_secret_bytes(&secret).expect("signer build")
    }

    #[test]
    fn signer_is_deterministic() {
        let a = signer_from_demo(Network::Undeployed);
        let b = signer_from_demo(Network::Undeployed);
        assert_eq!(a.account_id_bytes(), b.account_id_bytes());
        assert_eq!(a.compressed_pubkey(), b.compressed_pubkey());
    }

    #[test]
    fn account_ids_differ_per_seed() {
        // Demo seeds are network-aware (Undeployed uses the
        // pre-funded GENESIS_MINT_WALLET_SEED, others the public
        // demo seed) so we exercise both branches.
        let und = signer_from_demo(Network::Undeployed);
        let pre = signer_from_demo(Network::PreProd);
        assert_ne!(und.account_id_bytes(), pre.account_id_bytes());
    }

    #[test]
    fn signer_impls_subxt_signer_for_substrate_config() {
        use subxt::tx::Signer as _;
        let s = signer_from_demo(Network::Undeployed);
        let acct: AccountId32 = s.account_id();
        assert_eq!(acct.0, s.account_id_bytes());
        let sig = SubxtSigner::<SubstrateConfig>::sign(&s, b"hello");
        assert!(matches!(sig, MultiSignature::Ecdsa(_)));
    }

    #[test]
    fn signature_round_trip() {
        // The signature is over `blake2_256(msg)` (substrate's
        // MultiSignature::Ecdsa verification expects that
        // digest). We verify the same way: compute the prehash,
        // then `verify_prehash` on the recovered signature.
        let signer = signer_from_demo(Network::Undeployed);
        let msg = b"deploy-DID-test";
        let sig = signer.sign_envelope(msg);
        // 65-byte (r:32, s:32, v:1) layout.
        assert_eq!(sig.len(), 65);
        // Recovery ID must be 0..=3 (substrate ECDSA convention).
        assert!(sig[64] <= 3);

        use k256::ecdsa::signature::hazmat::PrehashVerifier;
        let vk = signer.signing_key.verifying_key();
        let r: [u8; 32] = sig[..32].try_into().unwrap();
        let s: [u8; 32] = sig[32..64].try_into().unwrap();
        let parsed = EcdsaSignature::from_scalars(r, s).unwrap();
        let digest = blake2_256(msg);
        vk.verify_prehash(&digest, &parsed)
            .expect("signature must verify against blake2_256 prehash");
    }
}
