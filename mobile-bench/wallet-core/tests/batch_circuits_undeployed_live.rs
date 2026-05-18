//! Live end-to-end test for the Operation Builder's batched
//! submission path. Mirrors what `DidOperationBuilder` does in
//! the UI when the user queues several ops and clicks "Submit
//! batch" — iterate sequentially through
//! `Wallet::call_did_circuit`, awaiting each call's terminal
//! `WizardStage` before starting the next.
//!
//! Sequence:
//!   1. Deploy a fresh DID (Rust pipeline)
//!   2. Load verifier keys for `addAlsoKnownAs` +
//!      `addVerificationMethod` via MaintenanceUpdate (Rust
//!      pipeline). The chain rejects ContractCall for any
//!      circuit whose VK isn't on-chain yet, so both VKs must
//!      be loaded before the batch.
//!   3. Submit the two write circuits sequentially via
//!      `call_did_circuit` (JS-built UnprovenTransaction → Rust
//!      balance / prove / submit).
//!   4. Resolve the DID and assert both changes landed —
//!      `also_known_as` has the inserted alias and
//!      `verification_method` has the new method.
//!
//! Gated behind `network-tests`. Brings the standalone stack up:
//!
//!   docker compose -f mobile-bench/scripts/standalone.yml up -d node indexer
//!   cargo test -p wallet-core --features network-tests \
//!     --test batch_circuits_undeployed_live -- --nocapture

#![cfg(feature = "network-tests")]

use futures::StreamExt;
use wallet_core::{Network, Wallet, WizardStage};

const ALIAS: &str = "https://alias.batch.example.com";
const KEY_ID: &str = "key-batch-0";

/// Convenience: build the JSON arg shape `addVerificationMethod`
/// expects. Same shape as `tests/js_inspect_circuits.rs::ed25519_vm`.
fn ed25519_vm_args(id: &str) -> serde_json::Value {
    serde_json::json!([{
        "id": id,
        // VerificationMethodType.JsonWebKey = 1
        "typ": 1,
        "publicKeyJwk": {
            // KeyType.OKP = 3, CurveType.Ed25519 = 0
            "kty": 3,
            "crv": 0,
            "x": { "$bigint": "1" },
            "y": { "$bigint": "2" },
        }
    }])
}

#[tokio::test]
async fn batch_two_write_circuits_on_undeployed_live() {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let w = Wallet::demo(Network::Undeployed);

    // 1. Deploy a fresh DID.
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
    tokio::time::sleep(std::time::Duration::from_secs(30)).await;

    // 2. Load both verifier keys. Counter increments per
    //    MaintenanceUpdate — start at 0, second one uses 1.
    for (counter, circuit) in [
        (0u32, "addAlsoKnownAs"),
        (1u32, "addVerificationMethod"),
    ] {
        let mut load_stream = std::pin::pin!(w.load_did_circuit(
            did_id.clone(),
            circuit.to_string(),
            counter,
        ));
        while let Some(stage) = load_stream.next().await {
            match stage {
                WizardStage::Done(o) => {
                    println!(
                        "[load {circuit} VK @ ctr {counter}] tx=0x{} block=0x{}",
                        hex::encode(o.tx_hash),
                        hex::encode(o.block_hash),
                    );
                    break;
                }
                WizardStage::Failed(e) => panic!("load {circuit} VK failed: {e}"),
                other => println!("[load {circuit} VK] {other:?}"),
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
    }

    // 3. Sequential batch submission — the exact loop
    //    DidOperationBuilder runs on "Submit batch". Each call
    //    is awaited to its terminal stage before the next; the
    //    chain's `version` field changes between calls (each
    //    write bumps it), so the next call's
    //    `prepareUnprovenCallTx` round-trip needs fresh state
    //    pulled via `indexer.contract_state` (which
    //    `Wallet::call_did_circuit` does internally).
    let batch: Vec<(&'static str, serde_json::Value)> = vec![
        ("addAlsoKnownAs", serde_json::json!([ALIAS])),
        ("addVerificationMethod", ed25519_vm_args(KEY_ID)),
    ];
    for (circuit, args) in &batch {
        let mut call_stream = std::pin::pin!(w.call_did_circuit(
            did_id.clone(),
            circuit.to_string(),
            args.clone(),
            controller_sk,
        ));
        let mut landed = false;
        while let Some(stage) = call_stream.next().await {
            match stage {
                WizardStage::Done(o) => {
                    println!(
                        "[call {circuit}] tx=0x{} block=0x{}",
                        hex::encode(o.tx_hash),
                        hex::encode(o.block_hash),
                    );
                    landed = true;
                    break;
                }
                WizardStage::Failed(e) => panic!("call {circuit} failed: {e}"),
                other => println!("[call {circuit}] {other:?}"),
            }
        }
        assert!(landed, "call {circuit} must reach Done");
        // Let the indexer pick up the new state before the next
        // call reads it. Same 15s settle as the single-circuit
        // live test.
        tokio::time::sleep(std::time::Duration::from_secs(15)).await;
    }

    // 4. Resolve and assert both writes landed.
    let resolved = w
        .resolve_did_full(&did_id.to_did_string())
        .await
        .expect("resolve after batch");
    println!(
        "[resolve] counter={} also_known_as={:?} vm_ids={:?}",
        resolved.maintenance_counter,
        resolved.document.also_known_as,
        resolved
            .document
            .verification_method
            .iter()
            .map(|vm| &vm.id)
            .collect::<Vec<_>>(),
    );

    assert!(
        resolved.document.also_known_as.iter().any(|a| a == ALIAS),
        "also_known_as must contain {ALIAS} (got {:?})",
        resolved.document.also_known_as,
    );
    assert!(
        resolved
            .document
            .verification_method
            .iter()
            .any(|vm| vm.id.ends_with(KEY_ID)),
        "verification_method must contain id ending {KEY_ID} (got {:?})",
        resolved
            .document
            .verification_method
            .iter()
            .map(|vm| &vm.id)
            .collect::<Vec<_>>(),
    );
    assert!(
        !resolved.document.deactivated,
        "DID must not be deactivated after these writes",
    );
}
