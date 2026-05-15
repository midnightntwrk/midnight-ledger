//! Offline coverage for DID write circuits via the Node harness.
//!
//! Drives every circuit whose preconditions are met by a fresh
//! empty-deploy state (no `remove*` / `update*` — those need a
//! prior write to populate state, follow-up tests can compose).
//! For each: build state, run circuit in JS, deserialise the
//! `ProofPreimage` on Rust side, assert structural invariants.
//!
//! Companion to `js_inspect_deactivate.rs`; that one covers the
//! simplest no-arg circuit, this one covers args + structured
//! types (VerificationMethod, Service).
//!
//! Run with:
//!   cargo test -p wallet-core --test js_inspect_circuits -- --nocapture

use wallet_core::js_bridge::{JsBridge, NodeChildBridge};

#[derive(serde::Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct InspectResult {
    circuit: String,
    public_transcript_len: i64,
    private_transcript_len: i64,
    preimage_hex: String,
    elapsed_ms: i64,
}

/// A single circuit invocation: the name + JSON args for it.
/// `serde_json::Value::Array` is the wire shape the harness expects.
fn step(circuit: &str, args: serde_json::Value) -> serde_json::Value {
    serde_json::json!({ "circuit": circuit, "args": args })
}

/// Run one inspect-circuit pass and assert preimage round-trips.
/// `setup` is a chain of prior calls used to evolve state before the
/// circuit under test runs (e.g. `addAlsoKnownAs` before
/// `removeAlsoKnownAs`). Returns the decoded `ProofPreimage` so
/// callers can do extra circuit-specific assertions.
async fn run_inspect(
    bridge: &NodeChildBridge,
    circuit: &str,
    state_hex: &str,
    contract_address_hex: &str,
    controller_secret_hex: &str,
    circuit_args: serde_json::Value,
    setup: serde_json::Value,
) -> transient_crypto::proofs::ProofPreimage {
    let r: InspectResult = bridge
        .call(
            "inspectCircuit",
            serde_json::json!({
                "circuit": circuit,
                "contractStateHex": state_hex,
                "contractAddressHex": contract_address_hex,
                "controllerSecretHex": controller_secret_hex,
                "circuitArgs": circuit_args,
                "setup": setup,
            }),
        )
        .await
        .unwrap_or_else(|e| panic!("inspectCircuit {circuit}: {e}"));
    assert_eq!(r.circuit, circuit);
    assert!(
        r.public_transcript_len > 0,
        "{circuit} produced empty public transcript",
    );
    let preimage_bytes = hex::decode(&r.preimage_hex).expect("preimage hex");
    let preimage: transient_crypto::proofs::ProofPreimage =
        serialize::tagged_deserialize(&preimage_bytes[..])
            .unwrap_or_else(|e| panic!("decode preimage for {circuit}: {e}"));
    let expected_key_loc = format!("midnight/did/{circuit}");
    assert_eq!(preimage.key_location.0, expected_key_loc);
    eprintln!(
        "[{circuit:30}] preimage {:4} B · pub {:3} ops · priv {} · {} ms",
        preimage_bytes.len(),
        r.public_transcript_len,
        r.private_transcript_len,
        r.elapsed_ms,
    );
    preimage
}

struct Setup {
    bridge: NodeChildBridge,
    state_hex: String,
    addr_hex: String,
    sk_hex: String,
}

fn fresh_setup() -> Setup {
    let controller_sk: [u8; 32] = rand::random();
    let ts_ms: u64 = 1_700_000_000_000;
    let state_hex = wallet_core::testing_initial_deploy_state_hex(&controller_sk, ts_ms)
        .expect("compose initial state");
    Setup {
        bridge: NodeChildBridge::spawn(&NodeChildBridge::default_harness_path())
            .expect("spawn harness"),
        state_hex,
        addr_hex: hex::encode([0u8; 32]),
        sk_hex: hex::encode(controller_sk),
    }
}

/// `{ "$bigint": "<decimal>" }` — placeholder the harness recognises
/// and revives as a JS BigInt before invoking the circuit. JSON has
/// no native bigint and JS Number loses precision past 2^53.
fn bigint(n: &str) -> serde_json::Value {
    serde_json::json!({ "$bigint": n })
}

/// Canonical valid VerificationMethod fixture — OKP / Ed25519,
/// satisfying the contract's curve constraint. Helper because
/// six tests need the same shape.
fn ed25519_vm(id: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        // VerificationMethodType.JsonWebKey = 1
        "typ": 1,
        "publicKeyJwk": {
            // KeyType.OKP = 3, CurveType.Ed25519 = 0
            "kty": 3,
            "crv": 0,
            "x": bigint("1"),
            "y": bigint("2"),
        }
    })
}

fn linked_domains_service(id: &str, endpoint: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "typ": "LinkedDomains",
        "serviceEndpoint": endpoint,
    })
}

/// `VerificationMethodRelation` enum tag (1..=5). 0 = Undefined,
/// rejected by the contract; the five valid values map to the
/// five DID Core relation slots.
const REL_AUTHENTICATION: i32 = 1;

