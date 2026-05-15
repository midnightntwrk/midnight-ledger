//! Step 6 smoke: the harness's `prepareUnprovenCallTx` produces a
//! SCALE-serialised `UnprovenTransaction` for a DID circuit call.
//! No chain involvement yet — purely "can JS build the tx?". Step
//! 7 (next commit) deserialises the bytes on Rust side and feeds
//! them through the existing balance/prove/submit pipeline.
//!
//! Run with:
//!   cargo test -p wallet-core --test js_prepare_call_tx -- --nocapture

use wallet_core::js_bridge::{JsBridge, NodeChildBridge};
use wallet_core::{Network, Wallet};

#[derive(serde::Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct PrepareResult {
    circuit: String,
    unproven_tx_hex: String,
    unproven_tx_bytes: i64,
    elapsed_ms: i64,
}

#[tokio::test]
async fn prepare_deactivate_unproven_call_tx() {
    // Fresh random controller sk + offline state pre-loaded with
    // the `deactivate` verifier key (mirrors the post-
    // MaintenanceUpdate state the chain holds after the wallet has
    // loaded that circuit on-chain).
    let controller_sk: [u8; 32] = rand::random();
    let ts_ms: u64 = 1_700_000_000_000;
    let state_hex = wallet_core::testing_deploy_state_with_circuits_hex(
        &controller_sk,
        ts_ms,
        &["deactivate"],
    )
    .expect("compose state with deactivate VK");

    // Wallet pulls a known seed so we have a stable coin/encryption pk
    // — these aren't yet used by `deactivate` itself, but the upstream
    // builder still needs them to commit the tx to a recipient.
    let wallet = Wallet::demo(Network::Undeployed);
    let coin_pk_hex = wallet.coin_public_key_hex().expect("coin pk");
    let enc_pk_hex = wallet.encryption_public_key_hex().expect("encryption pk");

    let bridge = NodeChildBridge::spawn(&NodeChildBridge::default_harness_path())
        .expect("spawn harness");

    let r: PrepareResult = bridge
        .call(
            "prepareUnprovenCallTx",
            serde_json::json!({
                "did": "did:midnight:undeployed:0000000000000000000000000000000000000000000000000000000000000000",
                "circuit": "deactivate",
                "circuitArgs": [],
                "contractStateHex": state_hex,
                // Offline state — chain-resolved address is unused by deactivate.
                "contractAddressHex": hex::encode([0u8; 32]),
                // Empty defaults: DID contracts don't touch Zswap; use chain initial params.
                "zswapChainStateHex": null,
                "ledgerParametersHex": null,
                "controllerSecretHex": hex::encode(controller_sk),
                "coinPublicKeyHex": coin_pk_hex,
                "encryptionPublicKeyHex": enc_pk_hex,
                "networkId": "undeployed",
            }),
        )
        .await
        .expect("prepareUnprovenCallTx");

    assert_eq!(r.circuit, "deactivate");
    assert!(
        r.unproven_tx_bytes > 100,
        "tx bytes suspiciously small: {}",
        r.unproven_tx_bytes,
    );
    let bytes = hex::decode(&r.unproven_tx_hex).expect("hex");
    assert_eq!(bytes.len() as i64, r.unproven_tx_bytes, "len mismatch");

    eprintln!(
        "[deactivate prepare] {} bytes UnprovenTransaction in {} ms",
        r.unproven_tx_bytes, r.elapsed_ms,
    );
}
