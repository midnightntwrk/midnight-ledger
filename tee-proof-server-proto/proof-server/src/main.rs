// This file is part of midnight-ledger.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

use clap::Parser;
use governor::{Quota, RateLimiter};
use midnight_proof_server_prototype::{create_app, AppState, SecurityConfig, PUBLIC_PARAMS};
use midnight_proof_server_prototype::worker_pool::WorkerPool;
use base_crypto::data_provider::{FetchMode, MidnightDataProvider, OutputMode};
use futures::future::join;
use ledger::dust::DustResolver;
use ledger::prove::Resolver;
use std::net::SocketAddr;
use std::num::NonZeroU32;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use transient_crypto::proofs::{KeyLocation, Resolver as ResolverT};

#[derive(Parser, Debug)]
#[command(name = "midnight-proof-server")]
#[command(about = "Axum-based ZK proof server for Midnight blockchain", long_about = None)]
struct Args {
    /// Port to listen on
    #[arg(long, env = "MIDNIGHT_PROOF_SERVER_PORT", default_value = "6300")]
    port: u16,

    /// Bind address (use "127.0.0.1" for localhost-only in Nitro Enclaves with vsock bridge)
    #[arg(long, env = "MIDNIGHT_PROOF_SERVER_BIND", default_value = "0.0.0.0")]
    bind: String,

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

    /// Skip pre-fetching ZSwap and Dust parameters at startup
    #[arg(long, env = "MIDNIGHT_PROOF_SERVER_NO_FETCH_PARAMS")]
    no_fetch_params: bool,

    /// Disable HTTPS/TLS (use plain HTTP instead)
    /// WARNING: This is NOT RECOMMENDED for production!
    #[arg(long, env = "MIDNIGHT_PROOF_SERVER_DISABLE_TLS")]
    disable_tls: bool,

    /// Path to TLS certificate file (PEM format)
    #[arg(long, env = "MIDNIGHT_PROOF_SERVER_TLS_CERT", default_value = "certs/cert.pem")]
    tls_cert: String,

    /// Path to TLS private key file (PEM format)
    #[arg(long, env = "MIDNIGHT_PROOF_SERVER_TLS_KEY", default_value = "certs/key.pem")]
    tls_key: String,

