//! One-shot helper: resolves each PreProd-default DID from the
//! indexer, walks its verification methods, and prints a
//! `key_id → did` mapping that the operator can fold into
//! `mobile-bench/dioxus-wallet/preprod_keys.json` so the App
//! imports each key tagged with the DID it belongs to.
//!
//! Run with:
//!   cargo test -p wallet-core --features network-tests \
//!     --test annotate_preprod_keys -- --ignored --nocapture
//!
//! Reads `/tmp/manager-keys.json` (the output of
//! `scripts/dump-manager-keys.mjs`) and prints an annotated copy
//! to stdout. Pipe to `preprod_keys.json` if happy:
//!
//!   cargo test ... -- --ignored --nocapture 2>&1 \
//!     | sed -n '/^\[/,/^\]/p' \
//!     > mobile-bench/dioxus-wallet/preprod_keys.json

#![cfg(feature = "network-tests")]

use serde_json::Value;
use std::collections::HashMap;
use std::fs;

use wallet_core::{Network, Wallet};

const DIDS: &[&str] = &[
    "6b6e06d6f9779b0e4a3596a02edba5539f5b435c07ff5c885f3855d8d8653801",
    "5914d2622abfb6f793c4b15c82692593500ecc481ae9b99a1655ad5e766dca4f",
    "ce785669eac7048652d239bd40286240bbe09f9f9c5d614631a3b256a2fec68a",
];

#[tokio::test]
#[ignore = "live indexer query — run manually"]
async fn annotate_preprod_keys() {
    let path = std::env::var("KEYS_JSON")
        .unwrap_or_else(|_| "/tmp/manager-keys.json".into());
    let raw = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {}: {e}", path));
    let mut keys: Value = serde_json::from_str(&raw).expect("parse keys json");
    let arr = keys.as_array_mut().expect("top-level array");

    // Build a map: pkjwk.x → DID for every VM across every DID.
    // Print full VM metadata (kty/crv/x/y/vm_id) so we can spot
    // encoding mismatches vs. truly different keys.
    let mut x_to_did: HashMap<String, String> = HashMap::new();
    let wallet = Wallet::demo(Network::PreProd);
    for addr in DIDS {
        let did = format!("did:midnight:preprod:{addr}");
        let resolved = wallet
            .resolve_did_full(&did)
            .await
            .unwrap_or_else(|e| panic!("resolve {did}: {e}"));
        eprintln!("=== {did} — {} VMs ===", resolved.document.verification_method.len());
        for vm in &resolved.document.verification_method {
            let jwk = serde_json::to_value(&vm.public_key_jwk)
                .expect("serialize jwk");
            eprintln!(
                "  vm.id={:<60} kty={:?} crv={:?} x={} y={}",
                vm.id,
                jwk.get("kty").and_then(Value::as_str).unwrap_or("?"),
                jwk.get("crv").and_then(Value::as_str).unwrap_or("?"),
                jwk.get("x").and_then(Value::as_str).unwrap_or("?"),
                jwk.get("y").and_then(Value::as_str).unwrap_or("(none)"),
            );
            if let Some(x) = jwk.get("x").and_then(Value::as_str) {
                x_to_did.insert(x.to_string(), did.clone());
            }
        }
    }

    // ID-fragment index: vm.id without the leading "#" → DID.
    // We also kept x_to_did above for strict matches. Use:
    //   1. x match (true cryptographic match) — preferred.
    //   2. id-fragment match (name-only match, common when the
    //      operator regenerated profile keys after publishing the
    //      VMs on-chain).
    let mut frag_to_did: HashMap<String, String> = HashMap::new();
    let wallet = Wallet::demo(Network::PreProd);
    for addr in DIDS {
        let did = format!("did:midnight:preprod:{addr}");
        let resolved = wallet
            .resolve_did_full(&did)
            .await
            .unwrap_or_else(|e| panic!("resolve {did}: {e}"));
        for vm in &resolved.document.verification_method {
            let frag = vm.id.trim_start_matches('#').to_string();
            frag_to_did.insert(frag, did.clone());
        }
    }
    let frag_list: Vec<&String> = frag_to_did.keys().collect();

    eprintln!("\n=== matching ===");
    let mut tagged = 0usize;
    for entry in arr.iter_mut() {
        let id = entry.get("id").and_then(Value::as_str).unwrap_or("").to_string();
        let pubx = entry
            .get("public_jwk")
            .and_then(|j| j.get("x"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let mut matched_did: Option<String> = None;
        let mut matched_via = "";
        // Strict crypto match first.
        if let Some(did) = x_to_did.get(&pubx) {
            matched_did = Some(did.clone());
            matched_via = "x-bytes";
        }
        // Fallback: exact id-fragment match.
        if matched_did.is_none() {
            if let Some(did) = frag_to_did.get(&id) {
                matched_did = Some(did.clone());
                matched_via = "id-exact";
            }
        }
        // Fallback: substring id match in ONE direction — the
        // profile-side id may have a prefix/suffix the on-chain
        // fragment lacks (e.g. `preprod-auth-main` → `#auth-main`,
        // `one-more-key-1` → `#one-more-key`). The reverse
        // direction (fragment contains id) is forbidden because
        // it leads to false positives like `key-1` matching
        // `#demo-key-1`. Prefer the LONGEST fragment match so a
        // profile id containing multiple fragments lands on the
        // most specific one.
        if matched_did.is_none() {
            let mut best: Option<&String> = None;
            for frag in &frag_list {
                if id.contains(frag.as_str()) {
                    if best.map(|b| frag.len() > b.len()).unwrap_or(true) {
                        best = Some(*frag);
                    }
                }
            }
            if let Some(frag) = best {
                matched_did = Some(frag_to_did[frag].clone());
                matched_via = "id-fuzzy";
            }
        }
        let did_str = matched_did.clone().unwrap_or_else(|| "(none)".into());
        eprintln!("  {id:20} → {did_str}  via={matched_via}");
        if let Some(did) = matched_did {
            entry["did"] = Value::String(did);
            tagged += 1;
        }
    }
    eprintln!("=== tagged {tagged} / {} keys ===", arr.len());

    // Drop the `meta` field — it's diagnostic from the dumper,
    // not consumed by the App. Keep the JSON lean.
    for entry in arr.iter_mut() {
        if let Some(obj) = entry.as_object_mut() {
            obj.remove("meta");
        }
    }

    // Write the annotated JSON directly to the App's bundled
    // file so the next `cargo build` picks it up via the
    // `include_str!("../preprod_keys.json")` baked into the
    // `preprod-live` feature.
    // Resolve relative to wallet-core's CARGO_MANIFEST_DIR so
    // `cargo test` can be invoked from any working directory.
    let out = std::env::var("OUT_FILE").unwrap_or_else(|_| {
        let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        manifest
            .parent()
            .expect("wallet-core has a parent dir")
            .join("dioxus-wallet")
            .join("preprod_keys.json")
            .to_string_lossy()
            .into_owned()
    });
    let count = arr.len();
    let body = serde_json::to_string_pretty(&keys).unwrap();
    fs::write(&out, body).unwrap_or_else(|e| panic!("write {out}: {e}"));
    eprintln!("wrote {count} keys to {out}");
}
