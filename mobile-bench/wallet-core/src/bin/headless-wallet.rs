//! `headless-wallet` — driver for the [`HeadlessWallet`] use-case
//! façade. Same flows the Dioxus shell drives (Bootstrap → OID4VP
//! login → OID4VCI issuance → self-verify), scriptable from a
//! shell.
//!
//! ## Quickstart
//!
//! ```bash
//! # bring the standalone env up first:
//! #   docker compose -f .../standalone/docker-compose.yml up -d
//! # plus the issuer on :3001.
//!
//! # 1. Bootstrap a DID. Seed defaults to a deterministic 32-byte
//! #    constant so re-running against a fresh env reproduces the
//! #    same identity.
//! headless-wallet \
//!     --seed 0x2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a \
//!     --vc-store /tmp/headless-vc.redb \
//!     bootstrap
//!
//! # 2. Use the DID printed above to drive an OID4VP login. The
//! #    QR URL comes from the issuer-mock's /login page.
//! headless-wallet --seed ... --vc-store ... login \
//!     --did did:midnight:undeployed:... \
//!     --qr-url 'openid4vp://...?request_uri=...'
//!
//! # 3. Then an OID4VCI issuance for a credential offer:
//! headless-wallet --seed ... --vc-store ... issue \
//!     --did did:midnight:undeployed:... \
//!     --qr-url 'openid-credential-offer://...'
//!
//! # 4. Then self-verify the freshly-issued VC:
//! headless-wallet --seed ... --vc-store ... verify --vc-uri urn:uuid:...
//! ```
//!
//! Every subcommand goes through `wallet_core::headless::HeadlessWallet`,
//! which is the same façade the integration tests
//! (`tests/headless_*_e2e.rs`) use — so a green CLI run against a
//! local standalone is strong evidence the use cases work
//! end-to-end. Spec: `docs/superpowers/specs/2026-06-03-hex-architecture-audit.md`
//! §9 (Future Improvement Candidates).

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use wallet_core::headless::{HeadlessConfig, HeadlessWallet};
use wallet_core::{DidId, Network};

#[derive(Parser, Debug)]
#[command(
    name = "headless-wallet",
    about = "Drive the wallet-core use cases against a live standalone env, no UI required"
)]
struct Args {
    /// 32-byte seed as 64 hex chars (with optional `0x` prefix),
    /// or any shorter string — non-32-byte input is SHA-256-hashed
    /// to 32 bytes. Same derivation `did-bootstrap` uses.
    #[arg(long, env = "HEADLESS_SEED")]
    seed: String,

    /// Path to the redb file backing the VC store. Created on
    /// first use if absent. The wallet's session state otherwise
    /// lives in-memory.
    #[arg(long, env = "HEADLESS_VC_STORE")]
    vc_store: PathBuf,

    /// Network to target. Defaults to the localhost standalone
    /// preset (`Undeployed`). For Yurii's tailnet build, pass
    /// `undeployed-yurii` — wires the laptop's tailscale IP into
    /// the indexer / node / proof-server URLs.
    #[arg(long, env = "HEADLESS_NETWORK", default_value = "undeployed")]
    network: String,

    /// Proof-server URL override. Defaults to the network's
    /// configured preset. Pass `--proof-server-url ""` to force
    /// the in-process `LocalProver` (slow; useful for
    /// proof-server-down debugging).
    #[arg(long, env = "MIDNIGHT_PROOF_SERVER_URL")]
    proof_server_url: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Bootstrap a fresh DID. Prints the DID + the 32-byte
    /// controller secret (hex) so subsequent
    /// `MaintenanceUpdate`-driving sessions can re-import.
    Bootstrap,
    /// Drive an OID4VP / SIOPv2 login. Prints the issuer's
    /// `session_id` + `status` on success.
    Login {
        #[arg(long)]
        did: String,
        #[arg(long)]
        qr_url: String,
    },
    /// Drive an OID4VCI Pre-Authorized Code Flow. Lands the
    /// issued VC into the configured VC store and prints its
    /// `vc_uri`.
    Issue {
        #[arg(long)]
        did: String,
        #[arg(long)]
        qr_url: String,
    },
    /// Re-verify a VC already landed in the VC store. Prints
    /// the verification outcome.
    Verify {
        #[arg(long)]
        vc_uri: String,
    },
}

fn seed_to_bytes(s: &str) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let hex_in = s.strip_prefix("0x").unwrap_or(s);
    if hex_in.len() == 64 {
        let mut bytes = [0u8; 32];
        if hex::decode_to_slice(hex_in, &mut bytes).is_ok() {
            return bytes;
        }
    }
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    h.finalize().into()
}

fn parse_network(s: &str) -> Result<Network> {
    Network::from_label(s).context(format!("unknown network: {s}"))
}

#[tokio::main]
async fn main() -> Result<()> {
    // wallet-core emits structured tracing events; with no
    // subscriber installed they're dropped silently. For the CLI
    // binary that's fine — operators who want them can
    // `RUST_LOG=info cargo run ...` once `tracing_subscriber` is
    // added as a dev-dep + initialised here. Kept minimal so the
    // binary has zero non-default dependencies beyond what
    // `did-bootstrap` already pulls in.
    let args = Args::parse();
    let seed = seed_to_bytes(&args.seed);
    let network = parse_network(&args.network)?;

    wallet_core::ensure_default_crypto_provider();

    let config = HeadlessConfig {
        network,
        seed,
        vc_store_path: args.vc_store,
        proof_server_url: args.proof_server_url,
    };
    let w = HeadlessWallet::connect(config).await?;

    match args.command {
        Command::Bootstrap => {
            let out = w.bootstrap(seed).await?;
            println!("{{");
            println!("  \"did\": \"{}\",", out.did.to_did_string());
            println!("  \"controller_sk\": \"{}\"", hex::encode(out.controller_sk));
            println!("}}");
        }
        Command::Login { did, qr_url } => {
            let holder = DidId::parse(&did).context("parse --did")?;
            // Re-bootstrap inside this session so the secret store
            // contains the right keys. Same `seed` ⇒ same keys; the
            // chain side rejects a duplicate `create_did` so this
            // works only on a fresh env. For long-lived sessions
            // (Bootstrap + Login + Issue + Verify in one process)
            // run all subcommands in a single `headless-wallet`
            // invocation once `--many-cmds` lands.
            w.bootstrap(seed).await.ok();
            let r = w.login(holder, &qr_url).await?;
            println!("{{");
            println!("  \"session_id\": \"{}\",", r.session_id);
            println!("  \"status\": \"{}\"", r.status);
            println!("}}");
        }
        Command::Issue { did, qr_url } => {
            let holder = DidId::parse(&did).context("parse --did")?;
            w.bootstrap(seed).await.ok();
            let vc_uri = w.request_credential(holder, &qr_url).await?;
            println!("{{");
            println!("  \"vc_uri\": \"{vc_uri}\"");
            println!("}}");
        }
        Command::Verify { vc_uri } => {
            // Verify only reads `vc_store` + on-chain DID — no
            // bootstrap needed.
            let r = w.verify(&vc_uri).await?;
            let json = serde_json::to_string_pretty(&format!("{r:?}"))?;
            println!("{json}");
        }
    }
    Ok(())
}
