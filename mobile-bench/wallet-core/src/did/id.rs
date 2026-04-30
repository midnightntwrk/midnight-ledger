//! `did:midnight:<network>:<contract_address>` codec.
//!
//! Mirrors `midnight-did/domain/src/midnight.ts`.
//!
//! The canonical form is the DID-string:
//! `did:midnight:<network_id>:<64-hex>` where `<network_id>` is one
//! of `mainnet | testnet | devnet | preview | preprod | qanet |
//! undeployed` (matching `NetworkId.ts`) and `<64-hex>` is the
//! lowercase hex encoding of a 32-byte Compact `ContractAddress`.
//!
//! gsd-wallet displays addresses in bech32m form `mn_did_*1…`; we
//! support that as a secondary parse target so the same input box
//! accepts either flavour.

use serde::de::{self, Deserializer, Visitor};
use serde::ser::Serializer;
use serde::{Deserialize, Serialize};

use crate::network::Network;

/// Length of a Compact ContractAddress in bytes.
pub const CONTRACT_ADDRESS_LEN: usize = 32;

/// Stable hex-encoded form of a 32-byte contract address. Stored
/// lowercase so the canonical DID-string is byte-identical across
/// platforms.
pub type ContractAddressBytes = [u8; CONTRACT_ADDRESS_LEN];

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DidId {
    pub network: Network,
    pub contract_address: ContractAddressBytes,
}

#[derive(Debug, thiserror::Error)]
pub enum DidIdError {
    #[error("expected DID-string with method `did:midnight:`, got: {0:?}")]
    NotMidnightDid(String),
    #[error("unknown midnight network id `{0}` — expected one of mainnet | testnet | preprod | preview | qanet | devnet | undeployed")]
    UnknownNetwork(String),
    #[error("contract address: {0}")]
    InvalidContractAddress(String),
    #[error("hex decode: {0}")]
    Hex(#[from] hex::FromHexError),
}

impl DidId {
    pub fn new(network: Network, contract_address: ContractAddressBytes) -> Self {
        Self { network, contract_address }
    }

    /// Parse either a canonical DID-string (`did:midnight:...`) or
    /// a bech32m alias (`mn_did_*1...`).
    pub fn parse(s: &str) -> Result<Self, DidIdError> {
        if let Some(rest) = s.strip_prefix("did:midnight:") {
            Self::parse_did_string(rest)
        } else if s.starts_with("mn_did") {
            Self::parse_bech32m(s)
        } else {
            Err(DidIdError::NotMidnightDid(s.to_string()))
        }
    }

    fn parse_did_string(rest: &str) -> Result<Self, DidIdError> {
        // `<network>:<hex>`
        let (net_str, hex_str) = rest
            .split_once(':')
            .ok_or_else(|| DidIdError::NotMidnightDid(format!("did:midnight:{rest}")))?;
        let network = parse_network_id(net_str)
            .ok_or_else(|| DidIdError::UnknownNetwork(net_str.to_string()))?;
        let bytes = decode_contract_address_hex(hex_str)?;
        Ok(Self::new(network, bytes))
    }

    fn parse_bech32m(_s: &str) -> Result<Self, DidIdError> {
        // gsd-wallet's bech32m alias parsing lands in Phase 2 once we
        // have a real fixture string to test against. Until then we
        // surface a clear error so callers don't silently treat the
        // input as ambiguous.
        Err(DidIdError::InvalidContractAddress(
            "bech32m mn_did_* alias parsing not yet implemented (Phase 2 of DID_PLAN)"
                .into(),
        ))
    }

    /// Canonical DID-string: `did:midnight:<network_id>:<lowercase 64-hex>`.
    pub fn to_did_string(&self) -> String {
        format!(
            "did:midnight:{}:{}",
            network_id_str(self.network),
            hex::encode(self.contract_address)
        )
    }

    /// Lowercase hex of the 32-byte contract address.
    pub fn contract_address_hex(&self) -> String {
        hex::encode(self.contract_address)
    }
}

impl std::fmt::Display for DidId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_did_string())
    }
}

// ── serde ─────────────────────────────────────────────────────────
// We serialise as the canonical DID-string for JSON-LD / DID Core
// compatibility; deserialising accepts either form via `parse`.

impl Serialize for DidId {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_str(&self.to_did_string())
    }
}

