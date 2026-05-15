//! Live integration test for `Wallet::create_did()` against the
//! local standalone Midnight stack. Gated behind `network-tests`.
//!
//! Run with:
//!   docker compose -f mobile-bench/scripts/standalone.yml up -d node indexer
//!   cargo test -p wallet-core --features network-tests \
//!     --test deploy_undeployed_live -- --nocapture

#![cfg(feature = "network-tests")]

use futures::StreamExt;
use wallet_core::{Network, Wallet, WizardStage};

#[tokio::test]
async fn deploy_did_on_undeployed_lands_in_block() {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let w = Wallet::demo(Network::Undeployed);
    println!("unshielded address: {}", w.unshielded_address().unwrap());
    println!("dust public key:    {}", w.dust_public_key_hex().unwrap());

    let stream = w.create_did();
    let mut stream = std::pin::pin!(stream);

    let mut outcome = None;
    while let Some(stage) = stream.next().await {
        match &stage {
            WizardStage::Done(o) => {
                println!(
                    "done: did={} tx=0x{} block=0x{}",
                    o.did_id.to_did_string(),
                    hex::encode(o.tx_hash),
                    hex::encode(o.block_hash),
                );
                outcome = Some(o.clone());
                break;
            }
            WizardStage::Failed(e) => panic!("pipeline failed: {e}"),
            other => println!("stage: {other:?}"),
        }
    }

    let outcome = outcome.expect("pipeline yielded Done");
    let did_string = outcome.did_id.to_did_string();
    assert!(did_string.starts_with("did:midnight:undeployed:"));
    let parsed = wallet_core::DidId::parse(&did_string).expect("parse did string");
    assert_eq!(parsed, outcome.did_id);
}
