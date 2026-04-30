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

use scale_info::scale::Decode;
use subxt::Metadata;

/// Run with a local stack:
///
/// ```bash
/// mobile-bench/scripts/standalone-up.sh
/// cargo run -p wallet-core --example probe_metadata
/// ```
///
/// Or fully offline against the bundled `.scale` files:
///
/// ```bash
/// MODE=offline cargo run -p wallet-core --example probe_metadata
/// ```
fn main() -> anyhow::Result<()> {
    let mode = std::env::var("MODE").unwrap_or_else(|_| "live".to_string());
    let metadata = if mode == "offline" {
        load_offline_metadata()?
    } else {
        load_live_metadata()?
    };

    // Pallets present, by name. From this list we pick the one
    // hosting the contract-deploy extrinsic for the next slice.
    println!("pallets in metadata:");
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

    // Discover the signature scheme: scan the type registry for
    // anything that *looks* like the runtime's signature enum.
    // subxt 0.44 doesn't expose extrinsic.signature_ty() publicly,
    // so we walk the registry by path-name match instead. The
    // chain's enum lives at `sp_runtime::MultiSignature` for stock
    // substrate, or a custom path if Midnight overrides it.
    let registry = metadata.types();
    println!("\nsignature-like types in registry:");
    for ty in registry.types.iter() {
        let name = ty.ty.path.to_string();
        if name.contains("Signature") || name.contains("MultiSig") {
            // Show the enum variants to know what schemes the chain accepts.
            if let scale_info::TypeDef::Variant(var) = &ty.ty.type_def {
                println!("  [{}] {}  variants:", ty.id, name);
                for v in var.variants.iter() {
                    println!("    [{:>3}] {}", v.index, v.name);
                }
            }
        }
    }

    Ok(())
}

fn load_offline_metadata() -> anyhow::Result<Metadata> {
    use std::path::Path;
    // Find the static .scale file in the cloned git checkout. The
    // hash is fixed by the `node-0.22.3` tag in our Cargo.toml.
    let candidates = [
        "/Users/ysh/.cargo/git/checkouts/midnight-node-a5e2d7071ca76673/6f0ef43/metadata/static/midnight_metadata_0.22.0.scale",
    ];
    let path = candidates
        .iter()
        .find(|p| Path::new(p).exists())
        .ok_or_else(|| anyhow::anyhow!("static metadata not found; pass MODE=live"))?;
    println!("offline mode: reading {path}\n");
    let bytes = std::fs::read(path)?;
    Ok(Metadata::decode(&mut &bytes[..])?)
}

fn load_live_metadata() -> anyhow::Result<Metadata> {
    use subxt::config::PolkadotConfig;
    use subxt::OnlineClient;
    let url = std::env::var("MIDNIGHT_NODE_WS")
        .unwrap_or_else(|_| "ws://127.0.0.1:9944".to_string());
    println!("connecting to {url} …");
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    rt.block_on(async {
        let api = OnlineClient::<PolkadotConfig>::from_url(&url).await?;
        let block = api.blocks().at_latest().await?;
        println!("connected. latest block = {}\n", block.number());
        Ok(api.metadata())
    })
}
