//! Live preprod tests. Disabled by default; enable with
//! `cargo test -p wallet-core --features network-tests` to actually
//! reach over the wire.

#![cfg(feature = "network-tests")]

use wallet_core::{IndexerClient, Network, NodeClient, probe_connectivity};

#[tokio::test]
async fn preprod_indexer_and_node_reachable() {
    let _ = tracing_subscriber::fmt::try_init();
    let result = probe_connectivity(Network::PreProd).await;
    eprintln!("preprod probe: {:#?}", result);

    assert!(
        result.indexer_http.reachable,
        "indexer http unreachable: {:?}",
        result.indexer_http.detail
    );
    assert!(
        result.indexer_ws.reachable,
        "indexer ws unreachable: {:?}",
        result.indexer_ws.detail
    );
    assert!(
        result.node_ws.reachable,
        "node ws unreachable: {:?}",
        result.node_ws.detail
    );
}

#[tokio::test]
async fn preprod_chain_tip_query() {
    let _ = tracing_subscriber::fmt::try_init();
    let client = IndexerClient::new(Network::PreProd).expect("client");
    let tip = client.chain_tip().await.expect("chain_tip");
    let tip = tip.expect("preprod always has a latest block");
    eprintln!("preprod chain tip: {:#?}", tip);

    assert!(tip.height > 0, "block height must advance past genesis");
    assert!(!tip.hash.is_empty(), "block hash must be populated");
    assert!(tip.timestamp_unix > 1_700_000_000, "timestamp looks plausible");
}

#[tokio::test]
async fn preprod_node_status_query() {
    let _ = tracing_subscriber::fmt::try_init();
    let client = NodeClient::connect(Network::PreProd).await.expect("connect");
    let status = client.status().await.expect("status");
    eprintln!("preprod node status: {:#?}", status);

    assert!(
        !status.finalized_head_hash.is_empty(),
        "finalized head must be populated"
    );
    assert!(
        status.finalized_head_hash.starts_with("0x"),
        "substrate hashes are 0x-prefixed: {}",
        status.finalized_head_hash
    );
}
