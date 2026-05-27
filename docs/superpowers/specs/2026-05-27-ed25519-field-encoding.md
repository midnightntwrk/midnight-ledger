# Ed25519-on-BLS-Field encoding for Midnight DID verification methods

> **🪦 SUPERSEDED 2026-05-27** by the upstream `Field → Bytes<32>` refactor
> of `PublicKeyJwk.x/y` in `midnight-did/packages/contract`.
> The wallet adopted the new lossless schema in commit `51ecff33`
> (`feat(did-auth): adopt the Bytes<32> PublicKeyJwk schema`); the
> Phase 1 cheat described below — 30-byte clamping the Ed25519 pubkey
> so it fits in BLS Fr — is no longer in the codebase. A 32-byte
> `Bytes<32>` slot holds the full compressed pubkey losslessly, and
> the wallet sends both `x` (the 32-byte pubkey) and `y` (32 zero
> bytes) directly without overflow concerns.
>
> Kept on disk for the historical rationale and the math reference
> on Ed25519 prime field (`p ≈ 5.79 × 10⁷⁶`) vs BLS Fr (`r ≈ 5.24 ×
> 10⁷⁶`), which still applies if anyone wonders why `Bytes<32>` was
> necessary instead of staying with `Field`.
>
> The current active blocker is the proving-key / circuit-IR
> input-count mismatch in `docs/superpowers/specs/2026-05-27-proving-key-input-mismatch.md`
> — completely orthogonal to this Ed25519 issue.