#[tokio::test]
async fn add_also_known_as() {
    let s = fresh_setup();
    run_inspect(
        &s.bridge,
        "addAlsoKnownAs",
        &s.state_hex,
        &s.addr_hex,
        &s.sk_hex,
        serde_json::json!(["https://alias.example.com"]),
        serde_json::json!([]),
    )
    .await;
}

#[tokio::test]
async fn remove_also_known_as() {
    // Needs the value present first; insert it in setup.
    let s = fresh_setup();
    run_inspect(
        &s.bridge,
        "removeAlsoKnownAs",
        &s.state_hex,
        &s.addr_hex,
        &s.sk_hex,
        serde_json::json!(["https://alias.example.com"]),
        serde_json::json!([
            step("addAlsoKnownAs", serde_json::json!(["https://alias.example.com"])),
        ]),
    )
    .await;
}

#[tokio::test]
async fn add_verification_method() {
    let s = fresh_setup();
    run_inspect(
        &s.bridge,
        "addVerificationMethod",
        &s.state_hex,
        &s.addr_hex,
        &s.sk_hex,
        serde_json::json!([ed25519_vm("key-0")]),
        serde_json::json!([]),
    )
    .await;
}

#[tokio::test]
async fn update_verification_method() {
    // Needs the id already present; setup adds the original entry.
    let s = fresh_setup();
    let original = ed25519_vm("key-0");
    let updated = serde_json::json!({
        "id": "key-0",
        "typ": 1,
        "publicKeyJwk": {
            "kty": 3,
            "crv": 0,
            "x": bigint("11"),
            "y": bigint("22"),
        }
    });
    run_inspect(
        &s.bridge,
        "updateVerificationMethod",
        &s.state_hex,
        &s.addr_hex,
        &s.sk_hex,
        serde_json::json!([updated]),
        serde_json::json!([step("addVerificationMethod", serde_json::json!([original]))]),
    )
    .await;
}

#[tokio::test]
async fn remove_verification_method() {
    // Needs the id present; not referenced by any relation
    // (the contract asserts each before allowing remove). Setup
    // just inserts a fresh VM, no relation references.
    let s = fresh_setup();
    run_inspect(
        &s.bridge,
        "removeVerificationMethod",
        &s.state_hex,
        &s.addr_hex,
        &s.sk_hex,
        serde_json::json!(["key-0"]),
        serde_json::json!([step("addVerificationMethod", serde_json::json!([ed25519_vm("key-0")]))]),
    )
    .await;
}

#[tokio::test]
async fn add_verification_method_relation() {
    // Needs the VM to exist before we can relate it.
    let s = fresh_setup();
    run_inspect(
        &s.bridge,
        "addVerificationMethodRelation",
        &s.state_hex,
        &s.addr_hex,
        &s.sk_hex,
        serde_json::json!([REL_AUTHENTICATION, "key-0"]),
        serde_json::json!([step("addVerificationMethod", serde_json::json!([ed25519_vm("key-0")]))]),
    )
    .await;
}

#[tokio::test]
async fn remove_verification_method_relation() {
    // Add the VM, add the relation, then test removing the relation.
    let s = fresh_setup();
    run_inspect(
        &s.bridge,
        "removeVerificationMethodRelation",
        &s.state_hex,
        &s.addr_hex,
        &s.sk_hex,
        serde_json::json!([REL_AUTHENTICATION, "key-0"]),
        serde_json::json!([
            step("addVerificationMethod", serde_json::json!([ed25519_vm("key-0")])),
            step("addVerificationMethodRelation", serde_json::json!([REL_AUTHENTICATION, "key-0"])),
        ]),
    )
    .await;
}

#[tokio::test]
async fn add_service() {
    let s = fresh_setup();
    run_inspect(
        &s.bridge,
        "addService",
        &s.state_hex,
        &s.addr_hex,
        &s.sk_hex,
        serde_json::json!([linked_domains_service(
            "svc-0",
            "https://example.com/.well-known/did-config",
        )]),
        serde_json::json!([]),
    )
    .await;
}

#[tokio::test]
async fn update_service() {
    let s = fresh_setup();
    run_inspect(
        &s.bridge,
        "updateService",
        &s.state_hex,
        &s.addr_hex,
        &s.sk_hex,
        serde_json::json!([linked_domains_service(
            "svc-0",
            "https://other.example.com/.well-known/did-config",
        )]),
        serde_json::json!([step(
            "addService",
            serde_json::json!([linked_domains_service(
                "svc-0",
                "https://example.com/.well-known/did-config",
            )]),
        )]),
    )
    .await;
}

#[tokio::test]
async fn remove_service() {
    let s = fresh_setup();
    run_inspect(
        &s.bridge,
        "removeService",
        &s.state_hex,
        &s.addr_hex,
        &s.sk_hex,
        serde_json::json!(["svc-0"]),
        serde_json::json!([step(
            "addService",
            serde_json::json!([linked_domains_service(
                "svc-0",
                "https://example.com/.well-known/did-config",
            )]),
        )]),
    )
    .await;
}

#[tokio::test]
async fn deactivate_no_args() {
    let s = fresh_setup();
    run_inspect(
        &s.bridge,
        "deactivate",
        &s.state_hex,
        &s.addr_hex,
        &s.sk_hex,
        serde_json::json!([]),
        serde_json::json!([]),
    )
    .await;
}
