//! Cross-implementation interop tests for the Rust port of
//! `midnight-did-jubjub-schnorr`.
//!
//! Drives the upstream JS reference (via the existing JSON-RPC
//! harness) and the in-tree
//! `secret_storage::jubjub_schnorr` module on the same inputs,
//! asserting they agree at two layers:
//!
//! 1. **Challenge hash** — `schnorrChallengeDigest` is the
//!    Poseidon-flavoured `transient_hash` over
//!    `[ann_x, ann_y, pk_x, pk_y, msg0..msg3]`. Both sides must
//!    produce the same `Fr` value (decimal-string compared).
//! 2. **Full verify** — a Rust-produced signature must be
//!    accepted by the on-chain `schnorrVerifyDigest` circuit
//!    when driven through the harness with the
//!    `getSchnorrReduction` witness.
//!
//! Run with:
//!   cargo test -p wallet-core --test jubjub_schnorr_interop -- --nocapture
//!
//! No network needed — purely offline, JS-vs-Rust comparison.

use transient_crypto::curve::{EmbeddedFr, EmbeddedGroupAffine, Fr};
use transient_crypto::hash::transient_hash;
use wallet_core::js_bridge::{JsBridge, NodeChildBridge};
use wallet_core::secret_storage::jubjub_schnorr::{
    derive_public_key_from_seed, encode_upstream, payload_to_digest, sign_payload_from_seed,
};

/// Tagged bigint envelope the harness expects. Wraps a decimal
/// string in `{ "$bigint": "<n>" }` so JSON can carry values
/// past 2^53.
fn bigint_dec(s: &str) -> serde_json::Value {
    serde_json::json!({ "$bigint": s })
}

/// Render an `Fr` as a decimal-string bigint envelope. Goes
/// through the `field_repr` path so the byte order matches
/// upstream's `bigintTo32Be` round-trip.
fn fr_to_bigint(f: &Fr) -> serde_json::Value {
    // `Fr` doesn't expose a direct decimal renderer, but it does
    // serialise to a leading-length LE byte form. The simplest
    // path is via `as_le_bytes` → base-10 string.
    let le = f.as_le_bytes();
    bigint_dec(&le_bytes_to_decimal(&le))
}

fn embedded_fr_to_bigint(f: &EmbeddedFr) -> serde_json::Value {
    let le = f.as_le_bytes();
    bigint_dec(&le_bytes_to_decimal(&le))
}

/// Convert a little-endian byte representation to a base-10
/// decimal string. Manual long-division to avoid pulling in
/// `num-bigint` just for this helper.
fn le_bytes_to_decimal(le: &[u8]) -> String {
    // Build the value as a big-endian vector of u32 limbs, then
    // repeatedly divide by 10 to extract decimal digits.
    let be: Vec<u8> = le.iter().rev().copied().collect();
    let mut limbs: Vec<u32> = Vec::with_capacity(be.len().div_ceil(4));
    for chunk in be.chunks(4) {
        let mut limb = 0u32;
        for &byte in chunk {
            limb = (limb << 8) | (byte as u32);
        }
        limbs.push(limb);
    }
    if limbs.iter().all(|&l| l == 0) {
        return "0".to_string();
    }
    let mut digits = Vec::new();
    while !limbs.iter().all(|&l| l == 0) {
        let mut rem: u64 = 0;
        for limb in limbs.iter_mut() {
            let acc = (rem << 32) | (*limb as u64);
            *limb = (acc / 10) as u32;
            rem = acc % 10;
        }
        digits.push((rem as u8) + b'0');
    }
    digits.reverse();
    String::from_utf8(digits).expect("ascii digits")
}

#[derive(serde::Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct ChallengeResult {
    challenge: String,
    elapsed_ms: i64,
}

#[derive(serde::Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct VerifyResult {
    verified: bool,
    error: Option<String>,
    elapsed_ms: i64,
}

/// Fixed seed → fixed (sk, pk) for reproducibility.
fn fixture_seed() -> [u8; 32] {
    let mut s = [0u8; 32];
    for (i, b) in s.iter_mut().enumerate() {
        *b = (i as u8).wrapping_mul(7);
    }
    s
}