**Date:** 2026-05-27
**Scope:** `crate wallet-core` — `did/bootstrap.rs`, `did/contract.rs`, `vc_self_verify`, the JS harness (read-only).
**Severity:** Phase 1 demo blocker for the OID4VP / SIOPv2 holder-auth path. The on-chain Ed25519 public-key bytes are currently a **30-byte prefix** of the 32-byte compressed pubkey — sufficient for the chain (which doesn't crypto-verify the bytes) but insufficient for the issuer-mock to reconstruct a working JWK and verify a SIOPv2 `id_token` JWS.
**Status:** ~~Open. Cheat shipped on `dioxus-vc-demo` in commit `7f732d66` so the contract stops rejecting `addVerificationMethod` with a Field overflow.~~ Superseded (see banner above).

## TL;DR

The Midnight DID contract stores verification-method public keys as:

```compact
export struct PublicKeyJwk { kty: KeyType, crv: CurveType, x: Field, y: Field }
```

where `Field` is the **BLS12-381 scalar field** (Fr, modulus `r ≈ 5.24 × 10⁷⁶` ≈ 2²⁵⁴·⁸⁵).

An Ed25519 compressed public key is 32 bytes interpreted as little-endian into the prime field `F_p` with `p = 2²⁵⁵ − 19 ≈ 5.79 × 10⁷⁶ > r`. Loaded big-endian into a bigint as the upstream `decodeFieldElement` does, the resulting value frequently **overflows BLS Fr** and the contract rejects the `addVerificationMethod` call with:

```
type error: addVerificationMethod argument 1 …
  expected … x: Field, y: Field …
```

Phase 1 ships a **lossy workaround**: clamp the bytes used as `x` to the first 30 bytes (240 bits, comfortably under `r`). This unblocks the on-chain pipeline because the contract never decompresses or verifies the pubkey, but it breaks any off-chain verifier that tries to reconstruct the Ed25519 verifying key from the resolved DID document — including the issuer-mock's `verifyIdToken` path that consumes `vm.publicKeyJwk` via `jose.importJWK(...)`.

Need a permanent encoding scheme that fits in `Field` AND round-trips losslessly so off-chain verifiers can rebuild the Ed25519 public key from the resolved doc.

## Where this lives in the stack

| Layer | File | Function |
|---|---|---|
| Wallet-side encode | `mobile-bench/wallet-core/src/did/bootstrap.rs` | `build_verification_method_json` — builds the JSON payload `prepareUnprovenCallTx` consumes. **This is where the cheat lives.** |
| JS harness | `mobile-bench/wallet-core/tests/js-harness/harness.mjs` | `prepareUnprovenCallTx` — receives the JSON, runs `{$bigint: "0x…"}` revive, hands to contract via `createUnprovenCallTxFromInitialStates`. Read-only consumer; doesn't transform. |
| Contract schema | `~/iohk/midnight-identity-workspace/midnight-did/contract/dist/did.compact` | `struct VerificationMethod` + `struct PublicKeyJwk` + `enum KeyType` + `enum CurveType` — source of truth for the shape. |
| Wallet-side decode | `mobile-bench/wallet-core/src/did/contract.rs` | `ledger_to_domain` + `decode_verification_methods` — read the on-chain `Field` values back into `PublicKeyJwk` shape with `x`/`y` as base64url strings. |
| Upstream encode | `~/iohk/midnight-identity-workspace/midnight-did/packages/api/src/ledger-mappers.ts` | `publicKeyJwkToLedger` — TS-side equivalent of our `build_verification_method_json`. Uses `decodeFieldElement(jwk.x)` — same overflow risk; the upstream integration tests sidestep it by using single-byte placeholders (`x: "Kg"`). |
| Upstream decode | `~/iohk/midnight-identity-workspace/midnight-did/packages/did/src/ledger-to-domain.ts` | `LedgerToDomain.publicKeyJwk` — inverse via `encodeFieldElement`. Round-trips losslessly only if the encoded bigint was the canonical big-endian byte rep of the pubkey AND fit in Field. |
| Off-chain JWK consumer | `~/iohk/midnight-identity-workspace/midnight-identity-solution-examples/IssuerDIDIT-mock/src/services/oid4vpVerifier.ts` | `verifyIdToken` — calls `jose.importJWK(vm.publicKeyJwk, "EdDSA")` to reconstruct the Ed25519 verifying key. **This is what breaks today.** |

## The bug, step by step

### What we do today (the cheat)

`build_verification_method_json` in `mobile-bench/wallet-core/src/did/bootstrap.rs`:

```rust
fn to_bigint_hex(b64url: &str, clamp_to_field: bool) -> String {
    match URL_SAFE_NO_PAD.decode(b64url.as_bytes()) {
        Ok(bytes) if !bytes.is_empty() => {
            let limit = if clamp_to_field && bytes.len() > 30 {
                30   // ← LOSSY: drops the last 2 bytes for Ed25519
            } else {
                bytes.len()
            };
            format!("0x{}", hex::encode(&bytes[..limit]))
        }
        _ => "0x0".to_string(),
    }
}
let clamp = matches!(jwk.crv, MidnightCurve::Ed25519);
```

For an Ed25519 verifying key with `jwk.x = base64url(pub_32B)`:

- Decode to 32 bytes (the compressed Edwards-y representation, with the sign of x in the top bit of byte 31 — Ed25519 is little-endian on the wire).
- Take the **first 30 bytes** (big-endian view) as the Field element.
- Drop bytes 30 and 31.

This makes the on-chain stored value a deterministic 30-byte function of the original pubkey — but **not invertible** to the original 32 bytes.

### What downstream readers expect

`vm.publicKeyJwk` in the resolved DID document is the input to `jose.importJWK(jwk, "EdDSA")`. `jose` expects `x` to be the **exact 32-byte** little-endian compressed Edwards-y form, base64url-encoded. Anything else → either `jose` throws on import, or constructs a verifying key that fails every signature verification.

The chain itself never decompresses or verifies the pubkey (per `did.compact:185-189` — `addVerificationMethod` just inserts the struct into a map after asserting `typ == JsonWebKey` and the curve matches the kty), so the chain accepts the lossy encoding silently.

### What breaks end-to-end

The OID4VP authentication flow that the dioxus-wallet's Identity Centre exercises:

1. Wallet calls `bootstrap_did_with_keys` → DID + Ed25519 (authentication) + Jubjub (assertionMethod) all land on chain.
2. Wallet runs `oid4vp_client::run_authentication`:
   - Builds a SIOPv2 id_token JWS with `alg: EdDSA`, `kid: did:…#key-auth`, signed by the holder's REAL 32-byte Ed25519 key in the secret store.
   - POSTs the JWS to the issuer's `/authorize-response`.
3. Issuer-mock `oid4vpVerifier.verifyIdToken`:
   - Resolves the holder DID → gets the DID document → finds `verificationMethod` whose id matches the JWS `kid`.
   - **Calls `jose.importJWK(vm.publicKeyJwk, "EdDSA")`** on the JWK whose `x` is now the truncated 30-byte form.
   - Either: import throws ("invalid key length"), or constructs a key with garbage final bytes → verifyJWT fails → issuer returns 401.
4. The flow stalls before KYC can start.

The wallet-side `cargo test` integration test `bootstrap_against_standalone_succeeds_and_doc_is_complete` PASSES because it only asserts the DID document has the relations populated — it doesn't reconstruct the pubkey from the JWK.

## The math constraint

| Curve | Field modulus | Approx |
|---|---|---|
| Ed25519 prime field `F_p` | `p = 2²⁵⁵ − 19` | `5.79 × 10⁷⁶` |
| BLS12-381 scalar field `F_r` | `r = 0x73eda753…fffffff00000001` | `5.24 × 10⁷⁶` |

`p > r`. Worse, `p`'s top byte is `0x7F` while `r`'s top byte is `0x73`, so an Ed25519 coordinate has roughly an `(0x7F − 0x73) / 0x7F ≈ 4.7%` chance of being above `r`. A 32-byte compressed pubkey (which is `y_coord` with the sign of `x_coord` in the top bit) is even more likely to land in the unsafe range because the top bit is often set.

**Any encoding of "Ed25519 public key" → "single `Field` element" loses information whenever the encoded value would exceed `r`.** Three possibilities:

a) Accept the loss (current Phase 1 cheat).
b) Encode the pubkey across **two `Field` elements** so the joint range is `r² ≈ 2.75 × 10¹⁵³` — comfortably larger than the 2²⁵⁶ space of a 32-byte pubkey.
c) Change the contract schema (`Bytes<32>` instead of `Field` for at least the `x` slot of Ed25519 VMs).

