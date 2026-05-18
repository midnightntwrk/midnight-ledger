//! Live end-to-end test for the Operation Builder's auto-load
//! path. Mirrors what happens when the user queues a write
//! circuit whose verifier key isn't on-chain yet — the
//! builder spawn closure prepends a `load_did_circuit`
//! MaintenanceUpdate, waits for the indexer to settle, then
//! runs the original `call_did_circuit`.
//!
//! Sequence:
//!   1. Deploy a fresh DID (no VKs registered).
//!   2. Resolve the DID and read `loaded_circuits` —
//!      should be empty (only the deploy circuit is implicit;
//!      no write-circuit VKs are bundled into deploy).
//!   3. For each queued op `(circuit, args)`:
//!        a. If circuit not in `loaded_circuits` set, run
//!           `load_did_circuit(circuit, counter)`; bump counter;
//!           settle 30s.
//!        b. Run `call_did_circuit(circuit, args, sk)`; settle 15s.
//!   4. Resolve and assert all writes landed + `loaded_circuits`
//!      now contains every circuit we exercised.
//!
//! Differs from `batch_circuits_undeployed_live.rs` in that the
//! caller no longer needs to manually order the load+call pairs
//! — the auto-load logic does it from the queue alone, given
//! the initial on-chain VK set + counter.
//!
//! Gated behind `network-tests`.

#![cfg(feature = "network-tests")]

use futures::StreamExt;
use std::collections::HashSet;
use wallet_core::{Network, Wallet, WizardStage};

const ALIAS: &str = "https://alias.autoload.example.com";
const SERVICE_ID: &str = "svc-autoload-0";

fn linked_domains_service_args(id: &str, endpoint: &str) -> serde_json::Value {
    serde_json::json!([{
        "id": id,
        "typ": "LinkedDomains",
        "serviceEndpoint": endpoint,
    }])
}

#[tokio::test]
async fn auto_load_two_unloaded_circuits_on_undeployed_live() {
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

    // 2. Resolve & inspect the initial VK set. A fresh deploy
    //    has no operations registered yet — every queued write
    //    will need an auto-load.
    let resolved0 = w
        .resolve_did_full(&did_id.to_did_string())
        .await
        .expect("initial resolve");
    println!(
        "[initial] counter={} loaded_circuits={:?}",
        resolved0.maintenance_counter, resolved0.loaded_circuits,
    );
    let mut loaded_set: HashSet<String> =
        resolved0.loaded_circuits.iter().cloned().collect();
    let mut counter_cursor = resolved0.maintenance_counter;

    // 3. Auto-load loop — exactly the shape `DidOperationBuilder`
    //    runs in the spawned submit closure.
    let queue: Vec<(&'static str, serde_json::Value)> = vec![
        ("addAlsoKnownAs", serde_json::json!([ALIAS])),
        (
            "addService",
            linked_domains_service_args(SERVICE_ID, "https://example.com/.well-known/did-config"),
        ),
    ];
    for (circuit, args) in &queue {
        if !loaded_set.contains(*circuit) {
            println!(
                "[auto-load {circuit}] @ counter={counter_cursor} (not in {:?})",
                loaded_set,
            );
            let mut load_stream = std::pin::pin!(w.load_did_circuit(
                did_id.clone(),
                (*circuit).to_string(),
                counter_cursor,
            ));
            let mut landed_load = false;
            while let Some(stage) = load_stream.next().await {
                match stage {
                    WizardStage::Done(o) => {
                        println!(
                            "[auto-load {circuit}] tx=0x{} block=0x{}",
                            hex::encode(o.tx_hash),
                            hex::encode(o.block_hash),
                        );
                        landed_load = true;
                        break;
                    }
                    WizardStage::Failed(e) => panic!("auto-load {circuit} failed: {e}"),
                    other => println!("[auto-load {circuit}] {other:?}"),
                }
            }
            assert!(landed_load, "auto-load {circuit} must reach Done");
            loaded_set.insert((*circuit).to_string());
            counter_cursor = counter_cursor.saturating_add(1);
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        } else {
            println!("[skip auto-load] {circuit} already on-chain");
        }

        println!("[call {circuit}] args={args}");
        let mut call_stream = std::pin::pin!(w.call_did_circuit(
            did_id.clone(),
            (*circuit).to_string(),
            args.clone(),
            controller_sk,
        ));
        let mut landed_call = false;
        while let Some(stage) = call_stream.next().await {
            match stage {
                WizardStage::Done(o) => {
                    println!(
                        "[call {circuit}] tx=0x{} block=0x{}",
                        hex::encode(o.tx_hash),
                        hex::encode(o.block_hash),
                    );
                    landed_call = true;
                    break;
                }
                WizardStage::Failed(e) => panic!("call {circuit} failed: {e}"),
                other => println!("[call {circuit}] {other:?}"),
            }
        }
        assert!(landed_call, "call {circuit} must reach Done");
        tokio::time::sleep(std::time::Duration::from_secs(15)).await;
    }

    // 4. Resolve and assert: alias + service both landed, and
    //    the on-chain VK set now includes both circuit names.
    let resolved1 = w
        .resolve_did_full(&did_id.to_did_string())
        .await
        .expect("final resolve");
    println!(
        "[final] counter={} loaded_circuits={:?} also_known_as={:?} services={:?}",
        resolved1.maintenance_counter,
        resolved1.loaded_circuits,
        resolved1.document.also_known_as,
        resolved1
            .document
            .service
            .iter()
            .map(|s| &s.id)
            .collect::<Vec<_>>(),
    );

    assert!(
        resolved1.document.also_known_as.iter().any(|a| a == ALIAS),
        "also_known_as must contain {ALIAS} (got {:?})",
        resolved1.document.also_known_as,
    );
    assert!(
        resolved1
            .document
            .service
            .iter()
            .any(|s| s.id.ends_with(SERVICE_ID)),
        "service list must contain {SERVICE_ID} (got {:?})",
        resolved1
            .document
            .service
            .iter()
            .map(|s| &s.id)
            .collect::<Vec<_>>(),
    );
    for circuit in ["addAlsoKnownAs", "addService"] {
        assert!(
            resolved1
                .loaded_circuits
                .iter()
                .any(|c| c == circuit),
            "loaded_circuits must contain {circuit} after auto-load (got {:?})",
            resolved1.loaded_circuits,
        );
    }
    assert_eq!(
        resolved1.maintenance_counter,
        resolved0.maintenance_counter + 2,
        "two auto-loads must bump counter by exactly 2",
    );
}
