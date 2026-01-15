// This file is part of midnight-ledger.
// Copyright (C) 2025 Midnight Foundation
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

#![deny(warnings)]

mod common {
    use std::sync::{LazyLock, OnceLock};
    use std::time::Duration;

    use actix_web::{dev::ServerHandle, rt};
    use midnight_proof_server::server;
    use midnight_proof_server::worker_pool::WorkerPool;
    use reqwest::Client;

    pub const DEFAULT_NUM_WORKERS: usize = 2;
    pub const DEFAULT_JOB_LIMIT: usize = 2;
    pub const REQUEST_TIMEOUT_SECS: u64 = 5;
    pub const LONG_REQUEST_TIMEOUT_SECS: u64 = 30;

    static LOGGER_INIT: OnceLock<()> = OnceLock::new();

    pub static HTTP_CLIENT: LazyLock<Client> = LazyLock::new(|| {
        Client::builder()
            .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
            .build()
            .expect("Failed to create HTTP client")
    });

    pub struct TestServer {
        pub handle: ServerHandle,
        pub port: u16,
    }

    impl TestServer {
        pub fn base_url(&self) -> String {
            format!("http://127.0.0.1:{}", self.port)
        }
    }

    pub fn init_logger() {
        LOGGER_INIT.get_or_init(|| {
            env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));
        });
    }

    pub fn build_client(timeout_secs: u64) -> Client {
        Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .build()
            .expect("Failed to create HTTP client")
    }

    pub fn start_server(num_workers: usize, job_limit: usize) -> TestServer {
        start_server_impl(num_workers, job_limit, false)
    }

    pub fn start_server_with_fetch_params(
        num_workers: usize,
        job_limit: usize,
        fetch_params: bool,
    ) -> TestServer {
        start_server_impl(num_workers, job_limit, fetch_params)
    }

    fn start_server_impl(num_workers: usize, job_limit: usize, fetch_params: bool) -> TestServer {
        init_logger();

        let (tx, rx) = std::sync::mpsc::channel();

        std::thread::spawn(move || {
            rt::System::new().block_on(async move {
                let pool = WorkerPool::new(num_workers, job_limit, 600.0);
                let (srv, bound_port) =
                    server(0, fetch_params, pool).expect("Failed to start server");
                tx.send((srv.handle(), bound_port))
                    .expect("Failed to send server handle");
                srv.await.expect("Server error");
            });
        });

        let (handle, port) = rx.recv().expect("Failed to receive server handle");
        log::info!("Started test server on port {}", port);

        TestServer { handle, port }
    }

    pub async fn stop_server(server: TestServer) {
        log::info!("Stopping server on port {}", server.port);
        server.handle.stop(false).await;
    }
}

mod test_data {
    use std::sync::{Arc, LazyLock};

    use base_crypto::signatures::Signature;
    use base_crypto::time::Timestamp;
    use coin_structure::coin;
    use ledger::structure::{
        ContractDeploy, Intent, ProofPreimageMarker, ProofPreimageVersioned, SignatureKind,
        Transaction,
    };
    use ledger::test_utilities::{Resolver, test_resolver, verifier_key};
    use onchain_runtime::state::{ContractOperation, ContractState, StateValue, stval};
    use rand::SeedableRng;
    use rand::rngs::StdRng;
    use storage::arena::Sp;
    use storage::db::{DB, InMemoryDB};
    use storage::storage::HashMap;
    use transient_crypto::commitment::PedersenRandomness;
    use zswap::Delta;

    pub static RESOLVER: LazyLock<Resolver> = LazyLock::new(|| test_resolver("fallible"));

