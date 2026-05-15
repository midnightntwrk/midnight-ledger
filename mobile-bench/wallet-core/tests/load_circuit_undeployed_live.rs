//! Live integration test for `Wallet::load_did_circuit()` against
//! the local standalone Midnight stack. Deploys a fresh DID
//! (committee = wallet's BIP340 verifying key, threshold 1) then
//! submits a MaintenanceUpdate that loads the addVerificationMethod
//! circuit's verifier key. Gated behind `network-tests`.
//!
//! Run with:
//!   docker compose -f mobile-bench/scripts/standalone.yml up -d node indexer
//!   cargo test -p wallet-core --features network-tests \
//!     --test load_circuit_undeployed_live -- --nocapture

#![cfg(feature = "network-tests")]

use futures::StreamExt;
use wallet_core::{Network, Wallet, WizardStage};

#[tokio::test]
async fn load_add_verification_method_after_fresh_deploy() {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let w = Wallet::demo(Network::Undeployed);

    // 1. Deploy a fresh DID. The committee includes the wallet's
    //    own BIP340 verifying key so MaintenanceUpdate is possible.
    let mut deploy_stream = std::pin::pin!(w.create_did());
    let mut did_id = None;
    while let Some(stage) = deploy_stream.next().await {
        match stage {
            WizardStage::Done(o) => {
                println!("[deploy] did={}", o.did_id.to_did_string());
                did_id = Some(o.did_id);
                break;
            }
            WizardStage::Failed(e) => panic!("deploy failed: {e}"),
            other => println!("[deploy] stage: {other:?}"),
        }
    }
    let did = did_id.expect("deploy yielded Done");

    // 2. Load the addVerificationMethod verifier key via a
    //    MaintenanceUpdate. Counter is 0 — the contract was just
    //    deployed and no maintenance update has yet bumped it.
    let mut load_stream = std::pin::pin!(w.load_did_circuit(
        did.clone(),
        "addVerificationMethod".to_string(),
        0,
    ));
    let mut load_outcome = None;
    while let Some(stage) = load_stream.next().await {
        match stage {
            WizardStage::Done(o) => {
                println!(
                    "[load] tx=0x{} block=0x{}",
                    hex::encode(o.tx_hash),
                    hex::encode(o.block_hash),
                );
                load_outcome = Some(o);
                break;
            }
            WizardStage::Failed(e) => panic!("load failed: {e}"),
            other => println!("[load] stage: {other:?}"),
        }
    }
    let outcome = load_outcome.expect("load yielded Done");
    assert_eq!(outcome.did_id, did, "did_id unchanged by maintenance update");
}
