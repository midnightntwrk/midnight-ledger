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

/// Run one inspect-circuit pass and assert preimage round-trips.
/// Returns the decoded `ProofPreimage` so callers can do extra
/// circuit-specific assertions.
async fn run_inspect(
    bridge: &NodeChildBridge,
    circuit: &str,
    state_hex: &str,
    contract_address_hex: &str,
    controller_secret_hex: &str,
    circuit_args: serde_json::Value,
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

#[tokio::test]
async fn add_also_known_as() {
    // Simplest "write" circuit — adds a string to the alsoKnownAs
    // set. Starting state has the set empty, so insert succeeds.
    let s = fresh_setup();
    run_inspect(
        &s.bridge,
        "addAlsoKnownAs",
        &s.state_hex,
        &s.addr_hex,
        &s.sk_hex,
        serde_json::json!(["https://alias.example.com"]),
    )
    .await;
}

#[tokio::test]
async fn add_verification_method() {
    // Inserts a verificationMethods[id] entry. Starting state has
    // the map empty, so insert succeeds. KeyType / CurveType /
    // VerificationMethodType are integer enums in the JS bindings;
    // `x` and `y` are Field elements (bigint).
    let s = fresh_setup();
    // Contract asserts kty=EC ↔ crv ∈ {Jubjub, P256}; kty=OKP ↔ crv=Ed25519.
    // Use OKP/Ed25519 — matches the on-chain "valid Ed25519 verification
    // method" case the upstream tests cover.
    let verification_method = serde_json::json!({
        "id": "key-0",
        // VerificationMethodType.JsonWebKey = 1
        "typ": 1,
        "publicKeyJwk": {
            // KeyType.OKP = 3
            "kty": 3,
            // CurveType.Ed25519 = 0
            "crv": 0,
            "x": bigint("1"),
            "y": bigint("2"),
        }
    });
    run_inspect(
        &s.bridge,
        "addVerificationMethod",
        &s.state_hex,
        &s.addr_hex,
        &s.sk_hex,
        serde_json::json!([verification_method]),
    )
    .await;
}

#[tokio::test]
async fn add_service() {
    // Inserts a services[id] entry. Empty map, insert succeeds.
    let s = fresh_setup();
    let service = serde_json::json!({
        "id": "svc-0",
        "typ": "LinkedDomains",
        "serviceEndpoint": "https://example.com/.well-known/did-config"
    });
    run_inspect(
        &s.bridge,
        "addService",
        &s.state_hex,
        &s.addr_hex,
        &s.sk_hex,
        serde_json::json!([service]),
    )
    .await;
}

#[tokio::test]
async fn deactivate_no_args() {
    // Smoke; full assertions live in `js_inspect_deactivate`.
    let s = fresh_setup();
    run_inspect(
        &s.bridge,
        "deactivate",
        &s.state_hex,
        &s.addr_hex,
        &s.sk_hex,
        serde_json::json!([]),
    )
    .await;
}
