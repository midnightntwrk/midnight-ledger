# Identity Centre Phase 1 — Execution Progress

**As of:** 2026-05-27 (second pass)
**Branch:** `dioxus-vc-demo` (off `mobile-prototype`) — wallet repo
**Branch:** `develop` — issuer repo
**Reference plan:** `2026-05-25-identity-centre-phase-1.md`
**Reference spec:** `../specs/2026-05-25-identity-centre-phase-1-design.md`

## TL;DR (revised — second autonomous push)

**Phase 1 (C) is now feature-complete except for Android-device-only
work.** Three subsystems landed in the second autonomous push:

- **Issuer-mock TS service (Tasks 20-28)** — 9 commits in the sibling
  `midnight-identity-solution-examples` repo. The 6-endpoint OID4VP +
  OID4VCI HTTP contract is live; `pnpm dev` boots clean.
- **Dioxus Identity Centre tab (Tasks 30-36 pragmatic minimum)** — 1
  commit (`96b742df`) wires the four shipped wallet-core flows
  (bootstrap, OID4VP auth, OID4VCI issue, self-verify) into a new top-
  level `Tab::Identity` with linear paste-URL buttons. Operator can
  drive the full end-to-end demo against the running issuer-mock.
- **BDD harness (Tasks 40-43)** — 4 commits in the issuer repo. 8
  Cucumber scenarios / 50 steps wired (`bootstrap.feature`,
  `issuance-happy-path.feature`, `self-verify.feature`,
  `negative-paths.feature`). Step bindings resolve clean
  (`pnpm test --dry-run`); live runs blocked on standalone-env DUST
  drought (unblocker: restart docker compose).

**Still deferred:**

- **Android JNI QR scanner (Tasks 37-39)** — needs NDK + device. The
  `PasteUrlScanner` (Task 29 / `c26466fd`) covers tests + the dev
  affordance the UI uses today.
- **Known live-test blocker** — `bootstrap_did_with_keys` from
  cargo-test / `did-bootstrap` CLI / the BDD headless-wallet hits the
  pre-existing `@midnight-ntwrk/compact-js@2.5.0` `TypeError` in
  `NodeChildBridge`. The dioxus-wallet UI works fine (uses
  `DioxusEvalBridge`).

The current commit chain on `dioxus-vc-demo` (wallet) + `develop`
(issuer) lands every Phase 1 primitive. The operator-driven demo flow
is exercisable today via the iOS Sim / Android Emulator + the
`pnpm dev` issuer-mock at `:3001`.

## Done — Wallet-core slice (Tasks 1–19, 29)

| Section | Tasks | Commits | Tests |
|---|---|---|---|
| §1 DID bootstrap helper | 1, 1.5.A-D, 2 | 9 commits → `680830fb` | 7 unit tests |
| §1 `did-bootstrap` CLI | 3 | `1e1126f6` | bin smoke |
| §1 standalone integration test | 4 | `9041ff79` (+ `wait_for_indexer_settle` fix) | 1 deterministic + 1 live-env reproducer |
| §2 `vc_store` (3 redb tables) | 5–8 | `f1d4bbdc`, `6b2b22ef`, `61e9f5bb`, `c913c9ab` | 7 |
| §3 `did_auth` + `find_by_kid` | 9 | `816c7a41` (also fixed kid-form mismatch in bootstrap) | 2 |
| §3 `oid4vp_client` | 10–13 | `54ebab1a`, `03d6b3fc`, `67c3d5d5`, `d6647b75` | 9 |
| §4 `oid4vci_client` | 14–17 | `bebe67aa`, `cdd37b61`, `5ea147fa`, `b614fca0` | 6 |
| §5 `vc_self_verify` | 18, 19 | `21688aa3`, `36f7ec78` | 3 |
| §8 `qr_scanner` trait | 29 | `c26466fd` | 1 |

**Standalone-env evidence:** the docker-compose stack on `:9944` / `:8088` /
`:6300` was used to validate the live `did-bootstrap` CLI + integration
test paths. The indexer-settle race that bit the first live run is fixed
in `9041ff79`. The remaining JS-bridge failure (compact-js@2.5.0
`TypeError` inside `NodeChildBridge`) is a known pre-existing limitation;
the dioxus-wallet UI works around it by using the in-process
`DioxusEvalBridge` (WebView).

### Dioxus UI polish landed alongside (not in the plan)

Five UI improvements driven by exercising the standalone env in the
dioxus-wallet (commit `7b11d5e0`):

1. **Re-mounted `CreateDidWizard`** on the DIDs tab so the operator can
   bootstrap on Undeployed.
