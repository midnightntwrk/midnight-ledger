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

use base_crypto::{self, time::Timestamp};
use ledger::structure::{Intent, LedgerState, SignatureKind, ContractDeploy, ProofMarker, ProofPreimageMarker, Transaction};
use ledger::verify::WellFormedStrictness;
use midnight_proof_server::worker_pool::WorkerPool;
use std::borrow::Cow;
use std::collections::HashSet;
use std::env;
use std::sync::Once;
use std::sync::mpsc::Receiver;
use std::time::Duration;
#[cfg(test)]
use std::{sync::mpsc, thread};
use storage::arena::Sp;
use transient_crypto::commitment::PedersenRandomness;
use transient_crypto::proofs::{KeyLocation, ProvingKeyMaterial, Resolver as _};
use zswap::Delta;

use actix_web::{dev::ServerHandle, rt};
use base_crypto::signatures::Signature;
use coin_structure::coin;
use function_name::named;
use lazy_static::lazy_static;
use onchain_runtime::state::{ContractOperation, ContractState, StateValue, stval};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use reqwest::Client;
use serialize::{Tagged, tagged_deserialize, tagged_serialize};
use storage::db::{DB, InMemoryDB};
use storage::storage::HashMap;

use ledger::test_utilities::{ProofServerProvider, Resolver, test_resolver, verifier_key};
use midnight_proof_server::server;
use onchain_runtime::cost_model::INITIAL_COST_MODEL;
use regex::Regex;

const NUM_WORKERS: usize = 2;
const LIMIT: usize = 2;
static mut SERVER_PORT: u16 = 0;
static INIT: Once = Once::new();

lazy_static! {
    static ref HTTP_CLIENT: Client = Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    static ref RESOLVER: Resolver = test_resolver("fallible");
}

fn build_client(timeout: u64) -> Client {
    Client::builder()
        .timeout(Duration::from_secs(timeout))
        .build()
        .unwrap()
}

pub fn setup_logger() -> () {
    INIT.call_once(|| {
        env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));
    });
}

fn get_host_and_port() -> String {
    #[allow(static_mut_refs)]
    unsafe {
        format!("http://127.0.0.1:{}", SERVER_PORT)
    }
}

async fn run_app(tx: mpsc::Sender<ServerHandle>, limit: usize) -> std::io::Result<()> {
    log::info!("Starting HTTP server at {}", get_host_and_port());
    let pool = WorkerPool::new(NUM_WORKERS, limit, 600.0);
    let (server, bound_port) = server(0, false, pool).unwrap();
    unsafe {
        SERVER_PORT = bound_port;
    }
    log::info!("Started HTTP server at {}", get_host_and_port());
    let _ = tx.send(server.handle());
    server.await
}

fn setup(limit: usize) -> Receiver<ServerHandle> {
    setup_logger();
    let (tx, rx) = mpsc::channel();

    log::info!("spawning thread for server");
    thread::spawn(move || {
        let server_future = run_app(tx, limit);
        rt::System::new().block_on(server_future)
    });
    rx
}

async fn stop_server(server_handle: ServerHandle) {
    log::info!("stopping server");
    server_handle.stop(false).await;
}

// Builds a request body that contains only the tagged transaction preimage
// (no ZK Config section).
async fn serialized_body_without_zk_config() -> Vec<u8> {
    let tx = valid_tx::<Signature, InMemoryDB>().await;
    let mut body = Vec::new();
    tagged_serialize(&tx, &mut body).expect("transaction-only payload should serialize");
    body
}

// Builds an invalid request body with two different ZK Configs back-to-back:
// `[(tx, zkA)] [zkB]`.
async fn serialized_body_with_double_zk_config() -> Vec<u8> {
    let mut payload = serialized_valid_body().await;
    let zswap_tx = valid_unbalanced_zswap(1);
    let mut zswap_tx_bytes = Vec::new();
    tagged_serialize(&zswap_tx, &mut zswap_tx_bytes)
        .expect("transaction-only payload should serialize");

    let mut zswap_payload = serialized_valid_zswap_body().await;
    let zswap_config = zswap_payload.split_off(zswap_tx_bytes.len());

    payload.extend_from_slice(&zswap_config);
    payload
}