    pub fn create_unbalanced_zswap_tx(
        num_outputs: usize,
    ) -> Transaction<Signature, ProofPreimageMarker, PedersenRandomness, InMemoryDB> {
        let mut rng = StdRng::seed_from_u64(0x42);
        let claim_amount = 100u128;
        let sks = zswap::keys::SecretKeys::from_rng_seed(&mut rng);

        let outputs = (0..num_outputs)
            .map(|_| {
                let coin = coin::Info::new(&mut rng, claim_amount, Default::default());
                zswap::Output::new::<_>(
                    &mut rng,
                    &coin,
                    None,
                    &sks.coin_public_key(),
                    Some(sks.enc_public_key()),
                )
                .expect("Failed to create output")
            })
            .fold(storage::storage::Array::new(), |acc, out| acc.push(out));

        let deltas = [Delta {
            token_type: Default::default(),
            value: -((claim_amount * num_outputs as u128) as i128),
        }]
        .into_iter()
        .collect();

        let mut offer = zswap::Offer {
            inputs: vec![].into(),
            outputs,
            transient: vec![].into(),
            deltas,
        };
        offer.normalize();

        Transaction::new(
            "local-test",
            Default::default(),
            Some(offer),
            Default::default(),
        )
    }

    pub async fn create_contract_deploy_tx<S: SignatureKind<D>, D: DB>()
    -> Transaction<S, ProofPreimageMarker, PedersenRandomness, D> {
        let mut rng = StdRng::seed_from_u64(0x42);

        let count_op = ContractOperation::new(verifier_key(&RESOLVER, "count").await);
        let contract = ContractState::new(
            stval!([(0u64), (false), (0u64)]),
            HashMap::new().insert(b"count"[..].into(), count_op),
            Default::default(),
        );

        let deploy = ContractDeploy::new(&mut rng, contract);
        let intent = Intent::empty(&mut rng, Timestamp::from_secs(3600)).add_deploy(deploy);
        let intents = HashMap::new().insert(1, intent);

        Transaction::new("local-test", intents, None, Default::default())
    }

    #[allow(deprecated)]
    pub async fn serialize_prove_tx_body() -> Vec<u8> {
        use ledger::test_utilities::serialize_request_body;
        let tx = create_contract_deploy_tx::<Signature, InMemoryDB>().await;
        serialize_request_body(&tx, &RESOLVER)
            .await
            .expect("Failed to serialize transaction")
    }

    #[allow(deprecated)]
    pub async fn serialize_zswap_body() -> Vec<u8> {
        use ledger::test_utilities::serialize_request_body;
        let tx = create_unbalanced_zswap_tx(1);
        serialize_request_body(&tx, &RESOLVER)
            .await
            .expect("Failed to serialize zswap transaction")
    }

    pub fn create_zswap_output_proof_preimage() -> ProofPreimageVersioned {
        let mut rng = StdRng::seed_from_u64(0x42);
        let sks = zswap::keys::SecretKeys::from_rng_seed(&mut rng);
        let coin = coin::Info::new(&mut rng, 100, Default::default());

        let output = zswap::Output::<_, InMemoryDB>::new(
            &mut rng,
            &coin,
            None,
            &sks.coin_public_key(),
            Some(sks.enc_public_key()),
        )
        .expect("Failed to create output");

        let ppi = (*output.proof).clone();
        ProofPreimageVersioned::V2(Arc::new(ppi))
    }
}

mod health_endpoints {
    use super::common::*;
    use std::env;

    #[tokio::test]
    async fn root_returns_ok_status() {
        let server = start_server(DEFAULT_NUM_WORKERS, DEFAULT_JOB_LIMIT);

        let response = HTTP_CLIENT
            .get(format!("{}/", server.base_url()))
            .send()
            .await
            .expect("Request failed");

        assert_eq!(response.status(), 200);

        let json: serde_json::Value = response.json().await.expect("Failed to parse JSON");
        assert_eq!(json["status"], "ok");

        stop_server(server).await;
    }

    #[tokio::test]
    async fn health_returns_ok_status() {
        let server = start_server(DEFAULT_NUM_WORKERS, DEFAULT_JOB_LIMIT);

        let response = HTTP_CLIENT
            .get(format!("{}/health", server.base_url()))
            .send()
            .await
            .expect("Request failed");

        assert_eq!(response.status(), 200);

        let json: serde_json::Value = response.json().await.expect("Failed to parse JSON");
        assert_eq!(json["status"], "ok");

        stop_server(server).await;
    }

