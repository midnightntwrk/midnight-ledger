//! Deserialize both `/tmp/our-tx.hex` and `/tmp/upstream-tx.hex` into the
//! same `Transaction<Signature, ProofMarker, PureGeneratorPedersen,
//! DefaultDB>` type and print structural diffs — proves the early
//! byte divergence at offset 71 corresponds to a specific field.
//!
//! Run:
//!   cargo test -p wallet-core --features network-tests \
//!     --test preprod_decode_diff -- --nocapture

#![cfg(feature = "network-tests")]

use base_crypto::signatures::Signature;
use ledger::structure::{ProofMarker, Transaction};
use serialize::tagged_deserialize;
use storage::DefaultDB;
use transient_crypto::commitment::PureGeneratorPedersen;

type Tx = Transaction<Signature, ProofMarker, PureGeneratorPedersen, DefaultDB>;

fn load_hex(path: &str) -> Vec<u8> {
    let h = std::fs::read_to_string(path).expect("read hex");
    hex::decode(h.trim()).expect("hex decode")
}

#[test]
fn diff_decoded_transactions() {
    let ours_bytes = load_hex("/tmp/our-tx.hex");
    let upst_bytes = load_hex("/tmp/upstream-tx.hex");

    println!("=== sizes ===");
    println!("  ours    : {} bytes", ours_bytes.len());
    println!("  upstream: {} bytes", upst_bytes.len());

    println!("\n=== deserializing ours ===");
    let ours: Tx = tagged_deserialize(&ours_bytes[..]).expect("our tx decode");

    println!("\n=== deserializing upstream ===");
    let upst: Tx = tagged_deserialize(&upst_bytes[..]).expect("upstream tx decode");

    // Force the comparison via Debug formatting; both should be Standard variant.
    println!("\n=== ours (Debug) ===");
    println!("{ours:#?}");
    println!("\n=== upstream (Debug) ===");
    println!("{upst:#?}");
}
