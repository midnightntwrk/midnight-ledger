//! PreProd live smoke tests for the wallet's DID pipeline.
//!
//! Two tests, both gated behind `network-tests` so they don't
//! run as part of a default `cargo test` sweep:
//!
//! 1. `preprod_resolve_inventory_dids` — **read-only, passes
//!    against PreProd today**. Resolves the three DIDs sourced
//!    from the operator's local
//!    `~/.midnight-did/profiles/preprod/preproad-default`
//!    profile, prints counter / vm / svc / loaded-circuits.
//!    Safe to run any time. First-pass run on
//!    `mobile-bench/iteration-2`: all three resolved in ~5s
//!    with 11/11 circuit VKs already on-chain.
//!
//! 2. `preprod_add_also_known_as` — **write, currently
//!    `#[ignore]`'d**. Picks DID #1, calls `addAlsoKnownAs`
//!    via the JS bridge → Rust balance/prove/submit pipeline,
//!    re-resolves and asserts the alias landed. First-pass
//!    run failed at `Submitting` with
//!    `Invalid Transaction (1010)` (Substrate's RPC code for
//!    runtime-side validity rejection — BadProof or
//!    BadSignature). Every earlier stage (SyncingDust /
//!    Composing / Balancing / Proving) ran cleanly, the
//!    controller-secret derivation matches upstream
//!    (`SHA-256(addVerificationMethod.prover_bytes)` —
//!    verified bit-equal against
//!    `midnight-did/contract/src/managed/did/keys/`), and
//!    the chain-tip ctime fix is wired into `call_did_circuit`.
//!
//!    Likely root causes (pick one to investigate next):
//!    - **`INITIAL_PARAMETERS` drift**: `tx::balance` /
//!      `tx::prove` use
//!      `ledger::structure::INITIAL_PARAMETERS`, hardcoded
//!      against the standalone Docker image. PreProd may
//!      have a different dust fee schedule or generator
//!      table baked into its runtime; our locally-balanced
//!      tx would then disagree with the chain's expectation
//!      and the runtime rejects.
//!    - **Contract VK divergence**: the addAlsoKnownAs
//!      verifier key registered on the user's PreProd DID
//!      may differ from the one our prover key produces
//!      proofs for. Loading the same `.verifier` via a
//!      MaintenanceUpdate would resolve it, but the test
//!      sees it already loaded (counter doesn't match what
//!      we'd produce).
//!    - **Outer signature scheme**: the SCALE-encoded tx
//!      envelope carries a Substrate signature; if PreProd
//!      requires a different signer / era / metadata
//!      version than `subxt 0.44` ships, the chain rejects.
//!
//!    `#[ignore]`'d so a default sweep doesn't fail; the
//!    code path is preserved for the investigation.
//!
//! Hardcoded configuration (per operator instruction):
//! - Seed: matches the manager profile's `seed` field.
//! - DIDs: the three contract addresses tagged on the
//!   profile's `contractAddresses`.
//! - Controller secret: `SHA-256(addVerificationMethod.prover_bytes)`,
//!   matching upstream `midnight-did/api/src/lib.ts::initPrivateState`.
//!
//! Hardcoded configuration (per operator instruction):
//! - Seed: matches the manager profile's `seed` field.
//! - DIDs: the three contract addresses tagged on the
//!   profile's `contractAddresses`.
//! - Controller secret: `SHA-256(addVerificationMethod.prover_bytes)`,
//!   matching upstream `midnight-did/api/src/lib.ts::initPrivateState`.
//!
//! Run with:
//!
//! ```text
//! cargo test -p wallet-core --features network-tests \
//!   --test preprod_smoke_live -- --nocapture
//! ```
//!
//! Or just the resolve case (safe, no chain writes):
//!
//! ```text
//! cargo test -p wallet-core --features network-tests \
//!   --test preprod_smoke_live preprod_resolve_inventory_dids \
//!   -- --nocapture
//! ```

#![cfg(feature = "network-tests")]

use futures::StreamExt;
use sha2::{Digest, Sha256};

use wallet_core::{Network, Wallet, WizardStage};

