//! Probe: do the verifier-key bytes registered on the
//! operator's PreProd DID match what our bundled
//! `.verifier` files would produce a proof for?
//!
//! Procedure: pull the contract state via the indexer,
//! decode the `operations` map, extract each circuit's
//! `VerifierKey`, **tag-serialise** it (so it carries the
//! same `"midnight:<tag>:"` framing the bundled `.verifier`
//! files already have), and compare SHA-256 fingerprints.
//!
//! ### History note (important — read before editing)
//!
//! An earlier version of this probe used `Serializable::serialize`
//! instead of `tagged_serialize`. `Serializable::serialize` writes
//! the bare payload; `tagged_serialize` prepends the global tag
//! prefix that the bundled files *do* carry. The result: every
//! circuit looked like a MISMATCH even when the underlying VKs
//! were byte-identical — purely because the tag prefix was on
//! one side and not the other. That false signal nucleated a
//! whole "PreProd VKs diverged from our bundle" story (which
//! propagated into `preprod_smoke_live` docs and a now-deleted
//! Path B test plan). Don't repeat that mistake: the apples-to-
//! apples form is `tagged_serialize`.
//!
//! ### 2026-05-28 schema refresh
//!
//! The upstream `feat!: Redesign DID verification method storage`
//! commit renamed every `add* / update* / remove*` circuit triple
//! into unified `set*(value, mutation)` entry points, and added
//! a dedicated SchnorrJubjub VM map (+ four `*SchnorrJubjub*` /
//! `rotateControllerKey` circuits). The bundled `.verifier`
//! files now reflect the new shape; the old `add*` / `update*`
//! `.verifier` files no longer exist on disk.
//!
//! Probe behaviour against a PreProd DID that still carries
//! the OLD circuit set: each on-chain operation name is looked
//! up in [`bundled_verifier_hex`]. Names not in the new bundle
//! (e.g. `addVerificationMethod`) come back as `None` and the
//! row prints as `UNBUNDLED` rather than `MATCH`/`MISMATCH` —
//! a deliberate signal that the chain still has a pre-refresh
//! contract and either the seed needs re-minting or the bundle
//! needs to be re-vendored from a matching upstream tag.
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
    let client = wallet_core::HttpIndexerClient::new(wallet_core::Network::PreProd)
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

/// Look up a bundled `.verifier` SHA by circuit name. Returns
/// `None` for any circuit name not in the post-2026-05-28
/// schema — the caller treats that as an `UNBUNDLED` row in
/// the diff table rather than a hard mismatch.
///
/// Names match exactly the set bundled at
/// `contracts/midnight-did/<name>.verifier`. Order is irrelevant.
fn bundled_verifier_hex(circuit: &str) -> Option<String> {
    let bytes: &[u8] = match circuit {
        "deactivate" => include_bytes!("../contracts/midnight-did/deactivate.verifier"),
        "removeSchnorrJubjubVerificationMethod" => include_bytes!(
            "../contracts/midnight-did/removeSchnorrJubjubVerificationMethod.verifier"
        ),
        "removeService" => include_bytes!("../contracts/midnight-did/removeService.verifier"),
        "removeVerificationMethod" => {
            include_bytes!("../contracts/midnight-did/removeVerificationMethod.verifier")
        }
        "rotateControllerKey" => {
            include_bytes!("../contracts/midnight-did/rotateControllerKey.verifier")
        }
        "setAlsoKnownAs" => include_bytes!("../contracts/midnight-did/setAlsoKnownAs.verifier"),
        "setSchnorrJubjubVerificationMethod" => include_bytes!(
            "../contracts/midnight-did/setSchnorrJubjubVerificationMethod.verifier"
        ),
        "setService" => include_bytes!("../contracts/midnight-did/setService.verifier"),
        "setVerificationMethod" => {
            include_bytes!("../contracts/midnight-did/setVerificationMethod.verifier")
        }
        "setVerificationMethodRelation" => {
            include_bytes!("../contracts/midnight-did/setVerificationMethodRelation.verifier")
        }
        "verifySchnorrJubjubDigestSignature" => include_bytes!(
            "../contracts/midnight-did/verifySchnorrJubjubDigestSignature.verifier"
        ),
        _ => return None,
    };
    Some(sha256_hex(bytes))
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
    let mut unbundled = Vec::new();
    for entry in state.operations.iter() {
        let (ep, op) = &*entry;
        let name = match std::str::from_utf8(&ep[..]) {
            Ok(s) => s.to_string(),
            Err(_) => continue,
        };
        // `ContractOperation::v2` is the current `Option<VerifierKey>`
        // slot. Apples-to-apples comparison with the bundled
        // `.verifier` files requires the full **tagged** form:
        // those files are produced by upstream's
        // `tagged_serialize`, which prepends `"midnight:<tag>:"`
        // before the payload. `Serializable::serialize` alone
        // writes only the payload — a previous version of this
        // test used that, which produced false MISMATCHes on
        // every entry because the tag prefix was in one side
        // and not the other.
        let mut on_chain_bytes = Vec::new();
        if let Some(vk) = op.latest() {
            serialize::tagged_serialize(vk, &mut on_chain_bytes)
                .expect("tagged_serialize VerifierKey");
        }
        let on_chain_hex = sha256_hex(&on_chain_bytes);
        match bundled_verifier_hex(&name) {
            None => {
                println!(
                    "  UNBUNDLED {name:35} chain={} (post-2026-05-28 bundle has no \
                     `{name}` — likely a pre-refresh PreProd contract)",
                    &on_chain_hex[..16],
                );
                unbundled.push(name);
            }
            Some(bundled_hex) => {
                let match_marker = if on_chain_hex == bundled_hex { "MATCH" } else { "MISMATCH" };
                println!(
                    "  {match_marker:9} {name:35} chain={} bundle={}",
                    &on_chain_hex[..16],
                    &bundled_hex[..16],
                );
                if on_chain_hex != bundled_hex {
                    mismatches.push((name, on_chain_hex, bundled_hex));
                }
            }
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
    if !unbundled.is_empty() {
        println!(
            "\n[summary] {} on-chain circuit(s) absent from our bundle: {:?}",
            unbundled.len(),
            unbundled,
        );
        println!(
            "          (2026-05-28 schema refresh renamed add*/update*/remove* to \
             set*(value, mutation); PreProd DIDs deployed pre-refresh still expose \
             the old names. Re-mint the DID against a fresh deploy or re-vendor \
             the contract artifacts to a matching upstream tag.)"
        );
    }
}
