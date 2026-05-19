//! Probe + (gated) write path for the DID maintenance authority.
//!
//! ### Why this file exists
//!
//! Originally written under the (now-refuted) hypothesis that the
//! operator's PreProd DIDs had stale on-chain VKs that needed to
//! be reloaded via `MaintenanceUpdate`. After fixing the
//! `preprod_vk_diff` probe to use `tagged_serialize`, all 11
//! on-chain VKs match our bundle byte-for-byte — so the VK reload
//! is NOT what the demo needs to succeed.
//!
//! The probe in this file remains valuable for a different
//! reason: it confirms whether our wallet's HD-derived
//! maintenance VK is in a DID's committee. For operator-deployed
//! DIDs the answer is NO (each was deployed with a per-DID
//! random key by upstream `@midnight-ntwrk/midnight-js-contracts::
//! deployContract`), so any future MaintenanceUpdate from our
//! wallet against those DIDs would be rejected at signature
//! verification — distinct from the BadProof we see on
//! ContractCall. For wallet-deployed DIDs the answer is YES (the
//! committee is set to our HD key at deploy time).
//!
//! The reload test is still here, gated behind
//! `PREPROD_AUTHORIZE_VK_RELOAD=1` and an in-test assertion that
//! our key is in the committee. It's a no-op for the current
//! PreProd DIDs (assertion fails fast, no DUST spent) but stays
//! useful for any future case where we own deploys.
//!
//! Two tests in this file:
//!
//! 1. `preprod_maintenance_authority_probe` (read-only, always
//!    runs under `network-tests`):
//!    - Decodes `ContractState` for each of the operator's three
//!      PreProd DIDs.
//!    - Prints `maintenance_authority.{committee, threshold,
//!      counter}`.
//!    - Compares the on-chain committee members to our wallet's
//!      `did_maintenance_verifying_key()` derived from the same
//!      PreProd seed. If our key is NOT in any DID's committee,
//!      Path B is unreachable — writes from our wallet will be
//!      rejected at the signature-check stage instead of the
//!      proof-verification stage.
//!
//! 2. `preprod_reload_vks_live` (write, gated behind
//!    `PREPROD_AUTHORIZE_VK_RELOAD=1`): loops `load_did_circuit`
//!    for all 11 circuit names against the first DID, walking the
//!    maintenance counter forward as each tx confirms. Each
//!    iteration replaces one stored VK with our bundled bytes.
//!    Spends DUST (one MaintenanceUpdate per circuit).
//!
//! Run with:
//!
//! ```text
//! cargo test -p wallet-core --features network-tests \
//!   --test preprod_maintenance_authority -- --nocapture
//!
//! # plus, for test 2:
//! PREPROD_AUTHORIZE_VK_RELOAD=1 cargo test ...
//! ```

#![cfg(feature = "network-tests")]

use futures::StreamExt;
use wallet_core::{Network, Wallet, WizardStage};

/// Hardcoded operator seed (also in `preprod_smoke_live.rs`). The
/// wallet uses this seed both to fund the MaintenanceUpdate and to
/// derive the maintenance-authority signing key — so it must match
/// what was used at deploy time.
const PREPROD_SEED_HEX: &str =
    "c1e8d986d10a2aff5d5f6fbf3d568f447b1cd46ccb190f838e0cf2707f5622a2";

const PREPROD_DID_ADDRESSES: &[&str] = &[
    "6b6e06d6f9779b0e4a3596a02edba5539f5b435c07ff5c885f3855d8d8653801",
    "5914d2622abfb6f793c4b15c82692593500ecc481ae9b99a1655ad5e766dca4f",
    "ce785669eac7048652d239bd40286240bbe09f9f9c5d614631a3b256a2fec68a",
];