#[tokio::test]
async fn challenge_matches_upstream_pure_circuit() {
    let bridge = NodeChildBridge::spawn(&NodeChildBridge::default_harness_path())
        .expect("spawn harness");

    let seed = fixture_seed();
    let pk = derive_public_key_from_seed(&seed);
    let payload = b"interop-test payload-A";
    let digest = payload_to_digest(payload);

    // Build a deterministic announcement so both sides have the
    // same input. Picking `G * 7` keeps the test reproducible.
    let announcement = EmbeddedGroupAffine::generator() * EmbeddedFr::from(7u64);

    // Pull each field-element input as a decimal bigint envelope.
    let req = serde_json::json!({
        "announcement": {
            "x": fr_to_bigint(&announcement.x().expect("announcement is not identity")),
            "y": fr_to_bigint(&announcement.y().expect("announcement is not identity")),
        },
        "publicKey": {
            "x": fr_to_bigint(&pk.x().expect("pk not identity")),
            "y": fr_to_bigint(&pk.y().expect("pk not identity")),
        },
        "digest": digest.iter().map(fr_to_bigint).collect::<Vec<_>>(),
    });

    let js: ChallengeResult = bridge
        .call("schnorrChallenge", req)
        .await
        .expect("schnorrChallenge rpc");

    // Rust-side: replicate the same `transient_hash([...])`
    // (no `% 2^248` reduction here — the harness returns the
    // full Fr value, before truncation).
    let elems = [
        announcement.x().expect("announcement.x not identity"),
        announcement.y().expect("announcement.y not identity"),
        pk.x().expect("pk.x not identity"),
        pk.y().expect("pk.y not identity"),
        digest[0],
        digest[1],
        digest[2],
        digest[3],
    ];
    let rust_full = transient_hash(&elems);
    let rust_dec = le_bytes_to_decimal(&rust_full.as_le_bytes());

    eprintln!("[challenge] js  = {}", js.challenge);
    eprintln!("[challenge] rust= {rust_dec}");
    eprintln!("[challenge] {} ms in JS", js.elapsed_ms);
    assert_eq!(
        rust_dec, js.challenge,
        "Rust transient_hash must match upstream pureCircuits.schnorrChallengeDigest",
    );
}

#[tokio::test]
async fn rust_signature_verifies_through_upstream_circuit() {
    let bridge = NodeChildBridge::spawn(&NodeChildBridge::default_harness_path())
        .expect("spawn harness");

    let seed = fixture_seed();
    let pk = derive_public_key_from_seed(&seed);
    let payload = b"interop-test payload-B";
    let digest = payload_to_digest(payload);
    let sig = sign_payload_from_seed(&seed, payload);

    // Sanity: Rust verify accepts its own signature first.
    assert!(
        wallet_core::secret_storage::jubjub_schnorr::verify_digest(&pk, &digest, &sig),
        "Rust verify must accept Rust signature",
    );

    let req = serde_json::json!({
        "announcement": {
            "x": fr_to_bigint(&sig.announcement.x().expect("ann not identity")),
            "y": fr_to_bigint(&sig.announcement.y().expect("ann not identity")),
        },
        "publicKey": {
            "x": fr_to_bigint(&pk.x().expect("pk not identity")),
            "y": fr_to_bigint(&pk.y().expect("pk not identity")),
        },
        "digest": digest.iter().map(fr_to_bigint).collect::<Vec<_>>(),
        "response": embedded_fr_to_bigint(&sig.response),
    });

    let js: VerifyResult = bridge
        .call("schnorrVerify", req)
        .await
        .expect("schnorrVerify rpc");
    eprintln!(
        "[verify] verified={} error={:?} ({} ms)",
        js.verified, js.error, js.elapsed_ms,
    );
    assert!(
        js.verified,
        "Upstream schnorrVerifyDigest must accept the Rust signature (error: {:?})",
        js.error,
    );
}

