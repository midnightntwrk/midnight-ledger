//! Survey the pallets / calls exposed by midnight-node-metadata's
//! `subxt!`-generated module. Run against a live node:
//!
//! ```bash
//! mobile-bench/scripts/standalone-up.sh
//! cargo run -p wallet-core --example probe_metadata
//! ```
//!
//! Does NOT modify state. Lists pallet names + a few well-known
//! calls so we can locate the `ContractDeploy` extrinsic for
//! Phase 3 next session.

use subxt::config::PolkadotConfig;
use subxt::OnlineClient;

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let url = std::env::var("MIDNIGHT_NODE_WS")
        .unwrap_or_else(|_| "ws://127.0.0.1:9944".to_string());
    println!("connecting to {url} …");
    // PolkadotConfig is a generic Config that should work against
    // any substrate-based chain for read-only RPCs. Submitting
    // extrinsics will need a Midnight-specific Config (next iter).
    let api = OnlineClient::<PolkadotConfig>::from_url(&url).await?;
    println!("connected.\n");

    // Latest block.
    let block = api.blocks().at_latest().await?;
    println!("latest block:");
    println!("  number = {}", block.number());
    println!("  hash   = {:?}", block.hash());

    // Runtime version.
    let rv = api.backend().current_runtime_version().await?;
    println!("\nruntime version:");
    println!("  spec_version        = {}", rv.spec_version);
    println!("  transaction_version = {}", rv.transaction_version);

    // Pallets present, by name. From this list we pick the one
    // hosting the contract-deploy extrinsic for the next slice.
    println!("\npallets in metadata:");
    let metadata = api.metadata();
    for pallet in metadata.pallets() {
        let n_calls = pallet
            .call_variants()
            .map(|v| v.len())
            .unwrap_or(0);
        let n_storage = pallet
            .storage()
            .map(|s| s.entries().len())
            .unwrap_or(0);
        println!(
            "  {:<28}  calls = {:>3}  storage = {:>3}",
            pallet.name(),
            n_calls,
            n_storage
        );
    }

    // Find the pallet most likely to host ContractDeploy. midnight-
    // ledger's pallet is typically called "Midnight" or "Ledger";
    // print all of its call variants so we can read off the right
    // one.
    let candidates = ["Midnight", "Ledger", "MidnightLedger", "Contracts", "Compact"];
    for cand in candidates {
        if let Some(pallet) = metadata.pallets().find(|p| p.name() == cand) {
            println!("\n{cand} pallet calls:");
            if let Some(variants) = pallet.call_variants() {
                for v in variants.iter() {
                    println!("  {:<40}  fields = {}", v.name, v.fields.len());
                }
            } else {
                println!("  (no call variants)");
            }
            break;
        }
    }

    Ok(())
}
