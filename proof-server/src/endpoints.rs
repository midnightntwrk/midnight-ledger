// This file is part of midnight-ledger.
// Copyright (C) Midnight Foundation
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");
// You may not use this file except in compliance with the License.
// You may obtain a copy of the License at
// http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

#![deny(unreachable_pub)]
#![deny(warnings)]
use actix_web::error::{ErrorBadRequest, ErrorPayloadTooLarge};
use actix_web::http::StatusCode;
use actix_web::web::{self, Bytes, BytesMut, Data, Payload};
use actix_web::{Error, HttpResponse, HttpResponseBuilder, Responder, get, post};
use base_crypto::data_provider::{self, MidnightDataProvider};
use base_crypto::data_provider::{FetchMode, OutputMode};
use futures_util::stream::StreamExt;
use hex::ToHex;
use introspection::Introspection;
use lazy_static::lazy_static;
use ledger::dust::DustResolver;
use ledger::prove::Resolver;
use ledger::structure::{
    INITIAL_TRANSACTION_COST_MODEL, ProofPreimageMarker, ProofPreimageVersioned, ProofVersioned,
    Signature, Transaction,
};
use rand::rngs::OsRng;
use serialize::{tagged_deserialize, tagged_serialize};
use std::collections::HashMap;
use std::io;
use std::sync::Arc;
use storage::db::InMemoryDB;
use tracing::{debug, info};
use transient_crypto::commitment::PedersenRandomness;
use transient_crypto::curve::Fr;
use transient_crypto::proofs::{
    KeyLocation, ParamsProver, ParamsProverProvider, ProvingKeyMaterial, Resolver as ResolverT,
    WrappedIr,
};

use zkir as zkir_v2;
use zswap::prove::ZswapResolver;

use crate::versioned_ir;
use crate::worker_pool::{JobStatus, WorkError, WorkerPool};

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

/// The default for [`BodyLimit`]: 256 MiB.
///
/// The largest built-in prover key (zswap spend) is about 25 MB, and prover keys roughly double
/// with each increment of `k`, so this leaves room for a transaction carrying several contract
/// keys.
pub const DEFAULT_MAX_BODY_SIZE: usize = 256 * 1024 * 1024;

/// The largest request body, in bytes, this server reads before rejecting the request with
/// HTTP 413.
#[derive(Clone, Copy, Debug)]
pub struct BodyLimit(pub usize);

impl Default for BodyLimit {
    fn default() -> Self {
        BodyLimit(DEFAULT_MAX_BODY_SIZE)
    }
}

async fn payload_to_bytes(mut payload: Payload, limit: &BodyLimit) -> Result<Bytes, Error> {
    let mut body = BytesMut::new();
    while let Some(chunk) = payload.next().await {
        let chunk = chunk?;
        if body.len().saturating_add(chunk.len()) > limit.0 {
            return Err(ErrorPayloadTooLarge(format!(
                "request body exceeds this server's limit of {} bytes",
                limit.0
            )));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body.freeze())
}

/// The largest circuit size (`k`) this server accepts for proving, or `None` for no limit.
///
/// Proving memory grows with `2^k`, so the limit lets operators reject circuits that would exceed
/// the host's memory instead of having the process killed mid-proof.
///
/// The limit is enforced where public parameters are loaded (see [`LimitedParams`]), on the `k`
/// the prover actually asks for. That is the prover key's `k`, which a request can choose
/// independently of its IR, so checking the IR alone would not bound it. It also avoids
/// synthesizing the circuit on the request path just to learn its `k`.
#[derive(Clone, Copy, Debug, Default)]
pub struct MaxK(pub Option<u8>);

impl MaxK {
    fn exceeded(&self, k: u8) -> Option<String> {
        match self.0 {
            Some(max_k) if k > max_k => Some(format!(
                "k={k} exceeds this server's limit of max_k={max_k}"
            )),
            _ => None,
        }
    }

    fn check(&self, k: u8) -> Result<(), Error> {
        match self.exceeded(k) {
            Some(msg) => Err(ErrorBadRequest(msg)),
            None => Ok(()),
        }
    }
}

/// The server's public parameters, refusing any `k` above [`MaxK`].
///
/// Every proving path loads parameters through this, so the limit holds whichever key or IR
/// the request supplied.
#[derive(Clone, Copy, Debug)]
pub(crate) struct LimitedParams(pub(crate) MaxK);

impl ParamsProverProvider for LimitedParams {
    async fn get_params(&self, k: u8) -> io::Result<ParamsProver> {
        if let Some(msg) = self.0.exceeded(k) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, msg));
        }
        PUBLIC_PARAMS.get_params(k).await
    }
}