// Builds an invalid body by swapping the values of two ZK Config entries.
// Result: {A -> zkB, B -> zkA} (keys unchanged). Server should reject with 400.
async fn serialized_body_with_swapped_zk_config_values() -> Option<Vec<u8>> {
    let valid_body = serialized_valid_body().await;
    let (tx, keys): (
        Transaction<Signature, ProofPreimageMarker, PedersenRandomness, InMemoryDB>,
        std::collections::HashMap<
            transient_crypto::proofs::KeyLocation,
            transient_crypto::proofs::ProvingKeyMaterial,
        >,
    ) = tagged_deserialize(&valid_body[..]).ok()?;

    let mut entries: Vec<_> = keys.into_iter().collect();
    if entries.len() < 2 {
        let (_, extra_keys): (
            Transaction<Signature, ProofPreimageMarker, PedersenRandomness, InMemoryDB>,
            std::collections::HashMap<
                transient_crypto::proofs::KeyLocation,
                transient_crypto::proofs::ProvingKeyMaterial,
            >,
        ) = tagged_deserialize(&serialized_valid_zswap_body().await[..]).ok()?;
        let mut existing_keys: HashSet<_> = entries.iter().map(|(key, _)| key.clone()).collect();
        entries.extend(
            extra_keys
                .into_iter()
                .filter(|(key, _)| existing_keys.insert(key.clone())),
        );
    }

    if entries.len() < 2 {
        return None;
    }

    let (k1, v1) = entries[0].clone();
    let (k2, v2) = entries[1].clone();
    entries[0] = (k1, v2);
    entries[1] = (k2, v1);

    let swapped_keys = entries
    .into_iter()
    .collect::<std::collections::HashMap<_, _>>();
    let mut payload = Vec::new();
    tagged_serialize(&(tx, swapped_keys), &mut payload).expect("swapped payload should serialize");
    Some(payload)
}

// Builds an invalid body by changing one of the ZK Config keys to reference a
// circuit ID that is not used in the transaction.
async fn serialized_body_with_wrong_circuit_id() -> Option<Vec<u8>> {
    let valid_body = serialized_valid_body().await;
    let (tx, keys): (
        Transaction<Signature, ProofPreimageMarker, PedersenRandomness, InMemoryDB>,
        std::collections::HashMap<
            transient_crypto::proofs::KeyLocation,
            transient_crypto::proofs::ProvingKeyMaterial,
        >,
    ) = tagged_deserialize(&valid_body[..]).ok()?;

    let mut entries: Vec<_> = keys.into_iter().collect();
    if entries.is_empty() {
        let (_, fallback_keys): (
            Transaction<Signature, ProofPreimageMarker, PedersenRandomness, InMemoryDB>,
            std::collections::HashMap<
                transient_crypto::proofs::KeyLocation,
                transient_crypto::proofs::ProvingKeyMaterial,
            >,
        ) = tagged_deserialize(&serialized_valid_zswap_body().await[..]).ok()?;
        entries = fallback_keys.into_iter().collect();
    }

    let (KeyLocation(original_id), value) = entries.pop()?;
    let wrong_key = KeyLocation(Cow::Owned(format!("{original_id}_wrong")));
    entries.push((wrong_key, value));

    let mutated_keys = entries
        .into_iter()
        .collect::<std::collections::HashMap<_, _>>();

    let mut payload = Vec::new();
    tagged_serialize(&(tx, mutated_keys), &mut payload).ok()?;
    Some(payload)
}

