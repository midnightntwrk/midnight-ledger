//! Headless Midnight wallet — CLI driver for every flow the
//! dioxus app exposes. Talks line-delimited JSON on stdin /
//! stdout; one verb per service method per the hex-architecture
//! design doc:
//!
//!   docs/superpowers/specs/2026-05-29-hexagonal-headless-wallet-design.md
//!   §2.4 (verbs + sample session)
//!
//! ## Protocol
//!
//!   Request:  `{"verb":"<name>","args":{…}}` (one per stdin line)
//!   Success:  `{"type":"result","verb":"<name>","ok":true,"data":{…}}`
//!   Error:    `{"type":"error","verb":"<name>","code":"<code>","message":"<text>"}`
//!
//! Tracing → stderr; JSON envelopes → stdout.
//!
//! ## Supported verbs (minimum-viable set)
//!
//! Lifecycle:
//!   - `connect`        — no-op verb (returns active network); the
//!                        wallet is already connected at startup
//!                        from CLI flags.
//!   - `bootstrap`      — bootstrap a fresh DID against the chain.
//!                        `args: { "seedHex": "<64-char hex>" }`
//!   - `quit` / EOF     — close the dispatcher and exit 0.
//!
//! Vault (mirror the dioxus connector verbs):
//!   - `vaultTotalLocked`     — `args: { "contractAddress": "<hex>" }`
//!   - `vaultListLocks`       — `args: { "contractAddress": "<hex>" }`
//!   - `vaultListCredentials` — `args: {}`
//!   - `vaultCreateLock`      — `args: { "contractAddress", "minAge",
//!                              "requireIssuingState"?, "issuingState"?,
//!                              "requireDocumentNumber"?,
//!                              "documentNumber"?, "maxClaimBaseUnits",
//!                              "verifierChallengeHex"?,
//!                              "initialAmountBaseUnits"? }`
//!   - `vaultDeposit`         — `args: { "contractAddress", "lockId",
//!                              "amountBaseUnits" }`
//!   - `vaultClaim`           — `args: { "contractAddress", "lockId",
//!                              "amountBaseUnits", "bundle",
//!                              "currentDay"? }`
//!
//! Identity (deferred — `bootstrap` lands first, login / issuance
//! arrive in a follow-up wave):
//!   - `login`              — TBD
//!   - `requestCredential`  — TBD
//!   - `verify`             — TBD

use std::path::PathBuf;

use anyhow::Context as _;
use clap::Parser;
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use tokio::io::{AsyncBufReadExt as _, BufReader};
use wallet_core::headless::{HeadlessConfig, HeadlessWallet};
use wallet_core::{Network, VaultLockPolicy};

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Cli {
    /// Target chain: undeployed (docker-compose, localhost),
    /// undeployedyurii (tailnet), preprod, etc. Mirrors the dioxus
    /// wallet's network names.
    #[arg(long, default_value = "undeployed")]
    network: String,

    /// 32-byte master seed as 64-char hex. Required: the wallet
    /// has to connect against a real seed even before `bootstrap`.
    /// The standalone chain pre-funds `0000…0001`
    /// (UNDEPLOYED_GENESIS_SEED_HEX); arbitrary seeds get only
    /// per-block emission.
    #[arg(long, env = "HEADLESS_SEED_HEX")]
    seed_hex: String,

    /// On-disk redb path for the VC store. Created on first use.
    #[arg(long, default_value = "/tmp/headless-wallet-vcs.redb")]
    vc_store_path: PathBuf,

    /// Override the proof-server URL. `""` opts into the in-process
    /// LocalProver (slow). Defaults to the network's configured
    /// proof server.
    #[arg(long, env = "MIDNIGHT_PROOF_SERVER_URL")]
    proof_server_url: Option<String>,
}