type TransactionProvePayload<S> = (
    Transaction<S, ProofPreimageMarker, PedersenRandomness, InMemoryDB>,
    HashMap<String, ProvingKeyMaterial>,
);

#[get("/version")]
pub(crate) async fn version() -> impl Responder {
    env!("CARGO_PKG_VERSION")
}

#[get("/fetch-params/{k}")]
pub(crate) async fn fetch_k(path: web::Path<u8>, max_k: Data<MaxK>) -> impl Responder {
    let k = path.into_inner();
    if !(0..=25).contains(&k) {
        return Err(ErrorBadRequest(format!("k={k} out of range")));
    }
    max_k.check(k)?;
    PUBLIC_PARAMS.0.fetch_k(k).await?;
    Ok("success")
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HealthResponse {
    status: &'static str,
    timestamp: time::OffsetDateTime,
}

pub(crate) async fn health() -> Result<web::Json<HealthResponse>, Error> {
    let status = HealthResponse {
        status: "ok",
        timestamp: time::OffsetDateTime::now_utc(),
    };
    Ok(web::Json(status))
}

#[derive(Clone, Copy, serde::Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
enum Status {
    Ok,
    Busy,
}

impl From<Status> for StatusCode {
    fn from(val: Status) -> Self {
        match val {
            Status::Ok => StatusCode::OK,
            Status::Busy => StatusCode::SERVICE_UNAVAILABLE,
        }
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ReadyResponse {
    status: Status,
    jobs_processing: usize,
    jobs_pending: usize,
    job_capacity: usize,
    timestamp: time::OffsetDateTime,
}

#[get("/ready")]
pub(crate) async fn ready(pool: web::Data<Arc<WorkerPool>>) -> Result<HttpResponse, Error> {
    let jobs_processing = pool.requests.processing_count().await;
    let jobs_pending = pool.requests.pending_count().await;
    let job_capacity = pool.requests.capacity;
    let status = ReadyResponse {
        status: if pool.requests.is_full().await {
            Status::Busy
        } else {
            Status::Ok
        },
        jobs_processing,
        jobs_pending,
        job_capacity,
        timestamp: time::OffsetDateTime::now_utc(),
    };

    let builder = HttpResponseBuilder::new(status.status.into()).json(status);
    Ok(builder)
}

#[get("/proof-versions")]
pub(crate) async fn proof_versions() -> impl Responder {
    let mut fields = ProofVersioned::introspection().fields;
    fields.retain(|x| x != "Dummy");
    format!("{:?}", fields)
}

#[post("/k")]
pub(crate) async fn get_k(
    pool: Data<Arc<WorkerPool>>,
    limit: Data<BodyLimit>,
    payload: Payload,
) -> Result<HttpResponse, Error> {
    info!("Starting to process request for /k...");
    let request = payload_to_bytes(payload, &limit).await?;
    debug!(
        "Received request: {}",
        (&request[..]).encode_hex::<String>()
    );

    // Computing `k` synthesizes the circuit, which costs time and memory that grow with the
    // request's IR. Run it in the worker pool, like proving, so concurrent requests are bounded
    // by the pool's workers and job capacity instead of each taking a thread of its own.
    let (_id, updates) = pool
        .submit_and_subscribe(move || {
            let k = versioned_ir::k(&request).map_err(WorkError::BadInput)?;
            Ok(format!("{k}").into_bytes())
        })
        .await?;
    let response = JobStatus::wait_for_success(&updates).await?;

    Ok(HttpResponse::Ok().body(response))
}

#[post("/check")]
pub(crate) async fn check(
    pool: Data<Arc<WorkerPool>>,
    limit: Data<BodyLimit>,
    payload: Payload,
) -> Result<HttpResponse, Error> {
    info!("Starting to process request for /check...");
    let request = payload_to_bytes(payload, &limit).await?;
    debug!(
        "Received request: {}",
        (&request[..]).encode_hex::<String>()
    );
    let (ppi, ir): (ProofPreimageVersioned, Option<WrappedIr>) =
        tagged_deserialize(&request[..]).map_err(ErrorBadRequest)?;
    let (_id, updates) = pool
        .submit_and_subscribe(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async move {
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
                            Box::new(move |_: KeyLocation| Box::pin(std::future::ready(Ok(None)))),
                        );
                        let proof_data = resolver
                            .resolve_key(ppi.key_location().clone())
                            .await
                            .map_err(|e| WorkError::BadInput(e.to_string()))?;

                        proof_data
                            .ok_or_else(|| {
                                WorkError::BadInput(format!(
                                    "couldn't find built-in key {}",
                                    ppi.key_location().0
                                ))
                            })?
                            .ir_source
                    }
                };
                let result = match ppi {
                    ProofPreimageVersioned::V2(ppi) => {
                        versioned_ir::check(ppi, &ir).map_err(WorkError::BadInput)?
                    }
                    // Footgun: If we add a new version, this needs to be covered here, but it's marked
                    // #[non_exhaustive], so we always need the base case.
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
    let response = JobStatus::wait_for_success(&updates).await?;

    Ok(HttpResponse::Ok().body(response))
}

#[post("/prove")]
pub(crate) async fn prove(
    pool: Data<Arc<WorkerPool>>,
    max_k: Data<MaxK>,
    limit: Data<BodyLimit>,
    payload: Payload,
) -> Result<HttpResponse, Error> {
    info!("Starting to process request for /prove...");
    let request = payload_to_bytes(payload, &limit).await?;
    debug!(
        "Received request: {}",
        (&request[..]).encode_hex::<String>()
    );
    let (ppi, data, binding_input): (
        ProofPreimageVersioned,
        Option<ProvingKeyMaterial>,
        Option<Fr>,
    ) = tagged_deserialize(&request[..]).map_err(ErrorBadRequest)?;

    let params = LimitedParams(**max_k);
    let data_resolver = data.clone();
    let (_id, updates) = pool
        .submit_and_subscribe(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async move {
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
                    Box::new(move |_: KeyLocation| {
                        Box::pin(std::future::ready(Ok(data_resolver.clone())))
                    }),
                );
                let proof = match ppi {
                    ProofPreimageVersioned::V2(mut ppi) => {
                        if let Some(binding_input) = binding_input {
                            let mut inner = (*ppi).clone();
                            inner.binding_input = binding_input;
                            ppi = Arc::new(inner);
                        }
                        let proving_data = match data {
                            Some(pkm) => pkm,
                            None => resolver
                                .resolve_key(ppi.key_location.clone())
                                .await
                                .map_err(|e| WorkError::BadInput(e.to_string()))?
                                .ok_or_else(|| {
                                    WorkError::BadInput(format!(
                                        "couldn't find key {}",
                                        ppi.key_location.0
                                    ))
                                })?,
                        };

                        let proof =
                            versioned_ir::prove(ppi, &proving_data.ir_source, &resolver, &params)
                                .await
                                .map_err(WorkError::BadInput)?
                                .0;

                        ProofVersioned::V2(proof)
                    }
                    // Footgun: If we add a new version, this needs to be covered here, but it's marked
                    // #[non_exhaustive], so we always need the base case.
                    _ => unreachable!(),
                };
                let mut response = Vec::new();
                tagged_serialize(&proof, &mut response)
                    .map_err(|e| WorkError::InternalError(e.to_string()))?;
                Ok(response)
            })
        })
        .await?;
    let response = JobStatus::wait_for_success(&updates).await?;

    Ok(HttpResponse::Ok().body(response))
}

