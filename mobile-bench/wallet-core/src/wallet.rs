use rand::{RngCore, SeedableRng, rngs::OsRng};
use rand_chacha::ChaCha20Rng;
use serialize::Serializable;
use zswap::keys::{Seed, SecretKeys};

use crate::js_bridge::JsBridge;
use crate::network::Network;

/// Shape of the `prepareUnprovenCallTx` harness response. Lives
/// here (rather than on a public type) because `call_did_circuit`
/// is the only consumer.
#[derive(serde::Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct PrepareUnprovenCallTxResult {
    unproven_tx_hex: String,
    #[allow(dead_code)] // surfaced for diagnostics
    elapsed_ms: i64,
}

/// Drive the harness's `prepareUnprovenCallTx` and return the hex
/// blob. Pulled out so `call_did_circuit` reads top-down without
/// the JSON-ferrying noise.
#[allow(clippy::too_many_arguments)]
async fn call_prepare_unproven(
    bridge: &crate::js_bridge::NodeChildBridge,
    did: String,
    circuit: String,
    circuit_args: serde_json::Value,
    contract_state_hex: String,
    contract_address_hex: String,
    controller_sk: [u8; 32],
    coin_public_key_hex: String,
    encryption_public_key_hex: String,
    network_id: String,
) -> Result<String, crate::js_bridge::JsBridgeError> {
    let r: PrepareUnprovenCallTxResult = bridge
        .call(
            "prepareUnprovenCallTx",
            serde_json::json!({
                "did": did,
                "circuit": circuit,
                "circuitArgs": circuit_args,
                "contractStateHex": contract_state_hex,
                "contractAddressHex": contract_address_hex,
                "zswapChainStateHex": serde_json::Value::Null,
                "ledgerParametersHex": serde_json::Value::Null,
                "controllerSecretHex": hex::encode(controller_sk),
                "coinPublicKeyHex": coin_public_key_hex,
                "encryptionPublicKeyHex": encryption_public_key_hex,
                "networkId": network_id,
            }),
        )
        .await?;
    Ok(r.unproven_tx_hex)
}

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

    /// Snapshot the unshielded UTXO set for this wallet's default
    /// address. See `docs/superpowers/specs/2026-05-14-unshielded-sync-design.md`.
    /// Opens a fresh `graphql-transport-ws` WebSocket to the
    /// indexer, replays UTXO create/spend events, terminates on
    /// the first `Progress` event, and returns the live set.
    pub async fn sync_unshielded(&self) -> Result<crate::UtxoSet, crate::UnshieldedError> {
        let address = self
            .unshielded_address()
            .map_err(|e| crate::UnshieldedError::InvalidAddress(e.to_string()))?;
        let cfg = self.network.config();
        crate::unshielded::snapshot::snapshot(cfg.indexer_ws_url, &address).await
    }

    /// Snapshot the wallet's DUST state by replaying the
    /// indexer's `dustLedgerEvents` stream into a fresh
    /// `DustLocalState`. The returned state is consumed by
    /// `tx::balance` to cover deploy/call fees.
    pub async fn sync_dust(
        &self,
    ) -> Result<crate::DustLocalState<storage::DefaultDB>, crate::DustError> {
        let sk = self
            .dust_secret_key()
            .map_err(|e| crate::DustError::InvalidPublicKey(e.to_string()))?;
        let cfg = self.network.config();
        let params = ledger::structure::INITIAL_PARAMETERS.dust;
        crate::dust::snapshot::snapshot(cfg.indexer_ws_url, &sk, params).await
    }

    /// Derive the DUST secret key for this wallet.
    ///
    /// The seed feeding `ledger::dust::DustSecretKey::derive_secret_key`
    /// is the BIP44 child at `m/44'/2400'/0'/2/0` (account 0, role
    /// Dust, index 0) — same path the upstream wallet SDKs use.
    pub fn dust_secret_key(&self) -> Result<ledger::dust::DustSecretKey, WalletError> {
        let child = crate::hd::derive_child_priv(&self.seed_bytes, 0, crate::hd::Role::Dust, 0)
            .map_err(|e| WalletError::Address(format!("hd: {e}")))?;
        Ok(ledger::dust::DustSecretKey::derive_secret_key(&child))
    }

    /// Hex-encoded 32-byte DUST public key (little-endian Fr bytes).
    /// Ready to feed as a `HexEncoded` indexer scalar for any
    /// future address-keyed DUST queries — `dustLedgerEvents`
    /// itself is global and doesn't need this, but it's the
    /// natural display form too.
    pub fn dust_public_key_hex(&self) -> Result<String, WalletError> {
        let sk = self.dust_secret_key()?;
        let pk = ledger::dust::DustPublicKey::from(sk);
        Ok(hex::encode(pk.0.as_le_bytes()))
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

    /// 32-byte commitment the on-chain DID contract stores as
    /// `controllerPublicKey`. The `publicKey` circuit in `did.compact`
    /// is:
    ///
    /// ```compact
    /// publicKey(sk) = persistentHash(["did:controller:pk" + pad32, sk]);
    /// ```
    ///
    /// i.e. `SHA-256("did:controller:pk" + 15 zero bytes || sk)`.
    /// Domain-separated commitment to the secret — the contract
    /// authorises the controller via this pk without learning sk.
    ///
    /// Each DID gets its own random sk (see [`Wallet::create_did`]);
    /// without that sk the wallet cannot update or deactivate the
    /// DID. Mirror in JS: `DIDContract.pureCircuits.publicKey(sk)`.
    pub fn controller_public_key_for(secret: &[u8; 32]) -> [u8; 32] {
        use sha2::{Digest, Sha256};
        let mut domain = [0u8; 32];
        let tag = b"did:controller:pk";
        domain[..tag.len()].copy_from_slice(tag);
        let mut hasher = Sha256::new();
        hasher.update(domain);
        hasher.update(secret);
        hasher.finalize().into()
    }

    /// Compose what the new DID's id *would be* if we deployed
    /// right now, without actually submitting anything. The full
    /// `ContractDeploy` payload is assembled from the wallet's
    /// controller commitment + a current-time `created`/`updated`
    /// stamp + a freshly-sampled 32-byte nonce; the resulting
    /// `deploy.address()` is wrapped as a [`crate::DidId`] on the
    /// wallet's network.
    ///
    /// Useful before submission to (a) verify our state composition
    /// is bit-for-bit what the chain would accept, and (b) show the
    /// would-be DID in the UI so the user knows what address they
    /// would control.
    /// Compose what the new DID's id *would be* given a caller-
    /// supplied pk-commitment, timestamp, and nonce. Each DID gets
    /// a fresh random `controller_sk`, so a wallet-level "what's my
    /// next DID" preview is no longer meaningful — pass the sk in
    /// via `pk_commitment = Wallet::controller_public_key_for(&sk)`
    /// to see the address before submission.
    pub fn create_did_preview_with(
        &self,
        pk_commitment: [u8; 32],
        timestamp_ms: u64,
        nonce: [u8; 32],
    ) -> Result<crate::DidId, crate::DidError> {
        let committee = vec![self
            .did_maintenance_verifying_key()
            .map_err(|e| crate::DidError::Indexer(e.to_string()))?];
        let deploy =
            crate::did::deploy::compose_deploy(pk_commitment, timestamp_ms, nonce, committee);
        let bytes: crate::ContractAddressBytes = deploy.address().0.0;
        Ok(crate::DidId::new(self.network, bytes))
    }

    /// BIP340-Schnorr signing key for the wallet's DID maintenance
    /// authority. Used to:
    ///   - populate the deploy's `ContractMaintenanceAuthority.committee`
    ///     (via [`did_maintenance_verifying_key`])
    ///   - sign `MaintenanceUpdate::data_to_sign()` when loading the
    ///     11 DID circuits' verifier keys post-deploy.
    /// Derived from the BIP-44 child at `m/44'/2400'/0'/0/0` —
    /// the same `NightExternal` path the unshielded address uses, so
    /// `unshielded_address` and the maintenance authority are bound
    /// to the same secret.
    pub fn did_maintenance_signing_key(
        &self,
    ) -> Result<base_crypto::signatures::SigningKey, WalletError> {
        let child = crate::hd::derive_child_priv(
            &self.seed_bytes,
            0,
            crate::hd::Role::NightExternal,
            0,
        )
        .map_err(|e| WalletError::Address(format!("hd: {e}")))?;
        base_crypto::signatures::SigningKey::from_bytes(&child)
            .map_err(|e| WalletError::Address(format!("schnorr sk: {e}")))
    }

    /// BIP340-Schnorr verifying key that goes into the contract's
    /// `ContractMaintenanceAuthority.committee` slot at deploy time.
    pub fn did_maintenance_verifying_key(
        &self,
    ) -> Result<base_crypto::signatures::VerifyingKey, WalletError> {
        Ok(self.did_maintenance_signing_key()?.verifying_key())
    }

    /// Submit a real deploy of the DID contract. Returns a
    /// `Stream<WizardStage>` so the UI renders progress.
    /// See docs/superpowers/specs/2026-05-15-did-deploy-submit-design.md.
    pub fn create_did(
        &self,
    ) -> impl futures::Stream<Item = crate::WizardStage> + Send + 'static {
        let network = self.network;
        let seed_bytes = self.seed_bytes;
        async_stream::stream! {
            // 1. SyncingDust
            yield crate::WizardStage::SyncingDust;
            let wallet = Wallet::from_seed(seed_bytes, network);
            let mut dust_state = match wallet.sync_dust().await {
                Ok(s) => s,
                Err(e) => { yield crate::WizardStage::Failed(format!("sync dust: {e}")); return; }
            };
            let dust_key = match wallet.dust_secret_key() {
                Ok(k) => k,
                Err(e) => { yield crate::WizardStage::Failed(format!("dust key: {e}")); return; }
            };

            // 2. Composing
            yield crate::WizardStage::Composing;
            // StdRng (not ThreadRng) so the returned Stream is Send.
            let mut rng = <rand::rngs::StdRng as rand::SeedableRng>::from_entropy();
            // Generate a fresh random controller secret per DID. The
            // wallet must persist this — without it we cannot supply
            // the `localSecretKey()` witness for any subsequent
            // update / deactivate circuit call on this DID.
            let controller_sk: [u8; 32] = rand::Rng::r#gen(&mut rng);
            let pk_commitment = Wallet::controller_public_key_for(&controller_sk);
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            let ttl = base_crypto::time::Timestamp::from_secs(now_ms / 1000 + 3600);
            tracing::debug!(
                now_s = now_ms / 1000,
                ttl_s = now_ms / 1000 + 3600,
                "create_did: intent ttl",
            );
            let nonce: [u8; 32] = rand::Rng::r#gen(&mut rng);
            let net_id = network.config().network_id;
            let maintenance_vk = match wallet.did_maintenance_verifying_key() {
                Ok(vk) => vk,
                Err(e) => {
                    yield crate::WizardStage::Failed(format!("maintenance key: {e}"));
                    return;
                }
            };
            let unproven = match crate::tx::build::build_deploy(
                pk_commitment,
                net_id,
                now_ms,
                nonce,
                ttl,
                &mut rng,
                vec![maintenance_vk],
            ) {
                Ok(t) => t,
                Err(e) => { yield crate::WizardStage::Failed(format!("compose: {e}")); return; }
            };

            // Preview DID id from the exact inputs we just used.
            let preview_id = match wallet.create_did_preview_with(pk_commitment, now_ms, nonce) {
                Ok(id) => id,
                Err(e) => { yield crate::WizardStage::Failed(format!("preview id: {e}")); return; }
            };

            // 3. Balancing
            yield crate::WizardStage::Balancing;
            let params = ledger::structure::INITIAL_PARAMETERS;
            // Pick a `ctime` the verifier will accept and that
            // matches our local commitment_tree.root().
            //
            // The verifier's `commitment_root` is looked up via
            // `root_history.get(ctime)` (exact match else predecessor;
            // entries inserted on every `post_block_update`). The
            // value must equal the root our local tree holds.
            //
            // Two constraints stacked:
            // 1. Validity window: `tblock - 3h ≤ ctime ≤ tblock` (the
            //    chain's `OutOfDustValidityWindow` check).
            // 2. Root match: between the most-recent block whose
            //    `root_history` we agree with and `ctime`, no
            //    dust events advanced the tree.
            //
            // On a wallet whose `sync_time` is recent (every block
            // generates a fresh DUST event for the holder),
            // `sync_time` itself satisfies both. After a chain
            // reset or for a wallet with only genesis allocation,
            // `sync_time` is stuck at block 0's tblock — way
            // outside the 3-hour window. Falling back to the
            // chain tip's tblock keeps us inside the window; the
            // root match holds because no dust events have
            // advanced our local tree (single-tenant standalone).
            let chain_tip_secs: u64 = match crate::IndexerClient::new(network) {
                Ok(c) => match c.chain_tip().await {
                    Ok(Some(t)) => (t.timestamp_unix as u64) / 1000,
                    _ => 0,
                },
                Err(_) => 0,
            };
            let ctime = if chain_tip_secs > dust_state.sync_time.to_secs() as u64 {
                base_crypto::time::Timestamp::from_secs(chain_tip_secs)
            } else {
                dust_state.sync_time
            };
            let mut ctx = crate::tx::balance::BalanceCtx {
                dust_state: &mut dust_state,
                dust_key: &dust_key,
                params: &params,
                time: ctime,
                ttl,
                network_id: net_id,
            };
            let balanced = match crate::tx::balance::balance(unproven, &mut ctx) {
                Ok(b) => b,
                Err(e) => { yield crate::WizardStage::Failed(format!("balance: {e}")); return; }
            };

            // 4. Proving
            yield crate::WizardStage::Proving;
            let prove_rng = <rand::rngs::StdRng as rand::SeedableRng>::from_entropy();
            let proven = match crate::tx::prove::prove(balanced, prove_rng).await {
                Ok(p) => p,
                Err(e) => { yield crate::WizardStage::Failed(format!("prove: {e}")); return; }
            };

            // 5. Submitting
            yield crate::WizardStage::Submitting;
            let bytes = match crate::tx::scale::scale_encode(&proven) {
                Ok(b) => b,
                Err(e) => { yield crate::WizardStage::Failed(format!("encode: {e}")); return; }
            };
            let signer = match wallet.midnight_signer() {
                Ok(s) => s,
                Err(e) => { yield crate::WizardStage::Failed(format!("signer: {e}")); return; }
            };
            let node = match crate::NodeClient::connect(network).await {
                Ok(n) => n,
                Err(e) => { yield crate::WizardStage::Failed(format!("node connect: {e}")); return; }
            };

            // 6. Confirming
            yield crate::WizardStage::Confirming;
            let submit = match node.submit_deploy(bytes, &signer).await {
                Ok(r) => r,
                Err(e) => { yield crate::WizardStage::Failed(format!("submit: {e}")); return; }
            };

            yield crate::WizardStage::Done(crate::DeployOutcome {
                did_id: preview_id,
                tx_hash: submit.tx_hash,
                block_hash: submit.block_hash,
                controller_sk,
            });
        }
    }

    /// Submit a MaintenanceUpdate that loads a single circuit's
    /// verifier key into a freshly-deployed DID contract. Reuses
    /// the same WizardStage pipeline as `create_did` — the Done
    /// outcome carries the maintenance-tx hash + block hash; the
    /// DID id is unchanged.
    ///
    /// `counter` is the maintenance-authority's counter value at
    /// the time of submission. For the first MaintenanceUpdate
    /// against a contract that was just deployed, this is 0;
    /// every successful update bumps it.
    ///
    /// `circuit` selects which bundled artifact to load. Today
    /// only `"addVerificationMethod"` is bundled — the other 10
    /// follow in a subsequent slice once their artifacts land.
    pub fn load_did_circuit(
        &self,
        did_id: crate::DidId,
        circuit_name: String,
        counter: u32,
    ) -> impl futures::Stream<Item = crate::WizardStage> + Send + 'static {
        use coin_structure::contract::ContractAddress;
        use base_crypto::hash::HashOutput;

        let network = self.network;
        let seed_bytes = self.seed_bytes;
        async_stream::stream! {
            // 1. SyncingDust
            yield crate::WizardStage::SyncingDust;
            let wallet = Wallet::from_seed(seed_bytes, network);
            let mut dust_state = match wallet.sync_dust().await {
                Ok(s) => s,
                Err(e) => { yield crate::WizardStage::Failed(format!("sync dust: {e}")); return; }
            };
            let dust_key = match wallet.dust_secret_key() {
                Ok(k) => k,
                Err(e) => { yield crate::WizardStage::Failed(format!("dust key: {e}")); return; }
            };

            // 2. Composing
            yield crate::WizardStage::Composing;
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            let ttl = base_crypto::time::Timestamp::from_secs(now_ms / 1000 + 3600);
            let mut rng = <rand::rngs::StdRng as rand::SeedableRng>::from_entropy();
            let net_id = network.config().network_id;

            let sk = match wallet.did_maintenance_signing_key() {
                Ok(k) => k,
                Err(e) => {
                    yield crate::WizardStage::Failed(format!("maintenance key: {e}"));
                    return;
                }
            };

            // Resolve the verifier key for this circuit from the bundled
            // 11-entry registry. Any name outside that set is a config
            // error — surface it before kicking off the network round trip.
            let vk = match crate::did::artifacts::parsed_verifier_key_by_name(&circuit_name) {
                Some(Ok(v)) => v,
                Some(Err(e)) => {
                    yield crate::WizardStage::Failed(format!("parse verifier key: {e}"));
                    return;
                }
                None => {
                    yield crate::WizardStage::Failed(format!(
                        "unknown circuit '{circuit_name}': not in the DID artifact registry",
                    ));
                    return;
                }
            };

            let contract_address = ContractAddress(HashOutput(did_id.contract_address));
            let unproven = match crate::tx::maintain::build_load_verifier_key(
                contract_address,
                &circuit_name,
                vk,
                counter,
                &sk,
                net_id,
                ttl,
                &mut rng,
            ) {
                Ok(t) => t,
                Err(e) => { yield crate::WizardStage::Failed(format!("compose: {e}")); return; }
            };

            // 3. Balancing
            yield crate::WizardStage::Balancing;
            let params = ledger::structure::INITIAL_PARAMETERS;
            // See `create_did` for the full rationale on ctime
            // selection (chain-tip vs sync_time).
            let chain_tip_secs: u64 = match crate::IndexerClient::new(network) {
                Ok(c) => match c.chain_tip().await {
                    Ok(Some(t)) => (t.timestamp_unix as u64) / 1000,
                    _ => 0,
                },
                Err(_) => 0,
            };
            let ctime = if chain_tip_secs > dust_state.sync_time.to_secs() as u64 {
                base_crypto::time::Timestamp::from_secs(chain_tip_secs)
            } else {
                dust_state.sync_time
            };
            let mut ctx = crate::tx::balance::BalanceCtx {
                dust_state: &mut dust_state,
                dust_key: &dust_key,
                params: &params,
                time: ctime,
                ttl,
                network_id: net_id,
            };
            let balanced = match crate::tx::balance::balance(unproven, &mut ctx) {
                Ok(b) => b,
                Err(e) => { yield crate::WizardStage::Failed(format!("balance: {e}")); return; }
            };

            // 4. Proving
            yield crate::WizardStage::Proving;
            let prove_rng = <rand::rngs::StdRng as rand::SeedableRng>::from_entropy();
            let proven = match crate::tx::prove::prove(balanced, prove_rng).await {
                Ok(p) => p,
                Err(e) => { yield crate::WizardStage::Failed(format!("prove: {e}")); return; }
            };

            // 5. Submitting
            yield crate::WizardStage::Submitting;
            let bytes = match crate::tx::scale::scale_encode(&proven) {
                Ok(b) => b,
                Err(e) => { yield crate::WizardStage::Failed(format!("encode: {e}")); return; }
            };
            let signer = match wallet.midnight_signer() {
                Ok(s) => s,
                Err(e) => { yield crate::WizardStage::Failed(format!("signer: {e}")); return; }
            };
            let node = match crate::NodeClient::connect(network).await {
                Ok(n) => n,
                Err(e) => { yield crate::WizardStage::Failed(format!("node connect: {e}")); return; }
            };

            // 6. Confirming
            yield crate::WizardStage::Confirming;
            let submit = match node.submit_deploy(bytes, &signer).await {
                Ok(r) => r,
                Err(e) => { yield crate::WizardStage::Failed(format!("submit: {e}")); return; }
            };

            yield crate::WizardStage::Done(crate::DeployOutcome {
                did_id,
                tx_hash: submit.tx_hash,
                block_hash: submit.block_hash,
                // MaintenanceUpdate does not mint a DID; no fresh sk.
                controller_sk: [0u8; 32],
            });
        }
    }

    /// Invoke a DID circuit on a deployed contract — the *Update*
    /// / *Deactivate* half of CRUD. The Compact circuit body runs
    /// in a Node-driven JS harness (the production WebView path
    /// uses the same code via Dioxus eval); the resulting
    /// `UnprovenTransaction` is balanced, proven, and submitted
    /// natively in Rust through the existing pipeline.
    ///
    /// `controller_sk` is the per-DID random secret the wallet
    /// minted at deploy time (in [`DeployOutcome::controller_sk`]).
    /// Without it, the circuit's `localSecretKey()` witness can't
    /// be supplied and the call fails the contract's controller
    /// assertion.
    ///
    /// `args_json` is a JSON array of circuit arguments. Each is
    /// passed through unchanged to JS — bigints use the
    /// `{ "$bigint": "n" }` tagged form. `[]` for `deactivate`.
    pub fn call_did_circuit(
        &self,
        did_id: crate::DidId,
        circuit: String,
        args_json: serde_json::Value,
        controller_sk: [u8; 32],
    ) -> impl futures::Stream<Item = crate::WizardStage> + Send + 'static {
        use coin_structure::contract::ContractAddress;
        use base_crypto::hash::HashOutput;

        let network = self.network;
        let seed_bytes = self.seed_bytes;
        async_stream::stream! {
            yield crate::WizardStage::SyncingDust;
            let wallet = Wallet::from_seed(seed_bytes, network);
            let mut dust_state = match wallet.sync_dust().await {
                Ok(s) => s,
                Err(e) => { yield crate::WizardStage::Failed(format!("sync dust: {e}")); return; }
            };
            let dust_key = match wallet.dust_secret_key() {
                Ok(k) => k,
                Err(e) => { yield crate::WizardStage::Failed(format!("dust key: {e}")); return; }
            };

            // 2. Composing — the heavy lift: JS-side Compact runtime
            //    builds the UnprovenTransaction. Rust pulls current
            //    contract state from the indexer first.
            yield crate::WizardStage::Composing;
            let coin_pk_hex = match wallet.coin_public_key_hex() {
                Ok(s) => s,
                Err(e) => { yield crate::WizardStage::Failed(format!("coin pk: {e}")); return; }
            };
            let enc_pk_hex = match wallet.encryption_public_key_hex() {
                Ok(s) => s,
                Err(e) => { yield crate::WizardStage::Failed(format!("encryption pk: {e}")); return; }
            };
            let indexer = match crate::IndexerClient::new(network) {
                Ok(c) => c,
                Err(e) => { yield crate::WizardStage::Failed(format!("indexer client: {e}")); return; }
            };
            let addr_hex = did_id.contract_address_hex();
            let info = match indexer.contract_state(&addr_hex).await {
                Ok(Some(i)) => i,
                Ok(None) => {
                    yield crate::WizardStage::Failed(format!(
                        "no on-chain state for {addr_hex} — was the DID deployed?",
                    ));
                    return;
                }
                Err(e) => { yield crate::WizardStage::Failed(format!("indexer: {e}")); return; }
            };

            // Spawn the harness + ask it to build an UnprovenTransaction
            // for this circuit call.
            let bridge = match crate::js_bridge::NodeChildBridge::spawn(
                &crate::js_bridge::NodeChildBridge::default_harness_path(),
            ) {
                Ok(b) => b,
                Err(e) => { yield crate::WizardStage::Failed(format!("spawn harness: {e}")); return; }
            };
            let unproven_hex = match call_prepare_unproven(
                &bridge,
                did_id.to_did_string(),
                circuit.clone(),
                args_json,
                info.state_hex,
                addr_hex.clone(),
                controller_sk,
                coin_pk_hex,
                enc_pk_hex,
                network.config().network_id.to_string(),
            )
            .await
            {
                Ok(h) => h,
                Err(e) => { yield crate::WizardStage::Failed(format!("prepare call tx: {e}")); return; }
            };
            let unproven_bytes = match hex::decode(&unproven_hex) {
                Ok(b) => b,
                Err(e) => { yield crate::WizardStage::Failed(format!("hex decode: {e}")); return; }
            };
            let unproven: crate::tx::build::UnprovenTx =
                match serialize::tagged_deserialize(&unproven_bytes[..]) {
                    Ok(t) => t,
                    Err(e) => {
                        yield crate::WizardStage::Failed(format!(
                            "deserialise unproven tx: {e}"
                        ));
                        return;
                    }
                };

            // 3. Balancing — same dust pipeline our deploy uses.
            yield crate::WizardStage::Balancing;
            let params = ledger::structure::INITIAL_PARAMETERS;
            let now_secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let ttl = base_crypto::time::Timestamp::from_secs(now_secs + 3600);
            // See `create_did` for the full rationale on ctime
            // selection (chain-tip vs sync_time).
            let chain_tip_secs: u64 = match crate::IndexerClient::new(network) {
                Ok(c) => match c.chain_tip().await {
                    Ok(Some(t)) => (t.timestamp_unix as u64) / 1000,
                    _ => 0,
                },
                Err(_) => 0,
            };
            let ctime = if chain_tip_secs > dust_state.sync_time.to_secs() as u64 {
                base_crypto::time::Timestamp::from_secs(chain_tip_secs)
            } else {
                dust_state.sync_time
            };
            let mut ctx = crate::tx::balance::BalanceCtx {
                dust_state: &mut dust_state,
                dust_key: &dust_key,
                params: &params,
                time: ctime,
                ttl,
                network_id: network.config().network_id,
            };
            let balanced = match crate::tx::balance::balance(unproven, &mut ctx) {
                Ok(b) => b,
                Err(e) => { yield crate::WizardStage::Failed(format!("balance: {e}")); return; }
            };

            // 4. Proving
            yield crate::WizardStage::Proving;
            let prove_rng = <rand::rngs::StdRng as rand::SeedableRng>::from_entropy();
            let proven = match crate::tx::prove::prove(balanced, prove_rng).await {
                Ok(p) => p,
                Err(e) => { yield crate::WizardStage::Failed(format!("prove: {e}")); return; }
            };

            // 5. Submitting
            yield crate::WizardStage::Submitting;
            let scale_bytes = match crate::tx::scale::scale_encode(&proven) {
                Ok(b) => b,
                Err(e) => { yield crate::WizardStage::Failed(format!("encode: {e}")); return; }
            };
            let signer = match wallet.midnight_signer() {
                Ok(s) => s,
                Err(e) => { yield crate::WizardStage::Failed(format!("signer: {e}")); return; }
            };
            let node = match crate::NodeClient::connect(network).await {
                Ok(n) => n,
                Err(e) => { yield crate::WizardStage::Failed(format!("node connect: {e}")); return; }
            };

            // 6. Confirming
            yield crate::WizardStage::Confirming;
            let submit = match node.submit_deploy(scale_bytes, &signer).await {
                Ok(r) => r,
                Err(e) => { yield crate::WizardStage::Failed(format!("submit: {e}")); return; }
            };

            // Resolve placeholder ContractAddress to silence
            // unused-import on the trait if we ever drop the
            // call-site below. Keeps the import live for future
            // intra-call address checks.
            let _: ContractAddress = ContractAddress(HashOutput(did_id.contract_address));

            yield crate::WizardStage::Done(crate::DeployOutcome {
                did_id,
                tx_hash: submit.tx_hash,
                block_hash: submit.block_hash,
                // ContractCall doesn't mint a DID; no fresh sk.
                controller_sk: [0u8; 32],
            });
        }
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

    /// Like [`resolve_did`] but also returns the on-chain housekeeping
    /// (`maintenance_counter`, last tx + block) the LoadCircuitPanel
    /// needs in order to compose the next `MaintenanceUpdate` without
    /// asking the user to track the counter manually.
    pub async fn resolve_did_full(
        &self,
        did: &str,
    ) -> Result<crate::ResolvedDid, crate::DidError> {
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

        let ledger_state =
            crate::did::contract::decode_did_ledger_state(&info.state_hex)?;
        let maintenance_counter =
            crate::did::contract::decode_maintenance_counter(&info.state_hex)?;
        let document = crate::did::contract::ledger_to_domain(&ledger_state, id);
        Ok(crate::ResolvedDid {
            document,
            maintenance_counter,
            last_block_height: info.last_block_height,
            last_tx_hash: info.last_tx_hash,
        })
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

    #[test]
    fn dust_public_key_hex_is_deterministic_per_seed() {
        // DustSecretKey doesn't derive PartialEq, so we compare
        // via the public-key hex (which round-trips through the
        // canonical Fr → 32-LE-bytes path).
        let a = Wallet::demo(Network::Undeployed).dust_public_key_hex().unwrap();
        let b = Wallet::demo(Network::Undeployed).dust_public_key_hex().unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn dust_public_key_hex_is_64_chars() {
        let hex = Wallet::demo(Network::Undeployed).dust_public_key_hex().unwrap();
        assert_eq!(hex.len(), 64, "expected 32-byte hex, got {hex}");
        assert!(hex.chars().all(|c| c.is_ascii_hexdigit() && !c.is_uppercase()));
    }

    #[test]
    fn dust_public_key_differs_per_seed() {
        let pre = Wallet::demo(Network::PreProd).dust_public_key_hex().unwrap();
        let und = Wallet::demo(Network::Undeployed).dust_public_key_hex().unwrap();
        assert_ne!(pre, und);
    }
}