    #[tokio::test]
    async fn version_returns_package_version() {
        let server = start_server(DEFAULT_NUM_WORKERS, DEFAULT_JOB_LIMIT);

        let response = HTTP_CLIENT
            .get(format!("{}/version", server.base_url()))
            .send()
            .await
            .expect("Request failed");

        assert_eq!(response.status(), 200);

        let version = response.text().await.expect("Failed to get response text");
        assert_eq!(version, env!("CARGO_PKG_VERSION"));

        stop_server(server).await;
    }

    #[tokio::test]
    async fn proof_versions_returns_supported_versions() {
        let server = start_server(DEFAULT_NUM_WORKERS, DEFAULT_JOB_LIMIT);

        let response = HTTP_CLIENT
            .get(format!("{}/proof-versions", server.base_url()))
            .send()
            .await
            .expect("Request failed");

        assert_eq!(response.status(), 200);

        let versions = response.text().await.expect("Failed to get response text");
        assert!(versions.contains("V2"), "Expected V2 in proof versions");

        stop_server(server).await;
    }
}

mod prove_tx_endpoint {
    use super::common::*;
    use super::test_data::*;
    use base_crypto::signatures::Signature;
    use ledger::structure::{ProofMarker, Transaction};
    use rand::rngs::StdRng;
    use rand::{Rng, SeedableRng};
    use regex::Regex;
    use serialize::tagged_deserialize;
    use storage::db::InMemoryDB;
    use transient_crypto::commitment::PedersenRandomness;

    #[tokio::test]
    async fn rejects_get_requests() {
        let server = start_server(DEFAULT_NUM_WORKERS, DEFAULT_JOB_LIMIT);

        let response = HTTP_CLIENT
            .get(format!("{}/prove-tx", server.base_url()))
            .send()
            .await
            .expect("Request failed");

        assert_eq!(response.status(), 404);

        stop_server(server).await;
    }

    #[tokio::test]
    async fn rejects_empty_body() {
        let server = start_server(DEFAULT_NUM_WORKERS, DEFAULT_JOB_LIMIT);

        let response = HTTP_CLIENT
            .post(format!("{}/prove-tx", server.base_url()))
            .send()
            .await
            .expect("Request failed");

        assert_eq!(response.status(), 400);

        let error_text = response.text().await.expect("Failed to get response text");
        assert!(error_text.contains("expected header tag"));
        assert!(error_text.contains(", got ''"));

        stop_server(server).await;
    }