impl<'de> Deserialize<'de> for DidId {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = DidId;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a `did:midnight:<network>:<64-hex>` string")
            }
            fn visit_str<E: de::Error>(self, s: &str) -> Result<DidId, E> {
                DidId::parse(s).map_err(|e| de::Error::custom(e.to_string()))
            }
        }
        de.deserialize_str(V)
    }
}

// ── helpers ───────────────────────────────────────────────────────

fn network_id_str(net: Network) -> &'static str {
    // Mirrors `midnight-wallet/packages/abstractions/src/NetworkId.ts`.
    // We keep `testnet` as a known string even though our `Network`
    // enum doesn't have a Testnet variant — DID strings minted by
    // earlier midnight-did versions may use it; round-tripping
    // requires support, see the `parse_network_id` map.
    match net {
        Network::Mainnet => "mainnet",
        Network::PreProd => "preprod",
        Network::Preview => "preview",
        Network::QaNet => "qanet",
        Network::DevNet => "devnet",
        Network::Undeployed => "undeployed",
    }
}

fn parse_network_id(s: &str) -> Option<Network> {
    match s {
        "mainnet" => Some(Network::Mainnet),
        // gsd-wallet historical alias — testnet ≡ preprod for our
        // purposes (the only `test*` HRP midnight-did has shipped).
        "testnet" | "preprod" => Some(Network::PreProd),
        "preview" => Some(Network::Preview),
        "qanet" => Some(Network::QaNet),
        "devnet" => Some(Network::DevNet),
        "undeployed" => Some(Network::Undeployed),
        _ => None,
    }
}

fn decode_contract_address_hex(s: &str) -> Result<ContractAddressBytes, DidIdError> {
    let bytes = hex::decode(s)?;
    bytes.try_into().map_err(|v: Vec<u8>| {
        DidIdError::InvalidContractAddress(format!(
            "expected {CONTRACT_ADDRESS_LEN} bytes, got {}",
            v.len()
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> DidId {
        DidId::new(
            Network::PreProd,
            [
                0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67,
                0x89, 0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef,
                0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef,
            ],
        )
    }

    #[test]
    fn did_string_round_trip() {
        let id = sample();
        let s = id.to_did_string();
        assert_eq!(
            s,
            "did:midnight:preprod:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        );
        assert_eq!(DidId::parse(&s).unwrap(), id);
    }

    #[test]
    fn round_trip_each_network() {
        for net in Network::ALL {
            let id = DidId::new(net, [0xaa; 32]);
            let s = id.to_did_string();
            let parsed = DidId::parse(&s).unwrap();
            assert_eq!(parsed, id, "round-trip failed for {net:?}");
        }
    }

    #[test]
    fn testnet_alias_resolves_to_preprod() {
        // gsd-wallet historical: testnet ≡ preprod
        let s = "did:midnight:testnet:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let parsed = DidId::parse(s).unwrap();
        assert_eq!(parsed.network, Network::PreProd);
    }

    #[test]
    fn rejects_non_midnight_method() {
        assert!(matches!(
            DidId::parse("did:web:example.test"),
            Err(DidIdError::NotMidnightDid(_))
        ));
    }

    #[test]
    fn rejects_unknown_network() {
        let s = "did:midnight:nope:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        assert!(matches!(DidId::parse(s), Err(DidIdError::UnknownNetwork(_))));
    }

    #[test]
    fn rejects_short_address() {
        let s = "did:midnight:preprod:dead";
        assert!(matches!(
            DidId::parse(s),
            Err(DidIdError::InvalidContractAddress(_))
        ));
    }

    #[test]
    fn rejects_non_hex_address() {
        let s = "did:midnight:preprod:zzzz567890abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        assert!(matches!(DidId::parse(s), Err(DidIdError::Hex(_))));
    }

    #[test]
    fn serde_round_trip() {
        let id = sample();
        let json = serde_json::to_string(&id).unwrap();
        // Should serialise as the bare did-string, not an object.
        assert_eq!(
            json,
            "\"did:midnight:preprod:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\""
        );
        let back: DidId = serde_json::from_str(&json).unwrap();
        assert_eq!(back, id);
    }

    #[test]
    fn bech32m_alias_returns_clear_phase2_error() {
        // Phase 2 will populate this; for now, surface a clear
        // diagnostic so callers know it's intentional.
        assert!(matches!(
            DidId::parse("mn_did_test1qfoo"),
            Err(DidIdError::InvalidContractAddress(msg)) if msg.contains("bech32m")
        ));
    }
}
