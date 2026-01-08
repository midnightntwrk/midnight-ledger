// This file is part of midnight-ledger.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

#![deny(unreachable_pub)]
#![deny(warnings)]

mod attestation;
mod nsm_attestation;
pub mod tls;

use attestation::attestation_handler;
use axum::{
    extract::{Path, Request, State},
    http::{StatusCode, header::{AUTHORIZATION, HeaderMap}},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router, body::Bytes,
};
use chrono::Utc;
use hex::ToHex;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
use sysinfo::System;
use tokio::sync::Mutex;
use tower_http::{
    cors::CorsLayer,
    limit::RequestBodyLimitLayer,
    timeout::TimeoutLayer,
    trace::TraceLayer,
};
use tracing::{debug, info, warn};

// Midnight ledger imports
use base_crypto::data_provider::{self, MidnightDataProvider};
use base_crypto::data_provider::{FetchMode, OutputMode};
use base_crypto::signatures::Signature;
use introspection::Introspection;
use lazy_static::lazy_static;
use ledger::dust::DustResolver;
use ledger::prove::Resolver;
use ledger::structure::{
    INITIAL_TRANSACTION_COST_MODEL, ProofPreimageMarker, ProofPreimageVersioned, ProofVersioned,
    Transaction,
};
use rand::rngs::OsRng;
use serialize::{tagged_deserialize, tagged_serialize};
use storage::db::InMemoryDB;
use transient_crypto::commitment::PedersenRandomness;
use transient_crypto::curve::Fr;
use transient_crypto::proofs::{
    KeyLocation, ProvingKeyMaterial, Resolver as ResolverTrait, WrappedIr, Zkir as ZkirTrait,
};
use zkir_v2::{IrSource as ZkirV2, LocalProvingProvider as ZkirV2Local};
use zswap::prove::ZswapResolver;

pub mod worker_pool;
use worker_pool::{JobStatus, WorkError, WorkerPool, WorkerPoolError};

// Initialize public parameters
lazy_static! {
    pub static ref PUBLIC_PARAMS: ZswapResolver = ZswapResolver(
        MidnightDataProvider::new(
            data_provider::FetchMode::OnDemand,
            data_provider::OutputMode::Log,
            zswap::ZSWAP_EXPECTED_FILES.to_vec(),
        )
        .expect("data provider initialization failed")
    );
}

// Type alias for transaction proving payload
type TransactionProvePayload<S> = (
    Transaction<S, ProofPreimageMarker, PedersenRandomness, InMemoryDB>,
    HashMap<String, ProvingKeyMaterial>,
);

// ============================================================================
// Memory Tracking
// ============================================================================

/// Helper function to get current process memory usage in bytes
fn get_memory_usage() -> Option<u64> {
    use sysinfo::ProcessesToUpdate;

    let pid = sysinfo::get_current_pid().ok()?;
    let mut system = System::new();

    // Refresh the specific process (API changed in sysinfo 0.33+)
    let updated = system.refresh_processes(ProcessesToUpdate::Some(&[pid]), false);
    if updated == 0 {
        tracing::warn!("Failed to refresh process info for PID {:?}", pid);
        return None;
    }

    system.process(pid).map(|process| process.memory())
}

// ============================================================================
// Configuration
// ============================================================================

#[derive(Clone)]
pub struct SecurityConfig {
    pub api_keys_hashed: HashSet<String>,
    pub auth_disabled: bool,
    pub rate_limit: u32,
    pub max_payload_size: usize,
}

impl SecurityConfig {
    pub fn new(
        api_keys: Vec<String>,
        auth_disabled: bool,
        rate_limit: u32,
        max_payload_size: usize,
    ) -> Self {
        let api_keys_hashed = api_keys
            .into_iter()
            .map(|key| hex::encode(Sha256::digest(key.as_bytes())))
            .collect();

        Self {
            api_keys_hashed,
            auth_disabled,
            rate_limit,
            max_payload_size,
        }
    }

    fn verify_api_key(&self, key: &str) -> bool {
        if self.auth_disabled {
            return true;
        }
        let hashed = hex::encode(Sha256::digest(key.as_bytes()));
        self.api_keys_hashed.contains(&hashed)
    }
}

// ============================================================================
// Application State
// ============================================================================

#[derive(Clone)]
pub struct AppState {
    pub worker_pool: Arc<WorkerPool>,
    pub security: SecurityConfig,
    pub rate_limiter: Arc<Mutex<governor::DefaultKeyedRateLimiter<String>>>,
    pub enable_fetch_params: bool,
}

