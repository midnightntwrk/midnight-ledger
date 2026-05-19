//! Probe: do the verifier-key bytes registered on the
//! operator's PreProd DID match what our bundled
//! `.verifier` files would produce a proof for?
//!
//! Procedure: pull the contract state via the indexer,
//! decode the `operations` map, extract each circuit's
//! `VerifierKey`, and compare its SHA-256 fingerprint to
//! our bundled `<circuit>.verifier` file's fingerprint.
//!
//! A mismatch on any single circuit explains the
//! `Invalid Transaction (1010)` BadProof rejection — the
//! chain verifies our proof against the on-chain VK; if
//! the on-chain VK came from a different circuit
//! compilation than our prover key targets, no proof will
//! ever pass.
//!
//! Read-only (just queries the indexer). Run with:
//!
//! ```text
//! cargo test -p wallet-core --features network-tests \
//!   --test preprod_vk_diff -- --nocapture
//! ```

#![cfg(feature = "network-tests")]

use sha2::{Digest, Sha256};

const PREPROD_DID_ADDR: &str =
    "6b6e06d6f9779b0e4a3596a02edba5539f5b435c07ff5c885f3855d8d8653801";

/// Pull the contract state hex for the address via the
/// PreProd indexer. Same path the wallet's
/// `Wallet::resolve_did_full` walks.
async fn fetch_contract_state_hex(addr: &str) -> String {
    let client = wallet_core::IndexerClient::new(wallet_core::Network::PreProd)
        .expect("IndexerClient::new");
    let info = client
        .contract_state(addr)
        .await
        .expect("contract_state rpc")
        .expect("contract has state");
    info.state_hex
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    let out = h.finalize();
    hex::encode(out)
}

fn bundled_verifier_hex(circuit: &str) -> String {
    let bytes: &[u8] = match circuit {
        "addAlsoKnownAs" => include_bytes!(
            "../contracts/midnight-did/addAlsoKnownAs.verifier"
        ),
        "addService" => include_bytes!(
            "../contracts/midnight-did/addService.verifier"
        ),
        "addVerificationMethod" => include_bytes!(
            "../contracts/midnight-did/addVerificationMethod.verifier"
        ),
        "addVerificationMethodRelation" => include_bytes!(
            "../contracts/midnight-did/addVerificationMethodRelation.verifier"
        ),
        "deactivate" => include_bytes!(
            "../contracts/midnight-did/deactivate.verifier"
        ),
        "removeAlsoKnownAs" => include_bytes!(
            "../contracts/midnight-did/removeAlsoKnownAs.verifier"
        ),
        "removeService" => include_bytes!(
            "../contracts/midnight-did/removeService.verifier"
        ),
        "removeVerificationMethod" => include_bytes!(
            "../contracts/midnight-did/removeVerificationMethod.verifier"
        ),
        "removeVerificationMethodRelation" => include_bytes!(
            "../contracts/midnight-did/removeVerificationMethodRelation.verifier"
        ),
        "updateService" => include_bytes!(
            "../contracts/midnight-did/updateService.verifier"
        ),
        "updateVerificationMethod" => include_bytes!(
            "../contracts/midnight-did/updateVerificationMethod.verifier"
        ),
        _ => panic!("unknown circuit name: {circuit}"),
    };
    sha256_hex(bytes)
}

#[tokio::test]
async fn preprod_vk_bytes_match_bundle() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let state_hex = fetch_contract_state_hex(PREPROD_DID_ADDR).await;

    let bytes = hex::decode(state_hex.trim_start_matches("0x")).expect("hex");
    use onchain_state::state::ContractState;
    use serialize::tagged_deserialize;
    use storage::DefaultDB;
    let state: ContractState<DefaultDB> =
        tagged_deserialize(&bytes[..]).expect("decode ContractState");

    println!(
        "[chain] {} circuits registered on DID {}",
        state.operations.iter().count(),
        PREPROD_DID_ADDR,
    );

    let mut mismatches = Vec::new();
    for entry in state.operations.iter() {
        let (ep, op) = &*entry;
        let name = match std::str::from_utf8(&ep[..]) {
            Ok(s) => s.to_string(),
            Err(_) => continue,
        };
        // `ContractOperation::v2` is the current `Option<VerifierKey>`
        // slot. Tag-serialise the inner VK to get the canonical
        // wire bytes so we're comparing apples to apples with
        // the bundled `.verifier` files (which are also tagged).
        let mut on_chain_bytes = Vec::new();
        if let Some(vk) = op.latest() {
            use serialize::Serializable;
            <transient_crypto::proofs::VerifierKey as Serializable>::serialize(
                vk,
                &mut on_chain_bytes,
            )
            .expect("VerifierKey serialize");
        }
        let on_chain_hex = sha256_hex(&on_chain_bytes);
        let bundled_hex = bundled_verifier_hex(&name);
        let match_marker = if on_chain_hex == bundled_hex { "MATCH" } else { "MISMATCH" };
        println!(
            "  {match_marker:8} {name:35} chain={} bundle={}",
            &on_chain_hex[..16],
            &bundled_hex[..16],
        );
        if on_chain_hex != bundled_hex {
            mismatches.push((name, on_chain_hex, bundled_hex));
        }
    }

    if !mismatches.is_empty() {
        println!(
            "\n[summary] {}/{} VKs diverge between chain and bundle:",
            mismatches.len(),
            state.operations.iter().count(),
        );
        for (name, chain, bundle) in &mismatches {
            println!("  {name}\n    chain : {chain}\n    bundle: {bundle}");
        }
        println!(
            "\nThis explains the `Invalid Transaction (1010)` BadProof rejection \
             on writes — the chain validates our proof against the *stored* VK, which \
             was put there by an earlier circuit compilation than the prover key \
             our wallet ships with."
        );
        // Don't `panic!` — we want the run to show its findings
        // even on failure. The mismatch IS the answer.
    }
}