const CIRCUIT_NAMES: &[&str] = &[
    "addAlsoKnownAs",
    "addService",
    "addVerificationMethod",
    "addVerificationMethodRelation",
    "deactivate",
    "removeAlsoKnownAs",
    "removeService",
    "removeVerificationMethod",
    "removeVerificationMethodRelation",
    "updateService",
    "updateVerificationMethod",
];

fn preprod_wallet() -> Wallet {
    let bytes = hex::decode(PREPROD_SEED_HEX).expect("seed hex");
    let seed: [u8; 32] = bytes.as_slice().try_into().expect("seed length");
    Wallet::from_seed(seed, Network::PreProd)
}

fn did_string(contract_hex: &str) -> String {
    format!("did:midnight:preprod:{contract_hex}")
}

/// Serialize a `VerifyingKey` to its tagged on-wire bytes so two
/// keys can be compared in hex even when the upstream type doesn't
/// expose a stable `Display` impl.
fn vk_hex(vk: &base_crypto::signatures::VerifyingKey) -> String {
    use serialize::Serializable;
    let mut buf = Vec::new();
    <base_crypto::signatures::VerifyingKey as Serializable>::serialize(vk, &mut buf)
        .expect("VerifyingKey serialize");
    hex::encode(buf)
}

/// Pull a DID's `ContractState` from the PreProd indexer.
async fn fetch_contract_state(
    addr: &str,
) -> onchain_state::state::ContractState<storage::DefaultDB> {
    let client = wallet_core::IndexerClient::new(Network::PreProd).expect("IndexerClient::new");
    let info = client
        .contract_state(addr)
        .await
        .expect("contract_state rpc")
        .expect("contract has state");
    let bytes = hex::decode(info.state_hex.trim_start_matches("0x")).expect("hex");
    serialize::tagged_deserialize(&bytes[..]).expect("decode ContractState")
}

#[tokio::test]
async fn preprod_maintenance_authority_probe() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let w = preprod_wallet();

    let our_vk = w
        .did_maintenance_verifying_key()
        .expect("derive maintenance VK");
    let our_hex = vk_hex(&our_vk);
    println!("[wallet] our maintenance VK: {our_hex}");

    let mut reachable = Vec::new();
    let mut unreachable = Vec::new();

    for addr in PREPROD_DID_ADDRESSES {
        let did = did_string(addr);
        println!("\n[probe] {did}");
        let state = fetch_contract_state(addr).await;
        let auth = &state.maintenance_authority;
        println!(
            "  threshold={} counter={} committee_size={}",
            auth.threshold,
            auth.counter,
            auth.committee.len(),
        );

        let mut found = false;
        for (i, pk) in auth.committee.iter().enumerate() {
            let pk_hex = vk_hex(pk);
            let marker = if pk_hex == our_hex { " <-- OURS" } else { "" };
            println!("    committee[{i}]: {pk_hex}{marker}");
            if pk_hex == our_hex {
                found = true;
            }
        }
        if found {
            reachable.push(addr.to_string());
        } else {
            unreachable.push(addr.to_string());
        }
    }

    println!("\n[summary]");
    println!(
        "  reachable (our key is in committee): {} DID(s)",
        reachable.len()
    );
    for a in &reachable {
        println!("    {a}");
    }
    println!(
        "  unreachable (our key is NOT in committee): {} DID(s)",
        unreachable.len()
    );
    for a in &unreachable {
        println!("    {a}");
    }

    if reachable.is_empty() {
        println!(
            "\n[verdict] None of our PreProd DIDs have our wallet's key in \
             their maintenance committee. Path B (VK reload via \
             MaintenanceUpdate) is unreachable from this wallet — \
             MaintenanceUpdate signatures will be rejected at the \
             signature-check stage, distinct from the BadProof rejection \
             we see on ContractCall. The only paths forward are:\n  \
             - deploy a fresh DID from this wallet (its committee will \
             be our key, by construction);\n  - or have the original \
             deployer's wallet run the VK reload."
        );
    } else {
        println!(
            "\n[verdict] At least one DID is reachable from this wallet. \
             Run the `preprod_reload_vks_live` test (gated behind \
             PREPROD_AUTHORIZE_VK_RELOAD=1) to overwrite the 11 stored \
             VKs with our bundled bytes."
        );
    }
}