    #[tokio::test]
    async fn rejects_json_body() {
        let server = start_server(DEFAULT_NUM_WORKERS, DEFAULT_JOB_LIMIT);

        let response = HTTP_CLIENT
            .post(format!("{}/prove-tx", server.base_url()))
            .header("Content-Type", "application/json")
            .body(r#"{"key": "value"}"#)
            .send()
            .await
            .expect("Request failed");

        assert_eq!(response.status(), 400);

        let error_text = response.text().await.expect("Failed to get response text");
        assert!(error_text.contains("expected header tag"));

        stop_server(server).await;
    }

    #[tokio::test]
    async fn proves_valid_transaction() {
        let server = start_server(DEFAULT_NUM_WORKERS, DEFAULT_JOB_LIMIT);
        let body = serialize_prove_tx_body().await;

        let response = HTTP_CLIENT
            .post(format!("{}/prove-tx", server.base_url()))
            .body(body)
            .send()
            .await
            .expect("Request failed");

        assert_eq!(response.status(), 200);

        let bytes = response
            .bytes()
            .await
            .expect("Failed to get response bytes");
        log::info!("Proving response: {} bytes", bytes.len());

        // Verify we can deserialize the proved transaction
        let _proof: Transaction<Signature, ProofMarker, PedersenRandomness, InMemoryDB> =
            tagged_deserialize(&bytes[..]).expect("Failed to deserialize proof");

        stop_server(server).await;
    }

    #[tokio::test]
    async fn rejects_repeated_body() {
        let server = start_server(DEFAULT_NUM_WORKERS, DEFAULT_JOB_LIMIT);
        let body = serialize_prove_tx_body().await;
        let doubled_body: Vec<u8> = body.iter().chain(body.iter()).copied().collect();

        let response = HTTP_CLIENT
            .post(format!("{}/prove-tx", server.base_url()))
            .body(doubled_body)
            .send()
            .await
            .expect("Request failed");

        assert_eq!(response.status(), 400);

        let error_text = response.text().await.expect("Failed to get response text");
        let pattern = Regex::new(r"^Not all bytes read deserializing '.*'; \d+ bytes remaining$")
            .expect("Invalid regex");
        assert!(
            pattern.is_match(&error_text),
            "Unexpected error message: {}",
            error_text
        );

        stop_server(server).await;
    }

    #[tokio::test]
    async fn rejects_corrupted_body() {
        let server = start_server(DEFAULT_NUM_WORKERS, DEFAULT_JOB_LIMIT);
        let mut body = serialize_prove_tx_body().await;

        let mut rng = StdRng::seed_from_u64(42);
        for _ in 0..50 {
            let idx = rng.gen_range(0..body.len());
            body[idx] = rng.r#gen();
        }

        let response = HTTP_CLIENT
            .post(format!("{}/prove-tx", server.base_url()))
            .body(body)
            .send()
            .await
            .expect("Request failed");

        assert_eq!(response.status(), 400);

        stop_server(server).await;
    }

    #[tokio::test]
    async fn handles_concurrent_requests() {
        let server = start_server(DEFAULT_NUM_WORKERS, 10);
        let body = serialize_prove_tx_body().await;

        let tasks: Vec<_> = (0..10)
            .map(|i| {
                let client = HTTP_CLIENT.clone();
                let url = format!("{}/prove-tx", server.base_url());
                let body = body.clone();

                tokio::spawn(async move {
                    let response = client.post(url).body(body).send().await?;
                    log::info!("Request {} completed with status {}", i, response.status());
                    assert_eq!(response.status(), 200);
                    Ok::<(), reqwest::Error>(())
                })
            })
            .collect();

        let results = futures::future::join_all(tasks).await;

        for result in results {
            result.expect("Task panicked").expect("Request failed");
        }

        stop_server(server).await;
    }

    #[tokio::test]
    async fn health_check_works_under_load() {
        let server = start_server(DEFAULT_NUM_WORKERS, DEFAULT_JOB_LIMIT);
        let body = serialize_zswap_body().await;

        let base_url = server.base_url();
        let _tasks: Vec<_> = (0..50)
            .map(|_| {
                let client = build_client(LONG_REQUEST_TIMEOUT_SECS);
                let url = format!("{}/prove-tx", base_url);
                let body = body.clone();
                tokio::spawn(async move { client.post(url).body(body).send().await })
            })
            .collect();

        tokio::time::sleep(std::time::Duration::from_millis(300)).await;

        let response = HTTP_CLIENT
            .get(format!("{}/health", server.base_url()))
            .send()
            .await
            .expect("Health check should succeed under load");

        assert_eq!(response.status(), 200);

        stop_server(server).await;
    }
}

mod ready_endpoint {
    use super::common::*;
    use super::test_data::*;
    use std::time::Duration;

