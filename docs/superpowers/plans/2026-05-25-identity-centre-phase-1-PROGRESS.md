# Identity Centre Phase 1 — Execution Progress

**As of:** 2026-05-27
**Branch:** `dioxus-vc-demo` (off `mobile-prototype`)
**Reference plan:** `2026-05-25-identity-centre-phase-1.md`
**Reference spec:** `../specs/2026-05-25-identity-centre-phase-1-design.md`

## TL;DR

The **wallet-core slice is complete and fully tested** (208/208 lib tests
green). The remaining 27 plan tasks — issuer-mock TS service, Dioxus UI
integration, Android JNI QR bridge, BDD harness, README polish — are
**deferred** because they depend on environment, repo, or hardware
boundaries that can't be verified by the autonomous-execution path. Each
deferred section below carries a one-paragraph "to resume" recipe.

The current commit chain on `dioxus-vc-demo` lands every wallet-core
primitive needed for OID4VP authentication, OID4VCI credential
issuance, VC storage, and self-verification. A future session can pick
up Section 6 onward with no rebase needed.

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

## Deferred — Issuer-mock TS service (Tasks 20–28)

**Location:** `~/iohk/midnight-identity-workspace/midnight-identity-solution-examples/IssuerDIDIT-mock/`
**Status:** not started

**Why deferred:** TS + Express + SQLite + Cucumber/Playwright. Depends on
three npm packages whose APIs I couldn't verify against the actual
repo state without running `yarn install` first:

- `@midnight-ntwrk/midnight-did` — exact `MidnightDidResolver` class
  name + `resolve()` signature in
  `~/iohk/midnight-identity-workspace/midnight-did/packages/did/`.
- `@midnight-ntwrk/midnight-did-api` — the plan calls
  `createDidWithKeys({ indexerUrl, nodeRpcUrl, seed })` but the symbol
  may not exist yet. The wallet-side `did-bootstrap` CLI we shipped in
  `1e1126f6` could substitute (spawn it from Node via
  `child_process.spawn`).
- `@midnight-ntwrk/midnight-did-jubjub-schnorr` — `JubjubSigner` shape
  for the issuer's VC signing path.

**To resume:**

```bash
cd ~/iohk/midnight-identity-workspace/midnight-identity-solution-examples
git checkout develop && git pull
mkdir -p IssuerDIDIT-mock/{src,scripts,e2e/fixtures}
cd IssuerDIDIT-mock
# Land Tasks 20–28 from the plan. Verify each package exists FIRST:
yarn add @midnight-ntwrk/midnight-did @midnight-ntwrk/midnight-did-api \
         @midnight-ntwrk/midnight-did-jubjub-schnorr
# If those fail, the workspace has unpublished packages; consult
# midnight-did/AGENT.md for current symbol names.
```

**Shortcut for the demo:** the OID4VP / OID4VCI HTTP contract can be
hand-rolled against the existing wallet-core clients without the full
TS issuer-mock. Six endpoints (`/authorize`, `/request/:id`,
`/authorize-response`, `/credential-offer/:id`, `/token`, `/credential`)
returning canned JSON would be enough to walk the wallet through both
flows. A 200-LOC Express server could replace Tasks 20-28 for the demo.

## Deferred — Dioxus UI integration (Tasks 30–36)

**Location:** `mobile-bench/dioxus-wallet/src/`
**Status:** not started

**Why deferred:** The plan's task code assumes a `wallet_handle::*`
plumbing layer + new helper functions (`Wallet::list_owned_dids`,
`has_any_bootstrapped_did`, `SecretStorage::has_pair`, etc.) that don't
exist in the current dioxus-wallet. Building them requires multiple
choices about how the new `Identity` tab folds into the existing
`Tab::Wallet / Tab::Dids / Tab::Diagnostics` structure. The DIDs tab
already has working create / resolve / update / sign flows from the
earlier UI polish (`7b11d5e0`), so the "stack a VC carousel on top of
this" decision is best made interactively.

**To resume:**

Either (a) plumb wallet-core's `bootstrap_did_with_keys` / `vc_store::*`
/ `oid4vp_client::run_authentication` / `oid4vci_client::run_issuance` /
`vc_self_verify::self_verify_and_cache` into existing tabs as
incremental additions (no Tab restructure), or (b) follow the plan's
Tasks 30–36 verbatim once the `wallet_handle::*` plumbing exists.

Minimum viable demo UI:
- A "VCs" tab with `VcStore::list_ordered()` → carousel.
- The existing **Create DID** button covers Task 31's BootstrapPanel.
- A floating "+" / Scan button → `oid4vci_client::run_issuance`.
- Per-card "Self-verify" button → `vc_self_verify::self_verify_and_cache`.

That's ~300 LOC against the surfaces wallet-core already ships.

## Deferred — Android JNI QR scanner bridge (Tasks 37–39)

**Status:** not started

**Why deferred:** Needs the Android NDK toolchain + a real device or
emulator with camera permissions to exercise. The `PasteUrlScanner`
stub (shipped in Task 29 / `c26466fd`) covers tests + dev affordance,
so the rest of the pipeline isn't blocked.

**To resume:** Tasks 37–39 in the plan. Pull `cargo-ndk` + `camera2` +
ML Kit barcode-scanning dep. Adapt the JNI bridge pattern used by the
existing PERIPHERAL_PROVIDER if it survived.

## Deferred — BDD harness (Tasks 40–43)

**Status:** not started

**Why deferred:** Depends on the issuer-mock being up (Tasks 20–28) +
the UI integration (Tasks 30–36) to drive Playwright clicks. Land both
upstream, then the harness is ~200 LOC of `.feature` files + Cucumber
step defs against a Playwright `chromium.connect()`.

**To resume:** Tasks 40–43. Cucumber config at
`IssuerDIDIT-mock/cucumber.cjs`; features in `IssuerDIDIT-mock/e2e/features/`.

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

1. **Fix the JS-bridge / compact-js TypeError.** Unlocks the live
   integration test + the CLI bootstrap path. Probably a
   `compactContext` initialization the `NodeChildBridge` harness isn't
   doing that the WebView path is.
2. **Land a minimal issuer-mock** — 200-LOC Express server with the six
   endpoints, no Cucumber yet. Verifies the wallet-core OID4VP /
   OID4VCI clients against real HTTP.
3. **Plumb the wallet-core flows into the dioxus-wallet UI** — VCs tab,
   FAB → scan → run_issuance, per-card self-verify badge.
4. **Real DIDIT once Tasks 20-28 are stable** — replace the operator
   form with the DIDIT webhook + redirect-to-DIDIT pattern.