(b) and (c) are the only lossless paths. (b) is wallet-side-only; (c) requires upstream agreement.

## How to reproduce the bug today

Prerequisites: standalone Midnight env running (`docker compose` on `:9944` / `:8088` / `:6300`), wallet-core branch `dioxus-vc-demo` at commit `7f732d66` or later.

```bash
cd /Users/ysh/iohk/midnight-ledger/.claude/worktrees/thirsty-lovelace-092f50

# Confirm the lossy clamp is in place
grep -n "let limit = if clamp_to_field" mobile-bench/wallet-core/src/did/bootstrap.rs
# Expected output (note "limit > 30 { 30 }"):
#   let limit = if clamp_to_field && bytes.len() > 30 { 30 } else { bytes.len() };

# Run the live integration test — this should PASS because the chain-only
# round-trip works fine; the cheat is on-chain-equivalent.
RUST_MIN_STACK=16777216 STANDALONE_RUN=1 cargo test \
    -p wallet-core --features test-support \
    --test did_bootstrap_standalone bootstrap_against_standalone \
    -- --ignored --nocapture

# Now add this test to demonstrate the OFF-CHAIN bug. Create
# tests/ed25519_round_trip_fails_off_chain.rs:
cat > mobile-bench/wallet-core/tests/ed25519_round_trip_fails_off_chain.rs <<'EOF'
//! Demonstrates that the current `build_verification_method_json` +
//! `ledger_to_domain` round-trip is LOSSY for Ed25519: the bytes
//! `jose.importJWK` would receive after resolve are NOT the same
//! bytes the wallet originally registered, so off-chain JWT
//! verification using the resolved JWK is doomed.

#![cfg(any(test, feature = "test-support"))]

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use wallet_core::secret_storage::{InMemorySecretStore, SecretStorage};
use wallet_core::{bootstrap_did_with_keys, derive_keys, test_support::stub_wallet};

#[tokio::test]
async fn ed25519_pubkey_does_not_round_trip_through_resolved_doc() {
    let seed = [0xa1u8; 32];
    let wallet = stub_wallet();
    let mut store = InMemorySecretStore::default();

    let out = bootstrap_did_with_keys(&wallet, &mut store, &seed)
        .await
        .expect("stub bootstrap");

    // What the SECRET STORE has: the real 32-byte compressed pubkey.
    let original_jwk = store
        .get_public_key(out.ed25519_ref.uuid())
        .await
        .expect("ed25519 jwk");
    let original_bytes = URL_SAFE_NO_PAD
        .decode(original_jwk.x.as_bytes())
        .expect("b64");
    assert_eq!(original_bytes.len(), 32, "Ed25519 compressed pubkey is 32 bytes");

    // What the RESOLVED DOCUMENT has after on-chain round-trip.
    let doc = wallet
        .resolve_did(&out.did.to_did_string())
        .await
        .expect("resolve");
    let resolved_vm = doc
        .verification_method
        .iter()
        .find(|vm| vm.id.ends_with("#key-auth"))
        .expect("key-auth VM");
    let resolved_bytes = URL_SAFE_NO_PAD
        .decode(resolved_vm.public_key_jwk.x.as_bytes())
        .expect("b64");

    // THIS IS THE BUG: the resolved bytes don't match the original
    // and aren't even 32 bytes long.
    assert_eq!(
        resolved_bytes.len(),
        32,
        "resolved Ed25519 pubkey should still be 32 bytes; got {} bytes",
        resolved_bytes.len()
    );
    assert_eq!(
        resolved_bytes, original_bytes,
        "resolved Ed25519 pubkey should equal the original; the lossy \
         clamp in build_verification_method_json drops the last 2 bytes"
    );
}
EOF

cargo test -p wallet-core --features test-support --test ed25519_round_trip_fails_off_chain 2>&1 | tail -15
# Expected: the test FAILS with one of:
#   "resolved Ed25519 pubkey should still be 32 bytes; got 30 bytes"
#   "resolved Ed25519 pubkey should equal the original; the lossy clamp..."
```

