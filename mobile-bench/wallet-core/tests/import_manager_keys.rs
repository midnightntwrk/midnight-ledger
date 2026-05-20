//! One-shot importer for keys exported from the
//! `midnight-did-manager` profile's `manager-secrets.json`.
//!
//! Workflow:
//!
//! 1. Run the Node helper to decrypt + extract raw 32-byte
//!    scalars from each key's PKCS8 / raw32 record:
//!
//!    ```text
//!    node mobile-bench/dioxus-wallet/scripts/dump-manager-keys.mjs
//!    ```
//!
//!    Writes `/tmp/manager-keys.json` (override via `OUT_FILE`).
//!
//! 2. Run this test to import them into the App's wallet store:
//!
//!    ```text
//!    WALLET_STORE_PASS=<passphrase> \
//!    cargo test -p wallet-core --test import_manager_keys \
//!      -- --ignored --nocapture
//!    ```
//!
//! Env overrides:
//! - `KEYS_JSON`        — input JSON (default `/tmp/manager-keys.json`)
//! - `WALLET_STORE_PATH` — wallet redb path
//!     (default `~/.midnight/wallet-prototype/wallet.redb`)
//! - `WALLET_STORE_PASS` — store passphrase. Required.
//! - `WALLET_NETWORK`   — `preprod` / `undeployed` / etc.
//!     (default `preprod`)
//!
//! Each key lands in the redb `KEYS` table under the active
//! wallet for the chosen network. The private bytes are wrapped
//! with the same `wrap_secret` envelope our `RedbSecretStore`
//! uses, so subsequent App launches see the rows transparently.
//!
//! Idempotent: re-running with the same JSON imports under fresh
//! UUIDs (the manager's `key_ref` UUIDs are NOT reused — the
//! `RedbSecretStore::import_key` API mints a fresh one per call).
//! Re-runs therefore create duplicates; clear via the App's
//! "Forget keys" affordance or by wiping the table if needed.

#![cfg(feature = "network-tests")]

use std::path::PathBuf;

use serde::Deserialize;
use wallet_core::Network;
use wallet_core::secret_storage::{
    ImportKeyInput, MidnightCurve, MidnightKeyType, SecretStorage,
};
use wallet_core::secret_storage::redb_secret_store::RedbSecretStore;
use wallet_core::store::WalletStore;

#[derive(Debug, Deserialize)]
struct ManagerKey {
    id: String,
    key_ref: String,
    kty: String,
    crv: String,
    private_key_hex: String,
}

fn parse_kty(s: &str) -> Option<MidnightKeyType> {
    match s {
        "OKP" => Some(MidnightKeyType::OKP),
        "EC" => Some(MidnightKeyType::EC),
        _ => None,
    }
}

fn parse_crv(s: &str) -> Option<MidnightCurve> {
    match s {
        "Ed25519" => Some(MidnightCurve::Ed25519),
        "Jubjub" => Some(MidnightCurve::Jubjub),
        "P-256" => Some(MidnightCurve::P256),
        _ => None,
    }
}

fn parse_network(s: &str) -> Option<Network> {
    match s.to_lowercase().as_str() {
        "preprod" => Some(Network::PreProd),
        "undeployed" => Some(Network::Undeployed),
        "mainnet" => Some(Network::Mainnet),
        "preview" => Some(Network::Preview),
        "qanet" => Some(Network::QaNet),
        "devnet" => Some(Network::DevNet),
        _ => None,
    }
}

fn default_store_path() -> PathBuf {
    let home =
        std::env::var_os("HOME").map(PathBuf::from).expect("HOME unset");
    home.join(".midnight").join("wallet-prototype").join("wallet.redb")
}

#[tokio::test]
#[ignore = "one-shot importer — run manually after dump-manager-keys.mjs"]
async fn import_manager_keys() {
    let pass = std::env::var("WALLET_STORE_PASS").expect(
        "WALLET_STORE_PASS not set — the App's wallet-store passphrase \
         is required to wrap imported secrets",
    );
    let keys_json = std::env::var("KEYS_JSON")
        .unwrap_or_else(|_| "/tmp/manager-keys.json".to_string());
    let store_path = std::env::var("WALLET_STORE_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| default_store_path());
    let network = std::env::var("WALLET_NETWORK")
        .ok()
        .as_deref()
        .and_then(parse_network)
        .unwrap_or(Network::PreProd);

    let json = std::fs::read_to_string(&keys_json)
        .unwrap_or_else(|e| panic!("read {keys_json}: {e}"));
    let keys: Vec<ManagerKey> = serde_json::from_str(&json)
        .unwrap_or_else(|e| panic!("parse {keys_json}: {e}"));
    println!("[input] {} keys from {keys_json}", keys.len());

    let store = WalletStore::open(&store_path, &pass)
        .unwrap_or_else(|e| panic!("open store {store_path:?}: {e}"));
    println!("[store] opened {store_path:?}");

    // The App's `find_or_create_wallet_for_network` is what
    // normally mints the row; we re-use the same lookup here.
    // First wallet whose meta's network matches → that's the
    // active one (single-wallet-per-network is the App's
    // current invariant).
    let target_tag: wallet_core::store::NetworkTag = network.into();
    let wallet_id = store
        .list_wallet_ids()
        .expect("list_wallet_ids")
        .into_iter()
        .find(|id| {
            store
                .wallet_meta(*id)
                .ok()
                .flatten()
                .map(|m| m.network == target_tag)
                .unwrap_or(false)
        })
        .unwrap_or_else(|| {
            panic!("no wallet found for {network:?} — unlock the App once first")
        });
    println!("[wallet] {wallet_id} on {network:?}");

    let mut secret_store = RedbSecretStore::new(store.clone(), wallet_id);
    let mut ok = 0usize;
    let mut skip = 0usize;
    for k in &keys {
        let (Some(kty), Some(crv)) = (parse_kty(&k.kty), parse_crv(&k.crv))
        else {
            println!("[skip] {} (kty={}, crv={})", k.id, k.kty, k.crv);
            skip += 1;
            continue;
        };
        let private_key = match hex::decode(&k.private_key_hex) {
            Ok(b) => b,
            Err(e) => {
                println!("[skip] {} hex decode: {e}", k.id);
                skip += 1;
                continue;
            }
        };
        let input = ImportKeyInput {
            id: k.id.clone(),
            private_key,
            kty,
            crv,
            did: None,
            purpose: None,
        };
        match secret_store.import_key(input).await {
            Ok((new_ref, _)) => {
                println!(
                    "[ok]   {:<22} {}/{}  new_ref={new_ref}  src_ref={}",
                    k.id, k.kty, k.crv, k.key_ref,
                );
                ok += 1;
            }
            Err(e) => {
                println!("[fail] {} ({}/{}): {e}", k.id, k.kty, k.crv);
                skip += 1;
            }
        }
    }
    println!(
        "[done] imported {ok}/{total} keys ({skip} skipped) into wallet {wallet_id}",
        total = keys.len(),
    );
}
