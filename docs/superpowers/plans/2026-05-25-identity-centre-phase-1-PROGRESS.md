# Identity Centre Phase 1 — Execution Progress

**As of:** 2026-05-27 (third pass — post `Bytes<32>` upstream refactor)
**Branch:** `dioxus-vc-demo` (off `mobile-prototype`) — wallet repo
**Branch:** `develop` — issuer repo
**Reference plan:** `2026-05-25-identity-centre-phase-1.md`
**Reference spec:** `../specs/2026-05-25-identity-centre-phase-1-design.md`

## TL;DR (third pass — schema-refactor adoption + new upstream blocker)

**Phase 1 (C) is feature-complete in the wallet, issuer-mock, UI, and
BDD-harness layers.** Every primitive from the original plan lands and
unit-tests green (208/208 in wallet-core, all bindings resolve in the
Cucumber dry-run).

**One active blocker (upstream artifact rebuild).** After the
2026-05-27 upstream `Field → Bytes<32>` refactor of `PublicKeyJwk`,
the wallet was ported to the new schema (commit `51ecff33`). The
contract type-check now passes — the live integration test reaches
the ZK prove() step — but fails there with `Expected 6 inputs,
received 8`. The compiled contract module (`contract/index.js`) emits
8 public inputs (two per `Bytes<32>` slot) while the bundled
`.prover`/`.verifier` keys expect 6 (one per slot). Out of wallet
scope; needs midnight-did to regenerate the proving key against the
current IR. Full diagnosis in
`docs/superpowers/specs/2026-05-27-proving-key-input-mismatch.md`.

**Resolved blockers (no longer in the way):**

- **compact-js dual-load `TypeError`** — fixed by re-symlinking
  `node_modules/@midnight-ntwrk/compact-js` into the pnpm virtual
  store. Out-of-tree, documented in commit `7f732d66`'s body.
- **Ed25519-on-Field lossy 30-byte cheat** — obsoleted by the
  upstream `Bytes<32>` refactor. The wallet now ships the full
  32-byte Ed25519 pubkey losslessly. The pre-refactor spec at
  `docs/superpowers/specs/2026-05-27-ed25519-field-encoding.md` has a
  "superseded" banner pointing at the prove-key spec.
- **Indexer-settle races across the bootstrap pipeline** — five
  pollers (`wait_for_indexer_settle`, `wait_for_counter`,
  `wait_for_vm_count`, `wait_for_authentication_count`,
  `wait_for_assertion_count`) cover every chain-write step in
  `bootstrap_did_with_keys`.
- **DUST drought on stale standalone env** — `docker compose down -v
  && up` resets the chain and refills the genesis wallet.

**Still deferred (no urgency):**

- **Android JNI QR scanner (Tasks 37-39)** — needs NDK + device. The
  `PasteUrlScanner` (Task 29 / `c26466fd`) covers tests + the dev
  affordance the UI uses today.

**Port shift (2026-05-27).** A parallel midnight task occupies the
upstream-default ports on this dev box. Our standalone env now uses
**`19944` / `18088` / `16300`** (host) for node RPC / indexer / proof
server. Container-internal ports stay at upstream defaults; the
remap lives in `/tmp/midnight-standalone/docker-compose.macos.yml`.
Wallet (`Network::Undeployed`) and issuer-mock (env-var defaults)
both updated in commits `e64d2efa` (wallet) and `aec21e8` (issuer).

The current commit chain on `dioxus-vc-demo` (wallet) + `develop`
(issuer) — **~40 signed-G commits across both repos** — lands every
Phase 1 primitive. The operator-driven demo flow will go end-to-end
the moment the upstream proving-key regen lands.

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

**Live runs** are currently blocked on the upstream proving-key /
circuit-IR input-count mismatch (the new blocker — see TL;DR section
+ `docs/superpowers/specs/2026-05-27-proving-key-input-mismatch.md`).
The earlier compact-js dual-load issue that blocked live runs is
fixed; the harness reaches prove() and fails there, same as the
`did-bootstrap` CLI and the wallet integration test. The harness
itself is correct; all 8 scenarios should go green the moment the
upstream prove-key regen lands.

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

The full wallet-core surface is testable today; chain-write paths
are pending the upstream proving-key regen (the new active blocker).

```bash
# 1. Standalone Midnight stack — note the +10000 host ports to evade
#    a parallel midnight task that occupies 9944/8088/6300.
cd /tmp/midnight-standalone
docker compose -f docker-compose.yml -f docker-compose.macos.yml up -d
# Container map:
#   host 19944 → node RPC (container 9944)
#   host 30334 → node p2p (container 30333)
#   host 18088 → indexer GraphQL (container 8088)
#   host 16300 → proof server (container 6300)
# Wait for indexer to report healthy.

# 2. Bootstrap a DID via the CLI
cd /Users/ysh/iohk/midnight-ledger/.claude/worktrees/thirsty-lovelace-092f50
cargo build -p wallet-core --bin did-bootstrap --features test-support
./target/debug/did-bootstrap --seed 0x4242...42 --out /tmp/keystore.json
# Caveats: the live bootstrap reaches `addVerificationMethod` but
# the contract's bundled proving key disagrees with `contract.js`'s
# input layout (6 vs 8 public inputs after the Bytes<32> refactor).
# Fails with `prove: Expected 6 inputs, received 8`. The wallet-side
# code is correct; needs the upstream artifact rebuild. See
# `docs/superpowers/specs/2026-05-27-proving-key-input-mismatch.md`.

# 3. Lib tests (no env needed)
cargo test -p wallet-core --features test-support --lib
# Expected: 208 passed.

# 4. Live integration test (with env up)
RUST_MIN_STACK=16777216 STANDALONE_RUN=1 cargo test \
  -p wallet-core --features test-support \
  --test did_bootstrap_standalone -- --ignored --nocapture
# Expected: bootstrap_is_deterministic_across_clean_runs passes;
# bootstrap_against_standalone_succeeds_and_doc_is_complete reaches
# prove() and fails with "Expected 6 inputs, received 8" until the
# upstream proving-key regen lands.

# 5. Issuer-mock (sibling repo) — reaches the same prove-key wall
cd ~/iohk/midnight-identity-workspace/midnight-identity-solution-examples/IssuerDIDIT-mock
pnpm install --frozen-lockfile --ignore-workspace
pnpm bootstrap          # blocks at the same prove() failure
pnpm dev                # server boots regardless; 6-endpoint contract live
```

## Suggested next sessions

1. **Land the upstream proving-key regen** (midnight-did
   maintainers, not us). The single biggest unlocker. Closes the
   live `did_bootstrap_standalone` integration test, the
   `did-bootstrap` CLI live path, the issuer-mock `pnpm bootstrap`
   path, and all 8 BDD scenarios simultaneously. Diagnosis +
   reproduction recipe in
   `docs/superpowers/specs/2026-05-27-proving-key-input-mismatch.md`.
2. **Polish the dioxus-wallet Identity Centre UI.** The shipped tab
   is pragmatic; Phase 1.5 / 2 work: a real carousel, a FAB, a
   native QR scanner via `QrScanner` trait + Android JNI (Tasks
   37-39). Independently progressable from #1.
3. **Real DIDIT integration.** Replace the operator KYC form with
   webhook + redirect-to-DIDIT. Spec §3 Phase 2.
4. **Verifier app (Phase B / 2).** Plain selective-disclosure verifier
   over OID4VP. Fresh spec + plan needed.
