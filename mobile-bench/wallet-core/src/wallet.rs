use rand::{RngCore, SeedableRng, rngs::OsRng};
use rand_chacha::ChaCha20Rng;
use serialize::Serializable;
use zswap::keys::{Seed, SecretKeys};

use crate::network::Network;

/// Stable hardcoded seed used by [`Wallet::demo`] for non-Undeployed
/// networks so the dev UI shows the *same* coin/encryption public
/// keys across launches. **Not a real wallet**: the bytes are
/// publicly committed, so anything funded against these keys is
/// everyone's money. The gsd-wallet-style W0–W3 genesis seeds for
/// other localnet flavours land in iter-2.
pub const DEMO_SEED_HEX: &str =
    "88b9e1f2a2bf22ec7e739e6d43abc16f593ebdc1460568cb16a7730700bda13c";

/// Pre-funded genesis seed for the standalone (`Undeployed`) Midnight
/// stack. Mirrors `GENESIS_MINT_WALLET_SEED` from
/// `midnightntwrk/example-counter/counter-cli/src/cli.ts` and the
/// upstream identity-examples standalone environments. The dev
/// chainspec (`CFG_PRESET=dev`) mints both NIGHT and DUST to this
/// wallet at genesis, so [`Wallet::demo`] auto-loads it whenever the
/// active network is [`crate::Network::Undeployed`] — that's the
/// only way to drive `Wallet::create_did` etc. against the local
/// stack without a manual top-up.
pub const UNDEPLOYED_GENESIS_SEED_HEX: &str =
    "0000000000000000000000000000000000000000000000000000000000000001";