// ============================================================================
// Error Handling
// ============================================================================

pub enum AppError {
    BadRequest(String),
    Unauthorized,
    TooManyRequests,
    ServiceUnavailable(String),
    InternalError(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg),
            AppError::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                "Invalid or missing API key".to_string(),
            ),
            AppError::TooManyRequests => (
                StatusCode::TOO_MANY_REQUESTS,
                "Rate limit exceeded".to_string(),
            ),
            AppError::ServiceUnavailable(msg) => (StatusCode::SERVICE_UNAVAILABLE, msg),
            AppError::InternalError(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
        };
        (status, message).into_response()
    }
}

impl From<WorkerPoolError> for AppError {
    fn from(err: WorkerPoolError) -> Self {
        match err {
            WorkerPoolError::JobQueueFull => {
                AppError::ServiceUnavailable("Job queue is full".to_string())
            }
            WorkerPoolError::ChannelClosed => {
                AppError::InternalError("Worker pool channel closed".to_string())
            }
            WorkerPoolError::JobMissing(id) => {
                AppError::BadRequest(format!("Job {} not found", id))
            }
            WorkerPoolError::JobNotPending(id) => {
                AppError::BadRequest(format!("Job {} is not pending", id))
            }
        }
    }
}

impl From<WorkError> for AppError {
    fn from(err: WorkError) -> Self {
        match err {
            WorkError::BadInput(msg) => AppError::BadRequest(msg),
            WorkError::InternalError(msg) => AppError::InternalError(msg),
            WorkError::CancelledUnexpectedly => {
                AppError::InternalError("Work cancelled unexpectedly".to_string())
            }
            WorkError::JoinError => AppError::InternalError("Task join error".to_string()),
        }
    }
}

// ============================================================================
// Middleware
// ============================================================================

/// Authentication middleware - validates API keys
async fn auth_middleware(
    State(state): State<AppState>,
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> Result<Response, AppError> {
    if state.security.auth_disabled {
        return Ok(next.run(request).await);
    }

    // Extract API key from Authorization header or X-API-Key header
    let api_key = headers
        .get(AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .or_else(|| headers.get("x-api-key").and_then(|h| h.to_str().ok()));

    match api_key {
        Some(key) if state.security.verify_api_key(key) => Ok(next.run(request).await),
        _ => {
            warn!("Authentication failed: invalid or missing API key");
            Err(AppError::Unauthorized)
        }
    }
}

/// Rate limiting middleware - limits requests per IP
async fn rate_limit_middleware(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, AppError> {
    // Extract client IP (simplified - in production use forwarded headers)
    let client_ip = request
        .headers()
        .get("x-forwarded-for")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("unknown")
        .to_string();

    let limiter = state.rate_limiter.lock().await;

    match limiter.check_key(&client_ip) {
        Ok(_) => Ok(next.run(request).await),
        Err(_) => {
            warn!("Rate limit exceeded for IP: {}", client_ip);
            Err(AppError::TooManyRequests)
        }
    }
}

// ============================================================================
// Response Types
// ============================================================================

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub timestamp: String,
}

#[derive(Serialize)]
pub struct ReadyResponse {
    pub status: String,
    #[serde(rename = "jobsProcessing")]
    pub jobs_processing: usize,
    #[serde(rename = "jobsPending")]
    pub jobs_pending: usize,
    #[serde(rename = "jobCapacity")]
    pub job_capacity: usize,
    pub timestamp: String,
}

#[derive(Serialize)]
pub struct VersionResponse {
    pub version: String,
}

// ============================================================================
// Handlers
// ============================================================================

/// Health check endpoint (public)
async fn health_handler() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_string(),
        timestamp: Utc::now().to_rfc3339(),
    })
}

/// Readiness check with queue stats (public)
async fn ready_handler(State(state): State<AppState>) -> Json<ReadyResponse> {
    let processing = state.worker_pool.requests.processing_count().await;
    let pending = state.worker_pool.requests.pending_count().await;
    let capacity = state.worker_pool.requests.capacity;

    Json(ReadyResponse {
        status: "ok".to_string(),
        jobs_processing: processing,
        jobs_pending: pending,
        job_capacity: capacity,
        timestamp: Utc::now().to_rfc3339(),
    })
}

/// Version endpoint (public)
async fn version_handler() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Proof versions endpoint (public)
async fn proof_versions_handler() -> String {
    let mut fields = ProofVersioned::introspection().fields;
    fields.retain(|x| x != "Dummy");
    format!("{:?}", fields)
}