Once this test exists and **fails**, the next step is the fix. After landing the fix, the same test should pass.

## Fix options

### Option A (recommended) — split-encode across `x` + `y`

Wallet-side change only. Encode the 32-byte compressed Ed25519 pubkey as:

- `x = first 16 bytes (big-endian bigint)` — fits in `r` (16 bytes = 128 bits).
- `y = last 16 bytes (big-endian bigint)` — same.

On resolve, `ledger_to_domain` re-concatenates `x_be_16 || y_be_16` and base64url-encodes the result back into the `vm.publicKeyJwk.x` slot. Issuer-mock then sees a standard 32-byte JWK and `jose.importJWK` works.

**Pros**

- Lossless for any 32-byte pubkey.
- No contract change; no upstream coordination.
- The Phase 1 `vc_self_verify` test fixture (Jubjub-signed VCs) is unaffected — only Ed25519 storage shape changes.

**Cons**

- Departs from the upstream's `publicKeyJwkToLedger` convention (which puts the full big-endian bigint in `x` and `0` in `y`). The wallet's resolved doc would diverge from a doc resolved by the upstream's `MidnightDIDResolver`. Two paths for fixing the divergence:
  - **Wallet-only**: keep the upstream resolver's behaviour as the canonical "raw on-chain" view; do the split/unsplit only inside our wallet-core. Issuer-mock would still see the broken upstream view unless it adopts our recombination logic.
  - **Two-sided**: propose the split-encode convention to the midnight-did maintainers as the documented Ed25519 encoding for `PublicKeyJwk`. Upstream `publicKeyJwkToLedger` adopts `if (crv == Ed25519) splitInto16x16(jwk.x) else decodeFieldElement(jwk.x), decodeFieldElement(jwk.y || "0")` and `LedgerToDomain.publicKeyJwk` mirrors. Then everyone (wallet, issuer-mock, midnight-did-resolver) round-trips correctly.