#[derive(Debug, thiserror::Error)]
pub enum WalletError {
    #[error("invalid seed length: expected 32 bytes, got {0}")]
    InvalidSeedLen(usize),
    #[error("hex decode: {0}")]
    Hex(#[from] hex::FromHexError),
    #[error("serialize: {0}")]
    Serialize(#[from] std::io::Error),
    #[error("address: {0}")]
    Address(String),
}

/// A bare wallet — seed + derived keys + which network it talks to.
/// Iter-1 step-1 has no balance, no UTXOs, no sync; that's iter-2.
pub struct Wallet {
    network: Network,
    keys: SecretKeys,
    seed_bytes: [u8; 32],
}

impl Wallet {
    /// Build from a raw 32-byte seed. Mirrors gsd-wallet's seed semantics
    /// (the seed *is* the wallet identity; no BIP39 yet).
    pub fn from_seed(seed: [u8; 32], network: Network) -> Self {
        let keys = SecretKeys::from(Seed::from(seed));
        Self { network, keys, seed_bytes: seed }
    }

    /// Demo wallet — uses [`UNDEPLOYED_GENESIS_SEED_HEX`] when the
    /// network is [`Network::Undeployed`] (so the wallet starts with
    /// real NIGHT + DUST balances on the local standalone stack),
    /// and the public-knowledge [`DEMO_SEED_HEX`] for every other
    /// network (where there's no funding implication). Both are
    /// stable across launches so the dev UI shows the same public
    /// keys.
    pub fn demo(network: Network) -> Self {
        let seed = if network == Network::Undeployed {
            UNDEPLOYED_GENESIS_SEED_HEX
        } else {
            DEMO_SEED_HEX
        };
        Self::from_seed_hex(seed, network)
            .expect("seed constants are 32-byte hex literals")
    }

    /// Generate a fresh wallet from `OsRng`. Seed is also returned via
    /// [`Self::seed_hex`] so the caller can persist it.
    pub fn new_random(network: Network) -> Self {
        let mut seed = [0u8; 32];
        OsRng.fill_bytes(&mut seed);
        Self::from_seed(seed, network)
    }

    /// Deterministic constructor — used in tests and to expose
    /// gsd-wallet's W0–W3 genesis seeds when we add the localnet quick
    /// start in iter-2.
    pub fn from_chacha_seed(rng_seed: u64, network: Network) -> Self {
        let mut rng = ChaCha20Rng::seed_from_u64(rng_seed);
        let mut seed = [0u8; 32];
        rng.fill_bytes(&mut seed);
        Self::from_seed(seed, network)
    }

    pub fn from_seed_hex(seed_hex: &str, network: Network) -> Result<Self, WalletError> {
        let bytes = hex::decode(seed_hex)?;
        let arr: [u8; 32] = bytes
            .try_into()
            .map_err(|v: Vec<u8>| WalletError::InvalidSeedLen(v.len()))?;
        Ok(Self::from_seed(arr, network))
    }

    pub fn network(&self) -> Network {
        self.network
    }

    pub fn seed_hex(&self) -> String {
        hex::encode(self.seed_bytes)
    }

    /// Hex-encoded coin public key. Useful as an "address-ish" display
    /// while we don't yet wire up Midnight's bech32 address format.
    pub fn coin_public_key_hex(&self) -> Result<String, WalletError> {
        let mut buf = Vec::new();
        let pk = self.keys.coin_public_key();
        Serializable::serialize(&pk, &mut buf)?;
        Ok(hex::encode(buf))
    }

    /// Hex-encoded encryption public key (used by ZSwap to encrypt coin
    /// info to this wallet).
    pub fn encryption_public_key_hex(&self) -> Result<String, WalletError> {
        let mut buf = Vec::new();
        let pk = self.keys.enc_public_key();
        Serializable::serialize(&pk, &mut buf)?;
        Ok(hex::encode(buf))
    }

    /// Bech32m-encoded unshielded NIGHT receive address for this
    /// wallet on the chosen network. This is the string the user
    /// pastes into a faucet to top the wallet up.
    pub fn unshielded_address(&self) -> Result<String, WalletError> {
        crate::address::unshielded_bech32m(&self.seed_bytes, self.network)
            .map_err(|e| WalletError::Address(e.to_string()))
    }

    /// 32-byte raw seed. Returned by-copy to keep callers honest
    /// about it being secret material. `cfg(test)` because it's
    /// only referenced from in-crate tests today; remove the gate
    /// when external callers need it.
    #[cfg(test)]
    pub(crate) fn seed_bytes(&self) -> [u8; 32] {
        self.seed_bytes
    }

    /// Substrate tx-envelope signer. Derives the same secp256k1
    /// secret scalar the DID controller uses (BIP32 path
    /// `m/44'/2400'/0'/0/0`, role `NightExternal`) and exposes it
    /// as an ECDSA-capable signer for `MultiSignature::Ecdsa(_)`.
    /// One key, two signature schemes — see
    /// `wallet_core::node::signer` module-doc.
    pub fn midnight_signer(&self) -> Result<crate::MidnightSigner, WalletError> {
        let secret = crate::hd::derive_child_priv(
            &self.seed_bytes,
            0,
            crate::hd::Role::NightExternal,
            0,
        )
        .map_err(|e| WalletError::Address(e.to_string()))?;
        crate::MidnightSigner::from_secret_bytes(&secret)
            .map_err(|e| WalletError::Address(e.to_string()))
    }

    /// Derive the controller signing key for DID operations.
    ///
    /// Mirrors `midnight-did-api`'s
    /// `HDWallet.fromSeed(seed).selectAccount(0).selectRole(Roles.NightExternal).deriveKeyAt(0)`.
    /// The 32-byte child secret seeds a BIP340 schnorr signing key
    /// whose verifying key (after SHA-256) becomes the
    /// `controllerPublicKey` in the on-chain DID state.
    pub fn did_controller_signing_key(
        &self,
    ) -> Result<base_crypto::signatures::SigningKey, WalletError> {
        let child = crate::hd::derive_child_priv(
            &self.seed_bytes,
            0,
            crate::hd::Role::NightExternal,
            0,
        )
        .map_err(|e| WalletError::Address(e.to_string()))?;
        base_crypto::signatures::SigningKey::from_bytes(&child)
            .map_err(|e| WalletError::Address(format!("signing key: {e}")))
    }

    /// 32-byte commitment that the on-chain DID contract stores as
    /// `controllerPublicKey`. **Not** a curve-derived public key —
    /// the `publicKey` circuit in `did.compact` defines it as:
    ///
    /// ```compact
    /// publicKey(sk) = persistentHash(["did:controller:pk" + pad32, sk]);
    /// ```
    ///
    /// i.e. `SHA-256("did:controller:pk" + 14 zero bytes || sk)`.
    /// This is a domain-separated commitment to the secret key,
    /// used by the contract to authorize the controller without
    /// revealing the key. The wallet computes the same value off-
    /// chain so it can pre-compute the deploy address before
    /// sending the deploy intent.
    pub fn did_controller_public_key(&self) -> Result<[u8; 32], WalletError> {
        use sha2::{Digest, Sha256};
        // The same secret scalar used for BIP340 schnorr signing
        // (BIP32 path m/44'/2400'/0'/0/0). The contract's
        // `localSecretKey()` witness must return these same bytes
        // at deploy time.
        let secret = crate::hd::derive_child_priv(
            &self.seed_bytes,
            0,
            crate::hd::Role::NightExternal,
            0,
        )
        .map_err(|e| WalletError::Address(e.to_string()))?;

        // Compact's `pad(32, "did:controller:pk")` = the ASCII
        // bytes followed by NULs out to 32 bytes (left-justified
        // pad — confirmed against `did.compact` Vector<2, Bytes<32>>
        // serialization).
        let mut domain = [0u8; 32];
        let tag = b"did:controller:pk";
        domain[..tag.len()].copy_from_slice(tag);

        let mut hasher = Sha256::new();
        hasher.update(domain);
        hasher.update(secret);
        Ok(hasher.finalize().into())
    }

    /// **Phase 3 stub** — returns
    /// [`crate::DidError::WriteNotImplemented`] today. Logs the
    /// derived `controllerPublicKey` so we can verify the key
    /// derivation visually before the deploy path lands.
    ///
    /// When fully wired, `create_did` will:
    /// 1. Derive the controller signing key
    ///    (`did_controller_signing_key`).
    /// 2. Build the constructor input with that
    ///    `controllerPublicKey`.
    /// 3. Submit the deploy extrinsic via `subxt` +
    ///    `midnight-node-metadata` (git dep, to be added).
    /// 4. Watch the indexer for the new contract's first
    ///    `ContractDeploy` action; return its address as a
    ///    [`crate::DidId`].
    /// 5. Optionally call `addVerificationMethod` to write the
    ///    initial VM (artifacts vendored at
    ///    `contracts/midnight-did/addVerificationMethod.*`).
    pub async fn create_did(&self) -> Result<crate::DidId, crate::DidError> {
        let pk = self
            .did_controller_public_key()
            .map_err(|e| crate::DidError::Indexer(e.to_string()))?;
        tracing::info!(
            controller_pk_hex = %hex::encode(pk),
            network = ?self.network,
            "create_did stub — would deploy DID contract with this controller"
        );
        Err(crate::DidError::WriteNotImplemented)
    }

    /// Resolve a Midnight DID to a [`crate::DidDocument`] by querying
    /// the indexer for the contract's current state and decoding it.
    ///
    /// **Phase 2a**: fetches the latest contract action from the
    /// indexer; full state decoding into a populated `DidDocument`
    /// lands in 2b. For now we return a placeholder document with
    /// the DID's id + the on-chain block height where the contract
    /// was last seen. If the address is unknown to the indexer,
    /// returns [`crate::DidError::Indexer`] with a clear message.
    pub async fn resolve_did(
        &self,
        did: &str,
    ) -> Result<crate::DidDocument, crate::DidError> {
        let id = crate::DidId::parse(did)?;
        if id.network != self.network {
            return Err(crate::DidError::Indexer(format!(
                "DID network {:?} does not match wallet network {:?}",
                id.network, self.network
            )));
        }

        let client = crate::IndexerClient::new(self.network)
            .map_err(|e| crate::DidError::Indexer(e.to_string()))?;
        let info = client
            .contract_state(&id.contract_address_hex())
            .await
            .map_err(|e| crate::DidError::Indexer(e.to_string()))?
            .ok_or_else(|| {
                crate::DidError::Indexer(format!(
                    "no contract action for address {} on {}",
                    id.contract_address_hex(),
                    self.network.label()
                ))
            })?;

        // Decode the on-chain state into a typed `DidLedgerState`,
        // then map it to a domain `DidDocument`. Phase 2b populates
        // the scalar fields (version, dates, deactivated); Phase 2c
        // walks the maps for VMs / services / relations.
        let ledger_state =
            crate::did::contract::decode_did_ledger_state(&info.state_hex)?;
        Ok(crate::did::contract::ledger_to_domain(&ledger_state, id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_seed_yields_stable_keys() {
        let w1 = Wallet::from_chacha_seed(0xdeadbeef, Network::PreProd);
        let w2 = Wallet::from_chacha_seed(0xdeadbeef, Network::PreProd);
        assert_eq!(w1.seed_hex(), w2.seed_hex());
        assert_eq!(
            w1.coin_public_key_hex().unwrap(),
            w2.coin_public_key_hex().unwrap()
        );
        assert_eq!(
            w1.encryption_public_key_hex().unwrap(),
            w2.encryption_public_key_hex().unwrap()
        );
    }

    #[test]
    fn seed_hex_roundtrip() {
        let w = Wallet::from_chacha_seed(7, Network::PreProd);
        let hex = w.seed_hex();
        let back = Wallet::from_seed_hex(&hex, Network::PreProd).unwrap();
        assert_eq!(w.coin_public_key_hex().unwrap(), back.coin_public_key_hex().unwrap());
    }

    #[test]
    fn invalid_seed_hex_rejected() {
        let result = Wallet::from_seed_hex("ab", Network::PreProd);
        assert!(matches!(result, Err(WalletError::InvalidSeedLen(_))));
    }

    #[test]
    fn demo_seed_is_well_formed_and_stable() {
        let w1 = Wallet::demo(Network::PreProd);
        let w2 = Wallet::demo(Network::PreProd);
        assert_eq!(w1.seed_hex(), DEMO_SEED_HEX);
        assert_eq!(
            w1.coin_public_key_hex().unwrap(),
            w2.coin_public_key_hex().unwrap()
        );
    }

    #[test]
    fn undeployed_demo_uses_genesis_seed() {
        let w = Wallet::demo(Network::Undeployed);
        assert_eq!(w.seed_hex(), UNDEPLOYED_GENESIS_SEED_HEX);
    }

    #[test]
    fn demo_seed_differs_per_network_class() {
        let pre = Wallet::demo(Network::PreProd);
        let und = Wallet::demo(Network::Undeployed);
        assert_ne!(pre.seed_hex(), und.seed_hex());
    }
}