/// Check endpoint - validates proof preimage (protected)
async fn check_handler(
    State(state): State<AppState>,
    body: Bytes,
) -> Result<Response, AppError> {
    let request_start = Instant::now();
    let payload_size = body.len();

    info!("🔵 Check request received");
    info!("   Payload size: {} bytes ({:.2} KB)", payload_size, payload_size as f64 / 1024.0);

    // Submit work to pool
    let (_id, updates) = state
        .worker_pool
        .submit_and_subscribe(move |handle| {
            handle.block_on(async move {
                let (ppi, ir): (ProofPreimageVersioned, Option<WrappedIr>) =
                    tagged_deserialize(&body[..])
                        .map_err(|e| WorkError::BadInput(e.to_string()))?;

                let ir = match ir {
                    Some(ir) => ir.0,
                    None => {
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
                            Box::new(move |_loc: KeyLocation| {
                                Box::pin(std::future::ready(Ok(None)))
                            }),
                        );

                        let proof_data = resolver
                            .resolve_key(ppi.key_location().clone())
                            .await
                            .map_err(|e| WorkError::BadInput(e.to_string()))?;

                        proof_data
                            .ok_or_else(|| {
                                WorkError::BadInput(format!(
                                    "couldn't find built-in key {}",
                                    &ppi.key_location().0
                                ))
                            })?
                            .ir_source
                    }
                };

                let result = match ppi {
                    ProofPreimageVersioned::V1(ppi) => {
                        let ir: ZkirV2 = tagged_deserialize(&mut &ir[..])
                            .map_err(|e| WorkError::BadInput(e.to_string()))?;
                        ppi.check(&ir)
                            .map_err(|e| WorkError::BadInput(e.to_string()))?
                    }
                    _ => unreachable!(),
                };

                let result = result
                    .into_iter()
                    .map(|i| i.map(|i| i as u64))
                    .collect::<Vec<_>>();

                let mut response = Vec::new();
                tagged_serialize(&result, &mut response)
                    .map_err(|e| WorkError::InternalError(e.to_string()))?;
                Ok(response)
            })
        })
        .await?;

    // Wait for result
    let result = JobStatus::wait_for_success(&updates).await?;

    let total_elapsed = request_start.elapsed();
    info!("✅ Check request completed in {:.3}s", total_elapsed.as_secs_f64());
    debug!("   Response size: {} bytes", result.len());

    Ok((StatusCode::OK, result).into_response())
}

/// Prove endpoint - generates ZK proof (protected)
async fn prove_handler(
    State(state): State<AppState>,
    body: Bytes,
) -> Result<Response, AppError> {
    let request_start = Instant::now();
    let payload_size = body.len();

    info!("Prove request received");
    info!("   Payload size: {} bytes ({:.2} KB)", payload_size, payload_size as f64 / 1024.0);

    // Get queue stats before submission (only if debug logging is enabled)
    if tracing::enabled!(tracing::Level::DEBUG) {
        let queue_pending = state.worker_pool.requests.pending_count().await;
        let queue_processing = state.worker_pool.requests.processing_count().await;
        debug!("   Queue stats - Pending: {}, Processing: {}", queue_pending, queue_processing);
    }

    let body_clone = body.clone();
    let submit_start = Instant::now();

    // Submit work to pool
    let (_id, updates) = state
        .worker_pool
        .submit_and_subscribe(move |handle| {
            let proof_start = Instant::now();
            handle.block_on(async move {
                let (ppi, data, binding_input): (
                    ProofPreimageVersioned,
                    Option<ProvingKeyMaterial>,
                    Option<Fr>,
                ) = tagged_deserialize(&body_clone[..])
                    .map_err(|e| WorkError::BadInput(e.to_string()))?;

                // Log proof type if debug logging is enabled
                if tracing::enabled!(tracing::Level::DEBUG) {
                    tracing::debug!("   Proof type: {}", ppi.key_location().0);
                }

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
                    Box::new(move |_loc: KeyLocation| {
                        Box::pin(std::future::ready(Ok(data.clone())))
                    }),
                );

                // Track memory before proof generation
                let mem_before = get_memory_usage();

                let proof = match ppi {
                    ProofPreimageVersioned::V1(ppi) => {
                        let mut ppi_owned = Arc::try_unwrap(ppi)
                            .unwrap_or_else(|arc| (*arc).clone());
                        if let Some(binding_input) = binding_input {
                            ppi_owned.binding_input = binding_input;
                        }
                        ProofVersioned::V1(
                            ppi_owned.prove::<ZkirV2>(OsRng, &*PUBLIC_PARAMS, &resolver)
                                .await
                                .map_err(|e| WorkError::BadInput(e.to_string()))?
                                .0,
                        )
                    }
                    _ => unreachable!(),
                };

                // Track memory after proof generation
                let mem_after = get_memory_usage();

                let mut response = Vec::new();
                tagged_serialize(&proof, &mut response)
                    .map_err(|e| WorkError::InternalError(e.to_string()))?;

                let proof_elapsed = proof_start.elapsed();

                tracing::info!("   Proof generation completed in {:.3}s", proof_elapsed.as_secs_f64());

                // Calculate memory delta and log
                match (mem_before, mem_after) {
                    (Some(before), Some(after)) => {
                        let delta = after as i64 - before as i64;
                        let delta_mb = delta as f64 / 1_048_576.0;
                        tracing::info!("   Memory delta: {:+.2} MB (before: {:.2} MB, after: {:.2} MB)",
                                delta_mb,
                                before as f64 / 1_048_576.0,
                                after as f64 / 1_048_576.0);
                    }
                    _ => {
                        tracing::info!("   Memory tracking unavailable");
                    }
                };
                tracing::debug!("   Proof response size: {} bytes ({:.2} KB)", response.len(), response.len() as f64 / 1024.0);

                Ok(response)
            })
        })
        .await?;

    let submit_elapsed = submit_start.elapsed();
    debug!("   Submission + queue time: {:.3}s", submit_elapsed.as_secs_f64());

    // Wait for result
    let result = JobStatus::wait_for_success(&updates).await?;

    let total_elapsed = request_start.elapsed();
    info!("Prove request completed in {:.3}s", total_elapsed.as_secs_f64());
    info!("   Response size: {} bytes ({:.2} KB)", result.len(), result.len() as f64 / 1024.0);

    Ok((StatusCode::OK, result).into_response())
}