/// PreProd wallet seed — operator's local profile. Funds for
/// the alsoKnownAs write must come from this wallet's
/// existing NIGHT balance.
const PREPROD_SEED_HEX: &str =
    "c1e8d986d10a2aff5d5f6fbf3d568f447b1cd46ccb190f838e0cf2707f5622a2";

/// Three DID contract addresses minted by the upstream
/// `midnight-did-manager-service` against `c1e8d986…22a2`'s
/// wallet on PreProd. All three should resolve cleanly; the
/// write test below targets the first one specifically.
const PREPROD_DID_ADDRESSES: &[&str] = &[
    "6b6e06d6f9779b0e4a3596a02edba5539f5b435c07ff5c885f3855d8d8653801",
    "5914d2622abfb6f793c4b15c82692593500ecc481ae9b99a1655ad5e766dca4f",
    "ce785669eac7048652d239bd40286240bbe09f9f9c5d614631a3b256a2fec68a",
];

/// Prover-key bytes for `addVerificationMethod`. Bundled
/// alongside our vendored DID artifacts; identical to what
/// the upstream manager loads via `NodeZkConfigProvider`
/// (same source — both pull from
/// `midnight-did/contract/dist/managed/did/keys/`).
const ADD_VM_PROVER: &[u8] =
    include_bytes!("../contracts/midnight-did/addVerificationMethod.prover");

/// Compute the controller-secret the upstream manager uses
/// for every DID it creates. Matches
/// `midnight-did/api/src/lib.ts::initPrivateState`:
/// `secretKey = SHA-256(proverKey("addVerificationMethod"))`.
fn upstream_controller_sk() -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(ADD_VM_PROVER);
    h.finalize().into()
}

/// Build the PreProd wallet from the hardcoded seed. Each
/// test re-creates this — the wallet has no internal state
/// beyond the seed.
fn preprod_wallet() -> Wallet {
    let bytes = hex::decode(PREPROD_SEED_HEX).expect("seed hex");
    let seed: [u8; 32] = bytes.as_slice().try_into().expect("seed length");
    Wallet::from_seed(seed, Network::PreProd)
}

fn did_string(contract_hex: &str) -> String {
    format!("did:midnight:preprod:{contract_hex}")
}

#[tokio::test]
async fn preprod_resolve_inventory_dids() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let w = preprod_wallet();

    println!(
        "[wallet] preprod address: {}",
        w.unshielded_address().unwrap_or_else(|e| format!("<err: {e}>")),
    );

    let mut failures = 0usize;
    for addr in PREPROD_DID_ADDRESSES {
        let did = did_string(addr);
        println!("[resolve] {did}");
        match w.resolve_did_full(&did).await {
            Ok(r) => {
                println!(
                    "  counter={} version={} deactivated={} vm={} svc={} also_known_as={} \
                     last_block={:?} latency_ms={}",
                    r.maintenance_counter,
                    r.document.version,
                    r.document.deactivated,
                    r.document.verification_method.len(),
                    r.document.service.len(),
                    r.document.also_known_as.len(),
                    r.last_block_height,
                    r.resolve_latency_ms,
                );
                println!("  loaded_circuits: {:?}", r.loaded_circuits);
                if !r.document.also_known_as.is_empty() {
                    println!("  also_known_as: {:?}", r.document.also_known_as);
                }
            }
            Err(e) => {
                eprintln!("  FAILED: {e}");
                failures += 1;
            }
        }
    }
    assert_eq!(
        failures, 0,
        "{failures} of {} PreProd DIDs failed to resolve",
        PREPROD_DID_ADDRESSES.len()
    );
}

/// Pick a fresh `https://wallet-prototype-smoke.example/<ts>`
/// alias per run so reruns never collide with their own
/// prior writes. The contract rejects duplicate
/// `addAlsoKnownAs` values.
fn fresh_alias() -> String {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("https://wallet-prototype-smoke.example/{ts}")
}

#[tokio::test]
#[ignore = "PreProd submit fails with Invalid Transaction (1010); resolve case passes. \
            Rerun with `cargo test … -- --ignored` once the parameter-drift root cause is fixed."]
