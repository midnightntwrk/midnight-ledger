//! Pull the SCALE-encoded inner-Midnight bytes for a known
//! `Midnight.send_mn_transaction` extrinsic on PreProd, given the
//! block hash and tx hash that contains it. Used as the upstream
//! half of the byte-diff investigation against our own wallet's
//! tx output.
//!
//! Why both hashes: substrate doesn't index by tx hash globally
//! without an archive node. With the block hash we go directly to
//! the right block and find the matching extrinsic by hash.
//!
//! Run with:
//!
//! ```text
//! PREPROD_TX_BLOCK_HASH=0xabc… \
//! PREPROD_TX_HASH=0xdef…       \
//! cargo test -p wallet-core --features network-tests \
//!   --test preprod_fetch_tx_bytes -- --nocapture
//! ```
//!
//! Writes the inner Midnight bytes (hex) to
//! `${WALLET_CORE_DUMP_TX:-/tmp/upstream-tx.hex}` for diffing.

#![cfg(feature = "network-tests")]

use parity_scale_codec::Decode;
use subxt::OnlineClient;
use subxt::SubstrateConfig;
use subxt::utils::H256;

const PREPROD_WS: &str = "wss://rpc.preprod.midnight.network";

fn env_hash(name: &str) -> H256 {
    let v = std::env::var(name).unwrap_or_else(|_| panic!("set {name}=0x…"));
    let hex_str = v.trim_start_matches("0x");
    let bytes = hex::decode(hex_str).expect("hex");
    assert_eq!(bytes.len(), 32, "{name} must be 32-byte hex");
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    H256(arr)
}

#[tokio::test]
async fn fetch_upstream_tx_bytes() {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let block_hash = env_hash("PREPROD_TX_BLOCK_HASH");
    let tx_hash = env_hash("PREPROD_TX_HASH");
    let out_path = std::env::var("WALLET_CORE_DUMP_TX")
        .unwrap_or_else(|_| "/tmp/upstream-tx.hex".to_string());

    let client = OnlineClient::<SubstrateConfig>::from_url(PREPROD_WS)
        .await
        .expect("connect PreProd");

    let block = client
        .blocks()
        .at(block_hash)
        .await
        .expect("fetch block by hash");
    let extrinsics = block.extrinsics().await.expect("decode extrinsics");

    let target_hex = hex::encode(tx_hash.0);
    println!(
        "[block] hash=0x{} extrinsics={}",
        hex::encode(block_hash.0),
        extrinsics.iter().count(),
    );
    // The indexer's `transaction.hash` is the *Midnight* tx hash
    // (blake2 of the inner tagged-serialized payload), NOT the
    // substrate extrinsic hash. So matching by extrinsic.hash() is
    // wrong. Instead, pick the (typically single) Midnight
    // `send_mn_transaction` extrinsic in the block.
    let mut hit = None;
    let mut all_midnight = Vec::new();
    for ex in extrinsics.iter() {
        let pallet = ex.pallet_name().unwrap_or("?").to_string();
        let variant = ex.variant_name().unwrap_or("?").to_string();
        let exh = hex::encode(ex.hash().0);
        println!(
            "  ex[{}] pallet={pallet} variant={variant} signed={} extrinsic_hash=0x{exh}",
            ex.index(),
            ex.is_signed(),
        );
        if pallet == "Midnight" && variant == "send_mn_transaction" {
            all_midnight.push(ex);
        }
    }
    if all_midnight.len() == 1 {
        hit = Some(all_midnight.into_iter().next().unwrap());
    } else if !all_midnight.is_empty() {
        // Multiple Midnight txs in this block; we don't have a
        // reliable selector. The Midnight tx-hash is blake2 of the
        // inner tagged-serialized bytes — compute and match.
        use base_crypto::hash::HashOutput;
        for ex in all_midnight {
            // call_bytes() = [pallet_idx][call_idx][Vec<u8> SCALE]
            // -> strip the two indices and the compact length
            let fb = ex.field_bytes();
            let inner: Vec<u8> = match Decode::decode(&mut &fb[..]) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let h: HashOutput = base_crypto::hash::persistent_hash(&inner);
            let h_hex = hex::encode(h.0);
            println!("    midnight_hash=0x{h_hex}");
            if h_hex == target_hex {
                hit = Some(ex);
                break;
            }
        }
    }
    let ex = hit.unwrap_or_else(|| {
        panic!("no matching Midnight send_mn_transaction in block 0x{}", hex::encode(block_hash.0))
    });

    let pallet = ex.pallet_name().expect("pallet name").to_string();
    let variant = ex.variant_name().expect("variant name").to_string();
    println!(
        "[found] tx=0x{} pallet={pallet} variant={variant} signed={} idx={}",
        hex::encode(ex.hash().0),
        ex.is_signed(),
        ex.index(),
    );

    assert_eq!(pallet, "Midnight", "expected Midnight pallet, got {pallet}");
    assert_eq!(
        variant, "send_mn_transaction",
        "expected send_mn_transaction, got {variant}",
    );

    // The call args: a single `Vec<u8>` field — the inner Midnight tx
    // bytes. `field_bytes()` is the SCALE-encoded fields portion of
    // the extrinsic (no signature/version envelope). For a single
    // `Vec<u8>` arg, the leading bytes are the compact-encoded length
    // followed by the payload.
    let field_bytes = ex.field_bytes();
    println!("[raw] field_bytes len={}", field_bytes.len());

    // Decode `Vec<u8>` from field_bytes.
    let inner: Vec<u8> =
        Decode::decode(&mut &field_bytes[..]).expect("decode Vec<u8> from field bytes");
    println!("[inner] {} bytes", inner.len());

    std::fs::write(&out_path, hex::encode(&inner)).expect("write hex dump");
    println!("[wrote] {} ({} hex chars)", out_path, hex::encode(&inner).len());

    // Quick sanity look at framing: first 16 bytes should start
    // with the upstream global tag (`midnight:…:`) when the inner
    // payload is `tagged_serialize`-framed.
    let head = &inner[..inner.len().min(64)];
    let tail_start = inner.len().saturating_sub(64);
    let tail = &inner[tail_start..];
    println!("  head[64]: {}", hex::encode(head));
    println!("  tail[64]: {}", hex::encode(tail));
    let ascii_head: String = inner
        .iter()
        .take(48)
        .map(|b| if (0x20..0x7f).contains(b) { *b as char } else { '.' })
        .collect();
    println!("  ascii[48]: {ascii_head}");
}
