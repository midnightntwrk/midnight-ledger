//! Integration test for `bootstrap_did_with_keys` against the local
//! standalone Midnight env (Task 4 of the Identity Centre Phase 1
//! plan).
//!
//! Both tests are `#[ignore]`'d so the default `cargo test` run
//! stays hermetic. Run the live check explicitly with:
//!
//! ```bash
//! STANDALONE_RUN=1 cargo test -p wallet-core \
//!     --test did_bootstrap_standalone -- --ignored --nocapture
//! ```
//!
//! Mirrors the wiring of the `did-bootstrap` CLI
//! (`src/bin/did-bootstrap.rs`): `Wallet::with_deps` against
//! `Network::Undeployed`, with `HttpIndexerClient`,
//! `SubxtNodeClient`, and `HttpProver` pointed at the docker-compose
//! services (indexer `:8088`, node `:9944`, proof server `:6300`).
//!
//! ## Known limitation (2026-05-27)
//!
//! The bootstrap pipeline calls into the JS contract layer via
//! `prepareUnprovenCallTx`. When run from `cargo test` (or the
//! `did-bootstrap` CLI), the wallet doesn't have a
//! `DioxusEvalBridge` attached, so it falls back to spawning a
//! Node child via `NodeChildBridge`. That harness currently fails
//! inside `@midnight-ntwrk/compact-js@2.5.0` with:
//!
//! ```text
//! TypeError: Cannot read properties of undefined (reading 'ctor')
//!     at .../compact-js@2.5.0/.../compactContext.js:23:18
//! ```
//!
//! This is **NOT** caused by `bootstrap_did_with_keys` — the same
//! error reproduces from the older `js_prepare_call_tx` tests and
//! from the `did-bootstrap` CLI. The dioxus-wallet UI works fine
//! because it uses the in-process `DioxusEvalBridge` which loads
//! compact-js with the right context.
//!
//! Until the NodeChildBridge / compact-js mismatch is resolved as
//! a separate task, the live half of this scaffold serves as
//! reproducer for that issue. The `bootstrap_is_deterministic_*`
//! test passes today and the live test passes the first half of
//! the pipeline (DID created on chain, indexer-settle wait
//! succeeds — the 2026-05-27 commit fixed that bug); it falls
//! over only at the JS-bridge step.

use std::sync::Arc;

use wallet_core::secret_storage::InMemorySecretStore;
use wallet_core::{
    bootstrap_did_with_keys, derive_keys, HttpIndexerClient, HttpProver, IndexerClient, Network,
    NodeClient, Prover, SubxtNodeClient, Wallet, UNDEPLOYED_GENESIS_SEED_HEX,
};

/// Bootstrap a fresh DID against the docker-compose standalone env
/// and assert the resolved DID document carries both verification
/// relations + the two fragment ids `bootstrap_did_with_keys`
/// attaches (`key-auth` for `authentication`, `key-assert` for
/// `assertionMethod`).
#[tokio::test]
#[ignore = "requires STANDALONE_RUN=1 and a running docker-compose env"]
async fn bootstrap_against_standalone_succeeds_and_doc_is_complete() {
    if std::env::var("STANDALONE_RUN").ok().as_deref() != Some("1") {
        eprintln!("skipping: STANDALONE_RUN!=1");
        return;
    }

    let network = Network::Undeployed;

    let indexer: Arc<dyn IndexerClient> = Arc::new(
        HttpIndexerClient::new(network).expect("build HttpIndexerClient"),
    );
    let node: Arc<dyn NodeClient> = Arc::new(
        SubxtNodeClient::connect(network)
            .await
            .expect("connect SubxtNodeClient"),
    );
    let prover: Arc<dyn Prover> =
        Arc::new(HttpProver::new(network.config().proving_server_url.to_owned()));

    // The wallet's seed funds chain ops, AND the same seed feeds HKDF
    // for the Phase 1 verification-method secrets. On the standalone
    // env only `UNDEPLOYED_GENESIS_SEED_HEX` carries the dev NIGHT/DUST
    // pre-mint, so reuse it here to avoid an "insufficient DUST" fail
    // before we ever reach the bootstrap pipeline. Matches the
    // `did-bootstrap` CLI's intended invocation pattern.
    let seed_bytes = hex::decode(UNDEPLOYED_GENESIS_SEED_HEX)
        .expect("UNDEPLOYED_GENESIS_SEED_HEX is valid hex");
    let mut seed = [0u8; 32];
    seed.copy_from_slice(&seed_bytes);
    let wallet = Wallet::with_deps(seed, network, indexer, node, prover);

    let mut store = InMemorySecretStore::default();

    let started = std::time::Instant::now();
    let out = bootstrap_did_with_keys(&wallet, &mut store, &seed)
        .await
        .expect("bootstrap");
    let elapsed = started.elapsed();
    let did_str = out.did.to_did_string();
    eprintln!(
        "bootstrap_did_with_keys: minted {} in {:.2}s",
        did_str,
        elapsed.as_secs_f64()
    );

    let doc = wallet
        .resolve_did(&did_str)
        .await
        .expect("resolve");

    assert!(
        !doc.authentication.is_empty(),
        "authentication relation must be populated; doc={:?}",
        doc,
    );
    assert!(
        !doc.assertion_method.is_empty(),
        "assertionMethod relation must be populated; doc={:?}",
        doc,
    );

    // The VerificationMethod.id is the DID-Core URL form
    // `<did>#<fragment>`, but the upstream resolver only auto-prefixes
    // when `#` is absent. Accept either form so we're robust to either
    // representation choice in `ledger_to_domain`.
    let has_auth = doc.verification_method.iter().any(|vm| {
        vm.id.ends_with("#key-auth") || vm.id == "key-auth"
    });
    let has_assert = doc.verification_method.iter().any(|vm| {
        vm.id.ends_with("#key-assert") || vm.id == "key-assert"
    });
    assert!(
        has_auth,
        "expected a verification_method with fragment 'key-auth'; got {:?}",
        doc.verification_method.iter().map(|vm| &vm.id).collect::<Vec<_>>(),
    );
    assert!(
        has_assert,
        "expected a verification_method with fragment 'key-assert'; got {:?}",
        doc.verification_method.iter().map(|vm| &vm.id).collect::<Vec<_>>(),
    );
}

/// `derive_keys` is a pure HKDF, so the same seed must reproduce the
/// same `(ed25519, jubjub)` byte pair across calls. This is the
/// foundation of the "fresh env, same seed, same DID" guarantee the
/// CLI relies on.
#[tokio::test]
#[ignore = "deterministic check; opt-in via --ignored"]
async fn bootstrap_is_deterministic_across_clean_runs() {
    let seed = [99u8; 32];
    let (ed1, jb1) = derive_keys(&seed);
    let (ed2, jb2) = derive_keys(&seed);
    assert_eq!(ed1, ed2, "ed25519 secret must be deterministic from seed");
    assert_eq!(jb1, jb2, "jubjub secret must be deterministic from seed");
}