#[tokio::test]
async fn preprod_reload_vks_live() {
    if std::env::var("PREPROD_AUTHORIZE_VK_RELOAD").as_deref() != Ok("1") {
        eprintln!(
            "[skip] PREPROD_AUTHORIZE_VK_RELOAD=1 not set. This test \
             spends real PreProd DUST (one MaintenanceUpdate per circuit \
             × 11 circuits)."
        );
        return;
    }

    let _ = rustls::crypto::ring::default_provider().install_default();
    let w = preprod_wallet();

    let target_addr = PREPROD_DID_ADDRESSES[0];
    let did_str = did_string(target_addr);
    println!("[target] {did_str}");

    // Sanity: confirm our key is in the committee before spending DUST.
    let state = fetch_contract_state(target_addr).await;
    let our_vk = w.did_maintenance_verifying_key().expect("derive vk");
    let our_hex = vk_hex(&our_vk);
    let in_committee = state
        .maintenance_authority
        .committee
        .iter()
        .any(|pk| vk_hex(pk) == our_hex);
    assert!(
        in_committee,
        "our maintenance VK ({our_hex}) is NOT in the committee for {did_str} — \
         the reload would fail at signature verification. Run the probe test \
         first to diagnose."
    );

    let initial_counter = state.maintenance_authority.counter;
    println!("[chain] initial maintenance counter = {initial_counter}");

    let did = wallet_core::DidId::parse(&did_str).expect("parse DID id");

    let mut counter = initial_counter;
    let mut failures: Vec<(String, String)> = Vec::new();
    for name in CIRCUIT_NAMES {
        println!("\n[reload] circuit={name} counter={counter}");
        let mut stream = std::pin::pin!(w.load_did_circuit(
            did.clone(),
            (*name).to_string(),
            counter,
        ));
        let mut outcome = None;
        while let Some(stage) = stream.next().await {
            match stage {
                WizardStage::Done(o) => {
                    println!(
                        "  done tx=0x{} block=0x{}",
                        hex::encode(o.tx_hash),
                        hex::encode(o.block_hash),
                    );
                    outcome = Some(o);
                    break;
                }
                WizardStage::Failed(e) => {
                    eprintln!("  FAILED: {e}");
                    failures.push(((*name).to_string(), e));
                    break;
                }
                other => println!("  stage: {other:?}"),
            }
        }
        if outcome.is_some() {
            counter += 1;
            // Give the indexer a beat to surface the maintenance tx
            // before the next sync_dust pulls the next UTXO set.
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        } else {
            // On failure, refetch the on-chain counter — if the tx
            // landed but our local view drifted, the next iteration
            // would otherwise blow up with the wrong counter.
            match fetch_contract_state(target_addr).await {
                state => {
                    let live = state.maintenance_authority.counter;
                    if live != counter {
                        println!("  counter drift: local={counter} chain={live}; resyncing");
                        counter = live;
                    }
                }
            }
        }
    }

    let final_state = fetch_contract_state(target_addr).await;
    println!(
        "\n[final] maintenance counter = {} (was {initial_counter})",
        final_state.maintenance_authority.counter,
    );

    if !failures.is_empty() {
        println!("\n[failures] {} / {}", failures.len(), CIRCUIT_NAMES.len());
        for (name, err) in &failures {
            println!("  {name}: {err}");
        }
        panic!(
            "{} circuit reload(s) failed — see log above. The chain may be in a \
             half-updated state; rerun this test to pick up where it stopped.",
            failures.len()
        );
    }

    println!("\n[done] all 11 VKs reloaded successfully for {did_str}");
}
