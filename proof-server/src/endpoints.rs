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
use actix_web::error::ErrorBadRequest;
use actix_web::http::StatusCode;
use actix_web::web::{self, Bytes, BytesMut, Data, Payload};
use actix_web::{Error, HttpResponse, HttpResponseBuilder, Responder, get, post};
use base_crypto::data_provider::{self, MidnightDataProvider};
use base_crypto::data_provider::{FetchMode, OutputMode};
use base_crypto::schnorr::Signature;
use futures_util::stream::StreamExt;
use hex::ToHex;
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
use std::collections::HashMap;
use std::sync::Arc;
use storage::db::InMemoryDB;
use tracing::{debug, info};
use transient_crypto::commitment::PedersenRandomness;
use transient_crypto::curve::Fr;
use transient_crypto::proofs::{KeyLocation, ProvingKeyMaterial, Resolver as ResolverT, WrappedIr};

use zkir as zkir_v2;
use zswap::prove::ZswapResolver;

use crate::versioned_ir;
use crate::worker_pool::{JobStatus, WorkError, WorkerPool};

#[cfg(feature = "gcp_cs")]
use {
    crate::ServerEncryptionKey,
    actix_web::HttpRequest,
    async_channel::Receiver,
    futures_util::stream::once,
    http_body_util::{BodyExt, Full},
    hyper::{Method, Request},
    hyper_util::client::legacy::Client as HyperClient,
    hyper_util::rt::TokioExecutor,
    hyperlocal::{UnixConnector, Uri},
    uuid::Uuid,
};

#[cfg(feature = "gcp_cs")]
const ENCRYPTION_TYPE_HEADER: &str = "encryption-type";
#[cfg(feature = "gcp_cs")]
const CLIENT_PUBLIC_KEY_HEADER: &str = "client-public-key";
#[cfg(feature = "gcp_cs")]
const REQUEST_NONCE_HEADER: &str = "request-nonce";
#[cfg(feature = "gcp_cs")]
const RESPONSE_NONCE_HEADER: &str = "response-nonce";
#[cfg(feature = "gcp_cs")]
const PROOF_JOB_ID_HEADER: &str = "proof-job-id";

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

async fn payload_to_bytes(mut payload: Payload) -> Result<Bytes, Error> {
    let mut body = BytesMut::new();
    while let Some(chunk) = payload.next().await {
        let chunk = chunk?;
        body.extend_from_slice(&chunk);
    }
    Ok(body.freeze())
}

type TransactionProvePayload<S> = (
    Transaction<S, ProofPreimageMarker, PedersenRandomness, InMemoryDB>,
    HashMap<String, ProvingKeyMaterial>,
);

#[cfg(feature = "gcp_cs")]
#[derive(serde::Deserialize)]
pub(crate) struct AttestationQuery {
    nonce: String,
}

#[cfg(feature = "gcp_cs")]
#[derive(serde::Serialize)]
struct GcpAttestationRequest {
    audience: String,
    nonces: Vec<String>,
    token_type: String,
}

#[cfg(feature = "gcp_cs")]
#[derive(Clone)]
struct EncryptedTransportHeaders {
    client_public_key_hex: String,
    request_nonce_hex: String,
}

#[cfg(feature = "gcp_cs")]
fn get_encrypted_transport_headers(
    req: &HttpRequest,
) -> Result<Option<EncryptedTransportHeaders>, Error> {
    let client_public_key = req.headers().get(CLIENT_PUBLIC_KEY_HEADER);
    let request_nonce = req.headers().get(REQUEST_NONCE_HEADER);

    match (client_public_key, request_nonce) {
        (None, None) => Ok(None),
        (Some(_), Some(_)) => Ok(Some(EncryptedTransportHeaders {
            client_public_key_hex: client_public_key
                .unwrap()
                .to_str()
                .map_err(|_| ErrorBadRequest(format!("invalid {CLIENT_PUBLIC_KEY_HEADER} header")))?
                .to_owned(),
            request_nonce_hex: request_nonce
                .unwrap()
                .to_str()
                .map_err(|_| ErrorBadRequest(format!("invalid {REQUEST_NONCE_HEADER} header")))?
                .to_owned(),
        })),
        _ => Err(ErrorBadRequest(format!(
            "{CLIENT_PUBLIC_KEY_HEADER} and {REQUEST_NONCE_HEADER} must both be provided"
        ))),
    }
}

