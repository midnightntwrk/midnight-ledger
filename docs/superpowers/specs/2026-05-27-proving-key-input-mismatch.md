# Proving-key / circuit-IR input-count mismatch — `addVerificationMethod`

**Date:** 2026-05-27
**Scope:** `~/iohk/midnight-identity-workspace/midnight-did/packages/contract/dist/managed/did/` — the bundled artifacts (compiled contract JS + halo2 `.prover` / `.verifier` keys).
**Severity:** Phase 1 demo blocker (downstream of the lossless-encoding fix; the wallet now passes the type check but can't generate a valid proof).
**Status:** Open. Out of wallet scope — requires regenerating the proving key against the current circuit IR. Supersedes `2026-05-27-ed25519-field-encoding.md`, which the `Field → Bytes<32>` refactor made obsolete.

## TL;DR

After the upstream switch from `x: Field, y: Field` to `x: Bytes<32>, y: Bytes<32>` in `PublicKeyJwk`, calling `addVerificationMethod` against a live standalone Midnight env fails inside the wallet's halo2 prover:

```
prove: prove: Expected 6 inputs, received 8
```

The compiled contract module (`packages/contract/dist/managed/did/contract/index.js`) emits **8 public inputs** for the circuit (4 protocol slots + 2 inputs per `Bytes<32>` split — high + low halves), while the bundled proving key (`addVerificationMethod.prover`) expects **6** (same slots but 1 input per `Bytes<32>`). The `.prover`, `.verifier`, and `contract/index.js` files all share the build timestamp `May 27 22:28:19`, so they were produced in the same build run — but one of the two compilation stages (Compact → IR or IR → halo2 keys) is using a stale lowering for `Bytes<32>`.

This is invisible to the wallet — there's nothing wallet-side that could bridge a 6/8 input mismatch. Need a regenerated proving key (or a contract module emitting the matching input count) from the upstream build pipeline.

## Where this lives

| Layer | File | What it does |
|---|---|---|
| Compact source | `~/iohk/midnight-identity-workspace/midnight-did/contract/dist/did.compact` (+ `packages/contract/src/did.compact`) | Declares `circuit addVerificationMethod(verificationMethod: VerificationMethod)`. The source still says `x: Field, y: Field` in the `PublicKeyJwk` struct — this hasn't been synced with the compiled artifacts. |
| Compiled contract module | `~/iohk/midnight-identity-workspace/midnight-did/packages/contract/dist/managed/did/contract/index.js` | The runtime-loaded module the harness consumes via `import("@midnight-ntwrk/midnight-did-contract")`. **Already updated to `Bytes<32>`** (the type-check error message embeds the schema string verbatim). Generates the witness input vector at call time. |
| Halo2 proving key | `~/iohk/midnight-identity-workspace/midnight-did/packages/contract/dist/managed/did/keys/addVerificationMethod.prover` (~2.8 MB) | Compiled circuit + KZG SRS commitments. **Mismatch with `contract/index.js`**: expects 6 public inputs where index.js produces 8. |
| Halo2 verifier key | `~/iohk/midnight-identity-workspace/midnight-did/packages/contract/dist/managed/did/keys/addVerificationMethod.verifier` (~2 KB) | Public verifier-side params. Same compilation as `.prover`; same 6-input layout. |
| Wallet caller | `mobile-bench/wallet-core/src/did/bootstrap.rs::build_verification_method_json` | Builds the JSON arg that `prepareUnprovenCallTx` consumes. Now sends the canonical `Bytes<32>` shape (commit `51ecff33`). |
| Harness wrapper | `mobile-bench/wallet-core/tests/js-harness/harness.mjs::prepareUnprovenCallTx` | Loads `compactJs.CompiledContract.make(...)`, calls `jsContracts.createUnprovenCallTxFromInitialStates(...)`. No transform between wallet JSON and contract — passes through. |

## How to reproduce

Pre-requisites: standalone Midnight env up, wallet repo on `dioxus-vc-demo` at commit `51ecff33` or later, midnight-did artifacts dated `2026-05-27 22:28:19` or later (i.e. the post-`Bytes<32>`-refactor build).

```bash
cd /Users/ysh/iohk/midnight-ledger/.claude/worktrees/thirsty-lovelace-092f50

# Confirm artifact mtimes
stat -f "%Sm %z %N" \
  ~/iohk/midnight-identity-workspace/midnight-did/packages/contract/dist/managed/did/contract/index.js \
  ~/iohk/midnight-identity-workspace/midnight-did/packages/contract/dist/managed/did/keys/addVerificationMethod.{prover,verifier}
# All three should report the same `2026-05-27 22:28:19` mtime —
# they came from the same build but disagree on input layout.

# Run the live integration test
RUST_MIN_STACK=16777216 STANDALONE_RUN=1 cargo test \
    -p wallet-core --features test-support \
    --test did_bootstrap_standalone bootstrap_against_standalone \
    -- --ignored --nocapture 2>&1 | tail -10
```

Expected output (verbatim, with the wallet now past type-check and into prove()):

```
[prepareUnprovenCallTx] partition[0/guaranteed]: program=...
[prepareUnprovenCallTx] partition[1/fallible]:  null

thread 'bootstrap_against_standalone_succeeds_and_doc_is_complete' (...) panicked at
mobile-bench/wallet-core/tests/did_bootstrap_standalone.rs:98:10:
bootstrap: AttachAuthn("create_did failed: prove: prove: Expected 6 inputs, received 8")
```

The "8 received" is the inputs the contract module generated; the "6 expected" is what the proving key was compiled to ingest.

## Why 6 vs 8 — the input layout

For `circuit addVerificationMethod(verificationMethod: VerificationMethod): []` where:

```compact
struct VerificationMethod {
  id: Opaque<"string">,
  typ: VerificationMethodType,
  publicKeyJwk: PublicKeyJwk,
};
struct PublicKeyJwk {
  kty: KeyType, crv: CurveType, x: Bytes<32>, y: Bytes<32>
};
```

The public inputs are the fields the circuit `disclose()`s. Counting:

| Field | Old (`Field`) layout | New (`Bytes<32>`) layout |
|---|---|---|
| `id` (opaque string) | 1 (hash digest) | 1 (hash digest) |
| `typ` (enum tag) | 1 | 1 |
| `kty` (enum tag) | 1 | 1 |
| `crv` (enum tag) | 1 | 1 |
| `x` | 1 (Field) | 2 (high 16 bytes as Field + low 16 bytes as Field) |
| `y` | 1 (Field) | 2 (high 16 bytes + low 16 bytes) |
| **Total** | **6** | **8** |

The proving key was generated against the **old 6-input** layout. The runtime contract module was updated to emit the **new 8-input** layout. They're out of sync by exactly 2 inputs — the extra Field slot for each `Bytes<32>` half.

## Fix paths

### Option 1 — Regenerate the proving key against the new IR (upstream task)

This is the right fix. The midnight-did build pipeline needs:

1. Recompile the `.compact` source — make sure it's updated to declare `x: Bytes<32>, y: Bytes<32>` (the `did.compact` text in `dist/` and `src/` still says `x: Field, y: Field`; the compiled `dist/managed/did/contract/index.js` already has the new shape, so somewhere between source-text and compiled-JS the schema got changed but the source file wasn't updated).
2. Re-run the IR lowering so the circuit witness layout matches what `contract/index.js` emits.
3. Re-run the trusted-setup ceremony (or key generation if it's a transparent SRS).
4. Re-bundle the new `.prover` / `.verifier` keys.

Validation: the integration test in this repo (`mobile-bench/wallet-core/tests/did_bootstrap_standalone.rs::bootstrap_against_standalone_succeeds_and_doc_is_complete`) should advance past the prove step and land all 8 expected on-chain assertions (DID created, both VMs attached, both relations populated). Run-time was ~127 s before this regression; should be similar.

### Option 2 — Patch `contract/index.js` to emit 6 inputs

Not recommended. The wallet only consumes the contract module via `import`; whatever input layout the runtime emits, the wallet ships through unchanged. So the fix is on the upstream side regardless. But if the keys can't be regenerated quickly, an emergency patch to `contract/index.js` to collapse each `Bytes<32>` into a single Field (e.g. via `Poseidon(x_be_32_bytes)`) would unblock smoke tests at the cost of round-trip recoverability — the same trade-off the now-obsolete `Field` schema had. Don't do this; chase Option 1.

### Option 3 — Revert to `Field` schema while keys are being regenerated

Roll the contract back to the pre-22:28 build. Loses the Ed25519 lossless-encoding win, but unblocks the existing 6-input proving key. Useful as a stopgap if Option 1 is more than a day out.

## Acceptance criteria

- [ ] `cargo test -p wallet-core --features test-support --lib` still 208+/208+ passing (no wallet changes required for this fix).
- [ ] `bootstrap_against_standalone_succeeds_and_doc_is_complete` advances past the prove step and reaches the on-chain assertions, completing in ≤ 180 s.
- [ ] The Ed25519 pubkey stored on chain round-trips losslessly through `Wallet::resolve_did` (32 bytes in, 32 bytes out). Use the `ed25519_round_trip_fails_off_chain` test pattern from `docs/superpowers/specs/2026-05-27-ed25519-field-encoding.md` as a regression — once the prove step is fixed, that test (which I never wrote) becomes the right shape to add.
- [ ] No new warnings under `#![deny(warnings)]` in the wallet build.

## Suspected root cause

The `contract/index.js` codegen was updated to spread `Bytes<32>` across two Field public inputs (presumably to keep each input ≤ Fr bounds — a single 32-byte BE value can exceed Fr modulus, same problem the wallet hit with the old `Field` schema). But the `compactc` → halo2 IR step that produced the `.prover`/`.verifier` was still using a one-input-per-Bytes<32> lowering — possibly because the IR lowering reads the `.compact` source file (which still says `x: Field, y: Field`), not the post-codegen view.

Quick check that would confirm or rule this out:

```bash
diff <(grep -c "x: Field" ~/iohk/midnight-identity-workspace/midnight-did/packages/contract/dist/managed/did/contract/index.js) \
     <(grep -c "x: Bytes<32>" ~/iohk/midnight-identity-workspace/midnight-did/packages/contract/dist/managed/did/contract/index.js)
```

If `index.js` only mentions `Bytes<32>` in error strings (not in actual codegen), the upstream may have updated the runtime type-check string but not the witness assembly. If `index.js` has both old and new schemas, the regeneration step was incomplete.

## References

- Wallet's adoption commit: `51ecff33` on `dioxus-vc-demo` — `feat(did-auth): adopt the Bytes<32> PublicKeyJwk schema`.
- Wallet's previous demo-blocker spec (now obsolete): `docs/superpowers/specs/2026-05-27-ed25519-field-encoding.md`.
- Live error: see "How to reproduce" above.
- Contract artifacts under investigation:
  - `~/iohk/midnight-identity-workspace/midnight-did/packages/contract/dist/managed/did/contract/index.js`
  - `~/iohk/midnight-identity-workspace/midnight-did/packages/contract/dist/managed/did/keys/addVerificationMethod.{prover,verifier}`
- Test that exercises the path: `mobile-bench/wallet-core/tests/did_bootstrap_standalone.rs::bootstrap_against_standalone_succeeds_and_doc_is_complete`.
- Upstream channel: midnight-did maintainers (responsible for the `dist/managed/did/` artifact bundle).