#[tokio::test]
async fn tampered_signature_rejected_by_upstream_circuit() {
    let bridge = NodeChildBridge::spawn(&NodeChildBridge::default_harness_path())
        .expect("spawn harness");

    let seed = fixture_seed();
    let pk = derive_public_key_from_seed(&seed);
    let payload = b"interop-test payload-C";
    let digest = payload_to_digest(payload);
    let mut sig = sign_payload_from_seed(&seed, payload);
    // Flip the response — the resulting signature should fail
    // both Rust and JS verification.
    sig.response = sig.response + EmbeddedFr::from(1u64);

    let req = serde_json::json!({
        "announcement": {
            "x": fr_to_bigint(&sig.announcement.x().expect("ann")),
            "y": fr_to_bigint(&sig.announcement.y().expect("ann")),
        },
        "publicKey": {
            "x": fr_to_bigint(&pk.x().expect("pk")),
            "y": fr_to_bigint(&pk.y().expect("pk")),
        },
        "digest": digest.iter().map(fr_to_bigint).collect::<Vec<_>>(),
        "response": embedded_fr_to_bigint(&sig.response),
    });

    let js: VerifyResult = bridge
        .call("schnorrVerify", req)
        .await
        .expect("schnorrVerify rpc");
    eprintln!("[tamper] verified={} error={:?}", js.verified, js.error);
    assert!(
        !js.verified,
        "Upstream circuit must reject a tampered Rust signature",
    );
}

#[derive(serde::Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct UpstreamDecodedPoint {
    x: String,
    y: String,
}

#[derive(serde::Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct UpstreamDecoded {
    announcement: UpstreamDecodedPoint,
    response: String,
}

#[derive(serde::Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct UpstreamVerifyResult {
    verified: bool,
    decoded: UpstreamDecoded,
    error: Option<String>,
    elapsed_ms: i64,
}

/// Bit-for-bit wire compat: encode the Rust signature in the
/// upstream 96-byte BE format, hand the hex blob to the harness
/// which decodes via upstream's own `decodeJubjubSignature`,
/// then runs the on-chain `schnorrVerifyDigest` circuit. If our
/// `encode_upstream` produces anything other than the bytes
/// upstream expects, this fails.
#[tokio::test]
async fn rust_upstream_encoded_signature_verifies_via_decode_jubjub_signature() {
    let bridge = NodeChildBridge::spawn(&NodeChildBridge::default_harness_path())
        .expect("spawn harness");

    let seed = fixture_seed();
    let pk = derive_public_key_from_seed(&seed);
    let payload = b"interop-test payload-upstream";
    let digest = payload_to_digest(payload);
    let sig = sign_payload_from_seed(&seed, payload);
    let sig_hex = hex::encode(encode_upstream(&sig));

    let req = serde_json::json!({
        "signatureHex": sig_hex,
        "publicKey": {
            "x": fr_to_bigint(&pk.x().expect("pk not identity")),
            "y": fr_to_bigint(&pk.y().expect("pk not identity")),
        },
        "digest": digest.iter().map(fr_to_bigint).collect::<Vec<_>>(),
    });

    let js: UpstreamVerifyResult = bridge
        .call("schnorrVerifyUpstreamEncoded", req)
        .await
        .expect("schnorrVerifyUpstreamEncoded rpc");
    eprintln!(
        "[upstream-encode] verified={} error={:?} decoded.response={} ({} ms)",
        js.verified, js.error, js.decoded.response, js.elapsed_ms,
    );

    // The decoder must reach the same (announcement, response)
    // we signed with. Convert Rust-side to decimal strings the
    // same way the harness does for comparison.
    let expected_ann_x = le_bytes_to_decimal(
        &sig.announcement.x().expect("ann.x").as_le_bytes(),
    );
    let expected_ann_y = le_bytes_to_decimal(
        &sig.announcement.y().expect("ann.y").as_le_bytes(),
    );
    let expected_response = le_bytes_to_decimal(&sig.response.as_le_bytes());
    assert_eq!(
        js.decoded.announcement.x, expected_ann_x,
        "upstream decode must recover the same announcement.x",
    );
    assert_eq!(
        js.decoded.announcement.y, expected_ann_y,
        "upstream decode must recover the same announcement.y",
    );
    assert_eq!(
        js.decoded.response, expected_response,
        "upstream decode must recover the same response scalar",
    );

    assert!(
        js.verified,
        "Upstream circuit must accept our upstream-encoded Rust signature (error: {:?})",
        js.error,
    );
}

