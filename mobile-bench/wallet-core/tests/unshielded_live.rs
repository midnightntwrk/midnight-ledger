//! Live integration tests for unshielded sync. Gated behind the
//! `network-tests` feature so CI/offline runs don't depend on
//! preprod reachability.
//!
//! Run with:
//!     cargo test -p wallet-core --features network-tests --test unshielded_live -- --nocapture

#![cfg(feature = "network-tests")]

use wallet_core::{Network, Wallet};

#[tokio::test]
async fn snapshot_preprod_demo_wallet() {
    // Initialize crypto provider once — required by the rustls
    // stack the indexer WS sits behind.
    let _ = rustls::crypto::ring::default_provider().install_default();

    let w = Wallet::demo(Network::PreProd);
    let address = w
        .unshielded_address()
        .expect("demo wallet has an unshielded address");
    println!("snapshot for: {address}");

    let started = std::time::Instant::now();
    let set = w
        .sync_unshielded()
        .await
        .expect("snapshot returns Ok against live preprod");
    let elapsed = started.elapsed();

    println!("sync took {} ms", elapsed.as_millis());
    println!("utxos: {}", set.len());

    // Assertion is shape-only — the demo wallet's address may be
    // empty on preprod. What we care about is that the call
    // terminated cleanly (not that it found UTXOs).
    for u in set.iter() {
        assert_eq!(u.owner, address, "every utxo's owner matches our address");
    }
}