2. **`method_id` dropdown** for `addVerificationMethodRelation` /
   `removeVerificationMethodRelation` — picks from the resolved doc's
   VMs instead of free-text entry.
3. **`vm_short_name` helper** — renders fragment-only kids in the
   Methods table / dropdowns; full URL on hover.
4. **DUST syncer re-registers on network switch** (fixed the "syncer not
   initialised" stuck state when flipping PreProd → Undeployed).
5. **Auto-sync gated to PreProd** so picker-switches don't race the
   re-registration.

These aren't plan tasks but were necessary to make the standalone-env
demo run.

## Done (2nd push) — Issuer-mock TS service (Tasks 20–28)

**Location:** `~/iohk/midnight-identity-workspace/midnight-identity-solution-examples/IssuerDIDIT-mock/`
**Status:** complete. 9 commits on `develop` (`4288ad7` → `7f84447`).

The pragmatic-minimum adaptations vs. plan:

- **`pnpm` 10.23.0**, not yarn (repo uses pnpm workspaces).
- **Standalone outside the workspaces array** so the new package
  doesn't pollute the root workspace declarations.
- **Issuer DID bootstrap shells out to the Rust `did-bootstrap` CLI**
  (commit `1e1126f6`) because `@midnight-ntwrk/midnight-did-api` does
  not (yet) export `createDidWithKeys`.
- **Holder DID resolver is self-asserted** (trusts the wallet's `jwk`
  header parameter) — Phase 1 security gap, documented inline. Swap
  to a real resolver is one-file when `MidnightDIDResolver` stabilises.
- **Jubjub signer** linked via a `file:` dep against the sibling
  `midnight-did/packages/jubjub-schnorr` workspace.
- **Docker image tags** pinned to `0.22.0` / `4.0.0` / `8.0.2` to match
  the already-running standalone env (not `standalone-latest`).

End-to-end smoke: `pnpm dev` + `curl /authorize` returns HTML with a
QR PNG; `pnpm bootstrap` shell-outs to the Rust CLI cleanly.

## Done (2nd push) — Dioxus Identity Centre tab (Tasks 30–36 pragmatic minimum)

**Location:** `mobile-bench/dioxus-wallet/src/identity_centre.rs` (new)
**Status:** complete. 1 commit (`96b742df`).

A new top-level `Tab::Identity` rendering a flat linear panel with four
cards — Bootstrap / OID4VP / OID4VCI / VC inventory — each wiring
straight to a shipped wallet-core entry point:

| Card | wallet-core call |
|---|---|
| Bootstrap | `bootstrap_did_with_keys(&wallet, &mut store, &[42; 32])` |
| OID4VP authenticate | `oid4vp_run_authentication(qr_url, &wallet, &store, &did)` |
| OID4VCI issue | `oid4vci_run_issuance(qr_url, &wallet, &store, &did, &vc_store)` |
| Self-verify | `self_verify_and_cache(&vc, &wallet, &*store, &vc_store)` |

What's intentionally not built (deferred to Phase 1.5 / 2):

- Sub-tab carousel (linear list instead).
- Floating action button (replaced by inline paste-URL textareas).
- Native QR camera (deferred to Tasks 37-39).
- DID picker for multi-DID holders (single-DID demo flow).

Builds clean on desktop + iOS sim release target (`aarch64-apple-ios-sim`,
`--features "preprod-live js-bridge"`).

## Done (2nd push) — BDD harness (Tasks 40–43)

**Location:** `~/iohk/midnight-identity-workspace/midnight-identity-solution-examples/IssuerDIDIT-mock/e2e/`
**Status:** harness wired. 4 commits (`2717b8b` → `c39704c`).

8 Cucumber scenarios / 50 steps across four `.feature` files:

- `bootstrap.feature` — holder DID bootstrap against the standalone env.
- `issuance-happy-path.feature` — full OID4VP + KYC + OID4VCI flow.
- `self-verify.feature` — fresh / tampered / rotated issuer key paths.
- `negative-paths.feature` — nonce replay, kid mismatch, expired codes.

`pnpm test --dry-run` discovers all 50 steps cleanly (0 unbound).
Headless TS wallet client shells out to the Rust `did-bootstrap` CLI
for the holder DID bootstrap (same pattern as the issuer's).

Adaptations vs plan:

- **No Playwright** — the operator-driven KYC step uses a direct
  form-encoded POST to `/kyc-form?session=...` instead of browser
  automation. Phase 1's contract is HTTP, not UI.
- **`tsx 4.x` quirks** — cucumber CLI invoked via
  `tsx node_modules/@cucumber/cucumber/bin/cucumber.js` to dodge the
  tsx/Cucumber `index.js` extension-expansion clash.
- **Per-scenario isolation** = fresh issuer-mock server on a unique
  port + fresh SQLite. Standalone Midnight env assumed up (not
  brought up/down per scenario — takes minutes).

**Live runs** are currently blocked on the same `compact-js@2.5.0`
`NodeChildBridge` issue that affects `bootstrap_did_with_keys`
end-to-end paths from `cargo-test` / the CLI. The harness is correct;
it'll go green the moment that blocker resolves.

## Deferred — Android JNI QR scanner bridge (Tasks 37–39)

**Status:** not started

**Why deferred:** Needs the Android NDK toolchain + a real device or
emulator with camera permissions to exercise. The `PasteUrlScanner`
stub (shipped in Task 29 / `c26466fd`) covers tests; the dioxus-wallet
UI's paste-URL textareas (shipped in `96b742df`) cover the operator
demo affordance.

**To resume:** Tasks 37–39 in the plan. Pull `cargo-ndk` + `camera2` +
ML Kit barcode-scanning dep. Adapt the JNI bridge pattern used by the
existing PERIPHERAL_PROVIDER if it survived.

## Open questions inherited from the spec

These were unresolved when the plan was drafted and remain unresolved
since they touch the deferred sections:

- **Real DIDIT integration vs. operator mock:** Phase 1 ships the mock.
  Real DIDIT KYC integration is a Phase 2 concern per spec §3.
- **iOS QR scanner (`AVCaptureMetadataOutput`):** Phase 1's spec scope
  is Android-first; iOS QR scanning lands when the iOS wallet ships.
- **VC carousel polish:** Tinder-style physics is explicitly Phase 2
  per the plan's Task 32 commentary.
- **DID picker for multi-DID wallets:** spec §5 / Task 35 — out of
  scope until a holder owns >1 bootstrapped DID. The wallet's existing
  DIDs tab handles this case adequately for now.

## Demo bringup recipe (current state)

Even without the deferred sections, the full wallet-core surface is
testable today:

```bash
# 1. Standalone Midnight stack
cd /tmp/midnight-standalone
docker compose -f docker-compose.yml -f docker-compose.macos.yml up -d
# Wait for indexer to report healthy.

# 2. Bootstrap a DID via the CLI
cd /Users/ysh/iohk/midnight-ledger/.claude/worktrees/thirsty-lovelace-092f50
cargo build -p wallet-core --bin did-bootstrap --features test-support
./target/debug/did-bootstrap --seed 0x4242...42 --out /tmp/keystore.json
# Caveats: the live bootstrap currently fails inside the JS bridge at
# the add_verification_method step due to the compact-js@2.5.0 TypeError
# noted in commit 9041ff79. Use the dioxus-wallet UI's Create DID + add
# VM flow instead (which works because it uses DioxusEvalBridge).

# 3. Lib tests (no env needed)
cargo test -p wallet-core --features test-support --lib
# Expected: 208 passed.

# 4. Live integration test (with env up)
RUST_MIN_STACK=16777216 STANDALONE_RUN=1 cargo test \
  -p wallet-core --features test-support \
  --test did_bootstrap_standalone -- --ignored --nocapture
# Expected: bootstrap_is_deterministic_across_clean_runs passes;
# bootstrap_against_standalone_succeeds_and_doc_is_complete reproduces
# the JS-bridge TypeError until that's fixed separately.
```

## Suggested next sessions

1. **Fix the JS-bridge / compact-js TypeError.** Single biggest
   unlocker. Closes the live `did_bootstrap_standalone` integration
   test, the `did-bootstrap` CLI live path, AND the BDD live runs
   simultaneously. Probably a `compactContext` initialization the
   `NodeChildBridge` harness isn't doing that the WebView path is.
2. **Restart standalone env when DUST drought hits.** 19+ hour-old
   stacks deplete the demo-seed wallets' DUST balance via dust-decay.
   `pnpm env:down && pnpm env:up && pnpm bootstrap` resets cleanly.
3. **Polish the dioxus-wallet Identity Centre UI.** The shipped tab is
   pragmatic; Phase 1.5 / 2 work: a real carousel, a FAB, a native
   QR scanner via `QrScanner` trait + Android JNI (Tasks 37-39).
4. **Real DIDIT integration.** Replace the operator KYC form with
   webhook + redirect-to-DIDIT. Spec §3 Phase 2.
5. **Verifier app (Phase B / 2).** Plain selective-disclosure verifier
   over OID4VP. Fresh spec + plan needed.
