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
use ledger::error::TransactionProvingError;
use ledger::structure::{
    ContractDeploy, Intent, LedgerState, ProofMarker, ProofPreimageMarker, SignatureKind,
    Transaction,
};
use ledger::verify::WellFormedStrictness;
use midnight_proof_server::worker_pool::WorkerPool;
use std::borrow::Cow;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::Path;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Once};
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
use reqwest::{Client, StatusCode};
use serialize::{Tagged, tagged_deserialize, tagged_serialize};
use storage::db::InMemoryDB;
use storage::storage::HashMap as StorageHashMap;

use ledger::test_utilities::{ProofServerProvider, Resolver, test_resolver, verifier_key};
use midnight_proof_server::server;
use onchain_runtime::cost_model::INITIAL_COST_MODEL;
use regex::Regex;

const NUM_WORKERS: usize = 2;
const LIMIT: usize = 2;
const CONCURRENT_PROVE_REQUESTS: usize = 10;
const HEALTH_LOAD_REQUESTS: usize = 50;
static mut SERVER_PORT: u16 = 0;
static INIT: Once = Once::new();

lazy_static! {
    static ref HTTP_CLIENT: Client = Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    static ref RESOLVER: Resolver = test_resolver("fallible");
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

// Builds an invalid body that contains only the tagged transaction preimage
// (no ZK Config section).
async fn serialized_invalid_body_without_zk_config() -> Vec<u8> {
    let tx = valid_tx::<Signature>().await;
    let mut body = Vec::new();
    tagged_serialize(&tx, &mut body).expect("transaction-only payload should serialize");
    body
}

// Builds an invalid request body with two different ZK Configs back-to-back:
// `[(tx, zkA)] [zkB]`.
async fn serialized_invalid_body_with_double_zk_config() -> Vec<u8> {
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
// Result: {A -> zkB, B -> zkA} (keys unchanged). Server should reject.
async fn serialized_invalid_body_with_swapped_zk_config_values() -> Option<Vec<u8>> {
    let valid_body = serialized_valid_body().await;
    let (tx, mut keys): (
        Transaction<Signature, ProofPreimageMarker, PedersenRandomness, InMemoryDB>,
        HashMap<KeyLocation, ProvingKeyMaterial>,
    ) = tagged_deserialize(&valid_body[..]).ok()?;

    if keys.len() < 2 {
        let (_, extra_keys): (
            Transaction<Signature, ProofPreimageMarker, PedersenRandomness, InMemoryDB>,
            HashMap<KeyLocation, ProvingKeyMaterial>,
        ) = tagged_deserialize(&serialized_valid_zswap_body().await[..]).ok()?;
        for (location, material) in extra_keys {
            keys.entry(location).or_insert(material);
            if keys.len() >= 2 {
                break;
            }
        }
    }

    ensure_minimum_distinct_keys(&mut keys);

    let mut key_iter = keys.keys();
    let first_key = match key_iter.next() {
        Some(key) => key.clone(),
        None => return None,
    };
    let second_key = match key_iter.next() {
        Some(key) => key.clone(),
        None => return None,
    };
    drop(key_iter);

    let (_, first_value) = keys
        .remove_entry(&first_key)
        .expect("first key should still exist");
    let (_, second_value) = keys
        .remove_entry(&second_key)
        .expect("second key should still exist");
    debug_assert!(keys.insert(first_key, second_value).is_none());
    debug_assert!(keys.insert(second_key, first_value).is_none());

    let mut payload = Vec::new();
    tagged_serialize(&(tx, keys), &mut payload).expect("swapped payload should serialize");
    Some(payload)
}

// Builds an invalid body by changing one of the ZK Config keys to reference a
// circuit ID that is not used in the transaction.
async fn serialized_invalid_body_with_wrong_circuit_id() -> Option<Vec<u8>> {
    let valid_body = serialized_valid_body().await;
    let (tx, mut keys): (
        Transaction<Signature, ProofPreimageMarker, PedersenRandomness, InMemoryDB>,
        HashMap<KeyLocation, ProvingKeyMaterial>,
    ) = tagged_deserialize(&valid_body[..]).ok()?;

    if keys.is_empty() {
        let (_, fallback_keys): (
            Transaction<Signature, ProofPreimageMarker, PedersenRandomness, InMemoryDB>,
            HashMap<KeyLocation, ProvingKeyMaterial>,
        ) = tagged_deserialize(&serialized_valid_zswap_body().await[..]).ok()?;
        keys = fallback_keys;
    }

    ensure_minimum_distinct_keys(&mut keys);

    let key_to_replace = match keys.keys().next() {
        Some(key) => key.clone(),
        None => return None,
    };

    let (KeyLocation(original_id), value) = keys
        .remove_entry(&key_to_replace)
        .expect("selected key should exist");
    let mut wrong_id = original_id.into_owned();
    wrong_id.push_str("_wrong");
    let wrong_key = KeyLocation(Cow::Owned(wrong_id));
    debug_assert!(keys.insert(wrong_key, value).is_none());

    let mut payload = Vec::new();
    tagged_serialize(&(tx, keys), &mut payload).ok()?;
    Some(payload)
}

// Ensures a request payload has at least two distinct proving-key entries, hydrating fixtures
// and synthesizing a tweaked clone when necessary so negative tests exercise swapping logic.
fn ensure_minimum_distinct_keys(keys: &mut HashMap<KeyLocation, ProvingKeyMaterial>) {
    if keys.len() >= 2 {
        return;
    }

    ensure_has_fixture(keys);
    if keys.len() >= 2 {
        return;
    }

    let Some((existing_key, existing_value)) =
        keys.iter().next().map(|(k, v)| (k.clone(), v.clone()))
    else {
        unreachable!("keys map cannot be empty after fixture hydration");
    };

    let mut mutated_value = existing_value;
    tweak_all(&mut mutated_value);

    let base_name = existing_key.0.as_ref();
    let mut counter = 1usize;
    let synthetic_key = loop {
        let candidate = if counter == 1 {
            format!("{base_name}_alt")
        } else {
            format!("{base_name}_alt{counter}")
        };

        if !keys.keys().any(|existing| existing.0.as_ref() == candidate) {
            break KeyLocation(Cow::Owned(candidate));
        }

        counter += 1;
    };

    keys.insert(synthetic_key, mutated_value);
}

// Loads the fallback proving-key fixture from disk when the resolver produces no entries.
fn ensure_has_fixture(keys: &mut HashMap<KeyLocation, ProvingKeyMaterial>) {
    if !keys.is_empty() {
        return;
    }

    let Some(test_dir) = env::var("MIDNIGHT_LEDGER_TEST_STATIC_DIR").ok() else {
        panic!(
            "no proving keys available; set MIDNIGHT_LEDGER_TEST_STATIC_DIR to the test artifacts directory"
        );
    };

    let base = Path::new(&test_dir).join("fallible");
    let keys_dir = base.join("keys");
    let zkir_dir = base.join("zkir");

    let prover_key = fs::read(keys_dir.join("count.prover")).expect("count prover key missing");
    let verifier_key =
        fs::read(keys_dir.join("count.verifier")).expect("count verifier key missing");
    let ir_source = fs::read(zkir_dir.join("count.bzkir")).expect("count IR source missing");

    keys.insert(
        KeyLocation(Cow::Owned("count".to_owned())),
        ProvingKeyMaterial {
            prover_key,
            verifier_key,
            ir_source,
        },
    );
}

// Mutates every component of a proving key so cloned fixtures remain distinct in negative tests.
fn tweak_all(material: &mut ProvingKeyMaterial) {
    fn tweak(bytes: &mut Vec<u8>) {
        if let Some(first) = bytes.first_mut() {
            *first ^= 0x01;
        } else {
            bytes.push(0x01);
        }
    }

    tweak(&mut material.prover_key);
    tweak(&mut material.verifier_key);
    tweak(&mut material.ir_source);
}

// Builds the **canonical /prove request body** for a normal valid transaction.
async fn serialized_valid_body() -> Vec<u8> {
    let tx = valid_tx::<Signature>().await;
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
async fn serialize_transaction_payload<S>(
    tx: &Transaction<S, ProofPreimageMarker, PedersenRandomness, InMemoryDB>,
    resolver: &Resolver,
) -> Vec<u8>
where
    S: SignatureKind<InMemoryDB> + Tagged,
{
    let circuits_used = tx
        .calls()
        .into_iter()
        .map(|(_, c)| String::from_utf8_lossy(&c.entry_point).into_owned())
        .collect::<Vec<_>>();

    let mut keys: HashMap<KeyLocation, ProvingKeyMaterial> = HashMap::new();
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

// Builds a ProofServerProvider bound to the shared resolver so tests can hit `/prove`.
fn proof_provider(base_url: String) -> ProofServerProvider<'static> {
    ProofServerProvider {
        base_url,
        resolver: &RESOLVER,
    }
}

// Spawns proving tasks that submit `tx` to the proof server, optionally spacing dispatches.
async fn spawn_proving_tasks(
    count: usize,
    base_url: String,
    tx: Arc<Transaction<Signature, ProofPreimageMarker, PedersenRandomness, InMemoryDB>>,
    delay_between: Option<Duration>,
    log_success: bool,
) -> Vec<tokio::task::JoinHandle<Result<(), TransactionProvingError<InMemoryDB>>>> {
    let mut tasks = Vec::with_capacity(count);
    for i in 0..count {
        let base_url = base_url.clone();
        let tx = Arc::clone(&tx);
        let handle = tokio::spawn(async move {
            let provider = proof_provider(base_url);
            let result = tx.prove(provider, &INITIAL_COST_MODEL).await.map(|_| ());
            if log_success && result.is_ok() {
                log::info!("Iteration {i:?} proved successfully");
            }
            result
        });
        tasks.push(handle);

        if let Some(delay) = delay_between {
            tokio::time::sleep(delay).await;
        }
    }
    tasks
}

// Waits for proving tasks to complete, asserting on queue-full tolerance appropriately.
async fn await_proving_results(
    tasks: Vec<tokio::task::JoinHandle<Result<(), TransactionProvingError<InMemoryDB>>>>,
    allow_queue_full: bool,
) {
    for task in futures::future::join_all(tasks).await {
        match task.expect("Proving task should not panic") {
            Ok(()) => {}
            Err(TransactionProvingError::Proving(err)) if allow_queue_full => {
                assert!(
                    err.to_string().contains("Job Queue full"),
                    "unexpected proving error: {err}"
                );
            }
            Err(TransactionProvingError::Proving(err)) => {
                panic!("unexpected proving failure: {err}");
            }
            Err(other) => panic!("unexpected proving failure: {other:?}"),
        }
    }
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

async fn valid_tx<S: SignatureKind<InMemoryDB>>()
-> Transaction<S, ProofPreimageMarker, PedersenRandomness, InMemoryDB> {
    valid_tx_with_network_id::<S>("local-test").await
}

async fn valid_tx_with_network_id<S: SignatureKind<InMemoryDB>>(
    network_id: &str,
) -> Transaction<S, ProofPreimageMarker, PedersenRandomness, InMemoryDB> {
    let mut rng = StdRng::seed_from_u64(0x42);

    let count_op = ContractOperation::new(verifier_key(&RESOLVER, "count").await);
    let contract = ContractState::new(
        stval!([(0u64), (false), (0u64)]),
        StorageHashMap::new().insert(b"count"[..].into(), count_op.clone()),
        Default::default(),
    );

    let deploy = ContractDeploy::new(&mut rng, contract.clone());

    let mut intents = StorageHashMap::<
        u16,
        Intent<S, ProofPreimageMarker, PedersenRandomness, InMemoryDB>,
        InMemoryDB,
    >::new();
    let intent = Intent::empty(&mut rng, Timestamp::from_secs(3600)).add_deploy(deploy);
    intents = intents.insert(1, intent);

    Transaction::new(network_id, intents, None, Default::default())
}

// Builds a large valid *unproven* transaction (preimage form) for stress-testing proving.
async fn large_valid_tx<S: SignatureKind<InMemoryDB>>(
    num_intents: usize,
) -> Transaction<S, ProofPreimageMarker, PedersenRandomness, InMemoryDB> {
    let mut rng = StdRng::seed_from_u64(0x4242);

    let count_op = ContractOperation::new(verifier_key(&RESOLVER, "count").await);
    let contract = ContractState::new(
        stval!([(0u64), (false), (0u64)]),
        StorageHashMap::new().insert(b"count"[..].into(), count_op.clone()),
        Default::default(),
    );

    let mut intents = StorageHashMap::<
        u16,
        Intent<S, ProofPreimageMarker, PedersenRandomness, InMemoryDB>,
        InMemoryDB,
    >::new();

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

fn set_network_id(value: &str) {
    // SAFETY: The tests run the proof server in a dedicated thread spawned for each
    // scenario, and we never mutate the process environment concurrently elsewhere.
    unsafe {
        env::set_var("NETWORK_ID", value);
    }
}

fn clear_network_id() {
    // SAFETY: See `set_network_id`; we own the lifecycle of the spawned server during tests.
    unsafe {
        env::remove_var("NETWORK_ID");
    }
}

// Asserts that a network-ID mismatch was returned by the server.
fn assert_network_id_error(msg: &str) {
    assert!(
        msg.contains("invalid network ID"),
        "expected network ID error, got '{msg}'"
    );
    assert!(
        msg.contains("Undeployed"),
        "expected error to mention expected network ID, got '{msg}'"
    );
    assert!(
        msg.contains("DevNet"),
        "expected error to mention found network ID, got '{msg}'"
    );
}

#[tokio::test]
async fn integration_tests() {
    let _server_handle = setup(LIMIT).recv().unwrap();

    test_root_should_return_status().await;
    test_health_should_return_status().await;
    test_proof_versions_should_return_status().await;
    test_version_should_return_current_version().await;
    test_prove_should_fail_on_get().await;
    test_prove_should_fail_on_empty_body().await;
    test_prove_should_fail_on_json().await;
    test_prove_should_fail_without_zk_config().await;
    test_prove_should_fail_with_double_zk_config().await;
    test_prove_should_fail_with_swapped_zk_config_values().await;
    test_prove_should_fail_with_wrong_circuit_id().await;
    test_prove_should_generate_valid_proof_for_smallest_transaction().await;
    test_prove_should_generate_valid_proof_for_big_transaction().await;
    test_prove_should_prove_correct_transaction().await;
    test_prove_should_fail_on_repeated_body().await;
    test_prove_should_fail_on_corrupted_body().await;
    test_health_check_still_works_when_server_is_fully_loaded().await;

    stop_server(_server_handle).await;

    test_prove_should_fail_with_mismatched_network_id().await;

    let _server_handle = setup(LIMIT).recv().unwrap();
    test_ready_reports_correct_job_numbers().await;
    stop_server(_server_handle).await;

    let _server_handle = setup(LIMIT).recv().unwrap();
    test_ready_reports_busy().await;
    stop_server(_server_handle).await;

    let _server_handle = setup(10).recv().unwrap();
    test_prove_should_handle_multiple_requests().await;
    stop_server(_server_handle).await;

    let _server_handle = setup(0).recv().unwrap();
    test_prove_should_handle_multiple_requests_with_zero_limit().await;
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
    assert_eq!(response.status(), StatusCode::OK);
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
    assert_eq!(response.status(), StatusCode::OK);
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
    assert_eq!(response.status(), StatusCode::OK);
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
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.text().await.unwrap(), env!("CARGO_PKG_VERSION"));
}

#[named]
async fn test_prove_should_fail_on_get() {
    setup_test(function_name!());

    let response = HTTP_CLIENT
        .get(format!("{}/prove", get_host_and_port()))
        .send()
        .await
        .unwrap();

    log::info!("Response code: {:?}", response.status());
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[named]
async fn test_prove_should_fail_on_empty_body() {
    setup_test(function_name!());

    let response = HTTP_CLIENT
        .post(format!("{}/prove", get_host_and_port()))
        .send()
        .await
        .unwrap();

    log::info!("Response code: {:?}", response.status());
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let resp_text = response.text().await.unwrap();
    assert!(resp_text.contains("expected header tag"));
    assert!(resp_text.contains(", got ''"));
}

#[named]
async fn test_prove_should_fail_on_json() {
    setup_test(function_name!());

    let response = HTTP_CLIENT
        .post(format!("{}/prove", get_host_and_port()))
        .header("Content-Type", "application/json")
        .body(r#"{"key": "value"}"#)
        .send()
        .await
        .unwrap();

    log::info!("Response code: {:?}", response.status());
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let resp_text = dbg!(response.text().await.unwrap());
    assert!(resp_text.contains("expected header tag"));
}

// Negative test: `/prove` must reject a payload without ZK Config.
// Given: A request body that contains only the tagged transaction preimage.
// When: POSTed to `/prove`.
// Then: The server responds with `Bad Request`.
#[named]
async fn test_prove_should_fail_without_zk_config() {
    setup_test(function_name!());

    let response = HTTP_CLIENT
        .post(format!("{}/prove", get_host_and_port()))
        .body(serialized_invalid_body_without_zk_config().await)
        .send()
        .await
        .unwrap();

    log::info!("Response code: {:?}", response.status());
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let resp_text = response.text().await.unwrap();
    assert!(resp_text.contains("expected header tag"));
}

// Negative test: `/prove` must reject payloads that include two different
// ZK Config blocks one after another (double ZK Config).
// Given:
// - A valid request `[(tx, zkA)]`.
// - A second, different ZK Config `zkB` (sourced from a different valid body).
// When: We append `zkB` after the first tuple and POST to `/prove`.
// Then: The server responds with `Bad Request`.
#[named]
async fn test_prove_should_fail_with_double_zk_config() {
    setup_test(function_name!());

    let response = HTTP_CLIENT
        .post(format!("{}/prove", get_host_and_port()))
        .body(serialized_invalid_body_with_double_zk_config().await)
        .send()
        .await
        .unwrap();

    log::info!("Response code: {:?}", response.status());
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let resp_text = response.text().await.unwrap();
    assert!(resp_text.contains("expected header tag"));
}

// Negative test: `/prove` must reject payloads where ZK Config values are value-swapped.
// Given:
// - A valid request `(tx, {A -> zkA, B -> zkB, ...})`.
// - We swap the proving material so that `{A -> zkB, B -> zkA}`.
// When: The malformed payload is POSTed to `/prove`.
// Then: The server responds with `Bad Request`.
#[named]
async fn test_prove_should_fail_with_swapped_zk_config_values() {
    setup_test(function_name!());

    let payload = serialized_invalid_body_with_swapped_zk_config_values()
        .await
        .expect(
            "expected to build payload with swapped proving material; ensure proving keys are available"
        );
    let response = HTTP_CLIENT
        .post(format!("{}/prove", get_host_and_port()))
        .body(payload)
        .send()
        .await
        .unwrap();

    log::info!("Response code: {:?}", response.status());
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let resp_text = response.text().await.unwrap();
    assert!(
        resp_text.contains("failed to find proving key")
            || resp_text.contains("expected header tag")
    );
}

// Negative test: `/prove` must reject payloads whose ZK Config contains an
// entry keyed by a circuit ID that is not part of the transaction.
// Given: A valid request `(tx, {A -> zkA, ...})`.
// When: We replace one key with `C_wrong` while keeping the proving material.
// Then: The server responds with `Bad Request`.
#[named]
async fn test_prove_should_fail_with_wrong_circuit_id() {
    setup_test(function_name!());

    let payload = serialized_invalid_body_with_wrong_circuit_id()
        .await
        .expect(
            "expected to build payload with wrong circuit identifier; ensure proving keys are available"
        );
    let response = HTTP_CLIENT
        .post(format!("{}/prove", get_host_and_port()))
        .body(payload)
        .send()
        .await
        .unwrap();

    log::info!("Response code: {:?}", response.status());
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let resp_text = response.text().await.unwrap();
    assert!(
        resp_text.contains("failed to find proving key")
            || resp_text.contains("expected header tag")
    );
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

    let provider = proof_provider(get_host_and_port());

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

    let provider = proof_provider(get_host_and_port());

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

// Negative test: proving must fail when the transaction `network_id` does not
// match the proof server's `NETWORK_ID`.
// Given: Server with NETWORK_ID="Undeployed" and a tx tagged "DevNet".
// When: We prove via `Transaction::prove` using a `ProofServerProvider`.
// Then: It fails with "invalid network ID", naming expected "Undeployed" and found "DevNet".
#[named]
async fn test_prove_should_fail_with_mismatched_network_id() {
    setup_test(function_name!());

    let previous_network_id = env::var("NETWORK_ID").ok();
    set_network_id("Undeployed");
    let server_handle = setup(LIMIT).recv().unwrap();

    let tx: Transaction<Signature, ProofPreimageMarker, PedersenRandomness, InMemoryDB> =
        valid_tx_with_network_id("DevNet").await;

    let provider = proof_provider(get_host_and_port());

    match tx.prove(provider, &INITIAL_COST_MODEL).await {
        Ok(proven) => {
            let mut strictness = WellFormedStrictness::default();
            strictness.enforce_balancing = false;
            let err = proven
                .well_formed(
                    &LedgerState::new("Undeployed"),
                    strictness,
                    Timestamp::from_secs(0),
                )
                .expect_err("expected well-formed check to fail when network IDs mismatch");
            assert_network_id_error(&err.to_string());
        }
        Err(TransactionProvingError::Proving(err)) => {
            assert_network_id_error(&err.to_string());
        }
        Err(other) => panic!("expected proving error from server, got {other:?}"),
    }
    stop_server(server_handle).await;

    match previous_network_id {
        Some(value) => set_network_id(&value),
        None => clear_network_id(),
    }
}

#[named]
async fn test_prove_should_prove_correct_transaction() {
    setup_test(function_name!());

    let tx: Transaction<Signature, ProofPreimageMarker, PedersenRandomness, InMemoryDB> =
        valid_tx::<Signature>().await;

    let provider = proof_provider(get_host_and_port());

    let proven = tx
        .prove(provider, &INITIAL_COST_MODEL)
        .await
        .expect("proving canonical transaction payload should succeed");

    let mut bytes = Vec::new();
    tagged_serialize(&proven, &mut bytes)
        .expect("proven transaction should serialize successfully");
    let proof: Transaction<Signature, ProofMarker, PedersenRandomness, InMemoryDB> =
        tagged_deserialize(&bytes[..]).expect("proven transaction should deserialize successfully");
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
async fn test_prove_should_handle_multiple_requests() {
    setup_test(function_name!());
    let base_url = get_host_and_port();
    let tx = Arc::new(valid_tx::<Signature>().await);
    let handles = spawn_proving_tasks(CONCURRENT_PROVE_REQUESTS, base_url, tx, None, true).await;
    await_proving_results(handles, false).await;
}

#[named]
async fn test_prove_should_fail_on_repeated_body() {
    setup_test(function_name!());

    let response = HTTP_CLIENT
        .post(format!("{}/prove", get_host_and_port()))
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
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let resp_text = dbg!(response.text().await.unwrap());
    assert!(
        resp_text.contains("expected header tag")
            || Regex::new(r"^Not all bytes read deserializing '.*'; \d+ bytes remaining$")
                .unwrap()
                .is_match(resp_text.as_str()),
        "unexpected response text: {resp_text}"
    );
}

#[named]
async fn test_prove_should_fail_on_corrupted_body() {
    setup_test(function_name!());
    let mut body = serialized_valid_body().await;
    let mut rng = rand::rngs::StdRng::seed_from_u64(42);
    // make 50 random edits
    for _ in 0..50 {
        let len = body.len();
        body[rng.gen_range(0..len)] = rng.r#gen();
    }

    let response = HTTP_CLIENT
        .post(format!("{}/prove", get_host_and_port()))
        .body(body)
        .send()
        .await
        .unwrap();

    let stat = response.status();
    dbg!(response.bytes().await.unwrap());
    log::info!("Response code: {:?}", stat);
    assert_eq!(stat, StatusCode::BAD_REQUEST);
}

#[named]
async fn test_prove_should_handle_multiple_requests_with_zero_limit() {
    setup_test(function_name!());
    let base_url = get_host_and_port();
    let tx = Arc::new(valid_tx::<Signature>().await);
    let handles =
        spawn_proving_tasks(CONCURRENT_PROVE_REQUESTS, base_url.clone(), tx, None, true).await;

    let resp = HTTP_CLIENT
        .get(format!("{}/ready", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json_resp = resp.json::<serde_json::Value>().await.unwrap();
    assert_eq!(json_resp.get("status").unwrap().as_str().unwrap(), "ok");
    assert_eq!(json_resp.get("jobCapacity").unwrap().as_u64().unwrap(), 0);
    await_proving_results(handles, false).await;
}

#[named]
async fn test_ready_reports_busy() {
    setup_test(function_name!());
    let num_reqs = LIMIT + NUM_WORKERS;
    let base_url = get_host_and_port();
    let tx = Arc::new(valid_unbalanced_zswap(1));
    let tasks = spawn_proving_tasks(
        num_reqs,
        base_url.clone(),
        tx,
        Some(Duration::from_millis(50)),
        false,
    )
    .await;
    // Wait for requests to send
    tokio::time::sleep(Duration::from_millis(100)).await;

    let resp = HTTP_CLIENT
        .get(format!("{}/ready", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
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

    await_proving_results(tasks, true).await;
}

#[named]
async fn test_ready_reports_correct_job_numbers() {
    setup_test(function_name!());
    let num_reqs = LIMIT - 1 + NUM_WORKERS;
    let base_url = get_host_and_port();
    let tx = Arc::new(valid_unbalanced_zswap(1));

    let now = std::time::Instant::now();
    let tasks = spawn_proving_tasks(
        num_reqs,
        base_url.clone(),
        tx,
        Some(Duration::from_millis(50)),
        false,
    )
    .await;
    // Wait for requests to send
    tokio::time::sleep(Duration::from_millis(100)).await;

    let resp = HTTP_CLIENT
        .get(format!("{}/ready", base_url))
        .send()
        .await
        .unwrap();
    let status = resp.status();
    let json_resp = resp.json::<serde_json::Value>().await.unwrap();
    println!("{:#?}", json_resp);
    assert_eq!(status, StatusCode::OK);
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

    await_proving_results(tasks, true).await;
}

#[named]
async fn test_health_check_still_works_when_server_is_fully_loaded() {
    setup_test(function_name!());
    let base_url = get_host_and_port();
    let tx = Arc::new(valid_unbalanced_zswap(1));
    let num_reqs = HEALTH_LOAD_REQUESTS;
    let tasks = spawn_proving_tasks(num_reqs, base_url.clone(), tx, None, false).await;
    // Wait for requests to send
    tokio::time::sleep(Duration::from_millis(300)).await;

    let resp = HTTP_CLIENT
        .get(format!("{}/health", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    await_proving_results(tasks, true).await;
}
