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
