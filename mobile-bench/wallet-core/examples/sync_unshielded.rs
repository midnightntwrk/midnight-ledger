//! Scripted unshielded-sync probe.
//!
//! Usage:
//!     cargo run -p wallet-core --example sync_unshielded -- preprod
//!
//! Prints the address, UTXO count, and per-token balance for the
//! demo wallet on the requested network. Used to bench/iterate on
//! the snapshot path without dragging the Dioxus app along.

use wallet_core::{Network, Wallet};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::try_init().ok();

    let arg = std::env::args().nth(1).unwrap_or_else(|| "preprod".into());
    let network = parse_network(&arg)?;
    let w = Wallet::demo(network);
    let address = w.unshielded_address()?;
    println!("network: {:?}", network);
    println!("address: {address}");

    let started = std::time::Instant::now();
    let set = w.sync_unshielded().await?;
    let elapsed = started.elapsed();

    println!("sync ms: {}", elapsed.as_millis());
    println!("utxos:   {}", set.len());
    for (token, value) in set.balance_by_token() {
        println!("  {}: {}", hex::encode(&token.0), value);
    }
    Ok(())
}

fn parse_network(s: &str) -> anyhow::Result<Network> {
    let lower = s.to_ascii_lowercase();
    Ok(match lower.as_str() {
        "mainnet" => Network::Mainnet,
        "preprod" => Network::PreProd,
        "preview" => Network::Preview,
        "qanet" => Network::QaNet,
        "devnet" => Network::DevNet,
        "undeployed" | "local" => Network::Undeployed,
        other => return Err(anyhow::anyhow!("unknown network: {other}")),
    })
}