The two-sided path is the long-term right answer; the wallet-only path is fine for Phase 1 demo.

### Option B — change the contract schema

Add a `Bytes<32>` variant or a new `keyMaterial` field to `struct PublicKeyJwk` that holds the raw bytes for curves where `Field` is the wrong primitive. Touches `did.compact`, breaks every deployed DID document on PreProd / Mainnet, requires recompiling verifier keys, etc. **Not recommended for Phase 1.** Long-term it's the cleanest fix.

### Option C — switch Ed25519 → Jubjub for authentication

Use Jubjub for both `authentication` and `assertionMethod`. Jubjub coordinates are Fr by construction, no overflow. But SIOPv2 / JOSE don't know about Jubjub — there's no `alg: "JubjubSchnorr"` in the JWS registry. Issuer-mock would need its own JWS verifier instead of `jose`. **Phase 2 spec discussion.**

## Recommended fix — implementation sketch

### Step 1: Update `build_verification_method_json` to split-encode

`mobile-bench/wallet-core/src/did/bootstrap.rs`:

```rust
use crate::secret_storage::{MidnightCurve, MidnightKeyType};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;

fn build_verification_method_json(
    did: &DidId,
    fragment: &str,
    jwk: &PublicJwk,
) -> serde_json::Value {
    // ... (kty_tag, crv_tag mapping as today) ...

    let (x_hex, y_hex) = match jwk.crv {
        // Ed25519 pubkey is 32 bytes; split across (x, y) so each
        // half (16 bytes / 128 bits) fits comfortably in BLS Fr.
        // The decode path in did/contract.rs::ledger_to_domain
        // reverses this by concatenating x_be_16 || y_be_16.
        MidnightCurve::Ed25519 => {
            let bytes = URL_SAFE_NO_PAD
                .decode(jwk.x.as_bytes())
                .unwrap_or_default();
            // Pad/truncate to exactly 32 bytes, then split 16/16.
            let mut buf = [0u8; 32];
            let n = bytes.len().min(32);
            buf[..n].copy_from_slice(&bytes[..n]);
            (
                format!("0x{}", hex::encode(&buf[..16])),
                format!("0x{}", hex::encode(&buf[16..])),
            )
        }
        // Jubjub / P-256: coordinates are already Fr-fitting.
        MidnightCurve::Jubjub | MidnightCurve::P256 => {
            let x = URL_SAFE_NO_PAD.decode(jwk.x.as_bytes()).unwrap_or_default();
            let y_b64 = jwk.y.as_deref().unwrap_or("");
            let y = URL_SAFE_NO_PAD.decode(y_b64.as_bytes()).unwrap_or_default();
            (
                format!("0x{}", hex::encode(&x)),
                if y.is_empty() {
                    "0x0".to_string()
                } else {
                    format!("0x{}", hex::encode(&y))
                },
            )
        }
    };

    serde_json::json!({
        "id": format!("#{}", fragment),
        "typ": 1,
        "publicKeyJwk": {
            "kty": kty_tag,
            "crv": crv_tag,
            "x": { "$bigint": x_hex },
            "y": { "$bigint": y_hex },
        }
    })
}
```

### Step 2: Update `ledger_to_domain` to re-concatenate for Ed25519

`mobile-bench/wallet-core/src/did/contract.rs::decode_verification_methods` currently builds `PublicKeyJwk.x = base64url(decoded_x_bytes)`. After the schema change, for Ed25519 it must:

```rust
match (kty, crv) {
    (MidnightKeyType::OKP, MidnightCurve::Ed25519) => {
        // x_bytes + y_bytes were stored as the high-16 and low-16
        // halves of the 32-byte compressed pubkey. Recombine and
        // present as a standard JWK (x: base64url(32 bytes), no y).
        let mut full = [0u8; 32];
        let x_offset = 16usize.saturating_sub(x.len());
        full[x_offset..16].copy_from_slice(&x[x.len().saturating_sub(16)..]);
        let y_offset = 32usize.saturating_sub(y.len()).max(16);
        full[y_offset..].copy_from_slice(&y[y.len().saturating_sub(16)..]);
        PublicKeyJwk {
            kty,
            crv,
            x: URL_SAFE_NO_PAD.encode(full),
            y: None,
        }
    }
    _ => PublicKeyJwk {
        kty,
        crv,
        x: URL_SAFE_NO_PAD.encode(&x),
        y: Some(URL_SAFE_NO_PAD.encode(&y)),
    },
}
```

(Watch the padding: bigints encoded with leading-zero stripping may come back as fewer than 16 bytes; left-pad with zeros to 16 bytes per half before concatenation.)

### Step 3: Drop the clamp

Remove the `clamp_to_field` parameter from `to_bigint_hex` in `build_verification_method_json` — once split-encode lands, no curve needs clamping.

### Step 4: Verification

The reproduction test in this doc (`ed25519_round_trip_fails_off_chain.rs`) goes from RED to GREEN. Also add an explicit round-trip unit test:

```rust
#[test]
fn split_encode_round_trips_arbitrary_32_byte_keys() {
    for trial in 0u8..32 {
        let pub_bytes: [u8; 32] = std::array::from_fn(|i| (i as u8).wrapping_add(trial));
        let jwk = PublicJwk {
            kty: MidnightKeyType::OKP,
            crv: MidnightCurve::Ed25519,
            x: URL_SAFE_NO_PAD.encode(pub_bytes),
            y: None,
        };
        // Round-trip: encode → on-chain Field shape → decode.
        let json = build_verification_method_json(&dummy_did(), "key-auth", &jwk);
        let (x_field, y_field) = extract_fields_from_json(&json);
        let recovered = ledger_pair_to_jwk_ed25519(x_field, y_field);
        let recovered_bytes = URL_SAFE_NO_PAD.decode(recovered.x.as_bytes()).unwrap();
        assert_eq!(recovered_bytes, pub_bytes,
            "trial {trial}: split-encode must round-trip");
    }
}
```

### Step 5: End-to-end smoke against the live env

Re-run `bootstrap_against_standalone_succeeds_and_doc_is_complete` — should pass in the same ~127 s.

Then bring up the issuer-mock + invoke the OID4VP flow from a fresh wallet (UI or BDD harness). The id_token POST should land with `{ status: "authenticated" }` instead of 401 nonce-mismatch / JWT-verify-failed.

## Acceptance criteria

- [ ] `ed25519_round_trip_fails_off_chain.rs` (the reproduction test) passes.
- [ ] `split_encode_round_trips_arbitrary_32_byte_keys` (the new unit test) passes for 32+ trials.
- [ ] Full lib test suite still green (`cargo test -p wallet-core --features test-support --lib` → 208+/208+ passing).
- [ ] `bootstrap_against_standalone_succeeds_and_doc_is_complete` still passes against a live standalone env in ≤ 180 s.
- [ ] Live end-to-end OID4VP authentication completes successfully against the running IssuerDIDIT-mock (`pnpm dev` on `:3001`). Captured in `IssuerDIDIT-mock/e2e/features/issuance-happy-path.feature` once the underlying compact-js / chain blockers also clear.
- [ ] The wallet's iOS / Android `Identity` tab can drive the OID4VP card (Section 2 of `IdentityCentrePanel`) through to `{ session_id, status: "authenticated" }`.

## Files to touch