#[post("/prove-tx")]
pub(crate) async fn prove_transaction(
    pool: Data<Arc<WorkerPool>>,
    max_k: Data<MaxK>,
    limit: Data<BodyLimit>,
    payload: Payload,
) -> Result<HttpResponse, Error> {
    info!("Starting to process request for /prove-tx...");
    let request = payload_to_bytes(payload, &limit).await?;
    debug!(
        "Received request: {}",
        (&request[..]).encode_hex::<String>()
    );
    let (tx, keys): TransactionProvePayload<Signature> =
        tagged_deserialize(&request[..]).map_err(ErrorBadRequest)?;
    let params = LimitedParams(**max_k);
    let (_id, updates) = pool
        .submit_and_subscribe(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async move {
                let mut response = Vec::new();
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
                        Box::pin(std::future::ready(Ok(keys.get(loc.0.as_ref()).cloned())))
                    }),
                );
                let provider = zkir_v2::LocalProvingProvider {
                    rng: OsRng,
                    params: &params,
                    resolver: &resolver,
                };
                // NOTE: The initial cost model here is part of why this is deprecated!
                // Use /prove instead!
                tagged_serialize(
                    &tx.prove(provider, &INITIAL_TRANSACTION_COST_MODEL.runtime_cost_model)
                        .await
                        .map_err(|e| WorkError::BadInput(e.to_string()))?,
                    &mut response,
                )
                .map_err(|e| WorkError::InternalError(e.to_string()))?;
                Ok(response)
            })
        })
        .await?;
    let response = JobStatus::wait_for_success(&updates).await?;
    Ok(HttpResponse::Ok().body(response))
}
