//! Use-case-by-use-case integration tests for the
//! `wallet_core::headless::HeadlessWallet` façade.
//!
//! These tests drive **live** dependencies — the local standalone
//! Midnight env (indexer / node / proof-server) plus the
//! `IssuerDIDIT-mock` service on `:3001`. They're `#[ignore]`'d so
//! `cargo test` stays hermetic; run the live suite explicitly:
//!
//! ```bash
//! # bring the chain up
//! docker compose -f .../standalone/docker-compose.yml up -d
//! # bring the issuer up on :3001
//! (cd .../IssuerDIDIT-mock && nvm use 24 && pnpm dev) &
//!
//! HEADLESS_LIVE=1 cargo test -p wallet-core --features test-support \
//!     --test headless_use_cases_e2e -- --ignored --nocapture
//! ```
//!
//! ## Scope
//!
//! Each test exercises ONE use case end-to-end through the
//! façade — same orchestrator the Dioxus shell drives, same
//! adapters the production wallet uses. The goal is "if this
//! suite is green, the use case works on real chain". Adding a
//! new use case = adding a new `#[tokio::test] #[ignore]` here
//! and letting the headless façade do the heavy lifting.
//!
//! Spec: `docs/superpowers/specs/2026-06-03-hex-architecture-audit.md`
//! §9 (headless wallet capabilities → use-case integration
//! tests).

#![cfg(feature = "test-support")]

use std::path::PathBuf;

use wallet_core::headless::{HeadlessConfig, HeadlessWallet};
use wallet_core::Network;

/// Run an async block on a dedicated `std::thread` with an 8-MiB
/// stack — the orchestrator state machines (`bootstrap_did_with_keys`,
/// `run_authentication`, `run_issuance`) blow the default
/// ~2-MiB test-thread stack on debug builds. This is the same
/// rationale the wallet-worker thread uses in `dioxus-wallet`:
/// see `docs/superpowers/specs/2026-06-02-wallet-worker-thread.md`.
fn run_on_fat_stack<F>(f: F)
where
    F: FnOnce() + Send + 'static,
{
    std::thread::Builder::new()
        .name("headless-test".into())
        .stack_size(8 << 20)
        .spawn(f)
        .expect("spawn fat-stack test thread")
        .join()
        .expect("test thread panicked");
}

/// Build a current-thread tokio runtime + block_on the future.
/// Pair with `run_on_fat_stack` for the full "deep async state
/// machine + 8-MiB stack" environment.
fn block_on<F: std::future::Future<Output = ()>>(f: F) {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build tokio runtime")
        .block_on(f);
}

/// Funded standalone genesis seed (`UNDEPLOYED_GENESIS_SEED_HEX`).
/// Every test in this file uses it because the live standalone
/// env grants this seed enough DUST + NIGHT to pay for tx fees.
const GENESIS_SEED: [u8; 32] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1,
];

/// Create a unique VC store path so concurrent test runs don't
/// trample each other's redb files. Uses the current PID + a
/// nanosecond timestamp.
fn fresh_vc_store_path(stem: &str) -> PathBuf {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("headless-{stem}-{}-{}.redb", std::process::id(), nanos))
}

fn live_config(stem: &str) -> HeadlessConfig {
    HeadlessConfig {
        network: Network::Undeployed,
        seed: GENESIS_SEED,
        vc_store_path: fresh_vc_store_path(stem),
        proof_server_url: Some("http://localhost:16300".into()),
    }
}

/// Read the issuer's /authorize page, extract the QR URL that
/// embeds the request_uri pointing at /request/<session_id>. The
/// issuer must be running on `:3001`.
async fn fetch_login_qr_url() -> String {
    let body = reqwest::get("http://localhost:3001/authorize")
        .await
        .expect("issuer /authorize unreachable — is the issuer-mock running?")
        .text()
        .await
        .expect("read /authorize body");
    // The login template embeds the QR URL inline as text.
    body.lines()
        .filter_map(|line| {
            let i = line.find("openid4vp://")?;
            let tail = &line[i..];
            // Cut at the first `<`, `"`, or whitespace.
            let end = tail
                .find(|c: char| c == '<' || c == '"' || c.is_whitespace())
                .unwrap_or(tail.len());
            Some(tail[..end].to_string())
        })
        .next()
        .expect("no openid4vp:// URL in /authorize response")
}

// ── Use case: Bootstrap a DID ───────────────────────────────────

/// The minimal happy path: connect to chain, bootstrap a DID,
/// confirm it has a sane structure. Proves the headless façade
/// wires its dependencies correctly + the JS bridge survives
/// outside the Dioxus runtime.
///
/// **Pre-conditions:** standalone env up.
#[test]
#[ignore = "needs live standalone env; opt in with --ignored"]
fn bootstrap_against_live_chain() {
    run_on_fat_stack(|| {
        block_on(async {
            wallet_core::ensure_default_crypto_provider();
            let w = HeadlessWallet::connect(live_config("bootstrap"))
                .await
                .expect("connect");
            let out = w.bootstrap(GENESIS_SEED).await.expect("bootstrap");

            let did_str = out.did.to_did_string();
            assert!(did_str.starts_with("did:midnight:undeployed:"), "{did_str}");
            let addr = did_str.trim_start_matches("did:midnight:undeployed:");
            assert_eq!(addr.len(), 64, "address should be 32 bytes hex: {addr}");
            assert!(
                addr.chars().all(|c| c.is_ascii_hexdigit()),
                "non-hex chars in {addr}",
            );
            assert_eq!(out.controller_sk.len(), 32);
        });
    });
}

// ── Use case: Bootstrap → OID4VP login round-trip ───────────────

/// Bootstrap, then drive an OID4VP login against the live issuer.
/// The issuer's /authorize endpoint starts a session + returns a
/// QR URL; the headless wallet scans it, mints an id_token, and
/// POSTs back. Success means the issuer responded with the
/// `"authenticated"` status.
///
/// **Pre-conditions:** standalone env + issuer-mock on `:3001`.
#[test]
#[ignore = "needs live standalone + issuer; opt in with --ignored"]
fn bootstrap_then_login_round_trip() {
    run_on_fat_stack(|| {
        block_on(async {
            wallet_core::ensure_default_crypto_provider();
            let w = HeadlessWallet::connect(live_config("login"))
                .await
                .expect("connect");
            let out = w.bootstrap(GENESIS_SEED).await.expect("bootstrap");

            // Pull a fresh QR URL from the issuer. Each call to
            // `/authorize` opens a new session, so two concurrent
            // test runs don't interfere with each other.
            let qr_url = fetch_login_qr_url().await;
            let r = w.login(out.did.clone(), &qr_url).await.expect("login");
            assert_eq!(
                r.status, "authenticated",
                "expected issuer to authenticate the holder, got status={status}, session_id={session_id}",
                status = r.status,
                session_id = r.session_id,
            );
            assert!(!r.session_id.is_empty(), "session_id should be non-empty");
        });
    });
}
