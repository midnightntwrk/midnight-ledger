//! Headless Midnight wallet — CLI driver for every flow the
//! dioxus app exposes.  Talks line-delimited JSON on stdin /
//! stdout; one verb per service method per the hex-architecture
//! design doc:
//!
//!   docs/superpowers/specs/2026-05-29-hexagonal-headless-wallet-design.md
//!   §2.4 (verbs + sample session)
//!   §2.5 (UI port adapter pattern — CliUiAdapter lives here)
//!
//! Wave A3 (this commit): binary skeleton.  Parses CLI flags and
//! prints the parsed config to stderr, then exits.  No verbs
//! dispatched yet — those land in wave E once wave C has
//! migrated the use-case bodies into the service layer that the
//! verbs would call.

use clap::Parser;

/// Headless Midnight wallet — drives every flow over a
/// line-delimited JSON protocol.  See the design doc §2.4 for
/// the protocol shape + verb list.
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Cli {
    /// Target chain: standalone (docker-compose), preprod, mainnet.
    #[arg(long, default_value = "standalone")]
    network: String,

    /// On-disk redb path for the wallet store.  Mutually
    /// exclusive with --in-memory-store.
    #[arg(long)]
    store_path: Option<std::path::PathBuf>,

    /// Use an in-memory store (data lost at exit).  For tests +
    /// quick local debug.
    #[arg(long, conflicts_with = "store_path")]
    in_memory_store: bool,

    /// Read the unlock passphrase from this env var.
    #[arg(long, env = "HEADLESS_PASSPHRASE")]
    passphrase_env: Option<String>,

    /// Read the unlock passphrase from stdin's first line.
    #[arg(long, conflicts_with = "passphrase_env")]
    passphrase_stdin: bool,

    /// Use this passphrase verbatim (test convenience — never
    /// pass a real passphrase here on a shared host).
    #[arg(long, conflicts_with_all = ["passphrase_env", "passphrase_stdin"])]
    passphrase: Option<String>,

    /// Proof-server URL.  Omit to use the in-process LocalProver
    /// (slow on debug builds).
    #[arg(long)]
    proof_server: Option<String>,

    /// Indexer HTTP URL.  Defaults per --network.
    #[arg(long)]
    indexer: Option<String>,

    /// Node WebSocket URL.  Defaults per --network.
    #[arg(long)]
    node: Option<String>,

    /// Replace HttpClient with a MockHttpClient driven from
    /// `http-mock-push` sidecar commands on stdin.  For the
    /// oid4vp / oid4vci integration tests.
    #[arg(long)]
    mock_http: bool,

    /// Replace indexer / node / prover with stubs.  For
    /// unit-style end-to-end runs without a live chain.
    #[arg(long)]
    mock_chain: bool,

    /// Dump MetricsSnapshot JSON to this path at exit.
    #[arg(long)]
    metrics_out: Option<std::path::PathBuf>,

    /// Interactive mode — prompt for input on stdin when
    /// `UserInterface::prompt_text` is called.  Default
    /// (non-interactive) takes prompt answers from the verb's
    /// `args` map.
    #[arg(long)]
    interactive: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Tracing to stderr so stdout stays clean for the
    // line-delimited JSON protocol (wave E).
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(filter)
        .init();

    let cli = Cli::parse();
    tracing::info!(?cli, "headless-wallet wave-A3 skeleton — config parsed");

    // Wave E wires the service container + verb dispatcher
    // here.  Skeleton just exits clean so we can ship the
    // crate scaffolding now without the protocol surface.
    eprintln!(
        "headless-wallet skeleton: parsed config, exiting.\n\
         The verb-dispatch loop lands in refactor wave E (per design doc §3)."
    );

    Ok(())
}