| Path | Change |
|---|---|
| `mobile-bench/wallet-core/src/did/bootstrap.rs` | Replace the `clamp_to_field` branch in `build_verification_method_json` with the split-encode logic above. Drop the `clamp_to_field` parameter on `to_bigint_hex`. Update the giant doc comment to describe the lossless split-encode convention. |
| `mobile-bench/wallet-core/src/did/contract.rs` | Update `decode_verification_methods` to recombine the split-encoded halves for Ed25519 VMs. Keep Jubjub / P-256 paths unchanged. |
| `mobile-bench/wallet-core/tests/ed25519_round_trip_fails_off_chain.rs` | Add per Step 1 of "How to reproduce" above. |
| `mobile-bench/wallet-core/src/did/bootstrap.rs` (tests block) | Add `split_encode_round_trips_arbitrary_32_byte_keys` unit test. |
| `docs/superpowers/plans/2026-05-25-identity-centre-phase-1-PROGRESS.md` | Move the Ed25519 cheat note from "Known limitation" to "Resolved (commit ...)". |
| Optional, two-sided: `~/iohk/midnight-identity-workspace/midnight-did/packages/api/src/ledger-mappers.ts` + `packages/did/src/ledger-to-domain.ts` | Mirror the wallet-side split-encode in upstream's `publicKeyJwkToLedger` / `LedgerToDomain.publicKeyJwk`. Coordinate with midnight-did maintainers. |

## References

| Source | What |
|---|---|
| `~/iohk/midnight-identity-workspace/midnight-did/contract/dist/did.compact` lines 100–120 | `struct PublicKeyJwk { kty, crv, x: Field, y: Field }` — schema source of truth. |
| `~/iohk/midnight-identity-workspace/midnight-did/packages/domain/src/crypto-codecs.ts` | `encodeFieldElement` / `decodeFieldElement` — the canonical base64url ↔ big-endian bigint codec, no modular reduction. Defines the on-chain `Field` value range. |
| `~/iohk/midnight-identity-workspace/midnight-did/packages/api/src/ledger-mappers.ts` lines 68–78 | `publicKeyJwkToLedger` — TS-side encoder. Same overflow risk as our Rust code; integration tests in `packages/api/src/test/did.api.test.ts:215` (and others) avoid it by using single-byte placeholders. |
| `~/iohk/midnight-identity-workspace/midnight-did/packages/did/src/ledger-to-domain.ts` lines 80–110 | `LedgerToDomain.publicKeyJwk` — TS-side decoder. The natural inverse: `encodeFieldElement(field)` → base64url. |
| `https://datatracker.ietf.org/doc/html/rfc8032#section-5.1.3` | RFC 8032 §5.1.3 — Ed25519 point encoding (32 bytes, little-endian, x's sign in top bit of byte 31). |
| `https://datatracker.ietf.org/doc/html/rfc8037#section-2` | RFC 8037 — JWK format for OKP keys. `x` is the public-key octet string base64url-encoded. |
| BLS12-381 scalar field modulus | `r = 0x73eda753299d7d483339d80809a1d80553bda402fffe5bfeffffffff00000001` (from `pairing-bls12381` and the Compact runtime). |
| Reproducer commit on `dioxus-vc-demo` | `7f732d66` — `fix(did-auth): land the live bootstrap_did_with_keys pipeline`. Lines 195–230 of `mobile-bench/wallet-core/src/did/bootstrap.rs` show the current cheat. |

## Out of scope for this fix

- Real Ed25519 sign/verify against an external party — currently the wallet-side `did_auth::sign_for_authentication` uses the holder's secret-store key correctly; the issue is exclusively on the verifier side which receives the on-chain JWK.
- Jubjub / P-256 encoding — already correct; coordinates are Fr by construction.
- The `compact-js@2.5.0` `NodeChildBridge` `TypeError` — already fixed by the symlink repair in commit `7f732d66`'s body (note: out-of-tree, in `midnight-did/node_modules/@midnight-ntwrk/compact-js`).
- Self-verify of VCs — VCs are Jubjub-signed; this Ed25519 issue does NOT affect `vc_self_verify`.

## Suggested PR title

`fix(did-auth): split-encode Ed25519 pubkey across (x, y) Field elements`
