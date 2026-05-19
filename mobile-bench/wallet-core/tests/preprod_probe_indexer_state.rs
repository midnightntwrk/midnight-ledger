//! Quick read-only probe: confirm our extended `contract_state.graphql`
//! query is actually returning the new `zswap_state_hex` and
//! `ledger_parameters_hex` fields on PreProd, so we can rule out a
//! GraphQL codegen issue as the reason the partition didn't flip.

#![cfg(feature = "network-tests")]

use wallet_core::{IndexerClient, Network};

const TARGET: &str = "6b6e06d6f9779b0e4a3596a02edba5539f5b435c07ff5c885f3855d8d8653801";

#[tokio::test]
async fn dump_contract_state_info() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = IndexerClient::new(Network::PreProd).expect("client");
    let info = client
        .contract_state(TARGET)
        .await
        .expect("rpc")
        .expect("has state");
    println!("[indexer] addr={}", info.address_hex);
    println!("[indexer] state_hex.len={}", info.state_hex.len());
    println!("[indexer] last_tx_hash={}", info.last_tx_hash);
    println!("[indexer] last_block_height={:?}", info.last_block_height);
    println!(
        "[indexer] zswap_state_hex: {}",
        match &info.zswap_state_hex {
            Some(s) => format!("Some(len={})  head={}", s.len(), &s[..s.len().min(64)]),
            None => "None".to_string(),
        }
    );
    println!(
        "[indexer] ledger_parameters_hex: {}",
        match &info.ledger_parameters_hex {
            Some(s) => format!("Some(len={})  head={}", s.len(), &s[..s.len().min(64)]),
            None => "None".to_string(),
        }
    );
}
