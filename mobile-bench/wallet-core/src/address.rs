//! Bech32m address codec for unshielded NIGHT.
//!
//! Mirrors `midnight-wallet/packages/address-format/src/index.ts`:
//! HRP layout `mn_addr` (mainnet) / `mn_addr_<networkId>` (every
//! other env). Payload is **SHA-256(BIP340-schnorr-x-only-pubkey)**
//! → 32 bytes — same as `coin_structure::coin::UserAddress::from(
//! VerifyingKey)` already does in this workspace.

use bech32::{Bech32m, Hrp};

use crate::hd::{HdError, Role, derive_child_priv};
use crate::network::Network;
use base_crypto::signatures::{SigningKey, VerifyingKey};
use coin_structure::coin::UserAddress;

#[derive(Debug, thiserror::Error)]
pub enum AddressError {
    #[error("hd: {0}")]
    Hd(#[from] HdError),
    #[error("invalid signing key: {0}")]
    InvalidSigningKey(String),
    #[error("bech32 encode: {0}")]
    Bech32(#[from] bech32::EncodeError),
    #[error("bech32 hrp: {0}")]
    Hrp(#[from] bech32::primitives::hrp::Error),
}

/// HRP for an unshielded NIGHT receive address per network.
/// Mainnet drops the suffix; every other env appends `_<networkId>`.
pub fn unshielded_hrp(network: Network) -> &'static str {
    match network {
        Network::Mainnet => "mn_addr",
        Network::PreProd => "mn_addr_preprod",
        Network::Preview => "mn_addr_preview",
        Network::QaNet => "mn_addr_qanet",
        Network::DevNet => "mn_addr_devnet",
        Network::Undeployed => "mn_addr_undeployed",
    }
}

/// Derive the unshielded receive address for `(seed, network)`.
///
/// Path: `m/44'/2400'/0'/0/0` (account 0, role NightExternal,
/// index 0) — the default the upstream `counter-cli` uses.
pub fn unshielded_bech32m(
    seed: &[u8; 32],
    network: Network,
) -> Result<String, AddressError> {
    let child_priv = derive_child_priv(seed, 0, Role::NightExternal, 0)?;
    let signing_key = SigningKey::from_bytes(&child_priv)
        .map_err(|e| AddressError::InvalidSigningKey(e.to_string()))?;
    let verifying_key: VerifyingKey = signing_key.verifying_key();
    // `From<VerifyingKey> for UserAddress` already does
    // SHA-256(binary_vec(pk)) — same algorithm as
    // `addressFromKey` in midnight-wallet.
    let user_address = UserAddress::from(verifying_key);
    let payload: [u8; 32] = user_address.0.0;

    let hrp = Hrp::parse(unshielded_hrp(network))?;
    let encoded = bech32::encode::<Bech32m>(hrp, &payload)?;
    Ok(encoded)
}

/// Truncated middle for display (`mn_addr_preprod1qx…f9zg`).
/// Used by the address pill; the full string remains available via
/// `unshielded_bech32m` for the copy button.
pub fn truncate_middle(s: &str, head: usize, tail: usize) -> String {
    if s.len() <= head + tail + 1 {
        s.to_string()
    } else {
        let head_part: String = s.chars().take(head).collect();
        let tail_part: String = s.chars().rev().take(tail).collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        format!("{head_part}…{tail_part}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Smoke: deterministic seed + network → deterministic, well-
    /// shaped bech32m string.
    #[test]
    fn demo_seed_yields_stable_preprod_address() {
        let seed = [0x88u8; 32];
        let a = unshielded_bech32m(&seed, Network::PreProd).unwrap();
        let b = unshielded_bech32m(&seed, Network::PreProd).unwrap();
        assert_eq!(a, b);
        assert!(
            a.starts_with("mn_addr_preprod1"),
            "expected preprod prefix, got {a}"
        );
        // bech32m payload is 32 bytes → 52 chars. Plus HRP +
        // separator. Exact length is HRP + 1 + 52 + 6 (checksum).
        assert_eq!(a.len(), "mn_addr_preprod".len() + 1 + 52 + 6);
    }

    #[test]
    fn mainnet_drops_network_suffix() {
        let seed = [0xabu8; 32];
        let a = unshielded_bech32m(&seed, Network::Mainnet).unwrap();
        assert!(a.starts_with("mn_addr1"), "got {a}");
    }

    #[test]
    fn different_networks_yield_different_addresses() {
        let seed = [0x55u8; 32];
        let pre = unshielded_bech32m(&seed, Network::PreProd).unwrap();
        let main = unshielded_bech32m(&seed, Network::Mainnet).unwrap();
        // Same payload bytes but different HRP, so the encoded
        // strings differ.
        assert_ne!(pre, main);
    }

    #[test]
    fn truncate_middle_short_string_unchanged() {
        assert_eq!(truncate_middle("mn_addr1xy", 6, 4), "mn_addr1xy");
    }

    #[test]
    fn truncate_middle_long_string_compressed() {
        let s = "mn_addr_preprod1qxabcdef0123456789xyzlongtail";
        let t = truncate_middle(s, 18, 6);
        assert_eq!(t, "mn_addr_preprod1qx…ngtail");
    }
}
