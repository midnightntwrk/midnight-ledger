//! BIP32 / BIP44 hierarchical key derivation for Midnight wallets.
//!
//! Mirrors `midnightntwrk/midnight-wallet/packages/hd/src/HDWallet.ts`.
//! Path layout: `m/44'/2400'/<account>'/<role>/<index>`. Purpose,
//! coin_type and account are hardened; role and index are
//! non-hardened. The 32-byte child private key is consumed verbatim
//! by per-role downstream code (e.g. unshielded NIGHT signing key).

use bip32::{ChildNumber, ExtendedPrivateKey, secp256k1::SecretKey};

/// BIP44 purpose. Constant per the BIP, hardened.
const PURPOSE: u32 = 44;
/// Midnight's registered SLIP-44 coin-type. Hardened.
const COIN_TYPE: u32 = 2400;

/// Wallet roles, mirroring `Roles` in the upstream
/// `packages/hd/src/HDWallet.ts`. Non-hardened in the path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum Role {
    NightExternal = 0,
    NightInternal = 1,
    Dust = 2,
    Zswap = 3,
    Metadata = 4,
}

#[derive(Debug, thiserror::Error)]
pub enum HdError {
    // bip32::Error is no_std and doesn't impl std::error::Error, so
    // we keep it as a string at the boundary.
    #[error("bip32: {0}")]
    Bip32(String),
}

impl From<bip32::Error> for HdError {
    fn from(e: bip32::Error) -> Self {
        HdError::Bip32(e.to_string())
    }
}

/// Derive the 32-byte child private key for `(account, role, index)`
/// from a 32-byte master seed.
///
/// Implements `m/44'/2400'/<account>'/<role>/<index>` exactly like
/// `@scure/bip32` `HDKey.fromMasterSeed(seed).derive(path)`. The
/// returned bytes are the secp256k1 secret scalar; downstream code
/// converts them to an asymmetric key via the role's curve (BIP340
/// schnorr for unshielded NIGHT).
pub(crate) fn derive_child_priv(
    seed: &[u8; 32],
    account: u32,
    role: Role,
    index: u32,
) -> Result<[u8; 32], HdError> {
    let xprv = ExtendedPrivateKey::<SecretKey>::new(seed.as_slice())?
        .derive_child(ChildNumber::new(PURPOSE, true)?)?
        .derive_child(ChildNumber::new(COIN_TYPE, true)?)?
        .derive_child(ChildNumber::new(account, true)?)?
        .derive_child(ChildNumber::new(role as u32, false)?)?
        .derive_child(ChildNumber::new(index, false)?)?;
    Ok(xprv.to_bytes().into())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Same seed → same child key, regardless of how many times we
    /// derive it. Catches accidental nondeterminism (e.g. random
    /// state leaking into the path).
    #[test]
    fn deterministic_child_derivation() {
        let seed = [0x42u8; 32];
        let a = derive_child_priv(&seed, 0, Role::NightExternal, 0).unwrap();
        let b = derive_child_priv(&seed, 0, Role::NightExternal, 0).unwrap();
        assert_eq!(a, b);
    }

    /// Different roles yield different child keys (otherwise the
    /// role enum is doing nothing).
    #[test]
    fn different_roles_yield_different_keys() {
        let seed = [0x42u8; 32];
        let ext = derive_child_priv(&seed, 0, Role::NightExternal, 0).unwrap();
        let int = derive_child_priv(&seed, 0, Role::NightInternal, 0).unwrap();
        let dust = derive_child_priv(&seed, 0, Role::Dust, 0).unwrap();
        assert_ne!(ext, int);
        assert_ne!(ext, dust);
        assert_ne!(int, dust);
    }

    /// Different indices at the same role yield different keys.
    #[test]
    fn different_indices_yield_different_keys() {
        let seed = [0x42u8; 32];
        let i0 = derive_child_priv(&seed, 0, Role::NightExternal, 0).unwrap();
        let i1 = derive_child_priv(&seed, 0, Role::NightExternal, 1).unwrap();
        assert_ne!(i0, i1);
    }
}
