//! Offline coverage for DID write circuits via the Node harness.
//!
//! Drives every circuit whose preconditions are met by a fresh
//! empty-deploy state (no `remove*` — those need a prior write
//! to populate state, follow-up tests can compose). For each:
//! build state, run circuit in JS, deserialise the
//! `ProofPreimage` on Rust side, assert structural invariants.
//!
//! Companion to `js_inspect_deactivate.rs`; that one covers the
//! simplest no-arg circuit, this one covers args + structured
//! types (VerificationMethod, Service, SchnorrJubjubVerificationMethod).
//!
//! 2026-05-28 schema refresh: the old `add* / update* / remove*`
//! circuit triples collapsed into unified `set*(value, mutation)`
//! entry points and the SchnorrJubjub VM map gained its own four
//! circuits + `rotateControllerKey`. The plain
//! `setVerificationMethod` circuit now REJECTS Jubjub JWKs — use
//! `setSchnorrJubjubVerificationMethod` for Jubjub keys instead.
//!
//! Run with:
//!   cargo test -p wallet-core --test js_inspect_circuits -- --nocapture

use wallet_core::js_bridge::{JsBridgeExt, NodeChildBridge};

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
/// circuit under test runs (e.g. `setAlsoKnownAs(Insert)` before
/// `setAlsoKnownAs(Remove)`). Returns the decoded `ProofPreimage`
/// so callers can do extra circuit-specific assertions.
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
        "[{circuit:38}] preimage {:4} B · pub {:3} ops · priv {} · {} ms",
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
/// satisfying the contract's curve constraint. Post-2026-05-28
/// schema: `x`/`y` are JWK base64url textual strings (NOT
/// bigints). Helper because half a dozen tests need the shape.
fn ed25519_vm(id: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        // VerificationMethodType.JsonWebKey = 1
        "typ": 1,
        "publicKeyJwk": {
            // KeyType.OKP = 3, CurveType.Ed25519 = 0
            "kty": 3,
            "crv": 0,
            // Placeholder base64url(32 zero bytes) — assertion of
            // on-curve membership happens for Jubjub only.
            "x": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            "y": "",
        }
    })
}

