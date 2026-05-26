//! `did-bootstrap` — CLI wrapper around `bootstrap_did_with_keys`.
//!
//! Invoked by shell scripts (issuer bootstrap, BDD harness setup,
//! demo recovery) that need to mint a Midnight DID with the
//! Phase 1 verification methods attached, without embedding Rust.
//!
//! Targets the local standalone Midnight env by default
//! (`Network::Undeployed` — indexer on `:8088`, node on `:9944`,
//! proof server on `:6300`).
//!
//! ## Adaptations vs the original Phase 1 plan
//!
//! The plan was written before the Wallet/SecretStorage APIs
//! settled. We diverge in three places:
//!
//! 1. `Wallet::connect_standalone` does not exist — we use the
//!    `with_deps` builder against `Network::Undeployed` to wire a
//!    real-deps `Wallet` from the in-tree `HttpIndexerClient`,
//!    `SubxtNodeClient`, and `HttpProver` / `LocalProver`.
//! 2. `Wallet::secret_store()` does not exist — the CLI owns its
//!    own `InMemorySecretStore` for the bootstrap run, which is
//!    sufficient because the keystore output (below) carries the
//!    raw secret bytes.
//! 3. `SecretStorage::export_secret` does not exist — instead, we
//!    re-derive the Ed25519 + Jubjub secret bytes deterministically
//!    via the public `derive_keys` HKDF helper from the same seed.
//!
//! ## Output keystore
//!
//! On success, writes a JSON file at `--out` with the shape:
//!
//! ```json
//! {
//!   "did": "did:midnight:undeployed:...",
//!   "ed25519": { "kid": "ed25519/authentication", "secret_hex": "..." },
//!   "jubjub":  { "kid": "jubjub/assertionMethod", "secret_hex": "..." }
//! }
//! ```

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::Parser;
use wallet_core::{
    bootstrap_did_with_keys, derive_keys, HttpIndexerClient, HttpProver, LocalProver, Network,
    Prover, SubxtNodeClient, Wallet,
};
use wallet_core::secret_storage::InMemorySecretStore;

#[derive(Parser, Debug)]
#[command(
    name = "did-bootstrap",
    about = "Create a Midnight DID with Ed25519 (authentication) + Jubjub (assertionMethod) keys against the local standalone env"
)]
struct Args {
    /// 32-byte seed as 64 hex chars (with optional `0x` prefix), or
    /// any shorter string — non-32-byte input is SHA-256-hashed to
    /// 32 bytes. The derivation is deterministic, so the same seed
    /// always reproduces the same DID against a fresh env.
    #[arg(long, env = "DID_BOOTSTRAP_SEED")]
    seed: String,

    /// Proof-server base URL. Defaults to the `Network::Undeployed`
    /// preset (`http://localhost:6300`). Pass `--proof-server-url ""`
    /// to force the in-process LocalProver instead.
    #[arg(long, env = "MIDNIGHT_PROOF_SERVER_URL")]
    proof_server_url: Option<String>,

    /// Output JSON keystore path.
    #[arg(long)]
    out: PathBuf,
}

/// Normalise the `--seed` flag to 32 bytes. Hex (with or without
/// `0x`) of exactly 64 chars is decoded as-is; everything else is
/// SHA-256-hashed.
fn seed_to_bytes(seed: &str) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let hex_in = seed.strip_prefix("0x").unwrap_or(seed);
    if hex_in.len() == 64 {
        if let Ok(bytes) = hex::decode(hex_in) {
            if bytes.len() == 32 {
                let mut out = [0u8; 32];
                out.copy_from_slice(&bytes);
                return out;
            }
        }
    }
    let mut hasher = Sha256::new();
    hasher.update(seed.as_bytes());
    hasher.finalize().into()
}

/// Inlined twin of the (crate-private) `chain::default_prover`:
/// `None` (or empty string) → in-process `LocalProver`;
/// `Some(url)` → `HttpProver` against the proof server. Inlined
/// rather than promoted to keep the trait-surface widening minimal.
fn build_prover(url: Option<&str>) -> Arc<dyn Prover> {
    match url.filter(|s| !s.is_empty()) {
        Some(u) => Arc::new(HttpProver::new(u.to_owned())),
        None => Arc::new(LocalProver),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let seed = seed_to_bytes(&args.seed);
    let network = Network::Undeployed;

    // Default the proof-server URL to the Undeployed preset when
    // the flag is absent (None) — passing `--proof-server-url ""`
    // explicitly opts into the LocalProver instead.
    let proof_url: Option<String> = match args.proof_server_url.as_deref() {
        None => Some(network.config().proving_server_url.to_owned()),
        Some("") => None,
        Some(u) => Some(u.to_owned()),
    };

    let indexer: Arc<dyn wallet_core::IndexerClient> =
        Arc::new(HttpIndexerClient::new(network).context("build HttpIndexerClient")?);
    let node: Arc<dyn wallet_core::NodeClient> = Arc::new(
        SubxtNodeClient::connect(network)
            .await
            .context("connect to SubxtNodeClient")?,
    );
    let prover = build_prover(proof_url.as_deref());

    let wallet = Wallet::with_deps(seed, network, indexer, node, prover);

    let mut secret_store = InMemorySecretStore::default();

    let result = bootstrap_did_with_keys(&wallet, &mut secret_store, &seed)
        .await
        .context("bootstrap_did_with_keys")?;

    // Re-derive the raw secret bytes so the keystore can be reloaded
    // by downstream callers (issuer, BDD harness). HKDF from the same
    // seed produces the same bytes that were imported in step 1 of
    // `bootstrap_did_with_keys`.
    let (ed_bytes, jb_bytes) = derive_keys(&seed);

    let json = serde_json::json!({
        "did": result.did.to_did_string(),
        "ed25519": {
            "kid": result.ed25519_ref.id(),
            "secret_hex": hex::encode(ed_bytes),
        },
        "jubjub": {
            "kid": result.jubjub_ref.id(),
            "secret_hex": hex::encode(jb_bytes),
        },
    });

    let pretty = serde_json::to_string_pretty(&json)?;
    std::fs::write(&args.out, pretty).with_context(|| {
        format!("write keystore to {}", args.out.display())
    })?;

    println!(
        "Bootstrapped {} -> {}",
        result.did.to_did_string(),
        args.out.display()
    );
    Ok(())
}