/// Prove transaction endpoint (protected)
async fn prove_tx_handler(
    State(state): State<AppState>,
    body: Bytes,
) -> Result<Response, AppError> {
    let request_start = Instant::now();
    info!("Prove-tx request received, payload size: {} bytes", body.len());

    // Submit work to pool
    let (_id, updates) = state
        .worker_pool
        .submit_and_subscribe(move |handle| {
            let proof_start = Instant::now();
            handle.block_on(async move {
                let (tx, keys): TransactionProvePayload<Signature> =
                    tagged_deserialize(&body[..])
                        .map_err(|e| WorkError::BadInput(e.to_string()))?;

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
                    Box::new(move |loc| {
                        Box::pin(std::future::ready(Ok(keys
                            .get(loc.0.as_ref())
                            .map(|v| v.clone()))))
                    }),
                );

                let provider = ZkirV2Local {
                    rng: OsRng,
                    params: &resolver,
                    resolver: &resolver,
                };

                // Track memory before proof generation
                let mem_before = get_memory_usage();

                // NOTE: The initial cost model here is part of why this is deprecated!
                // Use /prove instead!
                let proven_tx = tx.prove(provider, &INITIAL_TRANSACTION_COST_MODEL.runtime_cost_model)
                    .await
                    .map_err(|e| WorkError::BadInput(e.to_string()))?;

                // Track memory after proof generation
                let mem_after = get_memory_usage();

                let mut response = Vec::new();
                tagged_serialize(&proven_tx, &mut response)
                    .map_err(|e| WorkError::InternalError(e.to_string()))?;

                let proof_elapsed = proof_start.elapsed();

                tracing::info!("   Transaction proof generation completed in {:.3}s", proof_elapsed.as_secs_f64());

                // Calculate memory delta and log
                match (mem_before, mem_after) {
                    (Some(before), Some(after)) => {
                        let delta = after as i64 - before as i64;
                        let delta_mb = delta as f64 / 1_048_576.0;
                        tracing::info!("   Memory delta: {:+.2} MB (before: {:.2} MB, after: {:.2} MB)",
                                delta_mb,
                                before as f64 / 1_048_576.0,
                                after as f64 / 1_048_576.0);
                    }
                    _ => {
                        tracing::info!("   Memory tracking unavailable");
                    }
                };

                Ok(response)
            })
        })
        .await?;

    // Wait for result
    let result = JobStatus::wait_for_success(&updates).await?;

    let total_elapsed = request_start.elapsed();
    info!("Prove-tx request completed in {:.3}s", total_elapsed.as_secs_f64());

    Ok((StatusCode::OK, result).into_response())
}