/// SchnorrJubjub fixture — `(id, JubjubPoint)`. `JubjubPoint` is
/// `{ x: bigint, y: bigint }` per `CompactTypeJubjubPoint` in
/// `@midnight-ntwrk/compact-runtime`. The placeholder coordinates
/// here are NOT a valid Jubjub point; the offline inspect path
/// stops short of the on-curve check (that lives in the
/// `verifySchnorrJubjubDigestSignature` circuit).
fn schnorr_jubjub_vm(id: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "publicKey": {
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

/// `SetMutation::Insert == 1`, `SetMutation::Remove == 2`.
const SET_INSERT: i32 = 1;
const SET_REMOVE: i32 = 2;

/// `MapMutation::Insert == 1`, `MapMutation::Update == 2`.
const MAP_INSERT: i32 = 1;
const MAP_UPDATE: i32 = 2;

#[tokio::test]
async fn set_also_known_as_insert() {
    let s = fresh_setup();
    run_inspect(
        &s.bridge,
        "setAlsoKnownAs",
        &s.state_hex,
        &s.addr_hex,
        &s.sk_hex,
        serde_json::json!(["https://alias.example.com", SET_INSERT]),
        serde_json::json!([]),
    )
    .await;
}

#[tokio::test]
async fn set_also_known_as_remove() {
    // Needs the value present first; insert it in setup.
    let s = fresh_setup();
    run_inspect(
        &s.bridge,
        "setAlsoKnownAs",
        &s.state_hex,
        &s.addr_hex,
        &s.sk_hex,
        serde_json::json!(["https://alias.example.com", SET_REMOVE]),
        serde_json::json!([
            step(
                "setAlsoKnownAs",
                serde_json::json!(["https://alias.example.com", SET_INSERT]),
            ),
        ]),
    )
    .await;
}

#[tokio::test]
async fn set_verification_method_insert() {
    let s = fresh_setup();
    run_inspect(
        &s.bridge,
        "setVerificationMethod",
        &s.state_hex,
        &s.addr_hex,
        &s.sk_hex,
        serde_json::json!([ed25519_vm("key-0"), MAP_INSERT]),
        serde_json::json!([]),
    )
    .await;
}

#[tokio::test]
async fn set_verification_method_update() {
    // Needs the id already present; setup adds the original entry.
    let s = fresh_setup();
    let original = ed25519_vm("key-0");
    let updated = serde_json::json!({
        "id": "key-0",
        "typ": 1,
        "publicKeyJwk": {
            "kty": 3,
            "crv": 0,
            "x": "ERESExQVFhcYGRobHB0eHyAhIiMkJSYnKCkqKyw",
            "y": "",
        }
    });
    run_inspect(
        &s.bridge,
        "setVerificationMethod",
        &s.state_hex,
        &s.addr_hex,
        &s.sk_hex,
        serde_json::json!([updated, MAP_UPDATE]),
        serde_json::json!([step(
            "setVerificationMethod",
            serde_json::json!([original, MAP_INSERT]),
        )]),
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
        serde_json::json!([step(
            "setVerificationMethod",
            serde_json::json!([ed25519_vm("key-0"), MAP_INSERT]),
        )]),
    )
    .await;
}

#[tokio::test]
async fn set_schnorr_jubjub_verification_method_insert() {
    let s = fresh_setup();
    run_inspect(
        &s.bridge,
        "setSchnorrJubjubVerificationMethod",
        &s.state_hex,
        &s.addr_hex,
        &s.sk_hex,
        serde_json::json!([schnorr_jubjub_vm("key-sj-0"), MAP_INSERT]),
        serde_json::json!([]),
    )
    .await;
}

#[tokio::test]
async fn remove_schnorr_jubjub_verification_method() {
    let s = fresh_setup();
    run_inspect(
        &s.bridge,
        "removeSchnorrJubjubVerificationMethod",
        &s.state_hex,
        &s.addr_hex,
        &s.sk_hex,
        serde_json::json!(["key-sj-0"]),
        serde_json::json!([step(
            "setSchnorrJubjubVerificationMethod",
            serde_json::json!([schnorr_jubjub_vm("key-sj-0"), MAP_INSERT]),
        )]),
    )
    .await;
}

#[tokio::test]
async fn set_verification_method_relation_insert() {
    // Needs the VM to exist before we can relate it.
    let s = fresh_setup();
    run_inspect(
        &s.bridge,
        "setVerificationMethodRelation",
        &s.state_hex,
        &s.addr_hex,
        &s.sk_hex,
        serde_json::json!([REL_AUTHENTICATION, "key-0", SET_INSERT]),
        serde_json::json!([step(
            "setVerificationMethod",
            serde_json::json!([ed25519_vm("key-0"), MAP_INSERT]),
        )]),
    )
    .await;
}

#[tokio::test]
async fn set_verification_method_relation_remove() {
    // Add the VM, add the relation, then test removing the relation.
    let s = fresh_setup();
    run_inspect(
        &s.bridge,
        "setVerificationMethodRelation",
        &s.state_hex,
        &s.addr_hex,
        &s.sk_hex,
        serde_json::json!([REL_AUTHENTICATION, "key-0", SET_REMOVE]),
        serde_json::json!([
            step(
                "setVerificationMethod",
                serde_json::json!([ed25519_vm("key-0"), MAP_INSERT]),
            ),
            step(
                "setVerificationMethodRelation",
                serde_json::json!([REL_AUTHENTICATION, "key-0", SET_INSERT]),
            ),
        ]),
    )
    .await;
}

#[tokio::test]
async fn set_service_insert() {
    let s = fresh_setup();
    run_inspect(
        &s.bridge,
        "setService",
        &s.state_hex,
        &s.addr_hex,
        &s.sk_hex,
        serde_json::json!([
            linked_domains_service("svc-0", "https://example.com/.well-known/did-config"),
            MAP_INSERT,
        ]),
        serde_json::json!([]),
    )
    .await;
}

#[tokio::test]
async fn set_service_update() {
    let s = fresh_setup();
    run_inspect(
        &s.bridge,
        "setService",
        &s.state_hex,
        &s.addr_hex,
        &s.sk_hex,
        serde_json::json!([
            linked_domains_service(
                "svc-0",
                "https://other.example.com/.well-known/did-config",
            ),
            MAP_UPDATE,
        ]),
        serde_json::json!([step(
            "setService",
            serde_json::json!([
                linked_domains_service("svc-0", "https://example.com/.well-known/did-config"),
                MAP_INSERT,
            ]),
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
            "setService",
            serde_json::json!([
                linked_domains_service("svc-0", "https://example.com/.well-known/did-config"),
                MAP_INSERT,
            ]),
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

#[tokio::test]
async fn rotate_controller_key() {
    // `rotateControllerKey` takes a Bytes<32> new-controller-pk.
    // The chain's witness derivation seeds the current pk from
    // `localSecretKey()`; the new one just needs to differ.
    let s = fresh_setup();
    let new_pk_hex = "ff".repeat(32);
    run_inspect(
        &s.bridge,
        "rotateControllerKey",
        &s.state_hex,
        &s.addr_hex,
        &s.sk_hex,
        serde_json::json!([{ "$bytes": format!("0x{new_pk_hex}") }]),
        serde_json::json!([]),
    )
    .await;
}

/// Bonus: confirm the contract's curve-rejection assertion fires
/// when we feed `setVerificationMethod` a Jubjub JWK. Without this
/// guard the bootstrap regression would be silent.
///
/// Disabled by default — exists as a manual probe; the offline
/// harness panics on assertion failures, so the test would need
/// to be inverted (`#[should_panic]`) once we know the exact
/// panic shape from the Compact runtime. Left documented here
/// so future engineers know where to look.
#[tokio::test]
#[ignore = "Documents the assertSupportedVerificationMethod \
            rejection path. Enable + invert (#[should_panic]) once \
            the offline harness's assertion-failure surface is \
            stable."]
async fn set_verification_method_rejects_jubjub_jwk() {
    let s = fresh_setup();
    let jubjub_jwk_vm = serde_json::json!({
        "id": "key-jubjub-via-jwk",
        "typ": 1,
        "publicKeyJwk": {
            // KeyType.EC = 0, CurveType.Jubjub = 2
            "kty": 0,
            "crv": 2,
            "x": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            "y": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        }
    });
    run_inspect(
        &s.bridge,
        "setVerificationMethod",
        &s.state_hex,
        &s.addr_hex,
        &s.sk_hex,
        serde_json::json!([jubjub_jwk_vm, MAP_INSERT]),
        serde_json::json!([]),
    )
    .await;
}
