//! Offline end-to-end test: Rust → Node harness runs the real
//! Compact `deactivate` circuit against a synthesised fresh-deploy
//! DID state, returns a SCALE-serialised `ProofPreimage`. Rust
//! decodes the preimage and asserts its shape.
//!
//! This is step 3 of the Rust ↔ JS bridge plan. No chain
//! involvement — exercises the JS pipeline (witnesses, compact
//! runtime, circuit execution, preimage serialisation) and the
//! Rust pipeline (state composition, preimage deserialisation)
//! without a network dependency.
//!
//! Run with:
//!   cargo test -p wallet-core --test js_inspect_deactivate -- --nocapture

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

#[tokio::test]
async fn deactivate_circuit_produces_decodable_preimage() {
    // Fresh random controller secret + matching deploy state. The
    // public key inside the state's `controllerPublicKey` slot is
    // `Wallet::controller_public_key_for(sk)`, so the circuit's
    // `assert publicKey(localSecretKey()) == controllerPublicKey`
    // holds with our witness.
    let controller_sk: [u8; 32] = rand::random();
    let timestamp_ms: u64 = 1_700_000_000_000; // arbitrary; fits u64
    let state_hex = wallet_core::testing_initial_deploy_state_hex(
        &controller_sk,
        timestamp_ms,
    )
    .expect("compose initial state");
    assert!(!state_hex.is_empty(), "state hex must be non-empty");

    // Address is zero for offline state — circuit only uses it for
    // context binding, not equality checks against `id`.
    let contract_address_hex = hex::encode([0u8; 32]);
    let controller_secret_hex = hex::encode(controller_sk);

    let bridge = NodeChildBridge::spawn(&NodeChildBridge::default_harness_path())
        .expect("spawn harness");

    let r: InspectResult = bridge
        .call(
            "inspectCircuit",
            serde_json::json!({
                "circuit": "deactivate",
                "contractStateHex": state_hex,
                "contractAddressHex": contract_address_hex,
                "controllerSecretHex": controller_secret_hex,
                "circuitArgs": [],
            }),
        )
        .await
        .expect("inspectCircuit deactivate");

    assert_eq!(r.circuit, "deactivate");
    assert!(
        r.public_transcript_len > 0,
        "deactivate must produce a non-empty public transcript (got {})",
        r.public_transcript_len,
    );
    // deactivate's only witness output is `localSecretKey` — exactly
    // one private-transcript output expected. Loose assertion in
    // case the runtime pads / adds fields.
    assert!(
        r.private_transcript_len >= 1,
        "deactivate must have at least one private transcript output (got {})",
        r.private_transcript_len,
    );
    let preimage_bytes = hex::decode(&r.preimage_hex).expect("preimage is valid hex");
    assert!(
        preimage_bytes.len() > 100,
        "preimage suspiciously small: {} bytes",
        preimage_bytes.len(),
    );

    // Round-trip the preimage through the Rust SCALE/tagged-deserialise
    // path. The Compact-runtime-side `proofDataIntoSerializedPreimage`
    // is the JS counterpart of what
    // `transient_crypto::proofs::ProofPreimage` consumes — if shapes
    // diverge, this errors here.
    let preimage: transient_crypto::proofs::ProofPreimage =
        serialize::tagged_deserialize(&preimage_bytes[..])
            .expect("tagged_deserialize ProofPreimage");

    // `deactivate()` takes no Compact arguments, so `inputs`
    // (circuit input transcript) is legitimately empty. The
    // public-transcript-inputs encode the state queries the
    // circuit ran (controller-pk check, active flag, increments,
    // updated timestamp) — those must be non-empty.
    assert!(
        !preimage.public_transcript_inputs.is_empty(),
        "decoded preimage has empty public_transcript_inputs"
    );
    assert_eq!(
        preimage.key_location.0,
        "midnight/did/deactivate",
        "key_location must label the circuit"
    );

    eprintln!(
        "[deactivate offline] preimage {} bytes, pub-transcript {} ops, priv {} outputs, {} ms",
        preimage_bytes.len(),
        r.public_transcript_len,
        r.private_transcript_len,
        r.elapsed_ms,
    );
}