/// K parameter endpoint (protected)
async fn k_handler(body: Bytes) -> Result<Response, AppError> {
    info!("K parameter request received");
    if tracing::enabled!(tracing::Level::DEBUG) {
        // Only encode hex if debug logging is enabled (can be expensive for large payloads)
        debug!("Received request: {}", (&body[..]).encode_hex::<String>());
    }

    let zkir: ZkirV2 = tagged_deserialize(&body[..])
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    let k = zkir.k();

    Ok((StatusCode::OK, format!("{}", k)).into_response())
}

/// Fetch params endpoint - pre-fetches ZSwap parameters for given k value (public, optional)
async fn fetch_params_handler(Path(k): Path<u8>) -> Result<String, AppError> {
    if !(10..=24).contains(&k) {
        return Err(AppError::BadRequest(format!("k={} out of range (must be 10-24)", k)));
    }

    PUBLIC_PARAMS
        .0
        .fetch_k(k)
        .await
        .map_err(|e| AppError::InternalError(format!("Failed to fetch k={}: {}", k, e)))?;

    Ok("success".to_string())
}

/// PCR measurements endpoint - directs clients to canonical external PCR publication
/// PCRs are published externally to avoid circular dependency in reproducible builds
async fn pcr_measurements_handler() -> Result<Response, AppError> {
    info!("PCR measurements request received");

    // PCR measurements are published externally for several reasons:
    // 1. Avoids circular dependency (embedding PCRs changes the binary, which changes PCR0)
    // 2. Enables reproducible builds (anyone can rebuild and verify)
    // 3. Supports multiple publication channels for transparency
    // 4. Allows graceful updates and version management

    let version = env!("CARGO_PKG_VERSION");

    let response = serde_json::json!({
        "message": "PCR measurements are published externally for reproducibility and transparency",
        "version": version,
        "canonical_sources": {
            "github_release": format!("https://github.com/midnight/midnight-ledger/releases/download/v{}/pcr-measurements.json", version),
            "cdn": format!("https://cdn.midnight.network/proof-server/v{}/pcr-measurements.json", version),
            "latest": "https://proof-test.devnet.midnight.network/.well-known/pcr-measurements.json"
        },
        "verification_process": {
            "step1": "Fetch PCR measurements from canonical source",
            "step2": "Request attestation document from /attestation?nonce=<random>",
            "step3": "Decode attestation document (CBOR format)",
            "step4": "Verify certificate chain against AWS Nitro root certificate",
            "step5": "Compare PCRs in attestation document against published values",
            "step6": "Verify nonce matches and timestamp is recent"
        },
        "documentation": "https://docs.midnight.network/proof-server/attestation-verification",
        "notes": [
            "PCR0 uniquely identifies the enclave image (changes with code updates)",
            "PCR1 identifies the kernel and boot configuration (stable)",
            "PCR2 identifies CPU and memory configuration",
            "All PCRs are SHA384 hashes and are deterministically reproducible"
        ]
    });

    Ok((StatusCode::OK, Json(response)).into_response())
}

// ============================================================================
// Router Setup
// ============================================================================

pub fn create_app(state: AppState) -> Router {
    // Public routes (no authentication required)
    let mut public_routes = Router::new()
        .route("/", get(health_handler))  // Root endpoint aliases to health
        .route("/health", get(health_handler))
        .route("/ready", get(ready_handler))
        .route("/version", get(version_handler))
        .route("/proof-versions", get(proof_versions_handler))
        .route("/attestation", get(attestation_handler))
        .route("/.well-known/pcr-measurements.json", get(pcr_measurements_handler))
        .route("/pcr", get(pcr_measurements_handler));  // Convenience alias

    // Conditionally add /fetch-params/{k} endpoint
    if state.enable_fetch_params {
        public_routes = public_routes.route("/fetch-params/:k", get(fetch_params_handler));
    }

    // Protected routes (authentication required)
    let protected_routes = Router::new()
        .route("/check", post(check_handler))
        .route("/prove", post(prove_handler))
        .route("/prove-tx", post(prove_tx_handler))
        .route("/k", post(k_handler))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));

    // Combine routes and add global middleware
    // Note: Middleware layers are applied in reverse order (bottom-up)
    Router::new()
        .merge(public_routes)
        .merge(protected_routes)
        .with_state(state.clone())
        .layer(middleware::from_fn_with_state(
            state.clone(),
            rate_limit_middleware,
        ))
        .layer(RequestBodyLimitLayer::new(state.security.max_payload_size))
        .layer(TimeoutLayer::new(Duration::from_secs(660))) // 11 minutes
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive()) // Configure restrictive CORS in production
}