#[cfg(feature = "gcp_cs")]
fn get_encryption_type(req: &HttpRequest) -> Result<&'static str, Error> {
    match req.headers().get(ENCRYPTION_TYPE_HEADER) {
        Some(value) => match value.to_str().unwrap_or("").trim().to_lowercase().as_str() {
            "oidc" => Ok("OIDC"),
            "pki" => Ok("PKI"),
            _ => Err(ErrorBadRequest("the encryption-type is not set correctly")),
        },
        None => Err(ErrorBadRequest("the encryption-type is not set correctly")),
    }
}

#[get("/version")]
pub(crate) async fn version() -> impl Responder {
    env!("CARGO_PKG_VERSION")
}

#[get("/fetch-params/{k}")]
pub(crate) async fn fetch_k(path: web::Path<u8>) -> impl Responder {
    let k = path.into_inner();
    if !(0..=25).contains(&k) {
        return Err(ErrorBadRequest(format!("k={k} out of range")));
    }
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

#[cfg(feature = "gcp_cs")]
fn decode_proving_request(
    request: Bytes,
    transport_headers: &Option<EncryptedTransportHeaders>,
    server_encryption_key: &ServerEncryptionKey,
) -> Result<Vec<u8>, Error> {
    match transport_headers {
        Some(headers) => server_encryption_key
            .decrypt_request(
                &headers.client_public_key_hex,
                &headers.request_nonce_hex,
                &request,
            )
            .ok_or_else(|| ErrorBadRequest("failed to decrypt request payload")),
        None => Ok(request.to_vec()),
    }
}

#[cfg(feature = "gcp_cs")]
fn encode_proving_response_body(
    response: Vec<u8>,
    transport_headers: &Option<EncryptedTransportHeaders>,
    response_nonce_hex: &Option<String>,
    server_encryption_key: &ServerEncryptionKey,
) -> Result<Bytes, Error> {
    match transport_headers {
        Some(headers) => Ok(Bytes::from(
            server_encryption_key
                .encrypt_response_with_nonce(
                    &headers.client_public_key_hex,
                    response_nonce_hex.as_deref().ok_or_else(|| {
                        actix_web::error::ErrorInternalServerError(
                            "missing response nonce for encrypted payload",
                        )
                    })?,
                    &response,
                )
                .ok_or_else(|| {
                    actix_web::error::ErrorInternalServerError("failed to encrypt response payload")
                })?,
        )),
        None => Ok(Bytes::from(response)),
    }
}

#[cfg(feature = "gcp_cs")]
fn proving_response(
    job_id: Uuid,
    updates: Arc<Receiver<JobStatus>>,
    transport_headers: Option<EncryptedTransportHeaders>,
    server_encryption_key: ServerEncryptionKey,
) -> HttpResponse {
    let response_nonce_hex = transport_headers
        .as_ref()
        .map(|_| server_encryption_key.generate_response_nonce_hex());

    let mut builder = HttpResponse::Ok();
    builder.append_header((PROOF_JOB_ID_HEADER, job_id.to_string()));
    if let Some(response_nonce_hex) = response_nonce_hex.as_ref() {
        builder.append_header((RESPONSE_NONCE_HEADER, response_nonce_hex.clone()));
    }

    builder.streaming(once(async move {
        let response = JobStatus::wait_for_success(updates.as_ref()).await?;
        encode_proving_response_body(
            response,
            &transport_headers,
            &response_nonce_hex,
            &server_encryption_key,
        )
    }))
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
pub(crate) async fn get_k(payload: Payload) -> Result<HttpResponse, Error> {
    info!("Starting to process request for /k...");
    let request = payload_to_bytes(payload).await?;
    debug!(
        "Received request: {}",
        (&request[..]).encode_hex::<String>()
    );

    let k = versioned_ir::k(&request).map_err(ErrorBadRequest)?;

    Ok(HttpResponse::Ok().body(format!("{k}")))
}

#[cfg(feature = "gcp_cs")]
#[post("/attestation")]
pub(crate) async fn attestation(
    req: HttpRequest,
    query: web::Query<AttestationQuery>,
    server_encryption_key: Data<ServerEncryptionKey>,
) -> Result<HttpResponse, Error> {
    info!("Starting to process request for /attestation...");

    let encryption_type = get_encryption_type(&req)?;

    let payload = GcpAttestationRequest {
        audience: "https://sts.googleapis.com".to_string(),
        nonces: vec![
            query.nonce.clone(),
            server_encryption_key.public_key_hex().to_owned(),
        ],
        token_type: encryption_type.to_string(),
    };

    let body = serde_json::to_vec(&payload).map_err(actix_web::error::ErrorInternalServerError)?;

    let hyper_client: HyperClient<UnixConnector, Full<hyper::body::Bytes>> =
        HyperClient::builder(TokioExecutor::new()).build(UnixConnector);
    let uri: hyper::Uri = Uri::new("/run/container_launcher/teeserver.sock", "/v1/token").into();

    let request = Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header("Host", "localhost")
        .header("Content-Type", "application/json")
        .body(Full::new(hyper::body::Bytes::from(body)))
        .map_err(actix_web::error::ErrorInternalServerError)?;

    let response = hyper_client
        .request(request)
        .await
        .map_err(actix_web::error::ErrorInternalServerError)?;
    let response_bytes = response
        .collect()
        .await
        .map_err(actix_web::error::ErrorInternalServerError)?
        .to_bytes();
    let token = String::from_utf8(response_bytes.to_vec())
        .map_err(actix_web::error::ErrorInternalServerError)?;

    Ok(HttpResponse::Ok().json(serde_json::json!({ "token": token })))
}

#[post("/check")]
pub(crate) async fn check(
    pool: Data<Arc<WorkerPool>>,
    payload: Payload,
) -> Result<HttpResponse, Error> {
    info!("Starting to process request for /check...");
    let request = payload_to_bytes(payload).await?;
    debug!(
        "Received request: {}",
        (&request[..]).encode_hex::<String>()
    );
    let (ppi, ir): (ProofPreimageVersioned, Option<WrappedIr>) =
        tagged_deserialize(&request[..]).map_err(ErrorBadRequest)?;
    let (_id, updates) = pool
        .submit_and_subscribe(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
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
    #[cfg(feature = "gcp_cs")] req: HttpRequest,
    #[cfg(feature = "gcp_cs")] server_encryption_key: Data<ServerEncryptionKey>,
    payload: Payload,
) -> Result<HttpResponse, Error> {
    info!("Starting to process request for /prove...");
    let request = payload_to_bytes(payload).await?;

    #[cfg(feature = "gcp_cs")]
    let (transport_headers, request) = {
        let transport_headers = get_encrypted_transport_headers(&req)?;
        let request = decode_proving_request(request, &transport_headers, &server_encryption_key)?;
        (transport_headers, request)
    };

    debug!(
        "Received request: {}",
        (&request[..]).encode_hex::<String>()
    );

    let (ppi, data, binding_input): (
        ProofPreimageVersioned,
        Option<ProvingKeyMaterial>,
        Option<Fr>,
    ) = tagged_deserialize(&request[..]).map_err(ErrorBadRequest)?;

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

                        let proof = versioned_ir::prove(ppi, &proving_data.ir_source, &resolver)
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

    #[cfg(feature = "gcp_cs")]
    return Ok(proving_response(
        _id,
        updates,
        transport_headers,
        server_encryption_key.get_ref().clone(),
    ));

    #[cfg(not(feature = "gcp_cs"))]
    {
        let response = JobStatus::wait_for_success(&updates).await?;
        Ok(HttpResponse::Ok().body(response))
    }
}

#[post("/prove-tx")]
pub(crate) async fn prove_transaction(
    pool: Data<Arc<WorkerPool>>,
    #[cfg(feature = "gcp_cs")] req: HttpRequest,
    #[cfg(feature = "gcp_cs")] server_encryption_key: Data<ServerEncryptionKey>,
    payload: Payload,
) -> Result<HttpResponse, Error> {
    info!("Starting to process request for /prove-tx...");
    let request = payload_to_bytes(payload).await?;

    #[cfg(feature = "gcp_cs")]
    let (transport_headers, request) = {
        let transport_headers = get_encrypted_transport_headers(&req)?;
        let request = decode_proving_request(request, &transport_headers, &server_encryption_key)?;
        (transport_headers, request)
    };

    debug!(
        "Received request: {}",
        (&request[..]).encode_hex::<String>()
    );

    let (tx, keys): TransactionProvePayload<Signature> =
        tagged_deserialize(&request[..]).map_err(ErrorBadRequest)?;
    let (_id, updates) = pool
        .submit_and_subscribe(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
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
                    params: &resolver,
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

    #[cfg(feature = "gcp_cs")]
    return Ok(proving_response(
        _id,
        updates,
        transport_headers,
        server_encryption_key.get_ref().clone(),
    ));

    #[cfg(not(feature = "gcp_cs"))]
    {
        let response = JobStatus::wait_for_success(&updates).await?;
        Ok(HttpResponse::Ok().body(response))
    }
}