    #[tokio::test]
    async fn reports_correct_job_counts() {
        let server = start_server(DEFAULT_NUM_WORKERS, DEFAULT_JOB_LIMIT);
        let body = serialize_zswap_body().await;

        let num_requests = DEFAULT_JOB_LIMIT - 1 + DEFAULT_NUM_WORKERS;
        let base_url = server.base_url();

        for _ in 0..num_requests {
            let client = build_client(LONG_REQUEST_TIMEOUT_SECS);
            let url = format!("{}/prove-tx", base_url);
            let body = body.clone();

            tokio::spawn(async move { client.post(url).body(body).send().await });
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        tokio::time::sleep(Duration::from_millis(100)).await;

        let response = HTTP_CLIENT
            .get(format!("{}/ready", server.base_url()))
            .send()
            .await
            .expect("Request failed");

        let status = response.status();
        let json: serde_json::Value = response.json().await.expect("Failed to parse JSON");
        log::info!("Ready response: {:#?}", json);

        assert_eq!(status, 200);
        assert_eq!(json["status"], "ok");
        assert_eq!(json["jobsProcessing"], DEFAULT_NUM_WORKERS);
        assert_eq!(json["jobsPending"], num_requests - DEFAULT_NUM_WORKERS);

        stop_server(server).await;
    }

    #[tokio::test]
    async fn reports_busy_when_overloaded() {
        let server = start_server(DEFAULT_NUM_WORKERS, DEFAULT_JOB_LIMIT);
        let body = serialize_zswap_body().await;

        let num_requests = DEFAULT_JOB_LIMIT + DEFAULT_NUM_WORKERS;
        let base_url = server.base_url();

        for _ in 0..num_requests {
            let client = build_client(LONG_REQUEST_TIMEOUT_SECS);
            let url = format!("{}/prove-tx", base_url);
            let body = body.clone();

            tokio::spawn(async move { client.post(url).body(body).send().await });
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        tokio::time::sleep(Duration::from_millis(100)).await;

        let response = HTTP_CLIENT
            .get(format!("{}/ready", server.base_url()))
            .send()
            .await
            .expect("Request failed");

        assert_eq!(response.status(), 503);

        let json: serde_json::Value = response.json().await.expect("Failed to parse JSON");
        assert_eq!(json["status"], "busy");
        assert_eq!(json["jobsProcessing"], DEFAULT_NUM_WORKERS);
        assert_eq!(json["jobsPending"], num_requests - DEFAULT_NUM_WORKERS);

        stop_server(server).await;
    }

    #[tokio::test]
    async fn zero_capacity_allows_unlimited_jobs() {
        let server = start_server(DEFAULT_NUM_WORKERS, 0);
        let body = serialize_prove_tx_body().await;

        let base_url = server.base_url();
        let mut tasks = Vec::new();

        for _ in 0..10 {
            let client = HTTP_CLIENT.clone();
            let url = format!("{}/prove-tx", base_url);
            let body = body.clone();

            let task = tokio::spawn(async move {
                let response = client.post(url).body(body).send().await?;
                assert_eq!(response.status(), 200);
                Ok::<(), reqwest::Error>(())
            });
            tasks.push(task);
        }

        let response = HTTP_CLIENT
            .get(format!("{}/ready", server.base_url()))
            .send()
            .await
            .expect("Request failed");

        assert_eq!(response.status(), 200);

        let json: serde_json::Value = response.json().await.expect("Failed to parse JSON");
        assert_eq!(json["status"], "ok");
        assert_eq!(json["jobCapacity"], 0);

        for task in tasks {
            task.await.expect("Task panicked").expect("Request failed");
        }

        stop_server(server).await;
    }
}

mod k_endpoint {
    use super::common::*;
    use serialize::tagged_serialize;
    use transient_crypto::proofs::Zkir;
    use zkir as zkir_v2;

    fn create_minimal_ir_source() -> zkir_v2::IrSource {
        zkir_v2::IrSource {
            num_inputs: 1,
            do_communications_commitment: false,
            instructions: std::sync::Arc::new(vec![]),
        }
    }

    #[tokio::test]
    async fn returns_k_value_for_valid_ir() {
        let server = start_server(DEFAULT_NUM_WORKERS, DEFAULT_JOB_LIMIT);

        let ir = create_minimal_ir_source();
        let expected_k = ir.k();

        let mut body = Vec::new();
        tagged_serialize(&ir, &mut body).expect("Failed to serialize IR");

        let response = HTTP_CLIENT
            .post(format!("{}/k", server.base_url()))
            .body(body)
            .send()
            .await
            .expect("Request failed");

        assert_eq!(response.status(), 200);

        let k_str = response.text().await.expect("Failed to get response text");
        let k: u8 = k_str.parse().expect("Failed to parse k value");
        assert_eq!(k, expected_k, "K value mismatch");

        stop_server(server).await;
    }

    #[tokio::test]
    async fn rejects_empty_body() {
        let server = start_server(DEFAULT_NUM_WORKERS, DEFAULT_JOB_LIMIT);

        let response = HTTP_CLIENT
            .post(format!("{}/k", server.base_url()))
            .send()
            .await
            .expect("Request failed");

        assert_eq!(response.status(), 400);

        stop_server(server).await;
    }

    #[tokio::test]
    async fn rejects_invalid_ir_format() {
        let server = start_server(DEFAULT_NUM_WORKERS, DEFAULT_JOB_LIMIT);

        let response = HTTP_CLIENT
            .post(format!("{}/k", server.base_url()))
            .body(vec![0u8; 100])
            .send()
            .await
            .expect("Request failed");

        assert_eq!(response.status(), 400);

        let error_text = response.text().await.expect("Failed to get response text");
        assert!(
            error_text.contains("Unsupported ZKIR version")
                || error_text.contains("expected header tag"),
            "Unexpected error: {}",
            error_text
        );

        stop_server(server).await;
    }

    #[tokio::test]
    async fn rejects_get_requests() {
        let server = start_server(DEFAULT_NUM_WORKERS, DEFAULT_JOB_LIMIT);

        let response = HTTP_CLIENT
            .get(format!("{}/k", server.base_url()))
            .send()
            .await
            .expect("Request failed");

        assert_eq!(response.status(), 404);

        stop_server(server).await;
    }
}

mod check_endpoint {
    use super::common::*;
    use super::test_data::create_zswap_output_proof_preimage;
    use serialize::tagged_serialize;
    use transient_crypto::proofs::WrappedIr;

    #[tokio::test]
    async fn rejects_empty_body() {
        let server = start_server(DEFAULT_NUM_WORKERS, DEFAULT_JOB_LIMIT);

        let response = HTTP_CLIENT
            .post(format!("{}/check", server.base_url()))
            .send()
            .await
            .expect("Request failed");

        assert_eq!(response.status(), 400);

        stop_server(server).await;
    }

    #[tokio::test]
    async fn rejects_invalid_format() {
        let server = start_server(DEFAULT_NUM_WORKERS, DEFAULT_JOB_LIMIT);

        let response = HTTP_CLIENT
            .post(format!("{}/check", server.base_url()))
            .body(vec![0u8; 100])
            .send()
            .await
            .expect("Request failed");

        assert_eq!(response.status(), 400);

        stop_server(server).await;
    }

    #[tokio::test]
    async fn rejects_get_requests() {
        let server = start_server(DEFAULT_NUM_WORKERS, DEFAULT_JOB_LIMIT);

        let response = HTTP_CLIENT
            .get(format!("{}/check", server.base_url()))
            .send()
            .await
            .expect("Request failed");

        assert_eq!(response.status(), 404);

        stop_server(server).await;
    }

    #[tokio::test]
    async fn processes_valid_request() {
        let server = start_server(DEFAULT_NUM_WORKERS, DEFAULT_JOB_LIMIT);

        let versioned_ppi = create_zswap_output_proof_preimage();
        let ir: Option<WrappedIr> = None;

        let mut body = Vec::new();
        tagged_serialize(&(versioned_ppi, ir), &mut body)
            .expect("Failed to serialize check request");

        let client = build_client(LONG_REQUEST_TIMEOUT_SECS);
        let response = client
            .post(format!("{}/check", server.base_url()))
            .body(body)
            .send()
            .await
            .expect("Request failed");

        assert_eq!(
            response.status(),
            200,
            "Unexpected status: {}",
            response.status()
        );

        stop_server(server).await;
    }
}

mod prove_endpoint {
    use super::common::*;
    use super::test_data::create_zswap_output_proof_preimage;
    use serialize::{tagged_deserialize, tagged_serialize};
    use transient_crypto::proofs::ProvingKeyMaterial;

    #[tokio::test]
    async fn rejects_empty_body() {
        let server = start_server(DEFAULT_NUM_WORKERS, DEFAULT_JOB_LIMIT);

        let response = HTTP_CLIENT
            .post(format!("{}/prove", server.base_url()))
            .send()
            .await
            .expect("Request failed");

        assert_eq!(response.status(), 400);

        stop_server(server).await;
    }

    #[tokio::test]
    async fn rejects_invalid_format() {
        let server = start_server(DEFAULT_NUM_WORKERS, DEFAULT_JOB_LIMIT);

        let response = HTTP_CLIENT
            .post(format!("{}/prove", server.base_url()))
            .body(vec![0u8; 100])
            .send()
            .await
            .expect("Request failed");

        assert_eq!(response.status(), 400);

        stop_server(server).await;
    }

    #[tokio::test]
    async fn rejects_get_requests() {
        let server = start_server(DEFAULT_NUM_WORKERS, DEFAULT_JOB_LIMIT);

        let response = HTTP_CLIENT
            .get(format!("{}/prove", server.base_url()))
            .send()
            .await
            .expect("Request failed");

        assert_eq!(response.status(), 404);

        stop_server(server).await;
    }

    #[tokio::test]
    async fn processes_valid_request() {
        let server = start_server(DEFAULT_NUM_WORKERS, DEFAULT_JOB_LIMIT);

        let versioned_ppi = create_zswap_output_proof_preimage();
        let data: Option<ProvingKeyMaterial> = None;
        let binding_input: Option<transient_crypto::curve::Fr> = None;

        let mut body = Vec::new();
        tagged_serialize(&(versioned_ppi, data, binding_input), &mut body)
            .expect("Failed to serialize prove request");

        let client = build_client(LONG_REQUEST_TIMEOUT_SECS);
        let response = client
            .post(format!("{}/prove", server.base_url()))
            .body(body)
            .send()
            .await
            .expect("Request failed");

        assert_eq!(
            response.status(),
            200,
            "Unexpected status: {}",
            response.status()
        );

        let bytes = response
            .bytes()
            .await
            .expect("Failed to get response bytes");
        log::info!("Prove response: {} bytes", bytes.len());

        let _proof: ledger::structure::ProofVersioned =
            tagged_deserialize(&bytes[..]).expect("Failed to deserialize proof");

        stop_server(server).await;
    }
}

mod fetch_params_endpoint {
    use super::common::*;

    #[tokio::test]
    async fn not_available_by_default() {
        let server = start_server(DEFAULT_NUM_WORKERS, DEFAULT_JOB_LIMIT);

        let response = HTTP_CLIENT
            .get(format!("{}/fetch-params/8", server.base_url()))
            .send()
            .await
            .expect("Request failed");

        assert_eq!(response.status(), 404);

        stop_server(server).await;
    }

    #[tokio::test]
    async fn rejects_invalid_k() {
        let server = start_server_with_fetch_params(DEFAULT_NUM_WORKERS, DEFAULT_JOB_LIMIT, true);

        let response = HTTP_CLIENT
            .get(format!("{}/fetch-params/30", server.base_url()))
            .send()
            .await
            .expect("Request failed");

        assert_eq!(response.status(), 400);

        let error_text = response.text().await.expect("Failed to get response text");
        assert!(
            error_text.contains("out of range"),
            "Expected 'out of range' error, got: {}",
            error_text
        );

        stop_server(server).await;
    }

    #[tokio::test]
    async fn accepts_valid_k() {
        let server = start_server_with_fetch_params(DEFAULT_NUM_WORKERS, DEFAULT_JOB_LIMIT, true);

        let client = build_client(60);
        let response = client
            .get(format!("{}/fetch-params/8", server.base_url()))
            .send()
            .await
            .expect("Request failed");

        assert_eq!(response.status(), 200);

        let text = response.text().await.expect("Failed to get response text");
        assert_eq!(text, "success");

        stop_server(server).await;
    }
}
