//! Quick balance diagnostic for the Undeployed demo wallet on the
//! local standalone stack. Not a real test (always passes after
//! printing) — purely to surface what NIGHT + DUST the wallet
//! currently sees, so we can rule env-state in or out when balance
//! errors crop up.
//!
//! Run with:
//!   cargo test -p wallet-core --features network-tests \
//!     --test balance_diagnostic -- --nocapture

#![cfg(feature = "network-tests")]

use wallet_core::{Network, Wallet};

#[tokio::test]
async fn report_undeployed_demo_balances() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let w = Wallet::demo(Network::Undeployed);

    println!("Undeployed demo wallet");
    println!("  unshielded address : {}", w.unshielded_address().unwrap());
    println!("  coin pk            : {}", w.coin_public_key_hex().unwrap());

    let unshielded = w.sync_unshielded().await.expect("unshielded sync");
    let mut night_total: u128 = 0;
    for u in unshielded.iter() {
        night_total = night_total.saturating_add(u.value);
    }
    println!("  unshielded UTXOs   : {}", unshielded.len());
    println!("  NIGHT (atomic)     : {night_total}");

    let dust_state = w.sync_dust().await.expect("dust sync");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let now_ts = base_crypto::time::Timestamp::from_secs(now);
    println!("  dust sync_time     : {:?}", dust_state.sync_time);
    println!(
        "  dust @ now ({now}) : {} atomic units",
        dust_state.wallet_balance(now_ts)
    );
    println!(
        "  dust @ sync_time   : {} atomic units",
        dust_state.wallet_balance(dust_state.sync_time)
    );
}
