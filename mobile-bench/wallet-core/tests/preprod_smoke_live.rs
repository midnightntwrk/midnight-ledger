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
//! 2. `preprod_set_also_known_as_insert` — **write, currently
//!    `#[ignore]`'d**. Picks DID #1, calls `setAlsoKnownAs(Insert)`
//!    via the JS bridge → Rust balance/prove/submit pipeline,
//!    re-resolves and asserts the alias landed. First-pass
//!    run failed at `Submitting` with
//!    `Invalid Transaction (1010)` (BadProof rejection).
//!
//!    ### Diagnosis status
//!
//!    The earlier "VK divergence" hypothesis was based on a
//!    flawed probe (`preprod_vk_diff` v1) that compared the
//!    bundled `.verifier` file SHA against a *non*-tagged
//!    serialization of the on-chain `VerifierKey`. After
//!    fixing the probe to use `serialize::tagged_serialize`
//!    (which prepends `"midnight:<tag>:"` the same way the
//!    bundled files do), **all 11 on-chain VKs MATCH our
//!    bundle byte-for-byte**.
//!
//!    The remaining ruled-out causes:
//!    - Controller-secret derivation — matches upstream
//!      `SHA-256(addVerificationMethod.prover_bytes)`
//!      bit-equal (`hashProverKey` in
//!      `midnight-did/api/src/lightweight.ts`).
//!    - Prover-key bytes — bundle SHA matches every
//!      consumer-app `node_modules/...midnight-did-contract/`
//!      installation (verified Apr 29 / May 7 / May 13 builds
//!      all share `92fcba0b1020b503…` for
//!      `addVerificationMethod.prover`).
//!    - Chain-tip ctime drift — fix from `06db33af` is wired
//!      into `call_did_circuit`.
//!    - Node version drift — PreProd runs `0.22.2-71fc6804`,
//!      we're pinned to `node-0.22.3` (patch differences).
//!    - Maintenance-authority key — not relevant for
//!      `ContractCall` (only for `MaintenanceUpdate`); see
//!      `preprod_maintenance_authority_probe`.
//!
//!    With all four obvious knobs ruled out, the write
//!    needs to be re-run against the live chain so we can
//!    see what *actually* fails now. The earlier 1010 may
//!    have been the by-now-fixed ctime drift, or a transient
//!    PreProd issue, or a still-unidentified mismatch in
//!    the JS-bridge → Rust handoff. Un-ignore this test and
//!    run with `--nocapture` to surface the next signal.
//!
//! Hardcoded configuration (per operator instruction):
//! - Seed: matches the manager profile's `seed` field.
//! - DIDs: the three contract addresses tagged on the
//!   profile's `contractAddresses`.
//! - Controller secret: `SHA-256(setVerificationMethod.prover_bytes)`,
//!   matching upstream `midnight-did/api/src/lib.ts::initPrivateState`
//!   after the 2026-05-28 schema refresh (the upstream constant
//!   re-hashed the renamed prover key; behaviour against the live
//!   PreProd manager-service has NOT been re-probed).
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

/// Prover-key bytes for `setVerificationMethod`. Post-2026-05-28
/// schema refresh, the upstream's `addVerificationMethod` circuit
/// was renamed to `setVerificationMethod`; the bundled artifact
/// reflects the new name. Whether the manager-service's
/// controller-secret derivation actually hashes THIS prover key
/// (vs. some other constant after the refresh) has NOT been
/// re-probed — punted to a future "re-mint PreProd seed" task.
const SET_VM_PROVER: &[u8] =
    include_bytes!("../contracts/midnight-did/setVerificationMethod.prover");

/// Compute the controller-secret the upstream manager *probably*
/// uses for every DID it creates against the post-2026-05-28
/// schema. Mirrors the historical
/// `midnight-did/api/src/lib.ts::initPrivateState` pattern
/// (`secretKey = SHA-256(proverKey("<rename>"))`) but the rename
/// hasn't been verified against the running manager-service —
/// the `#[ignore]` annotation on the write test below keeps this
/// from being load-bearing in CI.
fn upstream_controller_sk() -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(SET_VM_PROVER);
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
#[ignore = "Spends real PreProd DUST. The earlier VK-divergence hypothesis was \
            refuted (see module docs). Run manually with \
            `cargo test … preprod_set_also_known_as_insert -- --ignored --nocapture` \
            to surface what actually fails on the live chain now. \
            NOTE: 2026-05-28 schema refresh changed circuit names + arg \
            shape; against a pre-refresh PreProd DID this call will fail \
            with `circuit not registered` / `wrong arity`. Re-mint the \
            target DID against a fresh post-refresh deploy first."]
async fn preprod_set_also_known_as_insert() {
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

    // 2. Auto-load the setAlsoKnownAs VK if it's not already
    //    in the contract's operations map. The manager has
    //    probably loaded the common set already, but this
    //    handles the case where it didn't. Post-2026-05-28
    //    schema refresh: the old `addAlsoKnownAs` / `removeAlsoKnownAs`
    //    pair collapsed into the single
    //    `setAlsoKnownAs(value, SetMutation)` entry point.
    let already_loaded = pre.loaded_circuits.iter().any(|c| c == "setAlsoKnownAs");
    if already_loaded {
        println!("[load] setAlsoKnownAs already on-chain, skipping MaintenanceUpdate");
    } else {
        println!("[load] setAlsoKnownAs VK @ counter {counter}");
        let mut stream = std::pin::pin!(w.load_did_circuit(
            did_id.clone(),
            "setAlsoKnownAs".to_string(),
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
                WizardStage::Failed(e) => panic!("load setAlsoKnownAs VK failed: {e}"),
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

    // 3. ContractCall: setAlsoKnownAs with a fresh, unique
    //    alias. The wallet's prover proves locally; the JS
    //    bridge builds the UnprovenTransaction.
    //
    //    Post-2026-05-28 the circuit takes a `SetMutation`
    //    discriminator: 1 = Insert, 2 = Remove. We're inserting.
    let alias = fresh_alias();
    const SET_MUTATION_INSERT: u8 = 1;
    println!("[call] setAlsoKnownAs Insert({alias})");
    let mut stream = std::pin::pin!(w.call_did_circuit(
        did_id.clone(),
        "setAlsoKnownAs".to_string(),
        serde_json::json!([alias, SET_MUTATION_INSERT]),
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
            WizardStage::Failed(e) => panic!("call setAlsoKnownAs failed: {e}"),
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