// Builds the **canonical /prove request body** for a normal valid transaction.
async fn serialized_valid_body() -> Vec<u8> {
    let tx = valid_tx::<Signature, InMemoryDB>().await;
    serialize_transaction_payload(&tx, &RESOLVER).await
}

// Builds the **canonical /prove request body** for the smallest transaction we use in tests
// (a minimal zswap variant).
async fn serialized_valid_zswap_body() -> Vec<u8> {
    let tx = valid_unbalanced_zswap(1);
    serialize_transaction_payload(&tx, &RESOLVER).await
}

// Serializes a given **unproven transaction (preimage)** plus resolver-provided proving keys
// into the exact tagged payload that `/prove` expects.
async fn serialize_transaction_payload<S, D>(
    tx: &Transaction<S, ProofPreimageMarker, PedersenRandomness, D>,
    resolver: &Resolver,
) -> Vec<u8>
where
    S: SignatureKind<D> + Tagged,
    D: DB,
{
    let circuits_used = tx
        .calls()
        .into_iter()
        .map(|(_, c)| String::from_utf8_lossy(&c.entry_point).into_owned())
        .collect::<Vec<_>>();

    let mut keys: std::collections::HashMap<KeyLocation, ProvingKeyMaterial> =
        std::collections::HashMap::new();
    for circuit in circuits_used {
        let location = KeyLocation(Cow::Owned(circuit));
        if let Some(material) = resolver
            .resolve_key(location.clone())
            .await
            .expect("resolver should resolve keys")
        {
            keys.insert(location, material);
        }
    }

    let mut payload = Vec::new();
    tagged_serialize(&(tx, keys), &mut payload).expect("transaction payload should serialize");
    payload
}