#[derive(Deserialize)]
struct Request {
    verb: String,
    #[serde(default)]
    args: Json,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum Response {
    Result {
        verb: String,
        ok: bool,
        data: Json,
    },
    Error {
        verb: String,
        code: String,
        message: String,
    },
}

fn parse_network(s: &str) -> anyhow::Result<Network> {
    match s.to_ascii_lowercase().as_str() {
        "undeployed" => Ok(Network::Undeployed),
        "undeployedyurii" | "tailnet" | "undeployed-tailscale" => Ok(Network::UndeployedYurii),
        "preprod" => Ok(Network::PreProd),
        "preview" => Ok(Network::Preview),
        "qanet" => Ok(Network::QaNet),
        "devnet" => Ok(Network::DevNet),
        "mainnet" => Ok(Network::Mainnet),
        other => anyhow::bail!("unknown network: {other}"),
    }
}

fn decode_hex32(hex_str: &str, field: &str) -> anyhow::Result<[u8; 32]> {
    let bytes = hex::decode(hex_str.trim_start_matches("0x"))
        .with_context(|| format!("{field}: not hex"))?;
    bytes
        .try_into()
        .map_err(|v: Vec<u8>| anyhow::anyhow!("{field}: expected 32 bytes, got {}", v.len()))
}

fn emit(resp: &Response) {
    let line = serde_json::to_string(resp).expect("serialize Response");
    println!("{line}");
}

fn ok(verb: &str, data: Json) -> Response {
    Response::Result {
        verb: verb.to_string(),
        ok: true,
        data,
    }
}

fn err(verb: &str, code: &str, message: impl Into<String>) -> Response {
    Response::Error {
        verb: verb.to_string(),
        code: code.to_string(),
        message: message.into(),
    }
}

fn json_as_u64(v: &Json) -> Option<u64> {
    v.as_u64()
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

fn json_as_u128(v: &Json) -> Option<u128> {
    // serde_json's Value::as_u128 doesn't exist; widen u64 and fall
    // back to parsing a string-numeral (the dApp + CLI both ship
    // amounts as decimal strings to survive the JSON bridge).
    v.as_u64()
        .map(u128::from)
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

fn json_as_u8(v: &Json) -> Option<u8> {
    json_as_u64(v).and_then(|u| u8::try_from(u).ok())
}

fn text_to_bytes32(args: &Json, key: &str) -> [u8; 32] {
    let s = args.get(key).and_then(|v| v.as_str()).unwrap_or("");
    let bytes = s.as_bytes();
    let mut out = [0u8; 32];
    let n = bytes.len().min(32);
    out[..n].copy_from_slice(&bytes[..n]);
    out
}

async fn handle_verb(wallet: &HeadlessWallet, verb: &str, args: Json) -> Response {
    match verb {
        "connect" => ok(
            verb,
            serde_json::json!({ "network": format!("{:?}", wallet.network()) }),
        ),

        "bootstrap" => {
            let seed_hex = match args.get("seedHex").and_then(|v| v.as_str()) {
                Some(s) => s,
                None => return err(verb, "bad-args", "missing seedHex"),
            };
            let seed = match decode_hex32(seed_hex, "seedHex") {
                Ok(b) => b,
                Err(e) => return err(verb, "bad-args", e.to_string()),
            };
            match wallet.bootstrap(seed).await {
                Ok(out) => ok(
                    verb,
                    serde_json::json!({
                        "did": out.did.to_did_string(),
                        "controllerSkHex": hex::encode(out.controller_sk),
                    }),
                ),
                Err(e) => err(verb, "bootstrap-failed", e.to_string()),
            }
        }

        "vaultTotalLocked" => match args.get("contractAddress").and_then(|v| v.as_str()) {
            Some(addr) => match wallet.vault_total_locked(addr.to_string()).await {
                Ok(total) => ok(
                    verb,
                    serde_json::json!({ "totalLockedBaseUnits": total.to_string() }),
                ),
                Err(e) => err(verb, "vault-read-failed", e.to_string()),
            },
            None => err(verb, "bad-args", "missing contractAddress"),
        },

        "vaultListLocks" => match args.get("contractAddress").and_then(|v| v.as_str()) {
            Some(addr) => match wallet.vault_list_locks(addr.to_string()).await {
                Ok(json) => ok(verb, json),
                Err(e) => err(verb, "vault-read-failed", e.to_string()),
            },
            None => err(verb, "bad-args", "missing contractAddress"),
        },

        "vaultListCredentials" => match wallet.vault_list_credentials() {
            Ok(creds) => ok(verb, serde_json::json!({ "credentials": creds })),
            Err(e) => err(verb, "vc-store-error", e.to_string()),
        },

        "vaultCreateLock" => handle_create_lock(wallet, verb, args).await,

        "vaultDeposit" => {
            let addr = match args.get("contractAddress").and_then(|v| v.as_str()) {
                Some(s) => s.to_string(),
                None => return err(verb, "bad-args", "missing contractAddress"),
            };
            let lock_id = match args.get("lockId").and_then(json_as_u64) {
                Some(v) => v,
                None => return err(verb, "bad-args", "missing/invalid lockId"),
            };
            let amount = match args.get("amountBaseUnits").and_then(json_as_u128) {
                Some(v) => v,
                None => return err(verb, "bad-args", "missing/invalid amountBaseUnits"),
            };
            match wallet.vault_deposit(addr, lock_id, amount).await {
                Ok(tx_hash) => ok(verb, serde_json::json!({ "txHash": tx_hash })),
                Err(e) => err(verb, "vault-deposit-failed", e.to_string()),
            }
        }

        "vaultClaim" => {
            let addr = match args.get("contractAddress").and_then(|v| v.as_str()) {
                Some(s) => s.to_string(),
                None => return err(verb, "bad-args", "missing contractAddress"),
            };
            let lock_id = match args.get("lockId").and_then(json_as_u64) {
                Some(v) => v,
                None => return err(verb, "bad-args", "missing/invalid lockId"),
            };
            let amount = match args.get("amountBaseUnits").and_then(json_as_u128) {
                Some(v) => v,
                None => return err(verb, "bad-args", "missing/invalid amountBaseUnits"),
            };
            let bundle = match args.get("bundle") {
                Some(b) if b.is_object() => b.clone(),
                _ => return err(verb, "bad-args", "missing bundle object"),
            };
            let current_day = args.get("currentDay").and_then(json_as_u64);
            match wallet
                .vault_claim(addr, lock_id, amount, bundle, current_day)
                .await
            {
                Ok(tx_hash) => ok(verb, serde_json::json!({ "txHash": tx_hash })),
                Err(e) => err(verb, "vault-claim-failed", e.to_string()),
            }
        }

        other => err(verb, "unknown-verb", format!("unsupported verb: {other}")),
    }
}

async fn handle_create_lock(wallet: &HeadlessWallet, verb: &str, args: Json) -> Response {
    let addr = match args.get("contractAddress").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => return err(verb, "bad-args", "missing contractAddress"),
    };
    let min_age = match args.get("minAge").and_then(json_as_u8) {
        Some(v) => v,
        None => return err(verb, "bad-args", "missing/invalid minAge (0-255)"),
    };
    let require_issuing_state = args
        .get("requireIssuingState")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let issuing_state = text_to_bytes32(&args, "issuingState");
    let require_document_number = args
        .get("requireDocumentNumber")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let document_number = text_to_bytes32(&args, "documentNumber");
    let max_claim = match args.get("maxClaimBaseUnits").and_then(json_as_u128) {
        Some(v) => v,
        None => return err(verb, "bad-args", "missing/invalid maxClaimBaseUnits"),
    };
    let challenge = match args.get("verifierChallengeHex").and_then(|v| v.as_str()) {
        Some(h) if !h.is_empty() => match decode_hex32(h, "verifierChallengeHex") {
            Ok(b) => b,
            Err(e) => return err(verb, "bad-args", e.to_string()),
        },
        _ => [0u8; 32],
    };
    let initial = args
        .get("initialAmountBaseUnits")
        .and_then(json_as_u128)
        .unwrap_or(0);
    let policy = VaultLockPolicy {
        min_age,
        require_issuing_state,
        required_issuing_state: issuing_state,
        require_document_number,
        required_document_number: document_number,
        max_claim,
        verifier_challenge_hash: challenge,
    };
    match wallet.vault_create_lock(addr, policy, initial).await {
        Ok(outcome) => ok(
            verb,
            serde_json::json!({
                "txHash": outcome.tx_hash,
                "lockId": outcome.lock_id.to_string(),
            }),
        ),
        Err(e) => err(verb, "vault-create-lock-failed", e.to_string()),
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(filter)
        .init();

    let cli = Cli::parse();
    let network = parse_network(&cli.network)?;
    let seed = decode_hex32(&cli.seed_hex, "--seed-hex")?;

    tracing::info!(
        network = ?network,
        vc_store = %cli.vc_store_path.display(),
        "headless-wallet: connecting"
    );

    let wallet = HeadlessWallet::connect(HeadlessConfig {
        network,
        seed,
        vc_store_path: cli.vc_store_path.clone(),
        proof_server_url: cli.proof_server_url.clone(),
    })
    .await
    .context("connect HeadlessWallet")?;

    // Initial banner so callers know we're ready (and what network).
    emit(&ok(
        "ready",
        serde_json::json!({
            "network": format!("{:?}", network),
            "vcStorePath": cli.vc_store_path.display().to_string(),
        }),
    ));

    let mut stdin = BufReader::new(tokio::io::stdin()).lines();
    while let Some(line) = stdin.next_line().await? {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line == "quit" || line == "exit" {
            break;
        }
        let req: Request = match serde_json::from_str(line) {
            Ok(r) => r,
            Err(e) => {
                emit(&err("?", "parse-error", e.to_string()));
                continue;
            }
        };
        let resp = handle_verb(&wallet, &req.verb, req.args).await;
        emit(&resp);
    }

    tracing::info!("headless-wallet: clean shutdown");
    Ok(())
}
