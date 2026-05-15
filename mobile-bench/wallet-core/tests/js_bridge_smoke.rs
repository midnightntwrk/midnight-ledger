//! Smoke test for the Node-backed [`wallet_core::js_bridge::JsBridge`].
//!
//! Verifies the JSON-RPC transport without involving the Compact
//! runtime, contract layer, or chain. This is step 1 of the
//! Rust ↔ JS bridge plan — once green, step 2 layers in
//! `bridgeInspectCircuit` and the real DID circuit work.
//!
//! Run with:
//!   cargo test -p wallet-core --test js_bridge_smoke -- --nocapture

use wallet_core::js_bridge::{JsBridge, NodeChildBridge};

#[derive(serde::Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct PingResult {
    ok: bool,
    version: String,
}

#[derive(serde::Deserialize, Debug)]
struct EchoResult {
    echoed: Option<String>,
}

#[tokio::test]
async fn ping_round_trip() {
    let bridge = NodeChildBridge::spawn(&NodeChildBridge::default_harness_path())
        .expect("spawn harness");
    let r: PingResult = bridge
        .call("ping", serde_json::Value::Null)
        .await
        .expect("ping call");
    assert!(r.ok);
    assert_eq!(r.version, "0.1.0");
}

#[tokio::test]
async fn echo_round_trip() {
    let bridge = NodeChildBridge::spawn(&NodeChildBridge::default_harness_path())
        .expect("spawn harness");
    let r: EchoResult = bridge
        .call("echo", serde_json::json!({ "message": "hello bridge" }))
        .await
        .expect("echo call");
    assert_eq!(r.echoed.as_deref(), Some("hello bridge"));
}

#[tokio::test]
async fn unknown_method_returns_js_error() {
    let bridge = NodeChildBridge::spawn(&NodeChildBridge::default_harness_path())
        .expect("spawn harness");
    let err = bridge
        .call::<_, serde_json::Value>(
            "doesNotExist",
            serde_json::Value::Null,
        )
        .await
        .expect_err("unknown method must error");
    let msg = err.to_string();
    assert!(
        msg.contains("unknown method"),
        "expected JS-side unknown-method error, got: {msg}"
    );
}

#[derive(serde::Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct ContractLayerInfo {
    contract_exports: Vec<String>,
    compact_runtime_exports: Vec<String>,
    circuit_names: Vec<String>,
    has_proof_data_into_serialized_preimage: bool,
    has_create_circuit_context: bool,
}

#[tokio::test]
async fn contract_layer_info_lists_all_11_did_circuits() {
    // The harness loads `@midnight-ntwrk/midnight-did-contract` +
    // `@midnight-ntwrk/compact-runtime` from the vendored copies
    // under `dioxus-wallet/assets/web/pkg/`. Asserts that:
    // 1. The vendor + npm wiring works (no module-resolution
    //    failures despite the symlinked file: deps).
    // 2. All 11 DID circuits we expect to call are present —
    //    catches an upstream rename or version drift early.
    // 3. The two compact-runtime helpers our circuit-call pipeline
    //    will use (`createCircuitContext`,
    //    `proofDataIntoSerializedPreimage`) exist.
    let bridge = NodeChildBridge::spawn(&NodeChildBridge::default_harness_path())
        .expect("spawn harness");
    let info: ContractLayerInfo = bridge
        .call("contractLayerInfo", serde_json::Value::Null)
        .await
        .expect("contractLayerInfo call");

    assert!(
        info.contract_exports.contains(&"DIDContract".to_string()),
        "contract package missing DIDContract export: {:?}",
        info.contract_exports,
    );

    // Two key compact-runtime helpers our ContractCall pipeline
    // depends on. If either is renamed upstream the test fails
    // here, not deep inside circuit execution.
    assert!(
        info.has_create_circuit_context,
        "compact-runtime missing createCircuitContext"
    );
    assert!(
        info.has_proof_data_into_serialized_preimage,
        "compact-runtime missing proofDataIntoSerializedPreimage"
    );

    // All 11 DID circuits, in any order.
    let expected = [
        "addAlsoKnownAs",
        "removeAlsoKnownAs",
        "addVerificationMethod",
        "updateVerificationMethod",
        "removeVerificationMethod",
        "addVerificationMethodRelation",
        "removeVerificationMethodRelation",
        "addService",
        "updateService",
        "removeService",
        "deactivate",
    ];
    for name in expected {
        assert!(
            info.circuit_names.iter().any(|n| n == name),
            "missing DID circuit '{name}' — present: {:?}",
            info.circuit_names,
        );
    }
}

#[tokio::test]
async fn multiple_sequential_calls_share_one_bridge() {
    // The harness is single-threaded; the bridge serialises calls.
    // Verify that holding a single bridge over multiple round-trips
    // works (i.e. the next_id counter and the read_line cursor stay
    // aligned).
    let bridge = NodeChildBridge::spawn(&NodeChildBridge::default_harness_path())
        .expect("spawn harness");
    for i in 0..5 {
        let r: EchoResult = bridge
            .call("echo", serde_json::json!({ "message": format!("msg-{i}") }))
            .await
            .unwrap_or_else(|e| panic!("echo {i}: {e}"));
        assert_eq!(r.echoed.as_deref(), Some(format!("msg-{i}").as_str()));
    }
}