fn valid_unbalanced_zswap(
    num_outputs: usize,
) -> Transaction<Signature, ProofPreimageMarker, PedersenRandomness, InMemoryDB> {
    let mut rng = StdRng::seed_from_u64(0x42);
    let mut outputs = storage::storage::Array::new();
    let claim_amount = 100;

    for _i in 0..num_outputs {
        let coin = coin::Info::new(&mut rng, claim_amount, Default::default());
        let sks = zswap::keys::SecretKeys::from_rng_seed(&mut rng);
        let out = zswap::Output::new::<_>(
            &mut rng,
            &coin,
            0,
            &sks.coin_public_key(),
            Some(sks.enc_public_key()),
        )
        .unwrap();
        outputs = outputs.push(out);
    }

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

async fn valid_tx<S: SignatureKind<D>, D: DB>()
-> Transaction<S, ProofPreimageMarker, PedersenRandomness, D> {
    let mut rng = StdRng::seed_from_u64(0x42);

    let count_op = ContractOperation::new(verifier_key(&RESOLVER, "count").await);
    let contract = ContractState::new(
        stval!([(0u64), (false), (0u64)]),
        HashMap::new().insert(b"count"[..].into(), count_op.clone()),
        Default::default(),
    );

    let deploy = ContractDeploy::new(&mut rng, contract.clone());

    let mut intents =
        HashMap::<u16, Intent<S, ProofPreimageMarker, PedersenRandomness, D>, D>::new();
    let intent = Intent::empty(&mut rng, Timestamp::from_secs(3600)).add_deploy(deploy);
    intents = intents.insert(1, intent);

    let tx: Transaction<S, ProofPreimageMarker, PedersenRandomness, D> =
        Transaction::new("local-test", intents, None, Default::default());
    tx
}

// Builds a large valid *unproven* transaction (preimage form) for stress-testing proving.
async fn large_valid_tx<S: SignatureKind<D>, D: DB>(
    num_intents: usize,
) -> Transaction<S, ProofPreimageMarker, PedersenRandomness, D> {
    let mut rng = StdRng::seed_from_u64(0x4242);

    let count_op = ContractOperation::new(verifier_key(&RESOLVER, "count").await);
    let contract = ContractState::new(
        stval!([(0u64), (false), (0u64)]),
        HashMap::new().insert(b"count"[..].into(), count_op.clone()),
        Default::default(),
    );

    let mut intents =
        HashMap::<u16, Intent<S, ProofPreimageMarker, PedersenRandomness, D>, D>::new();

    for i in 0..num_intents {
        let deploy = ContractDeploy::new(&mut rng, contract.clone());
        let intent = Intent::empty(&mut rng, Timestamp::from_secs(3600)).add_deploy(deploy);
        intents = intents.insert(i as u16 + 1, intent);
    }

    Transaction::new("local-test", intents, None, Default::default())
}

fn setup_test(name: &str) {
    log::info!("Running test: {}", name);
}

#[tokio::test]
async fn integration_tests() {
    let _server_handle = setup(LIMIT).recv().unwrap();

    test_root_should_return_status().await;
    test_health_should_return_status().await;
    test_proof_versions_should_return_status().await;
    test_version_should_return_current_version().await;
    test_prove_tx_should_fail_on_get().await;
    test_prove_tx_should_fail_on_empty_body().await;
    test_prove_tx_should_fail_on_json().await;
    test_prove_should_fail_without_zk_config().await;
    test_prove_should_fail_with_double_zk_config().await;
    test_prove_should_fail_with_swapped_zk_config_values().await;
    test_prove_should_fail_with_wrong_circuit_id().await;
    test_prove_should_generate_valid_proof_for_smallest_transaction().await;
    test_prove_should_generate_valid_proof_for_big_transaction().await;
    test_prove_tx_should_prove_correct_tx().await;
    test_prove_tx_should_fail_on_repeated_body().await;
    test_prove_tx_should_fail_on_corrupted_body().await;
    test_health_check_still_works_when_server_is_fully_loaded().await;

    stop_server(_server_handle).await;

    let _server_handle = setup(LIMIT).recv().unwrap();
    test_ready_reports_correct_job_numbers().await;
    stop_server(_server_handle).await;

    let _server_handle = setup(LIMIT).recv().unwrap();
    test_ready_reports_busy().await;
    stop_server(_server_handle).await;

    let _server_handle = setup(10).recv().unwrap();
    test_prove_tx_should_be_able_to_validate_multiple_txs().await;
    stop_server(_server_handle).await;

    let _server_handle = setup(0).recv().unwrap();
    test_prove_tx_should_be_able_to_validate_multiple_txs_with_zero_limit().await;
    stop_server(_server_handle).await;
}

#[named]
async fn test_root_should_return_status() {
    setup_test(function_name!());

    let response = HTTP_CLIENT
        .get(format!("{}/", get_host_and_port()))
        .send()
        .await
        .unwrap();

    log::info!("Response code: {:?}", response.status());
    assert_eq!(response.status(), 200);
    let json_resp = response.json::<serde_json::Value>().await.unwrap();
    assert_eq!(json_resp.get("status").unwrap().as_str().unwrap(), "ok");
}

#[named]
async fn test_health_should_return_status() {
    setup_test(function_name!());

    let response = HTTP_CLIENT
        .get(format!("{}/health", get_host_and_port()))
        .send()
        .await
        .unwrap();

    log::info!("Response code: {:?}", response.status());
    assert_eq!(response.status(), 200);
    let json_resp = response.json::<serde_json::Value>().await.unwrap();
    assert_eq!(json_resp.get("status").unwrap().as_str().unwrap(), "ok");
}

#[named]
async fn test_proof_versions_should_return_status() {
    setup_test(function_name!());

    let response = HTTP_CLIENT
        .get(format!("{}/proof-versions", get_host_and_port()))
        .send()
        .await
        .unwrap();

    log::info!("Response code: {:?}", response.status());
    assert_eq!(response.status(), 200);
    assert_eq!(response.text().await.unwrap(), "[\"V1\"]");
}

#[named]
async fn test_version_should_return_current_version() {
    setup_test(function_name!());

    let response = HTTP_CLIENT
        .get(format!("{}/version", get_host_and_port()))
        .send()
        .await
        .unwrap();

    log::info!("Response code: {:?}", response.status());
    assert_eq!(response.status(), 200);
    assert_eq!(response.text().await.unwrap(), env!("CARGO_PKG_VERSION"));
}

#[named]
async fn test_prove_tx_should_fail_on_get() {
    setup_test(function_name!());

    let response = HTTP_CLIENT
        .get(format!("{}/prove-tx", get_host_and_port()))
        .send()
        .await
        .unwrap();

    log::info!("Response code: {:?}", response.status());
    assert_eq!(response.status(), 404);
}

#[named]
async fn test_prove_tx_should_fail_on_empty_body() {
    setup_test(function_name!());

    let response = HTTP_CLIENT
        .post(format!("{}/prove-tx", get_host_and_port()))
        .send()
        .await
        .unwrap();

    log::info!("Response code: {:?}", response.status());
    assert_eq!(response.status(), 400);
    let resp_text = response.text().await.unwrap();
    assert!(resp_text.contains("expected header tag"));
    assert!(resp_text.contains(", got ''"));
}

#[named]
async fn test_prove_tx_should_fail_on_json() {
    setup_test(function_name!());

    let response = HTTP_CLIENT
        .post(format!("{}/prove-tx", get_host_and_port()))
        .header("Content-Type", "application/json")
        .body(r#"{"key": "value"}"#)
        .send()
        .await
        .unwrap();

    log::info!("Response code: {:?}", response.status());
    assert_eq!(response.status(), 400);
    let resp_text = dbg!(response.text().await.unwrap());
    assert!(resp_text.contains("expected header tag"));
}

// Negative test: `/prove` must reject a payload without ZK Config.
// Given: A request body that contains only the tagged transaction preimage.
// When: POSTed to `/prove`.
// Then: The server responds with `400 Bad Request`.
#[named]
async fn test_prove_should_fail_without_zk_config() {
    setup_test(function_name!());

    let response = HTTP_CLIENT
        .post(format!("{}/prove", get_host_and_port()))
        .body(serialized_body_without_zk_config().await)
        .send()
        .await
        .unwrap();

    log::info!("Response code: {:?}", response.status());
    assert_eq!(response.status(), 400);
    let resp_text = response.text().await.unwrap();
    assert!(resp_text.contains("expected header tag"));
}

// Negative test: `/prove` must reject payloads that include two different
// ZK Config blocks one after another (double ZK Config).
// Given:
// - A valid request `[(tx, zkA)]`.
// - A second, different ZK Config `zkB` (sourced from a different valid body).
// When: We append `zkB` after the first tuple and POST to `/prove`.
// Then: The server responds with `400 Bad Request`.
#[named]
async fn test_prove_should_fail_with_double_zk_config() {
    setup_test(function_name!());

    let response = HTTP_CLIENT
        .post(format!("{}/prove", get_host_and_port()))
        .body(serialized_body_with_double_zk_config().await)
        .send()
        .await
        .unwrap();

    log::info!("Response code: {:?}", response.status());
    assert_eq!(response.status(), 400);
    let resp_text = response.text().await.unwrap();
    assert!(resp_text.contains("expected header tag"));
}

// Negative test: `/prove` must reject payloads where ZK Config values are value-swapped.
// Given:
// - A valid request `(tx, {A -> zkA, B -> zkB, ...})`.
// - We swap the proving material so that `{A -> zkB, B -> zkA}`.
// When: The malformed payload is POSTed to `/prove`.
// Then: The server responds with `400 Bad Request`.
#[named]
async fn test_prove_should_fail_with_swapped_zk_config_values() {
    setup_test(function_name!());

    if let Some(payload) = serialized_body_with_swapped_zk_config_values().await {
        let response = HTTP_CLIENT
            .post(format!("{}/prove", get_host_and_port()))
            .body(payload)
            .send()
            .await
            .unwrap();

        log::info!("Response code: {:?}", response.status());
        assert_eq!(response.status(), 400);
        let resp_text = response.text().await.unwrap();
        assert!(resp_text.contains("failed to find proving key"));
    }
}

// Negative test: `/prove` must reject payloads whose ZK Config contains an
// entry keyed by a circuit ID that is not part of the transaction.
// Given: A valid request `(tx, {A -> zkA, ...})`.
// When: We replace one key with `C_wrong` while keeping the proving material.
// Then: The server responds with `400 Bad Request`.
#[named]
async fn test_prove_should_fail_with_wrong_circuit_id() {
    setup_test(function_name!());

    if let Some(payload) = serialized_body_with_wrong_circuit_id().await {
        let response = HTTP_CLIENT
            .post(format!("{}/prove", get_host_and_port()))
            .body(payload)
            .send()
            .await
            .unwrap();

        log::info!("Response code: {:?}", response.status());
        assert_eq!(response.status(), 400);
        let resp_text = response.text().await.unwrap();
        assert!(resp_text.contains("failed to find proving key"));
    }
}

// Scenario: Generate valid proof – smallest size transaction
// Given: Minimal transaction on "local-test" (no inputs/outputs; default fields)
// When: It is proved via `Transaction::prove` using a `ProofServerProvider` (new `/prove` flow)
// Then: We obtain a proven transaction that:
//   - round-trips through tagged (de)serialization, and
//   - is well-formed
#[named]
async fn test_prove_should_generate_valid_proof_for_smallest_transaction() {
    setup_test(function_name!());

    let tx: Transaction<Signature, ProofPreimageMarker, PedersenRandomness, InMemoryDB> =
        Transaction::new("local-test", Default::default(), None, Default::default());

    let provider = ProofServerProvider {
        base_url: get_host_and_port(),
        resolver: &RESOLVER,
    };

    let proven = tx
        .prove(provider, &INITIAL_COST_MODEL)
        .await
        .expect("proving smallest transaction should succeed");

    let mut bytes = Vec::new();
    tagged_serialize(&proven, &mut bytes)
        .expect("proven transaction should serialize successfully");
    let round_trip: Transaction<Signature, ProofMarker, PedersenRandomness, InMemoryDB> =
        tagged_deserialize(&bytes[..]).expect("proven transaction should deserialize successfully");

    let mut strictness = WellFormedStrictness::default();
    strictness.enforce_balancing = false;
    round_trip
        .well_formed(
            &LedgerState::new("local-test"),
            strictness,
            Timestamp::from_secs(0),
        )
        .expect("proven smallest transaction should be well formed");
}

// Scenario: Generate valid proof – big transaction
// Given: Large contract deployment transaction on "local-test" with many intents
// When: It is proved via `Transaction::prove` using a `ProofServerProvider`
// Then: We obtain a proven transaction that can be serialized, deserialized, and validated
#[named]
async fn test_prove_should_generate_valid_proof_for_big_transaction() {
    setup_test(function_name!());

    const NUM_INTENTS: usize = 32;
    let tx: Transaction<Signature, ProofPreimageMarker, PedersenRandomness, InMemoryDB> =
        large_valid_tx(NUM_INTENTS).await;

    let provider = ProofServerProvider {
        base_url: get_host_and_port(),
        resolver: &RESOLVER,
    };

    let proven = tx
        .prove(provider, &INITIAL_COST_MODEL)
        .await
        .expect("proving big transaction should succeed");

    let mut bytes = Vec::new();
    tagged_serialize(&proven, &mut bytes)
        .expect("proven transaction should serialize successfully");
    let round_trip: Transaction<Signature, ProofMarker, PedersenRandomness, InMemoryDB> =
        tagged_deserialize(&bytes[..]).expect("proven transaction should deserialize successfully");

    let mut strictness = WellFormedStrictness::default();
    strictness.enforce_balancing = false;
    round_trip
        .well_formed(
            &LedgerState::new("local-test"),
            strictness,
            Timestamp::from_secs(0),
        )
        .expect("proven big transaction should be well formed");
}

#[named]
async fn test_prove_tx_should_prove_correct_tx() {
    setup_test(function_name!());

    let response = HTTP_CLIENT
        .post(format!("{}/prove-tx", get_host_and_port()))
        .body(serialized_valid_body().await)
        .send()
        .await
        .unwrap();

    log::info!("Response code: {:?}", response.status());
    assert_eq!(response.status(), 200);
    let bytes = dbg!(response.bytes().await.unwrap());
    log::info!("Proving response: {} bytes", bytes.len());
    let proof: Transaction<Signature, ProofMarker, PedersenRandomness, InMemoryDB> =
        tagged_deserialize(&bytes[..]).unwrap();
    let mut strictness = WellFormedStrictness::default();
    strictness.enforce_balancing = false;
    proof
        .well_formed(
            &LedgerState::new("local-test"),
            strictness,
            Timestamp::from_secs(0),
        )
        .unwrap();
}

#[named]
async fn test_prove_tx_should_be_able_to_validate_multiple_txs() {
    setup_test(function_name!());
    let mut handles = Vec::new();

    for i in 0..10 {
        let client = HTTP_CLIENT.clone();
        let handle = tokio::spawn(async move {
            let body = serialized_valid_body().await;
            let response = client
                .post(format!("{}/prove-tx", get_host_and_port()))
                .body(body)
                .send()
                .await?;
            log::info!("Iteration: {:?}, Response code: {:?}", i, response.status());
            assert_eq!(response.status(), 200);
            Ok::<(), reqwest::Error>(())
        });
        handles.push(handle);
    }

    let results = futures::future::join_all(handles).await;

    for result in results {
        result
            .expect("Request should not fail")
            .expect("Request should not panic");
    }
}

#[named]
async fn test_prove_tx_should_fail_on_repeated_body() {
    setup_test(function_name!());

    let response = HTTP_CLIENT
        .post(format!("{}/prove-tx", get_host_and_port()))
        .body(
            serialized_valid_body()
                .await
                .repeat(2)
                .into_iter()
                .collect::<Vec<u8>>(),
        )
        .send()
        .await
        .unwrap();

    log::info!("Response code: {:?}", response.status());
    assert_eq!(response.status(), 400);
    assert!(
        Regex::new(r"^Not all bytes read deserializing '.*'; \d+ bytes remaining$")
            .unwrap()
            .is_match(dbg!(response.text().await.unwrap().as_str()))
    );
}

#[named]
async fn test_prove_tx_should_fail_on_corrupted_body() {
    setup_test(function_name!());
    let mut body = serialized_valid_body().await;
    let mut rng = rand::rngs::StdRng::seed_from_u64(42);
    // make 50 random edits
    for _ in 0..50 {
        let len = body.len();
        body[rng.gen_range(0..len)] = rng.r#gen();
    }

    let response = HTTP_CLIENT
        .post(format!("{}/prove-tx", get_host_and_port()))
        .body(body)
        .send()
        .await
        .unwrap();

    let stat = response.status();
    dbg!(response.bytes().await.unwrap());
    log::info!("Response code: {:?}", stat);
    assert_eq!(stat, 400);
}

#[named]
async fn test_prove_tx_should_be_able_to_validate_multiple_txs_with_zero_limit() {
    setup_test(function_name!());
    let mut handles = Vec::new();

    for i in 0..10 {
        let client = HTTP_CLIENT.clone();
        let handle = tokio::spawn(async move {
            let response = client
                .post(format!("{}/prove-tx", get_host_and_port()))
                .body(serialized_valid_body().await)
                .send()
                .await?;
            log::info!("Iteration: {:?}, Response code: {:?}", i, response.status());
            assert_eq!(response.status(), 200);
            Ok::<(), reqwest::Error>(())
        });
        handles.push(handle);
    }

    let resp = HTTP_CLIENT
        .get(format!("{}/ready", get_host_and_port()))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let json_resp = resp.json::<serde_json::Value>().await.unwrap();
    assert_eq!(json_resp.get("status").unwrap().as_str().unwrap(), "ok");
    assert_eq!(json_resp.get("jobCapacity").unwrap().as_u64().unwrap(), 0);

    let results = futures::future::join_all(handles).await;

    for result in results {
        result
            .expect("Request should not fail")
            .expect("Request should not panic");
    }
}

#[named]
async fn test_ready_reports_busy() {
    setup_test(function_name!());
    let body = serialized_valid_zswap_body().await;
    let mut tasks = vec![];
    let num_reqs = LIMIT + NUM_WORKERS;
    for _ in 0..num_reqs {
        let fut = build_client(30)
            .post(format!("{}/prove-tx", get_host_and_port()))
            .body(body.clone())
            .send();
        let task = tokio::spawn(fut);
        tasks.push(task);

        // Wait for job to get picked up
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    // Wait for requests to send
    tokio::time::sleep(Duration::from_millis(100)).await;

    let resp = HTTP_CLIENT
        .get(format!("{}/ready", get_host_and_port()))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 503);
    let json_resp = resp.json::<serde_json::Value>().await.unwrap();
    assert_eq!(
        json_resp.get("jobsProcessing").unwrap().as_u64().unwrap(),
        NUM_WORKERS as u64
    );
    assert_eq!(
        json_resp.get("jobsPending").unwrap().as_u64().unwrap(),
        (num_reqs - NUM_WORKERS) as u64
    );
    assert_eq!(json_resp.get("status").unwrap().as_str().unwrap(), "busy");
}

#[named]
async fn test_ready_reports_correct_job_numbers() {
    setup_test(function_name!());
    let body = serialized_valid_zswap_body().await;
    let mut tasks = vec![];
    let num_reqs = LIMIT - 1 + NUM_WORKERS;

    let now = std::time::Instant::now();
    for _ in 0..num_reqs {
        let fut = build_client(30)
            .post(format!("{}/prove-tx", get_host_and_port()))
            .body(body.clone())
            .send();
        let task = tokio::spawn(fut);
        tasks.push(task);
        // Wait for request to be picked up
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    // Wait for requests to send
    tokio::time::sleep(Duration::from_millis(100)).await;

    let resp = HTTP_CLIENT
        .get(format!("{}/ready", get_host_and_port()))
        .send()
        .await
        .unwrap();
    let status = resp.status();
    let json_resp = resp.json::<serde_json::Value>().await.unwrap();
    println!("{:#?}", json_resp);
    assert_eq!(status, 200);
    assert_eq!(json_resp.get("status").unwrap().as_str().unwrap(), "ok");
    assert_eq!(
        json_resp.get("jobsProcessing").unwrap().as_u64().unwrap(),
        NUM_WORKERS as u64
    );
    assert_eq!(
        json_resp.get("jobsPending").unwrap().as_u64().unwrap(),
        (num_reqs - NUM_WORKERS) as u64
    );

    println!("elapsed: {:?}", now.elapsed());
}

#[named]
async fn test_health_check_still_works_when_server_is_fully_loaded() {
    setup_test(function_name!());
    let body = serialized_valid_zswap_body().await;
    let mut tasks = vec![];
    let num_reqs = 50;
    for _ in 0..num_reqs {
        let fut = build_client(30)
            .post(format!("{}/prove-tx", get_host_and_port()))
            .body(body.clone())
            .send();
        let task = tokio::spawn(fut);
        tasks.push(task);
    }
    // Wait for requests to send
    tokio::time::sleep(Duration::from_millis(300)).await;

    let resp = HTTP_CLIENT
        .get(format!("{}/health", get_host_and_port()))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}