    /// Generate self-signed certificate if cert files don't exist
    #[arg(long, env = "MIDNIGHT_PROOF_SERVER_AUTO_GENERATE_CERT")]
    auto_generate_cert: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    // Install rustls crypto provider only if TLS is enabled
    // This must be done before any TLS operations
    let enable_tls = !args.disable_tls;
    if enable_tls {
        rustls::crypto::ring::default_provider()
            .install_default()
            .expect("Failed to install rustls crypto provider");
    }

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
        warn!("WARNING: AUTHENTICATION DISABLED - This is DANGEROUS in production!");
    }

    // Pre-fetch ZSwap and Dust parameters (unless disabled)
    if !args.no_fetch_params {
        info!("Ensuring zswap key material is available...");
        let resolver = Resolver::new(
            PUBLIC_PARAMS.clone(),
            DustResolver(
                MidnightDataProvider::new(
                    FetchMode::OnDemand,
                    OutputMode::Log,
                    ledger::dust::DUST_EXPECTED_FILES.to_owned(),
                )
                .expect("data provider initialization failed"),
            ),
            Box::new(move |_: KeyLocation| Box::pin(std::future::ready(Ok(None)))),
        );

        // Pre-fetch k parameters (10-15) in parallel
        let ks = futures::future::join_all((10..=15).map(|k| PUBLIC_PARAMS.0.fetch_k(k)));

        // Pre-fetch all built-in keys in parallel
        let keys = futures::future::join_all(
            [
                "midnight/zswap/spend",
                "midnight/zswap/output",
                "midnight/zswap/sign",
                "midnight/dust/spend",
            ]
            .into_iter()
            .map(|k| resolver.resolve_key(KeyLocation(k.into()))),
        );

        // Wait for all downloads to complete
        let (ks, keys) = join(ks, keys).await;
        ks.into_iter().collect::<Result<Vec<_>, _>>()?;
        keys.into_iter().collect::<Result<Vec<_>, _>>()?;

        info!("✓ ZSwap and Dust key material validated and cached");
    } else {
        info!("Skipping pre-fetch of parameters (--no-fetch-params enabled)");
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
    let addr: SocketAddr = format!("{}:{}", args.bind, args.port)
        .parse()
        .expect("Invalid bind address");

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

    // Start server with TLS or plain HTTP
    if enable_tls {
        info!("TLS/HTTPS enabled");

        // Check if certificates exist
        let certs_exist = midnight_proof_server_prototype::tls::check_cert_files(
            &args.tls_cert,
            &args.tls_key,
        ).is_ok();

        // Generate self-signed cert if needed
        if !certs_exist {
            if args.auto_generate_cert {
                info!("Certificate files not found, generating self-signed certificate...");
                midnight_proof_server_prototype::tls::generate_self_signed_cert(
                    &args.tls_cert,
                    &args.tls_key,
                )?;
            } else {
                error!("TLS certificate files not found!");
                error!("  Expected certificate: {}", args.tls_cert);
                error!("  Expected private key: {}", args.tls_key);
                error!("");
                error!("Options:");
                error!("  1. Generate self-signed certificate (testing only):");
                error!("     cargo run --bin midnight-proof-server-prototype -- --auto-generate-cert");
                error!("");
                error!("  2. Use existing certificates:");
                error!("     cargo run --bin midnight-proof-server-prototype -- --tls-cert /path/to/cert.pem --tls-key /path/to/key.pem");
                error!("");
                error!("  3. Disable TLS (NOT RECOMMENDED for production):");
                error!("     cargo run --bin midnight-proof-server-prototype -- --enable-tls=false");
                error!("");
                return Err("TLS enabled but certificate files not found. See error message above for options.".into());
            }
        }

        // Load TLS configuration
        let tls_config = axum_server::tls_rustls::RustlsConfig::from_pem_file(
            &args.tls_cert,
            &args.tls_key,
        ).await?;

        info!("Starting Midnight Proof Server v{} with HTTPS", env!("CARGO_PKG_VERSION"));
        info!("Listening on: https://{}", addr);
        info!("  Certificate: {}", args.tls_cert);
        info!("  Private Key: {}", args.tls_key);

        // Create shutdown handle for graceful shutdown
        let handle = axum_server::Handle::new();

        // Spawn graceful shutdown handler
        tokio::spawn(shutdown_signal(handle.clone()));

        // Start HTTPS server with graceful shutdown support
        axum_server::bind_rustls(addr, tls_config)
            .handle(handle)
            .serve(app.into_make_service())
            .await?;
    } else {
        warn!("⚠️  TLS/HTTPS is DISABLED - this is NOT RECOMMENDED for production!");
        warn!("⚠️  Witness data and API keys will be transmitted in plaintext.");
        info!("Starting Midnight Proof Server v{} with HTTP (insecure)", env!("CARGO_PKG_VERSION"));
        info!("Listening on: http://{}", addr);

        // Start plain HTTP server with graceful shutdown
        let listener = tokio::net::TcpListener::bind(&addr).await?;

        // Create graceful shutdown future
        let shutdown_future = async {
            use tokio::signal;

            let ctrl_c = async {
                signal::ctrl_c()
                    .await
                    .expect("Failed to install Ctrl+C signal handler");
            };

            #[cfg(unix)]
            let terminate = async {
                signal::unix::signal(signal::unix::SignalKind::terminate())
                    .expect("Failed to install SIGTERM signal handler")
                    .recv()
                    .await;
            };

            #[cfg(not(unix))]
            let terminate = std::future::pending::<()>();

            tokio::select! {
                _ = ctrl_c => {
                    info!("📡 Received Ctrl+C signal (HTTP mode)");
                },
                _ = terminate => {
                    info!("📡 Received SIGTERM signal (HTTP mode)");
                },
            }

            info!("🛑 Initiating graceful shutdown (HTTP mode)...");
        };

        axum::serve(listener, app)
            .with_graceful_shutdown(shutdown_future)
            .await?;
    }

    Ok(())
}

/// Graceful shutdown signal handler
///
/// Listens for SIGTERM (on Unix) or Ctrl+C and initiates graceful shutdown
/// with a 30-second timeout for active connections to complete.
async fn shutdown_signal(handle: axum_server::Handle<SocketAddr>) {
    use tokio::signal;

    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C signal handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("Failed to install SIGTERM signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            info!("📡 Received Ctrl+C signal");
        },
        _ = terminate => {
            info!("📡 Received SIGTERM signal");
        },
    }

    info!("🛑 Initiating graceful shutdown...");
    info!("   Waiting up to 30 seconds for active connections to complete");

    // Graceful shutdown with 30 second timeout
    handle.graceful_shutdown(Some(std::time::Duration::from_secs(30)));
}
