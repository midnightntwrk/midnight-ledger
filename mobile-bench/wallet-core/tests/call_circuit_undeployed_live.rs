//! Live end-to-end test for `Wallet::call_did_circuit`:
//!   1. Deploy a fresh DID (Rust pipeline)
//!   2. Load the `deactivate` verifier key via MaintenanceUpdate
//!      (Rust pipeline)
//!   3. Invoke the `deactivate` circuit via the JS-driven path
//!      under test (JS builds the UnprovenTransaction, Rust
//!      balances dust + proves + submits)
//!   4. Resolve the DID and assert `deactivated == true`
//!
//! Gated behind `network-tests`. Brings the standalone stack up:
//!
//!   docker compose -f mobile-bench/scripts/standalone.yml up -d node indexer
//!   cargo test -p wallet-core --features network-tests \
//!     --test call_circuit_undeployed_live -- --nocapture

#![cfg(feature = "network-tests")]

use futures::StreamExt;
use wallet_core::{Network, Wallet, WizardStage};

#[tokio::test]
async fn deactivate_on_undeployed_live() {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let w = Wallet::demo(Network::Undeployed);

    // 1. Deploy a fresh DID. The Done outcome carries the random
    //    `controller_sk` we'll use as the deactivate witness.
    let mut deploy_stream = std::pin::pin!(w.create_did());
    let mut deploy_outcome = None;
    while let Some(stage) = deploy_stream.next().await {
        match stage {
            WizardStage::Done(o) => {
                println!("[deploy] did={}", o.did_id.to_did_string());
                deploy_outcome = Some(o);
                break;
            }
            WizardStage::Failed(e) => panic!("deploy failed: {e}"),
            other => println!("[deploy] {other:?}"),
        }
    }
    let outcome = deploy_outcome.expect("deploy must complete");
    let did_id = outcome.did_id.clone();
    let controller_sk = outcome.controller_sk;

    // Let the indexer settle before we read state for the
    // MaintenanceUpdate + circuit call (mirrors the 30s used by
    // `load_circuit_undeployed_live`).
    tokio::time::sleep(std::time::Duration::from_secs(30)).await;

    // 2. Load the `deactivate` verifier key. Counter is 0 — fresh
    //    deploy, no maintenance updates yet.
    let mut load_stream = std::pin::pin!(w.load_did_circuit(
        did_id.clone(),
        "deactivate".to_string(),
        0,
    ));
    while let Some(stage) = load_stream.next().await {
        match stage {
            WizardStage::Done(o) => {
                println!(
                    "[load deactivate VK] tx=0x{} block=0x{}",
                    hex::encode(o.tx_hash),
                    hex::encode(o.block_hash),
                );
                break;
            }
            WizardStage::Failed(e) => panic!("load deactivate VK failed: {e}"),
            other => println!("[load deactivate VK] {other:?}"),
        }
    }
    tokio::time::sleep(std::time::Duration::from_secs(30)).await;

    // 3. Call the deactivate circuit. JS builds the UnprovenTransaction;
    //    Rust handles balance/prove/submit.
    let mut call_stream = std::pin::pin!(w.call_did_circuit(
        did_id.clone(),
        "deactivate".to_string(),
        serde_json::json!([]),
        controller_sk,
    ));
    let mut call_outcome = None;
    while let Some(stage) = call_stream.next().await {
        match stage {
            WizardStage::Done(o) => {
                println!(
                    "[call deactivate] tx=0x{} block=0x{}",
                    hex::encode(o.tx_hash),
                    hex::encode(o.block_hash),
                );
                call_outcome = Some(o);
                break;
            }
            WizardStage::Failed(e) => panic!("deactivate call failed: {e}"),
            other => println!("[call deactivate] {other:?}"),
        }
    }
    let call_outcome = call_outcome.expect("call deactivate must complete");
    assert_eq!(call_outcome.did_id, did_id);

    // 4. Resolve the DID and assert it's deactivated.
    tokio::time::sleep(std::time::Duration::from_secs(15)).await;
    let resolved = w
        .resolve_did_full(&did_id.to_did_string())
        .await
        .expect("resolve after deactivate");
    println!(
        "[resolve] counter={} deactivated={} last_block={:?}",
        resolved.maintenance_counter,
        resolved.document.deactivated,
        resolved.last_block_height,
    );
    assert!(
        resolved.document.deactivated,
        "DID must be marked deactivated after the call",
    );
}