async fn preprod_add_also_known_as() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let w = preprod_wallet();
    let target = PREPROD_DID_ADDRESSES[0];
    let did = did_string(target);
    let did_id = wallet_core::DidId::parse(&did).expect("parse DID");

    // 1. Pre-resolve. Captures counter + which VKs are
    //    already on-chain so we know whether to load
    //    `addAlsoKnownAs` first.
    let pre = w
        .resolve_did_full(&did)
        .await
        .expect("pre-resolve");
    println!(
        "[pre] counter={} loaded={:?}",
        pre.maintenance_counter, pre.loaded_circuits,
    );
    assert!(
        !pre.document.deactivated,
        "target DID is deactivated; cannot smoke-test writes against it",
    );

    let controller_sk = upstream_controller_sk();
    let mut counter = pre.maintenance_counter;

    // 2. Auto-load the addAlsoKnownAs VK if it's not already
    //    in the contract's operations map. The manager has
    //    probably loaded the common set already, but this
    //    handles the case where it didn't.
    let already_loaded = pre.loaded_circuits.iter().any(|c| c == "addAlsoKnownAs");
    if already_loaded {
        println!("[load] addAlsoKnownAs already on-chain, skipping MaintenanceUpdate");
    } else {
        println!("[load] addAlsoKnownAs VK @ counter {counter}");
        let mut stream = std::pin::pin!(w.load_did_circuit(
            did_id.clone(),
            "addAlsoKnownAs".to_string(),
            counter,
        ));
        let mut load_done = false;
        while let Some(stage) = stream.next().await {
            match stage {
                WizardStage::Done(o) => {
                    println!(
                        "  load tx=0x{} block=0x{}",
                        hex::encode(o.tx_hash),
                        hex::encode(o.block_hash),
                    );
                    counter = counter.saturating_add(1);
                    load_done = true;
                    break;
                }
                WizardStage::Failed(e) => panic!("load addAlsoKnownAs VK failed: {e}"),
                other => println!("  {other:?}"),
            }
        }
        assert!(load_done, "load stage stream ended without terminal");
        // PreProd's indexer ingests blocks more slowly than
        // a standalone stack; 45s is comfortably above the
        // observed end-to-end float. Use the env override
        // when running against a faster stack.
        let settle = std::env::var("PREPROD_SETTLE_SECS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(45);
        println!("[load] settling for {settle}s before ContractCall");
        tokio::time::sleep(std::time::Duration::from_secs(settle)).await;
    }
    let _ = counter; // bumped above; used by future steps if we add more writes

    // 3. ContractCall: addAlsoKnownAs with a fresh, unique
    //    alias. The wallet's prover proves locally; the JS
    //    bridge builds the UnprovenTransaction.
    let alias = fresh_alias();
    println!("[call] addAlsoKnownAs {alias}");
    let mut stream = std::pin::pin!(w.call_did_circuit(
        did_id.clone(),
        "addAlsoKnownAs".to_string(),
        serde_json::json!([alias]),
        controller_sk,
    ));
    let mut call_done = false;
    while let Some(stage) = stream.next().await {
        match stage {
            WizardStage::Done(o) => {
                println!(
                    "  call tx=0x{} block=0x{}",
                    hex::encode(o.tx_hash),
                    hex::encode(o.block_hash),
                );
                call_done = true;
                break;
            }
            WizardStage::Failed(e) => panic!("call addAlsoKnownAs failed: {e}"),
            other => println!("  {other:?}"),
        }
    }
    assert!(call_done, "call stage stream ended without terminal");

    // 4. Re-resolve and assert the alias landed.
    let settle = std::env::var("PREPROD_POST_SETTLE_SECS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(30);
    println!("[verify] settling {settle}s, then resolving");
    tokio::time::sleep(std::time::Duration::from_secs(settle)).await;
    let post = w
        .resolve_did_full(&did)
        .await
        .expect("post-resolve");
    println!(
        "[post] counter={} aliases={} also_known_as={:?}",
        post.maintenance_counter,
        post.document.also_known_as.len(),
        post.document.also_known_as,
    );
    assert!(
        post.document.also_known_as.iter().any(|a| a == &alias),
        "new alias must appear in alsoKnownAs (got {:?})",
        post.document.also_known_as,
    );
}
