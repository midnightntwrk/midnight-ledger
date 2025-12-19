// This file is part of midnight-ledger.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

use clap::Parser;
use governor::{Quota, RateLimiter};
use midnight_proof_server_prototype::{create_app, AppState, SecurityConfig};
use midnight_proof_server_prototype::worker_pool::WorkerPool;
use std::net::SocketAddr;
use std::num::NonZeroU32;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Parser, Debug)]
#[command(name = "midnight-proof-server")]
#[command(about = "Axum-based ZK proof server for Midnight blockchain", long_about = None)]
struct Args {
    /// Port to listen on
    #[arg(long, env = "MIDNIGHT_PROOF_SERVER_PORT", default_value = "6300")]
    port: u16,

    /// API keys (comma-separated for multiple keys)
    #[arg(long, env = "MIDNIGHT_PROOF_SERVER_API_KEY")]
    api_key: Option<String>,

    /// Disable authentication (DANGEROUS - only for development!)
    #[arg(long, env = "MIDNIGHT_PROOF_SERVER_DISABLE_AUTH")]
    disable_auth: bool,

    /// Rate limit (requests per second per IP)
    #[arg(long, env = "MIDNIGHT_PROOF_SERVER_RATE_LIMIT", default_value = "10")]
    rate_limit: u32,

    /// Maximum payload size in bytes
    #[arg(
        long,
        env = "MIDNIGHT_PROOF_SERVER_MAX_PAYLOAD_SIZE",
        default_value = "10485760"
    )]
    max_payload_size: usize,

    /// Number of worker threads
    #[arg(long, env = "MIDNIGHT_PROOF_SERVER_NUM_WORKERS", default_value = "16")]
    num_workers: usize,

    /// Job queue capacity (0 = unlimited)
    #[arg(long, env = "MIDNIGHT_PROOF_SERVER_JOB_CAPACITY", default_value = "0")]
    job_capacity: usize,

    /// Job timeout in seconds
    #[arg(
        long,
        env = "MIDNIGHT_PROOF_SERVER_JOB_TIMEOUT",
        default_value = "600"
    )]
    job_timeout: f64,

    /// Enable verbose logging
    #[arg(long, env = "MIDNIGHT_PROOF_SERVER_VERBOSE")]
    verbose: bool,

    /// Enable /fetch-params endpoint (for fetching ZSwap parameters)
    #[arg(long, env = "MIDNIGHT_PROOF_SERVER_ENABLE_FETCH_PARAMS")]
    enable_fetch_params: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    // Setup logging
    let log_level = if args.verbose {
        tracing::Level::DEBUG
    } else {
        tracing::Level::INFO
    };

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| {
                    // Use underscore instead of hyphen for env var
                    format!("midnight_proof_server_prototype={}", log_level).into()
                }),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Validate configuration
    if !args.disable_auth && args.api_key.is_none() {
        error!("ERROR: API key is required for production use!");
        error!("  Set --api-key <KEY> or --disable-auth (development only)");
        std::process::exit(1);
    }

    if args.disable_auth {
        warn!("⚠️  AUTHENTICATION DISABLED - This is DANGEROUS in production!");
    }

    // Parse API keys
    let api_keys: Vec<String> = args
        .api_key
        .as_ref()
        .map(|keys| keys.split(',').map(|s| s.trim().to_string()).collect())
        .unwrap_or_default();

    // Create security config
    let security = SecurityConfig::new(
        api_keys,
        args.disable_auth,
        args.rate_limit,
        args.max_payload_size,
    );

    // Log security configuration
    info!("Security configuration:");
    info!("  Authentication: {}", if args.disable_auth { "DISABLED" } else { "ENABLED" });
    info!("  Rate limit: {} req/s per IP", args.rate_limit);
    info!("  Max payload size: {} bytes ({:.1} MB)",
        args.max_payload_size,
        args.max_payload_size as f64 / 1_048_576.0
    );

    // Create worker pool
    info!("Initializing worker pool:");
    info!("  Workers: {}", args.num_workers);
    info!("  Job capacity: {}", if args.job_capacity == 0 {
        "unlimited".to_string()
    } else {
        args.job_capacity.to_string()
    });
    info!("  Job timeout: {:.1} seconds", args.job_timeout);

    let worker_pool = Arc::new(WorkerPool::new(
        args.num_workers,
        args.job_capacity,
        args.job_timeout,
    ));

    // Create rate limiter
    let quota = Quota::per_second(NonZeroU32::new(args.rate_limit).unwrap());
    let rate_limiter = Arc::new(Mutex::new(RateLimiter::keyed(quota)));

    // Create application state
    let state = AppState {
        worker_pool,
        security,
        rate_limiter,
        enable_fetch_params: args.enable_fetch_params,
    };

    // Create router
    let app = create_app(state);

    // Setup server address
    let addr = SocketAddr::from(([0, 0, 0, 0], args.port));

    info!("Starting Midnight Proof Server (Axum) v{}", env!("CARGO_PKG_VERSION"));
    info!("Listening on: http://{}", addr);
    info!("");
    info!("Public endpoints (no auth):");
    info!("  GET  /               - Root (health check)");
    info!("  GET  /health         - Health check");
    info!("  GET  /ready          - Readiness + queue stats");
    info!("  GET  /version        - Server version");
    info!("  GET  /proof-versions - Supported proof versions");
    if args.enable_fetch_params {
        info!("  GET  /fetch-params/{{k}} - Fetch ZSwap parameters (k=10-24)");
    }
    info!("");
    info!("Protected endpoints (auth required):");
    info!("  POST /check          - Validate proof preimage");
    info!("  POST /prove          - Generate ZK proof");
    info!("  POST /prove-tx       - Prove transaction");
    info!("  POST /k              - Get security parameter");
    info!("");

    // Start server
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app)
        .await?;

    Ok(())
}
