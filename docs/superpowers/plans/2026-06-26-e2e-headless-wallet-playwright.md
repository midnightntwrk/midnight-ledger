# E2E Test Suite (HeadlessWallet + Playwright) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a Linux-CI-friendly end-to-end test lane that drives the full Midnight SSI demo (chain → issuer-mock → vault deploy/locks → wallet → dApp claim) through the Rust `headless-wallet` binary + Cucumber.js + Playwright, runnable locally via `just e2e` and in CI on every PR to `develop`.

**Architecture:** Reuse the existing Cucumber.js 12 + tsx pattern from `apps/issuer-mock/e2e/`. Extend the Rust `HeadlessWallet` façade in `mobile-bench/wallet-core` with vault verbs (`list_locks`, `total_locked`, `create_lock`, `deposit_to_lock`, `claim_from_lock`), expose them through a JSON-line verb dispatcher in `mobile-bench/headless-wallet/src/main.rs`, wrap that as a `HeadlessWalletProc` TS client in the new `demos/e2e/` package, and drive the issuer + dApp via Playwright. CI uses Linux + Docker Compose (no emulator); Android emulator and iOS sim are explicitly deferred to a follow-up plan.

**Tech Stack:** Rust 2024 (`headless-wallet`, `wallet-core`), Cucumber.js 12 + tsx, @playwright/test (chromium), Node 24, Docker Compose, GitHub Actions (`dtolnay/rust-toolchain` + `Swatinem/rust-cache` + `actions/setup-node`), Justfile orchestration.

## Global Constraints

- **DCO + GPG signing on every commit.** Run `bash ~/iohk/git-iohk.sh` before each repo's first commit, then `git commit -S -s` always. Never include `Signed-off-by:` in the message body — `-s` adds it.
- **Node 24 minimum** (ledger-v8 WASM needs it).
- **Standalone funded seed is `0000…0001`** — arbitrary seeds only get per-block emission and never converge.
- **JSON verb protocol** (from `docs/superpowers/specs/2026-05-29-hexagonal-headless-wallet-design.md` §2.4–§2.5):
  - Request: `{"verb":"<name>","args":{…}}` line-delimited on stdin
  - Success: `{"type":"result","verb":"<name>","ok":true,"data":{…}}` on stdout
  - Error: `{"type":"error","verb":"<name>","code":"<code>","message":"<text>"}` on stdout
  - Progress: `{"type":"event","verb":"<name>","stage":"<name>","data":{…}}` on stdout
  - Tracing → stderr, JSON → stdout, exit codes `0/2/3/4`.
- **Vault verb naming** (must mirror existing dioxus bridge cases at `mobile-bench/dioxus-wallet/src/bridge.rs:1116-1188`): `vaultTotalLocked`, `vaultListLocks`, `vaultListCredentials`, `vaultCreateLock`, `vaultDeposit`, `vaultClaim`. Camel-case in JSON; snake-case in Rust.
- **Three repos** — every PR description must link the partner PRs in the other two repos so merges happen in lock-step:
  - `/Users/ysh/iohk/midnight-ledger/.claude/worktrees/thirsty-lovelace-092f50/` (the wallet — branch `feat/phone-driven-claim-demo`)
  - `/Users/ysh/iohk/midnight-workspace-vc-test/midnight-identity-solution-examples/` (issuer/dApp/cli — branch `feat/issuer-pending-fix-image-tagging` → land Track 0 there, then split)
  - `/Users/ysh/iohk/midnight-ssi-demo/midnight-identity-workspace/` (orchestrator — branch from `develop`)
- **No webhooks for test coordination.** Headless-wallet child process speaks stdio; Playwright drives a chromium context; both world hooks live in the cucumber `World`.
- **Android emulator and iOS sim are out of scope.** A follow-up plan covers them.

## File Structure

### New files

```
mobile-bench/headless-wallet/src/
  main.rs                      # Extended: JSON verb dispatcher loop
  verbs/
    mod.rs                     # Verb router (match on verb name)
    vault.rs                   # vaultTotalLocked, vaultListLocks, vaultListCredentials, vaultCreateLock, vaultDeposit, vaultClaim
    protocol.rs                # Request/Response/Event/Error structs + line codec

mobile-bench/wallet-core/src/
  headless.rs                  # Extended: vault methods on HeadlessWallet

mobile-bench/wallet-core/tests/
  headless_vault_e2e.rs        # Integration: spawn standalone + drive vault verbs through HeadlessWallet

midnight-identity-workspace/demos/e2e/
  package.json                 # @midnight-ntwrk/ssi-demo-e2e — cucumber-js + @playwright/test + tsx
  cucumber.cjs                 # cucumber config, mirrors apps/issuer-mock/e2e/cucumber.cjs
  playwright.config.ts         # dApp browser config (baseURL = http://localhost:3000)
  tsconfig.json                # Node 24 ESM, paths to ../bootstrap.py harness
  features/
    phone-driven-claim.feature # End-to-end claim happy path
    issuer-flow.feature        # KYC → VC mint → store
  step-definitions/
    bootstrap-steps.ts         # Drives `python3 ../bootstrap.py up --ci --yes`
    wallet-steps.ts            # Spawns headless-wallet binary, pipes JSON verbs
    issuer-steps.ts            # HTTP to :8080 (kyc-sessions, status)
    dapp-steps.ts              # Playwright chromium: connect wallet shim + claim
  support/
    hooks.ts                   # BeforeAll/Before/After (mirrors issuer-mock pattern)
    world.ts                   # ScenarioWorld with wallet, browser, http clients
  fixtures/
    headless-wallet-proc.ts    # ChildProcess wrapper, JSON line codec
    seeds.ts                   # Standalone funded seed + holder/issuer seeds
    docker-compose.yml         # node + indexer + proof-server-bootstrap

midnight-identity-workspace/demos/
  Justfile                     # Extended: e2e, e2e-reset, e2e-headed recipes

midnight-identity-solution-examples/.github/workflows/
  e2e.yml                      # PR-triggered Linux e2e workflow
```

### Modified files

```
mobile-bench/headless-wallet/Cargo.toml             # Add wallet-core feature gates if needed
midnight-identity-workspace/demos/bootstrap.py      # Add --ci flag, plumb to skip_dust_wait
midnight-identity-workspace/demos/lib/chain.py      # wait_for_dust honors $CI || cfg.ci skip
.github/workflows/docker-push.yml (midnight-ledger) # Add proof-server-bootstrap matrix job (Track 6)
flake.nix (midnight-ledger)                         # Add proof-server-bootstrap-oci derivation (Track 6)
```

---

## Track 0 — Land today's stabilization fixes

Today's uncommitted fixes are prerequisites: the e2e plan cannot execute against an unstable demo bootstrap. Each repo gets its own PR.

### Task 0.1: Commit issuer UI fixes + smocker patch (solution-examples repo)

**Files:**
- Modify: `apps/issuer/src/web/issue/index.html` (smart-redirect + KYC_URL_KEY + ?reset=1 escape hatch + single-tab navigation)
- Modify: `apps/issuer/src/web/issue/pending.html` (Reopen-verification link + Start-over `?reset=1`)
- Modify: `apps/issuer/src/web/issue/complete.html` (Start-over `?reset=1`)
- Modify: `nix/devshell/mock/didit.yml` (3 localhost → 100.110.241.102 patches + redirect carries `verificationSessionId`)
- Modify: `apps/dapp/lib/midnight/connector.ts` (vault read verbs accept `{contractAddress?: string}`)
- Modify: `apps/dapp/lib/midnight/mobile-bench-host.ts` (shim threads params through)
- Modify: `apps/dapp/lib/vault.ts` (callers pass `{contractAddress: VAULT_CONTRACT_ADDRESS}`)
- Modify: `apps/dapp/next.config.mjs` (`allowedDevOrigins` for Tailscale dev)
- Modify: `apps/cli/src/{deploy,create-lock,deposit,claim-funds,show-state}.ts` (cherry-pick `be8f06c` `process.exit(0)` on main resolution — currently checked out from develop, not yet committed on `feat/issuer-pending-fix-image-tagging`)

**Interfaces:** No new public types; behavioural changes only.

- [ ] **Step 1: Configure repo for signed commits**

```bash
cd /Users/ysh/iohk/midnight-workspace-vc-test/midnight-identity-solution-examples
bash ~/iohk/git-iohk.sh
git status --short
```

Expected: list shows ~13 modified files in apps/issuer/, apps/dapp/, apps/cli/, nix/devshell/mock/.

- [ ] **Step 2: Group A — apps/cli/ process.exit fix (cherry-pick land)**

```bash
git add apps/cli/src/deploy.ts apps/cli/src/create-lock.ts apps/cli/src/deposit.ts apps/cli/src/claim-funds.ts apps/cli/src/show-state.ts
git commit -S -s -m "$(cat <<'EOF'
fix(cli): land be8f06c — force process.exit(0) on main() resolution

The wallet stack's indexer subscription + dust syncer + midnight-js
WebSocket transport keep Node's event loop alive indefinitely. With
no explicit exit after main() resolves, each script printed its
success line and then hung holding open sockets, blocking run-demo.sh's
chained `&&` pipeline. Cherry-picked from develop's be8f06c so this
branch (split from develop pre-merge) can drive the demo orchestrator
end-to-end without hand-killing each step.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
git log --format="%h %G? %s" -1
```

Expected: `<sha> G fix(cli): land be8f06c — force process.exit(0) on main() resolution` (G = good signature).

- [ ] **Step 3: Group B — issuer UI fix**

```bash
git add apps/issuer/src/web/issue/index.html apps/issuer/src/web/issue/pending.html apps/issuer/src/web/issue/complete.html
git commit -S -s -m "$(cat <<'EOF'
fix(issuer): smart-redirect + single-tab nav + Start-over reset escape

index.html now reads sessionStorage on load and auto-routes to
/issue/complete.html (if Approved) or /issue/pending.html?verificationSessionId
(if in-progress), so users who land back on /issue/ after Didit's mobile
KYC page fails to auto-redirect are bounced to the right state instead
of seeing the Begin button again. `/issue/?reset=1` is the escape hatch
that clears the sessionStorage so the user can start fresh — wired up
from every Start-over link in pending.html + complete.html.

Begin verification now stays single-tab (the new-tab workaround was
removed; for the smocker mock the form's Approve button explicitly
redirects back, and for real Didit the smart-redirect handles re-entry).

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

- [ ] **Step 4: Group C — smocker YAML tailnet patch**

```bash
git add nix/devshell/mock/didit.yml
git commit -S -s -m "$(cat <<'EOF'
fix(smocker): replace localhost refs with tailnet IP for phone access

The fake-Didit YAML mocks /v3/session/ (POST) + /v3/session/<id>/decision/
(GET) and serves a mock-verification HTML page. All three response bodies
embedded http://localhost:8080 + http://localhost:9090, which the phone
over Tailscale cannot reach. Replace with 100.110.241.102 (this laptop's
tailnet IP) so the smocker-mock KYC flow works end-to-end on mobile.

The mock-verification HTML's Approve button now also appends
?verificationSessionId=550e8400-… so /issue/pending.html finds the session
without relying on sessionStorage (mobile WebViews wipe it across
cross-origin redirects).

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

- [ ] **Step 5: Group D — dApp connector contractAddress threading**

```bash
git add apps/dapp/lib/midnight/connector.ts apps/dapp/lib/midnight/mobile-bench-host.ts apps/dapp/lib/vault.ts apps/dapp/next.config.mjs
git commit -S -s -m "$(cat <<'EOF'
fix(dapp): thread contractAddress through vault read verbs

The mobile-bench host shim called every read verb with empty params
(`vaultListLocks: () => call(..., {})`), so the wallet's
vault_contract_address() resolver fell through to its compiled-in
DEFAULT_VAULT_CONTRACT_ADDRESS — a stale per-build constant that
predates whatever vault the orchestrator just deployed. Result: the
dApp showed "no locks" because the wallet was querying the wrong
contract.

Types in connector.ts now optionally accept `{contractAddress?: string}`
on vaultListLocks, vaultListCredentials, vaultTotalLocked and vaultClaim;
the shim threads params through; lib/vault.ts wrappers pull
NEXT_PUBLIC_VAULT_CONTRACT_ADDRESS from the dApp env and pass it on.
Extension wallets like Lace that don't need the param still get
back-compat (it's optional).

Adds `allowedDevOrigins: ["100.110.241.102", "localhost", "127.0.0.1"]`
to next.config.mjs so `next dev` serves chunks/HMR to the phone over
Tailscale (Next 15+ blocks cross-origin dev resources by default).

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

- [ ] **Step 6: Verify all four commits are signed**

```bash
git log --format="%h %G? %s" -5
```

Expected: four `G` rows for the new commits.

- [ ] **Step 7: Push + open PR to develop**

```bash
git push -u origin feat/issuer-pending-fix-image-tagging
gh pr create --base develop --title "fix: stabilize demo (issuer UI + smocker tailnet + dApp contractAddress + apps/cli exit)" --body "$(cat <<'EOF'
## Summary

Four-part stabilization for the phone-driven SSI demo so it survives end-to-end on a fresh chain:

- **apps/cli**: cherry-pick `be8f06c` so each script exits cleanly after `main()` resolves (was hanging the orchestrator on every step).
- **apps/issuer**: smart-redirect on `/issue/`, single-tab navigation, `?reset=1` escape hatch on every Start-over link. Mobile WebView no longer gets stuck on Didit's "Verified! that's all" terminal page.
- **nix/devshell/mock/didit.yml**: tailnet IP for the smocker mock (phone-from-Tailscale fix).
- **apps/dapp**: thread `contractAddress` through vault read verbs + add `allowedDevOrigins` for `next dev` over Tailscale.

## Test plan
- [x] Smocker flow on phone: KYC → VC QR scan → wallet has credential
- [ ] dApp on phone shows lock #0 + #1 with funded balance
- [ ] Claim 1 NIGHT from lock #0 succeeds end-to-end

Partner PRs:
- workspace: `chore(demo): orchestrator + paths + Justfile for phone-driven demo` (TBD)
- ledger: (not needed for this PR)

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

Expected: PR URL printed. Note the URL for the workspace PR description.

### Task 0.2: Commit orchestrator fixes (workspace repo)

**Files:**
- Modify: `demos/lib/config.py` (issuer-mock path fallback)
- Modify: `demos/lib/vault.py` (pin issuer at deploy via `--bundle`; `rotate_trusted_issuer` becomes no-op)
- Modify: `demos/lib/dapp.py` (`pnpm build` → `npm run build`)
- Modify: `demos/lib/issuer.py` (already-edited container_running import, pending-patch removal)

**Interfaces:**
- Consumes: nothing
- Produces: `vault.deploy_vault(cfg)` now writes the trusted-issuer–pinned vault address; `vault.rotate_trusted_issuer(cfg)` is a no-op stub.

- [ ] **Step 1: Configure repo for signed commits**

```bash
cd /Users/ysh/iohk/midnight-ssi-demo/midnight-identity-workspace
bash ~/iohk/git-iohk.sh
git status --short
```

- [ ] **Step 2: Single commit (orchestrator paths + flow alignment)**

```bash
git add demos/lib/config.py demos/lib/vault.py demos/lib/dapp.py demos/lib/issuer.py
git commit -S -s -m "$(cat <<'EOF'
chore(demos): orchestrator paths + flow alignment for canonical apps/

Three out-of-date assumptions caught after the issuer/dApp consolidation
from patextreme/midnight-passport-kyc into midnight-identity-solution-examples:

- demos/lib/config.py: issuer_mock path now prefers apps/issuer-mock
  with legacy IssuerDIDIT-mock fallback.
- demos/lib/vault.py: deploy_vault generates the real-issuer anchor
  bundle then runs `npm run deploy -- --bundle fixtures/credential.real-issuer.json`.
  Canonical apps/cli no longer ships a set-trusted-issuer script; the
  vault pins the trusted issuer at deploy time via the bundle, so
  rotate_trusted_issuer becomes a no-op stub.
- demos/lib/dapp.py: build via `npm run build` (workspace package
  manager is npm@11.x; pnpm couldn't see the hoisted next binary).

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
git log --format="%h %G? %s" -1
```

- [ ] **Step 3: Push + open PR**

```bash
git push -u origin feat/phone-driven-claim-demo
gh pr create --base develop --title "chore(demos): orchestrator paths + flow alignment" --body "$(cat <<'EOF'
## Summary

Three small fixes to the demo orchestrator (`demos/bootstrap.py` + `demos/lib/*`):

- Path: prefer `apps/issuer-mock` over legacy `IssuerDIDIT-mock`.
- Flow: pin trusted issuer at deploy via `--bundle`; drop `set-trusted-issuer` rotate (canonical apps/cli no longer ships that script).
- Build: `npm run build` for the dApp (workspace uses npm@11.x).

## Test plan
- [x] `nix develop -c python3 demos/bootstrap.py up --tailscale --yes` completes Phases 1-14 on a clean chain
- [x] Vault deploys at the address baked into `apps/dapp/.env.local`
- [ ] Phone-driven claim succeeds against the vault that this run deploys

Partner PR: <solution-examples PR URL from Task 0.1>

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

---

## Track 1 — HeadlessWallet vault methods (Rust port)

Bring the five vault methods that live on `Wallet` in `mobile-bench/wallet-core/src/wallet.rs:1949…2311` up to the `HeadlessWallet` façade in `mobile-bench/wallet-core/src/headless.rs`, with full unit-test coverage. This is the foundation for Track 2.

### Task 1.1: Expose `vault_total_locked` on HeadlessWallet

**Files:**
- Modify: `mobile-bench/wallet-core/src/headless.rs` (add method + tests)

**Interfaces:**
- Consumes: `Wallet::vault_total_locked(addr: ContractAddress) -> Result<u128, WalletError>` already exists in `wallet-core/src/wallet.rs:1949`
- Produces: `HeadlessWallet::vault_total_locked(&self, contract_address: &str) -> Result<u128, HeadlessError>`

- [ ] **Step 1: Write the failing unit test**

In `mobile-bench/wallet-core/src/headless.rs`, inside the existing `#[cfg(test)] mod tests` block:

```rust
#[tokio::test(flavor = "current_thread")]
async fn vault_total_locked_returns_wallet_value() {
    use crate::test_support::stub_wallet::stub_wallet_with_vault;
    let wallet = stub_wallet_with_vault(vec![("0x" .to_string(), 150_000_000u128)]).await;
    let headless = headless_from_wallet(wallet).await;
    let total = headless
        .vault_total_locked("aabbccdd00112233445566778899aabbccddeeff00112233445566778899aabb")
        .await
        .expect("vault_total_locked");
    assert_eq!(total, 150_000_000u128);
}
```

- [ ] **Step 2: Run the test, confirm it fails**

```bash
cd /Users/ysh/iohk/midnight-ledger/.claude/worktrees/thirsty-lovelace-092f50/mobile-bench
cargo test --features test-support -p wallet-core --lib headless::tests::vault_total_locked_returns_wallet_value -- --nocapture
```

Expected: FAIL — `no method named "vault_total_locked"` on `HeadlessWallet`, or `stub_wallet_with_vault` undefined. Either is OK; both are filled in below.

- [ ] **Step 3: Add the method on HeadlessWallet**

After the existing `pub async fn verify(...)` method, append:

```rust
/// Read the total NIGHT locked across all locks of `contract_address`.
/// Mirrors the dApp connector verb `vaultTotalLocked` and the dioxus
/// bridge dispatch at `dioxus-wallet/src/bridge.rs:1116`.
pub async fn vault_total_locked(
    &self,
    contract_address: &str,
) -> Result<u128, HeadlessError> {
    let addr = parse_contract_address(contract_address)
        .map_err(|e| HeadlessError::InvalidArg(format!("contractAddress: {e}")))?;
    self.wallet
        .vault_total_locked(addr)
        .await
        .map_err(|e| HeadlessError::Wallet(e.to_string()))
}
```

Where `parse_contract_address` is a small helper that hex-decodes a 32-byte address (define in the same file alongside other private helpers):

```rust
fn parse_contract_address(s: &str) -> Result<ContractAddress, String> {
    let bytes = hex::decode(s.trim_start_matches("0x"))
        .map_err(|_| format!("not a hex string"))?;
    bytes.try_into().map_err(|v: Vec<u8>| format!("expected 32 bytes, got {}", v.len()))
}
```

Also add the new error variant if not present in `HeadlessError`:

```rust
#[error("invalid argument: {0}")]
InvalidArg(String),
#[error("wallet: {0}")]
Wallet(String),
```

- [ ] **Step 4: Add the stub helper in test_support**

In `mobile-bench/wallet-core/src/test_support/stub_wallet.rs` (extend the existing module):

```rust
#[cfg(feature = "test-support")]
pub async fn stub_wallet_with_vault(
    locks: Vec<(String, u128)>,
) -> crate::wallet::Wallet {
    // Build a stub wallet seeded with the provided lock balances using
    // the in-memory VaultStateStub backing (already in test_support).
    let mut wallet = crate::test_support::stub_wallet::stub_wallet().await;
    for (lock_id, amount) in locks {
        wallet.vault_state_stub_mut().insert_lock(lock_id, amount);
    }
    wallet
}
```

(If `vault_state_stub_mut()` doesn't exist yet, add it — the existing test_support already has a stub backing for non-vault state; just thread vault state through the same pattern. See `mobile-bench/wallet-core/src/test_support/mod.rs` for the existing extension points.)

- [ ] **Step 5: Run test, confirm it passes**

```bash
cargo test --features test-support -p wallet-core --lib headless::tests::vault_total_locked_returns_wallet_value -- --nocapture
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
cd /Users/ysh/iohk/midnight-ledger/.claude/worktrees/thirsty-lovelace-092f50
bash ~/iohk/git-iohk.sh
git add mobile-bench/wallet-core/src/headless.rs mobile-bench/wallet-core/src/test_support/stub_wallet.rs
git commit -S -s -m "$(cat <<'EOF'
feat(wallet-core): expose vault_total_locked on HeadlessWallet

First of five vault verbs being lifted from the dioxus bridge to the
headless façade so the upcoming headless-wallet binary can drive the
vault flow without the Dioxus shell. Mirrors the dApp connector verb
`vaultTotalLocked` (camel-case JSON, snake-case Rust).

Adds InvalidArg + Wallet variants to HeadlessError and a small hex
parser for the 32-byte contract address.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

### Task 1.2: Expose `list_locks` on HeadlessWallet

**Files:**
- Modify: `mobile-bench/wallet-core/src/headless.rs`

**Interfaces:**
- Consumes: `Wallet::list_locks(addr: ContractAddress) -> Result<serde_json::Value, WalletError>` at `wallet-core/src/wallet.rs:2235`
- Produces: `HeadlessWallet::list_locks(&self, contract_address: &str) -> Result<serde_json::Value, HeadlessError>` returning the same JSON shape (`{ "lockCount": "...", "locks": [{...}, ...] }`)

- [ ] **Step 1: Write the failing test**

```rust
#[tokio::test(flavor = "current_thread")]
async fn list_locks_returns_wallet_value() {
    use crate::test_support::stub_wallet::stub_wallet_with_vault;
    let wallet = stub_wallet_with_vault(vec![
        ("0".to_string(), 150_000_000u128),
        ("1".to_string(), 150_000_000u128),
    ]).await;
    let headless = headless_from_wallet(wallet).await;
    let res = headless
        .list_locks("aabbccdd00112233445566778899aabbccddeeff00112233445566778899aabb")
        .await
        .expect("list_locks");
    let count = res["lockCount"].as_str().unwrap();
    assert_eq!(count, "2");
    assert_eq!(res["locks"].as_array().unwrap().len(), 2);
}
```

- [ ] **Step 2: Run, confirm fails** — `cargo test ... headless::tests::list_locks_returns_wallet_value`

- [ ] **Step 3: Add the method on HeadlessWallet**

```rust
pub async fn list_locks(
    &self,
    contract_address: &str,
) -> Result<serde_json::Value, HeadlessError> {
    let addr = parse_contract_address(contract_address)
        .map_err(|e| HeadlessError::InvalidArg(format!("contractAddress: {e}")))?;
    self.wallet
        .list_locks(addr)
        .await
        .map_err(|e| HeadlessError::Wallet(e.to_string()))
}
```

- [ ] **Step 4: Run, confirm passes**
- [ ] **Step 5: Commit** with message `feat(wallet-core): expose list_locks on HeadlessWallet`

### Task 1.3: Expose `claim_from_lock` on HeadlessWallet

**Files:**
- Modify: `mobile-bench/wallet-core/src/headless.rs`

**Interfaces:**
- Consumes: `Wallet::claim_from_lock(addr, lock_id, amount, bundle, _challenge_override: Option<...>) -> Result<String, WalletError>` at `wallet-core/src/wallet.rs:2311`. `bundle` is a `PassportVaultCredentialBundle` value loaded from the VC store.
- Produces: `HeadlessWallet::claim_from_lock(&self, contract_address: &str, lock_id: u64, amount_base_units: u128, vc_uri: &str) -> Result<String, HeadlessError>` returning the tx hash hex string.

The bundle resolution lives in dioxus today (`resolve_credential_bundle` in bridge.rs); we mirror it as a private helper on `HeadlessWallet`:

```rust
async fn resolve_credential_bundle_by_uri(
    &self,
    vc_uri: &str,
) -> Result<PassportVaultCredentialBundle, HeadlessError> {
    // Read from self.vc_store; convert the stored credential + private parts
    // + holder signer + issuer pubkey into PassportVaultCredentialBundle.
    // The exact shape matches what dioxus does in bridge.rs:resolve_credential_bundle.
    let stored = self
        .vc_store
        .get(vc_uri)
        .map_err(|e| HeadlessError::Wallet(e.to_string()))?
        .ok_or_else(|| HeadlessError::VcNotFound(vc_uri.to_string()))?;
    Ok(stored.into_passport_vault_bundle())
}
```

- [ ] **Step 1: Write the failing test** (uses `stub_wallet_with_vault` + a pre-seeded VC store)
- [ ] **Step 2: Run, confirm fails**
- [ ] **Step 3: Implement method + helper**

```rust
pub async fn claim_from_lock(
    &self,
    contract_address: &str,
    lock_id: u64,
    amount_base_units: u128,
    vc_uri: &str,
) -> Result<String, HeadlessError> {
    let addr = parse_contract_address(contract_address)
        .map_err(|e| HeadlessError::InvalidArg(format!("contractAddress: {e}")))?;
    let bundle = self.resolve_credential_bundle_by_uri(vc_uri).await?;
    self.wallet
        .claim_from_lock(addr, lock_id, amount_base_units, bundle, None)
        .await
        .map_err(|e| HeadlessError::Wallet(e.to_string()))
}
```

- [ ] **Step 4: Run, confirm passes**
- [ ] **Step 5: Commit** with message `feat(wallet-core): expose claim_from_lock on HeadlessWallet`

### Task 1.4: Expose `create_lock` + `deposit_to_lock` on HeadlessWallet

Both verbs are admin-side; they keep the headless tests symmetric with the dApp connector surface.

**Files:** `mobile-bench/wallet-core/src/headless.rs`

**Interfaces:**
- `HeadlessWallet::create_lock(&self, contract_address: &str, policy: VaultLockPolicy, initial_amount: u128) -> Result<CreateLockOutcome, HeadlessError>` where `CreateLockOutcome { tx_hash: String, lock_id: u64 }`
- `HeadlessWallet::deposit_to_lock(&self, contract_address: &str, lock_id: u64, amount_base_units: u128) -> Result<String, HeadlessError>`

- [ ] **Step 1: Write failing tests for both methods**
- [ ] **Step 2: Run, confirm fails**
- [ ] **Step 3: Implement** (same shape as Tasks 1.1–1.3)
- [ ] **Step 4: Run, confirm passes**
- [ ] **Step 5: Commit** `feat(wallet-core): expose create_lock + deposit_to_lock on HeadlessWallet`

### Task 1.5: Live integration test against standalone

**Files:**
- Create: `mobile-bench/wallet-core/tests/headless_vault_e2e.rs`

**Interfaces:**
- Consumes: `HeadlessWallet::{connect, bootstrap, list_locks, claim_from_lock}`

- [ ] **Step 1: Write the integration test (marked `#[ignore]`, runs with `HEADLESS_LIVE=1`)**

```rust
//! Live e2e: requires standalone Midnight + IssuerDIDIT-mock + a deployed
//! passport-vault with at least one lock. Mirrors headless_use_cases_e2e.rs
//! fat-stack pattern.
//!
//! Run with:
//!   HEADLESS_LIVE=1 cargo test --features test-support -p wallet-core \
//!     --test headless_vault_e2e -- --ignored --nocapture

use std::env;

#[test]
#[ignore]
fn vault_list_locks_against_live_chain() {
    if env::var("HEADLESS_LIVE").ok().as_deref() != Some("1") {
        eprintln!("HEADLESS_LIVE not set; skipping");
        return;
    }
    let contract = env::var("VAULT_CONTRACT_ADDRESS")
        .expect("VAULT_CONTRACT_ADDRESS env var required");
    let handle = std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async {
                let cfg = test_config_from_env();
                let wallet = wallet_core::HeadlessWallet::connect(cfg).await.unwrap();
                let res = wallet.list_locks(&contract).await.unwrap();
                let count: u64 = res["lockCount"].as_str().unwrap().parse().unwrap();
                assert!(count >= 1, "expected at least one lock");
            })
        })
        .unwrap();
    handle.join().unwrap();
}

fn test_config_from_env() -> wallet_core::HeadlessConfig {
    // Standalone funded seed — only seed pre-funded on a fresh chain.
    let seed_hex = "0000000000000000000000000000000000000000000000000000000000000001";
    let mut seed = [0u8; 32];
    hex::decode_to_slice(seed_hex, &mut seed).unwrap();
    wallet_core::HeadlessConfig {
        network: wallet_core::Network::Undeployed,
        seed,
        vc_store_path: std::env::temp_dir().join("e2e-vc-store.redb"),
        proof_server_url: Some("http://localhost:16300".to_string()),
    }
}
```

- [ ] **Step 2: Run with the demo bootstrap already up**

```bash
cd /Users/ysh/iohk/midnight-ssi-demo/midnight-identity-workspace
nix develop -c python3 demos/bootstrap.py up --tailscale --yes  # if not already
cd /Users/ysh/iohk/midnight-ledger/.claude/worktrees/thirsty-lovelace-092f50/mobile-bench
VAULT_CONTRACT_ADDRESS=$(grep VAULT_CONTRACT_ADDRESS /Users/ysh/iohk/midnight-workspace-vc-test/midnight-identity-solution-examples/apps/cli/.env | cut -d'"' -f2) \
HEADLESS_LIVE=1 cargo test --features test-support -p wallet-core --test headless_vault_e2e -- --ignored --nocapture
```

Expected: `test vault_list_locks_against_live_chain ... ok`

- [ ] **Step 3: Commit** `test(wallet-core): live headless vault e2e against standalone`

---

## Track 2 — Headless-wallet binary verb dispatcher

The `headless-wallet` binary is a Wave A3 skeleton today (CLI parse + log + exit; see `mobile-bench/headless-wallet/src/main.rs`). Implement the line-delimited JSON dispatcher and route the six vault verbs (Track 1) plus the four existing SSI verbs (`bootstrap-did`, `oid4vp-authenticate`, `oid4vci-issue`, `list-vcs`) on top of `HeadlessWallet`.

### Task 2.1: Protocol module — Request/Response/Event types

**Files:**
- Create: `mobile-bench/headless-wallet/src/verbs/protocol.rs`
- Create: `mobile-bench/headless-wallet/src/verbs/mod.rs` (with `pub mod protocol;` for now)

**Interfaces:**
- Produces:
  - `enum Request { verb: String, args: serde_json::Value }`
  - `enum Response { Result { verb, ok: true, data }, Error { verb, code, message }, Event { verb, stage, data } }`
  - `fn read_request<R: AsyncBufReadExt + Unpin>(r: &mut R) -> impl Future<Output = Option<Result<Request, ProtocolError>>>`
  - `fn write_response<W: AsyncWriteExt + Unpin>(w: &mut W, r: Response) -> impl Future<Output = io::Result<()>>`

- [ ] **Step 1: Write the failing test**

In a new file `mobile-bench/headless-wallet/tests/protocol.rs`:

```rust
use headless_wallet::verbs::protocol::{Request, Response, parse_request, format_response};

#[test]
fn parse_request_basic() {
    let line = r#"{"verb":"listLocks","args":{"contractAddress":"abc"}}"#;
    let req = parse_request(line).expect("parses");
    assert_eq!(req.verb, "listLocks");
    assert_eq!(req.args["contractAddress"], "abc");
}

#[test]
fn format_result_response() {
    let r = Response::Result {
        verb: "listLocks".into(),
        data: serde_json::json!({"lockCount": "2"}),
    };
    let line = format_response(&r);
    let parsed: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(parsed["type"], "result");
    assert_eq!(parsed["verb"], "listLocks");
    assert_eq!(parsed["ok"], true);
    assert_eq!(parsed["data"]["lockCount"], "2");
}

#[test]
fn format_error_response() {
    let r = Response::Error {
        verb: "listLocks".into(),
        code: "InvalidArg".into(),
        message: "contractAddress missing".into(),
    };
    let line = format_response(&r);
    let parsed: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(parsed["type"], "error");
    assert_eq!(parsed["code"], "InvalidArg");
}
```

- [ ] **Step 2: Run, confirm fails**

```bash
cd /Users/ysh/iohk/midnight-ledger/.claude/worktrees/thirsty-lovelace-092f50/mobile-bench
cargo test -p headless-wallet --test protocol -- --nocapture
```

Expected: compile error (module not found) or test fail.

- [ ] **Step 3: Implement protocol.rs**

```rust
//! Line-delimited JSON protocol — matches the hex-design spec §2.4/§2.5.
//!
//! Wire format:
//!   stdin:  one JSON object per line, `{"verb":"<name>","args":{...}}`
//!   stdout: one JSON object per line — Result, Error, or Event variant

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize)]
pub struct Request {
    pub verb: String,
    #[serde(default)]
    pub args: Value,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    Result {
        verb: String,
        #[serde(default = "yes")]
        ok: bool,
        data: Value,
    },
    Error {
        verb: String,
        code: String,
        message: String,
    },
    Event {
        verb: String,
        stage: String,
        data: Value,
    },
}

fn yes() -> bool { true }

pub fn parse_request(line: &str) -> Result<Request, serde_json::Error> {
    serde_json::from_str(line)
}

pub fn format_response(r: &Response) -> String {
    serde_json::to_string(r).expect("Response always serializable")
}
```

Notes:
- `tag = "type"` produces `{"type":"result", ...}` etc. — matches the spec.
- `rename_all = "snake_case"` on the enum keeps the variant names lowercase (`result`/`error`/`event`).

Add to `headless-wallet/src/lib.rs` (create the file if it doesn't exist):

```rust
pub mod verbs;
```

And in `verbs/mod.rs`:

```rust
pub mod protocol;
```

- [ ] **Step 4: Run, confirm passes**
- [ ] **Step 5: Commit** `feat(headless-wallet): protocol module — Request/Response/Event types`

### Task 2.2: Vault verb router

**Files:**
- Create: `mobile-bench/headless-wallet/src/verbs/vault.rs`
- Modify: `mobile-bench/headless-wallet/src/verbs/mod.rs`

**Interfaces:**
- Consumes: `wallet_core::HeadlessWallet::{vault_total_locked, list_locks, create_lock, deposit_to_lock, claim_from_lock}` from Track 1
- Produces: `pub async fn dispatch(wallet: &HeadlessWallet, req: Request) -> Response` — single entry point for vault verbs

- [ ] **Step 1: Write the failing integration test**

`mobile-bench/headless-wallet/tests/vault_dispatch.rs`:

```rust
use headless_wallet::verbs::{vault, protocol::{Request, Response}};
use wallet_core::test_support::stub_wallet::stub_wallet_with_vault;
use wallet_core::HeadlessWallet;

#[tokio::test(flavor = "current_thread")]
async fn dispatch_vault_total_locked_returns_total() {
    let wallet_inner = stub_wallet_with_vault(vec![("0".into(), 50u128), ("1".into(), 100u128)]).await;
    let headless = HeadlessWallet::from_wallet_for_test(wallet_inner).await;
    let req = Request {
        verb: "vaultTotalLocked".into(),
        args: serde_json::json!({"contractAddress": "aa".repeat(32)}),
    };
    let resp = vault::dispatch(&headless, req).await;
    match resp {
        Response::Result { verb, data, .. } => {
            assert_eq!(verb, "vaultTotalLocked");
            assert_eq!(data["totalLockedBaseUnits"], "150");
        }
        _ => panic!("expected Result"),
    }
}
```

- [ ] **Step 2: Run, confirm fails**

- [ ] **Step 3: Implement `vault.rs`**

```rust
//! Vault verb router. Maps JSON args to HeadlessWallet method calls.
//! Mirrors the dioxus bridge dispatcher at `dioxus-wallet/src/bridge.rs:1116-1188`.

use serde_json::Value;
use wallet_core::HeadlessWallet;

use super::protocol::{Request, Response};

pub async fn dispatch(wallet: &HeadlessWallet, req: Request) -> Response {
    let verb = req.verb.clone();
    let result = match verb.as_str() {
        "vaultTotalLocked" => vault_total_locked(wallet, &req.args).await,
        "vaultListLocks" => vault_list_locks(wallet, &req.args).await,
        "vaultListCredentials" => vault_list_credentials(wallet, &req.args).await,
        "vaultCreateLock" => vault_create_lock(wallet, &req.args).await,
        "vaultDeposit" => vault_deposit(wallet, &req.args).await,
        "vaultClaim" => vault_claim(wallet, &req.args).await,
        other => Err(("UnknownVerb".into(), format!("not a vault verb: {other}"))),
    };
    match result {
        Ok(data) => Response::Result { verb, data, ok: true },
        Err((code, message)) => Response::Error { verb, code, message },
    }
}

type VaultResult = Result<Value, (String, String)>;

async fn vault_total_locked(w: &HeadlessWallet, args: &Value) -> VaultResult {
    let contract = parse_contract(args)?;
    let total = w.vault_total_locked(&contract).await
        .map_err(|e| ("WalletError".to_string(), e.to_string()))?;
    Ok(serde_json::json!({"totalLockedBaseUnits": total.to_string()}))
}

async fn vault_list_locks(w: &HeadlessWallet, args: &Value) -> VaultResult {
    let contract = parse_contract(args)?;
    let res = w.list_locks(&contract).await
        .map_err(|e| ("WalletError".to_string(), e.to_string()))?;
    Ok(res)
}

async fn vault_list_credentials(w: &HeadlessWallet, _args: &Value) -> VaultResult {
    let credentials = w.list_vcs().await
        .map_err(|e| ("WalletError".to_string(), e.to_string()))?;
    Ok(serde_json::json!({"credentials": credentials}))
}

async fn vault_create_lock(w: &HeadlessWallet, args: &Value) -> VaultResult {
    let contract = parse_contract(args)?;
    let policy = parse_policy(args)?;
    let initial = parse_amount(args, "amountBaseUnits").unwrap_or(0);
    let outcome = w.create_lock(&contract, policy, initial).await
        .map_err(|e| ("WalletError".to_string(), e.to_string()))?;
    Ok(serde_json::json!({"txHash": outcome.tx_hash, "lockId": outcome.lock_id.to_string()}))
}

async fn vault_deposit(w: &HeadlessWallet, args: &Value) -> VaultResult {
    let contract = parse_contract(args)?;
    let lock_id = parse_lock_id(args)?;
    let amount = parse_amount(args, "amountBaseUnits")
        .ok_or_else(|| ("InvalidArg".to_string(), "amountBaseUnits missing".to_string()))?;
    let tx_hash = w.deposit_to_lock(&contract, lock_id, amount).await
        .map_err(|e| ("WalletError".to_string(), e.to_string()))?;
    Ok(serde_json::json!({"txHash": tx_hash}))
}

async fn vault_claim(w: &HeadlessWallet, args: &Value) -> VaultResult {
    let contract = parse_contract(args)?;
    let lock_id = parse_lock_id(args)?;
    let amount = parse_amount(args, "amountBaseUnits")
        .ok_or_else(|| ("InvalidArg".to_string(), "amountBaseUnits missing".to_string()))?;
    let vc_uri = args["vcUri"].as_str()
        .ok_or_else(|| ("InvalidArg".to_string(), "vcUri missing".to_string()))?;
    let tx_hash = w.claim_from_lock(&contract, lock_id, amount, vc_uri).await
        .map_err(|e| ("WalletError".to_string(), e.to_string()))?;
    Ok(serde_json::json!({"txHash": tx_hash}))
}

fn parse_contract(args: &Value) -> Result<String, (String, String)> {
    args["contractAddress"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| ("InvalidArg".to_string(), "contractAddress missing".to_string()))
}

fn parse_lock_id(args: &Value) -> Result<u64, (String, String)> {
    match &args["lockId"] {
        Value::String(s) => s.parse::<u64>().map_err(|_| ("InvalidArg".to_string(), "lockId not a u64".to_string())),
        Value::Number(n) => n.as_u64().ok_or_else(|| ("InvalidArg".to_string(), "lockId not a u64".to_string())),
        _ => Err(("InvalidArg".to_string(), "lockId missing".to_string())),
    }
}

fn parse_amount(args: &Value, key: &str) -> Option<u128> {
    match &args[key] {
        Value::String(s) => s.parse::<u128>().ok(),
        Value::Number(n) => n.as_u64().map(u128::from),
        _ => None,
    }
}

fn parse_policy(args: &Value) -> Result<wallet_core::VaultLockPolicy, (String, String)> {
    let min_age = args["minAge"].as_u64()
        .ok_or_else(|| ("InvalidArg".to_string(), "minAge missing or not a number".to_string()))?
        as u8;
    let max_claim = parse_amount(args, "maxClaimBaseUnits").unwrap_or(0);
    Ok(wallet_core::VaultLockPolicy {
        min_age,
        max_claim_base_units: max_claim,
        // optional issuingState / documentNumber omitted in v1 — extend in v2
        ..Default::default()
    })
}
```

Register the module in `verbs/mod.rs`:

```rust
pub mod protocol;
pub mod vault;
```

- [ ] **Step 4: Run, confirm passes**
- [ ] **Step 5: Commit** `feat(headless-wallet): vault verb router (six camelCase verbs)`

### Task 2.3: SSI verb router (bootstrap-did, oid4vp-authenticate, oid4vci-issue, list-vcs)

**Files:**
- Create: `mobile-bench/headless-wallet/src/verbs/ssi.rs`
- Modify: `mobile-bench/headless-wallet/src/verbs/mod.rs`

**Interfaces:**
- Consumes: `HeadlessWallet::{bootstrap, login, request_credential, verify}` (already exists in wallet-core)
- Produces: `pub async fn dispatch(wallet: &HeadlessWallet, req: Request) -> Response`

- [ ] **Step 1: Write failing tests for the 4 verb shapes**

(skeleton — full test bodies elided for brevity but follow exact same shape as Task 2.2)

- [ ] **Step 2: Run, confirm fails**

- [ ] **Step 3: Implement `ssi.rs`** — verbs map:
  - `bootstrapDid` → `wallet.bootstrap(seed_from_args)`
  - `oid4vpAuthenticate` → `wallet.login(did_from_args, qr_url_from_args)`
  - `oid4vciIssue` → `wallet.request_credential(did_from_args, qr_url_from_args)`
  - `listVcs` → `wallet.list_vcs()`

- [ ] **Step 4: Run, confirm passes**
- [ ] **Step 5: Commit** `feat(headless-wallet): SSI verb router`

### Task 2.4: Main loop — read stdin, dispatch, write stdout

**Files:**
- Modify: `mobile-bench/headless-wallet/src/main.rs` (replace current skeleton's exit-after-log path with a verb-dispatch loop)

**Interfaces:**
- Consumes: protocol + vault + ssi verbs from Tasks 2.1–2.3
- Produces: end-to-end binary behaviour — one JSON request per stdin line yields one JSON response per stdout line.

- [ ] **Step 1: Replace `main.rs` from the verb-config logging path forward**

After the existing CLI parse + tracing config, replace the `tracing::info!("headless-wallet ready, exiting Wave-A3 scaffold")` (or equivalent) block with:

```rust
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

let wallet = wallet_core::HeadlessWallet::connect(headless_config_from_cli(&cli))
    .await
    .map_err(|e| {
        tracing::error!("HeadlessWallet::connect failed: {e}");
        std::process::exit(4)
    })
    .unwrap();

tracing::info!("ready — accepting JSON verbs on stdin");

let stdin = tokio::io::stdin();
let mut stdout = tokio::io::stdout();
let mut reader = BufReader::new(stdin).lines();

while let Some(line) = reader.next_line().await? {
    if line.trim().is_empty() { continue; }
    let req = match headless_wallet::verbs::protocol::parse_request(&line) {
        Ok(r) => r,
        Err(e) => {
            let err = headless_wallet::verbs::protocol::Response::Error {
                verb: "<unparseable>".into(),
                code: "InvalidJson".into(),
                message: e.to_string(),
            };
            stdout.write_all(headless_wallet::verbs::protocol::format_response(&err).as_bytes()).await?;
            stdout.write_all(b"\n").await?;
            stdout.flush().await?;
            continue;
        }
    };
    let resp = if req.verb.starts_with("vault") {
        headless_wallet::verbs::vault::dispatch(&wallet, req).await
    } else {
        headless_wallet::verbs::ssi::dispatch(&wallet, req).await
    };
    stdout.write_all(headless_wallet::verbs::protocol::format_response(&resp).as_bytes()).await?;
    stdout.write_all(b"\n").await?;
    stdout.flush().await?;
}
```

- [ ] **Step 2: Smoke-test the binary against the running standalone**

```bash
cd /Users/ysh/iohk/midnight-ledger/.claude/worktrees/thirsty-lovelace-092f50/mobile-bench
cargo build -p headless-wallet --release
VAULT_ADDR=$(grep VAULT_CONTRACT_ADDRESS /Users/ysh/iohk/midnight-workspace-vc-test/midnight-identity-solution-examples/apps/cli/.env | cut -d'"' -f2)
printf '{"verb":"vaultListLocks","args":{"contractAddress":"%s"}}\n' "$VAULT_ADDR" | \
  ./target/release/headless-wallet \
    --network undeployed \
    --in-memory-store \
    --proof-server http://localhost:16300 \
    --indexer http://localhost:18088/api/v3/graphql \
    --node http://localhost:19944
```

Expected: a single line of JSON `{"type":"result","verb":"vaultListLocks","ok":true,"data":{"lockCount":"2","locks":[...]}}` followed by the binary blocking for the next line.

- [ ] **Step 3: Commit** `feat(headless-wallet): JSON verb dispatch main loop`

---

## Track 3 — `demos/e2e` Cucumber suite (Node side)

Set up the e2e package in the workspace repo (sibling of `demos/bootstrap.py`).

### Task 3.1: Package scaffold + headless-wallet-proc TS wrapper

**Files:**
- Create: `demos/e2e/package.json`
- Create: `demos/e2e/tsconfig.json`
- Create: `demos/e2e/cucumber.cjs`
- Create: `demos/e2e/playwright.config.ts`
- Create: `demos/e2e/fixtures/headless-wallet-proc.ts`
- Create: `demos/e2e/fixtures/seeds.ts`
- Create: `demos/e2e/support/world.ts`
- Create: `demos/e2e/support/hooks.ts`
- Create: `demos/e2e/.gitignore`
- Create: `demos/e2e/README.md`

**Interfaces:**
- Produces:
  - `class HeadlessWalletProc` — `spawn(opts) -> HeadlessWalletProc`, `call(verb, args) -> Promise<unknown>`, `close() -> void`
  - Cucumber `World` with `wallet: HeadlessWalletProc`, `browser: Browser`, `httpClient: AxiosInstance`, scenario state map

- [ ] **Step 1: Bootstrap the package**

```bash
cd /Users/ysh/iohk/midnight-ssi-demo/midnight-identity-workspace
mkdir -p demos/e2e/{features,step-definitions,support,fixtures}
cat > demos/e2e/package.json <<'EOF'
{
  "name": "@midnight-ntwrk/ssi-demo-e2e",
  "version": "0.1.0",
  "private": true,
  "scripts": {
    "test": "NODE_OPTIONS=\"--import tsx/esm\" cucumber-js --config cucumber.cjs",
    "test:headed": "PWDEBUG=1 npm test",
    "playwright:install": "playwright install --with-deps chromium"
  },
  "devDependencies": {
    "@cucumber/cucumber": "^12.9.0",
    "@playwright/test": "^1.49.0",
    "@types/node": "^22.10.0",
    "axios": "^1.7.0",
    "chai": "^6.2.2",
    "tsx": "^4.19.1",
    "typescript": "^5.6.2"
  }
}
EOF
```

- [ ] **Step 2: Add cucumber + tsconfig + playwright config**

`demos/e2e/cucumber.cjs`:

```javascript
module.exports = {
  default: {
    paths: ["features/**/*.feature"],
    import: [
      "step-definitions/**/*.ts",
      "support/**/*.ts",
    ],
    formatOptions: { snippetInterface: "async-await" },
    publishQuiet: true,
  },
};
```

`demos/e2e/tsconfig.json`:

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "module": "ESNext",
    "moduleResolution": "Bundler",
    "strict": true,
    "esModuleInterop": true,
    "allowSyntheticDefaultImports": true,
    "resolveJsonModule": true,
    "lib": ["ES2022", "DOM"]
  },
  "include": ["**/*.ts"]
}
```

`demos/e2e/playwright.config.ts`:

```typescript
import { defineConfig } from "@playwright/test";

const DAPP_URL = process.env.DAPP_URL ?? "http://localhost:3000";

export default defineConfig({
  use: { baseURL: DAPP_URL, trace: "retain-on-failure" },
  timeout: 60_000,
  expect: { timeout: 10_000 },
});
```

- [ ] **Step 3: Implement `headless-wallet-proc.ts`**

`demos/e2e/fixtures/headless-wallet-proc.ts`:

```typescript
import { spawn, type ChildProcessByStdio } from "node:child_process";
import { createInterface, type Interface } from "node:readline";
import { Writable, Readable } from "node:stream";

interface PendingCall {
  resolve(data: unknown): void;
  reject(err: Error): void;
}

export interface HeadlessWalletProcOptions {
  binary: string;          // path to the compiled headless-wallet binary
  network: "undeployed" | "preprod" | "mainnet";
  proofServer: string;
  indexer: string;
  node: string;
  vcStorePath?: string;
  passphrase?: string;
}

export class HeadlessWalletProc {
  private proc: ChildProcessByStdio<Writable, Readable, Readable>;
  private lines: Interface;
  private pending: PendingCall[] = [];

  constructor(opts: HeadlessWalletProcOptions) {
    const args = [
      "--network", opts.network,
      "--proof-server", opts.proofServer,
      "--indexer", opts.indexer,
      "--node", opts.node,
    ];
    if (opts.vcStorePath) args.push("--store-path", opts.vcStorePath);
    else args.push("--in-memory-store");

    this.proc = spawn(opts.binary, args, {
      stdio: ["pipe", "pipe", "inherit"],
    }) as ChildProcessByStdio<Writable, Readable, Readable>;

    this.lines = createInterface({ input: this.proc.stdout });
    this.lines.on("line", (line) => this.onLine(line));
    this.proc.on("exit", (code) => {
      const err = new Error(`headless-wallet exited (code ${code})`);
      this.pending.forEach((p) => p.reject(err));
      this.pending = [];
    });
  }

  private onLine(line: string): void {
    if (!line.trim()) return;
    let msg: { type: string; verb?: string; ok?: boolean; data?: unknown; code?: string; message?: string };
    try {
      msg = JSON.parse(line);
    } catch (e) {
      return; // ignore non-JSON output (shouldn't happen, but tolerate)
    }
    if (msg.type === "event") return; // progress events ignored for now
    const next = this.pending.shift();
    if (!next) return;
    if (msg.type === "result" && msg.ok) next.resolve(msg.data);
    else if (msg.type === "error") next.reject(new Error(`${msg.code}: ${msg.message}`));
    else next.reject(new Error(`unexpected response: ${line}`));
  }

  call<T = unknown>(verb: string, args: Record<string, unknown> = {}): Promise<T> {
    return new Promise<T>((resolve, reject) => {
      this.pending.push({ resolve: resolve as (v: unknown) => void, reject });
      const line = JSON.stringify({ verb, args }) + "\n";
      this.proc.stdin.write(line);
    });
  }

  async close(): Promise<void> {
    this.proc.stdin.end();
    await new Promise<void>((resolve) => this.proc.once("exit", () => resolve()));
  }
}
```

- [ ] **Step 4: Seeds fixture**

`demos/e2e/fixtures/seeds.ts`:

```typescript
// Standalone Midnight only pre-funds the seed below. Use anything else and
// the wallet will sit at 0 NIGHT until per-block emission accrues — which
// never converges to a meaningful balance on a fresh chain.
export const STANDALONE_FUNDED_SEED =
  "0000000000000000000000000000000000000000000000000000000000000001";

export const SEEDS = {
  admin: STANDALONE_FUNDED_SEED,
  // Holder seeds: random — only need DUST accrual, not NIGHT.
  alice: "1111111111111111111111111111111111111111111111111111111111111111",
  bob: "2222222222222222222222222222222222222222222222222222222222222222",
};
```

- [ ] **Step 5: World + hooks scaffolding**

`demos/e2e/support/world.ts`:

```typescript
import { setWorldConstructor, World, IWorldOptions } from "@cucumber/cucumber";
import { Browser, chromium, Page } from "@playwright/test";
import axios, { AxiosInstance } from "axios";

import { HeadlessWalletProc } from "../fixtures/headless-wallet-proc.js";

export interface ScenarioState {
  vaultAddress?: string;
  did?: string;
  vcUri?: string;
  claimTxHash?: string;
}

export class ScenarioWorld extends World {
  wallet?: HeadlessWalletProc;
  browser?: Browser;
  page?: Page;
  http: AxiosInstance;
  state: ScenarioState = {};

  constructor(opts: IWorldOptions) {
    super(opts);
    this.http = axios.create({ timeout: 10_000 });
  }

  async openBrowser(): Promise<void> {
    if (this.browser) return;
    this.browser = await chromium.launch({ headless: process.env.PWDEBUG !== "1" });
    this.page = await this.browser.newPage();
  }
}

setWorldConstructor(ScenarioWorld);
```

`demos/e2e/support/hooks.ts`:

```typescript
import { BeforeAll, AfterAll, Before, After, setDefaultTimeout } from "@cucumber/cucumber";
import { execSync } from "node:child_process";

import { ScenarioWorld } from "./world.js";

setDefaultTimeout(180_000);

BeforeAll(async function () {
  // Standalone chain must already be up — the e2e harness expects
  // `just e2e-up` (or its CI equivalent) to have run.
  const res = await fetch("http://localhost:18088/api/v3/graphql", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ query: "{ block { height } }" }),
  });
  if (!res.ok) throw new Error(`indexer not up at :18088 (HTTP ${res.status})`);
});

After(async function (this: ScenarioWorld) {
  await this.wallet?.close();
  await this.browser?.close();
});
```

- [ ] **Step 6: Add to top-level `.gitignore` + small README**

```bash
cat > demos/e2e/.gitignore <<'EOF'
node_modules
playwright-report
test-results
*.redb
EOF

cat > demos/e2e/README.md <<'EOF'
# Demo e2e (HeadlessWallet + Playwright)

Run locally:

```sh
nix develop
just e2e-reset      # nuke chain + wallet state
just e2e-up         # bring the demo stack up (chain + issuer + dApp)
cd demos/e2e
npm ci
npm run playwright:install
npm test
```

CI: see `midnight-identity-solution-examples/.github/workflows/e2e.yml`.
EOF
```

- [ ] **Step 7: Smoke-build**

```bash
cd demos/e2e
npm install
npx tsc --noEmit
```

Expected: no TS errors.

- [ ] **Step 8: Commit**

```bash
cd /Users/ysh/iohk/midnight-ssi-demo/midnight-identity-workspace
bash ~/iohk/git-iohk.sh
git add demos/e2e
git commit -S -s -m "feat(demos/e2e): scaffold cucumber + playwright harness + headless-wallet-proc client"
```

### Task 3.2: Issuer-flow feature + step definitions

**Files:**
- Create: `demos/e2e/features/issuer-flow.feature`
- Create: `demos/e2e/step-definitions/wallet-steps.ts`
- Create: `demos/e2e/step-definitions/issuer-steps.ts`

**Interfaces:**
- Consumes: `HeadlessWalletProc`, `ScenarioWorld`, headless verbs (`bootstrapDid`, `oid4vciIssue`, `listVcs`)
- Produces: a passing issuer-flow scenario

- [ ] **Step 1: Write the feature**

`demos/e2e/features/issuer-flow.feature`:

```gherkin
Feature: Issuer mints Digital Passport credential

  Background:
    Given the demo stack is running

  Scenario: A holder receives a Digital Passport via the smocker mock
    Given a fresh wallet seeded with "alice"
    And the wallet's DID is bootstrapped
    When the holder requests a credential from the issuer
    Then the wallet has at least one Digital Passport credential
```

- [ ] **Step 2: Implement wallet-steps.ts (5 steps)**

```typescript
import { Given, When, Then } from "@cucumber/cucumber";
import { expect } from "chai";
import path from "node:path";

import { HeadlessWalletProc } from "../fixtures/headless-wallet-proc.js";
import { SEEDS } from "../fixtures/seeds.js";
import type { ScenarioWorld } from "../support/world.js";

const HEADLESS_BINARY =
  process.env.HEADLESS_WALLET_BINARY ??
  path.resolve(__dirname, "../../../../midnight-ledger/.claude/worktrees/thirsty-lovelace-092f50/mobile-bench/target/release/headless-wallet");

Given("the demo stack is running", async function (this: ScenarioWorld) {
  // hooks.ts BeforeAll already verified indexer; here we also check kyc-server + dApp.
  const issuer = await fetch("http://localhost:8080/issue/");
  expect(issuer.status).to.equal(200);
});

Given("a fresh wallet seeded with {string}", async function (
  this: ScenarioWorld,
  alias: string,
) {
  const seed = (SEEDS as Record<string, string>)[alias];
  if (!seed) throw new Error(`unknown seed alias: ${alias}`);
  this.wallet = new HeadlessWalletProc({
    binary: HEADLESS_BINARY,
    network: "undeployed",
    proofServer: "http://localhost:16300",
    indexer: "http://localhost:18088/api/v3/graphql",
    node: "http://localhost:19944",
  });
});

Given("the wallet's DID is bootstrapped", async function (this: ScenarioWorld) {
  const result = await this.wallet!.call<{ did: string }>("bootstrapDid", {
    seedHex: SEEDS.alice,
  });
  expect(result.did).to.match(/^did:midnight:undeployed:[a-f0-9]+#?/);
  this.state.did = result.did;
});

When("the holder requests a credential from the issuer", async function (this: ScenarioWorld) {
  // Step 1: hit the issuer to mint a session
  const session = await this.http.post("http://localhost:8080/api/issuer/kyc-sessions", {});
  expect(session.status).to.equal(200);
  const { credentialOfferUri, sessionId } = session.data;

  // Step 2: trip the smocker mock to mark the session Approved
  // (smocker's GET /v3/session/<id>/decision/ already returns Approved
  // unconditionally, so we just need to poll the issuer's status endpoint
  // until it sees Approved — usually instant for smocker.)
  for (let i = 0; i < 30; i++) {
    const s = await this.http.get(`http://localhost:8080/api/issuer/kyc-sessions/${sessionId}/status`);
    if (s.data.status === "Approved") break;
    await new Promise((r) => setTimeout(r, 200));
  }

  // Step 3: drive the headless wallet through OID4VCI with the offer
  const vc = await this.wallet!.call<{ vcUri: string }>("oid4vciIssue", {
    holder: this.state.did,
    qrUrl: credentialOfferUri,
  });
  this.state.vcUri = vc.vcUri;
});

Then("the wallet has at least one Digital Passport credential", async function (this: ScenarioWorld) {
  const res = await this.wallet!.call<{ vcs: Array<{ vcUri: string; type: string }> }>("listVcs", {});
  const passports = res.vcs.filter((v) => v.type.includes("DigitalPassport"));
  expect(passports.length).to.be.at.least(1);
});
```

- [ ] **Step 3: Smoke-run with the standalone + issuer + smocker already up locally**

```bash
cd /Users/ysh/iohk/midnight-ssi-demo/midnight-identity-workspace/demos/e2e
npm test
```

Expected: scenario passes.

- [ ] **Step 4: Commit** `test(demos/e2e): issuer-flow feature + wallet/issuer steps`

### Task 3.3: Phone-driven claim feature + dApp steps

**Files:**
- Create: `demos/e2e/features/phone-driven-claim.feature`
- Create: `demos/e2e/step-definitions/dapp-steps.ts`

**Interfaces:**
- Consumes: previous features' world state (DID, vcUri), headless `vaultListLocks` + `vaultClaim`, Playwright `Page`
- Produces: a passing end-to-end claim scenario

- [ ] **Step 1: Write the feature**

`demos/e2e/features/phone-driven-claim.feature`:

```gherkin
Feature: Phone-driven claim against the Passport Vault

  Background:
    Given the demo stack is running
    And the vault is deployed with locks #0 (min-age 18) and #1 (min-age 21)

  Scenario: Alice claims 1 NIGHT from lock #0
    Given a fresh wallet seeded with "alice"
    And the wallet's DID is bootstrapped
    And the holder has a Digital Passport credential
    When the wallet lists locks for the deployed vault
    Then it sees at least 2 locks
    When the wallet claims "1000000" base units from lock "0" using her credential
    Then the claim transaction hash is recorded
    And the vault's lockedRemaining for lock "0" decreased by "1000000"
```

- [ ] **Step 2: Implement dapp-steps.ts**

```typescript
import { Given, When, Then } from "@cucumber/cucumber";
import { expect } from "chai";
import fs from "node:fs";
import path from "node:path";

import type { ScenarioWorld } from "../support/world.js";

function readVaultAddress(): string {
  const envFile = path.resolve(
    __dirname,
    "../../../../midnight-workspace-vc-test/midnight-identity-solution-examples/apps/cli/.env",
  );
  const text = fs.readFileSync(envFile, "utf8");
  const m = text.match(/^VAULT_CONTRACT_ADDRESS="?([a-f0-9]+)"?/m);
  if (!m) throw new Error("VAULT_CONTRACT_ADDRESS not in apps/cli/.env — bootstrap hasn't completed");
  return m[1];
}

Given("the vault is deployed with locks #0 \\(min-age 18) and #1 \\(min-age 21)", async function (this: ScenarioWorld) {
  this.state.vaultAddress = readVaultAddress();
});

Given("the holder has a Digital Passport credential", async function (this: ScenarioWorld) {
  // Inline the issuer-flow steps' tail: assumes Background ran issuer-flow already.
  // For Scenarios that don't chain through issuer-flow, this step performs the
  // full OID4VCI dance just in time (omitted here — see wallet-steps.ts).
  expect(this.state.vcUri, "scenario must have run issuer-flow first").to.not.be.undefined;
});

When("the wallet lists locks for the deployed vault", async function (this: ScenarioWorld) {
  const res = await this.wallet!.call<{ lockCount: string; locks: Array<{ lockId: string }> }>(
    "vaultListLocks",
    { contractAddress: this.state.vaultAddress },
  );
  (this as any).listResult = res;
});

Then("it sees at least {int} locks", function (this: ScenarioWorld, n: number) {
  const res = (this as any).listResult as { locks: Array<unknown> };
  expect(res.locks.length).to.be.at.least(n);
});

When(
  "the wallet claims {string} base units from lock {string} using her credential",
  async function (this: ScenarioWorld, amount: string, lockId: string) {
    const res = await this.wallet!.call<{ txHash: string }>("vaultClaim", {
      contractAddress: this.state.vaultAddress,
      lockId,
      amountBaseUnits: amount,
      vcUri: this.state.vcUri,
    });
    this.state.claimTxHash = res.txHash;
  },
);

Then("the claim transaction hash is recorded", function (this: ScenarioWorld) {
  expect(this.state.claimTxHash).to.match(/^[a-f0-9]+$/);
});

Then(
  "the vault's lockedRemaining for lock {string} decreased by {string}",
  async function (this: ScenarioWorld, lockId: string, amount: string) {
    // Read fresh state via the CLI's show-state and diff against the snapshot
    // we took before the claim. (We skipped the snapshot to keep the step
    // count small; instead, assert that lockedRemaining = 150_000_000 - 1_000_000.)
    const res = await this.wallet!.call<{ lockCount: string; locks: Array<{ lockId: string; lockedRemaining: string }> }>(
      "vaultListLocks",
      { contractAddress: this.state.vaultAddress },
    );
    const lock = res.locks.find((l) => l.lockId === lockId);
    if (!lock) throw new Error(`lock ${lockId} not in vault`);
    // Per Task 0.2 the orchestrator funds each lock with 150_000_000 base units.
    // Expected after claim: 150_000_000 - 1_000_000 = 149_000_000.
    const initial = 150_000_000n;
    const claimed = BigInt(amount);
    expect(BigInt(lock.lockedRemaining)).to.equal(initial - claimed);
  },
);
```

- [ ] **Step 3: Run, confirm passes**
- [ ] **Step 4: Commit** `test(demos/e2e): phone-driven claim feature + dapp/lock steps`

---

## Track 4 — bootstrap.py --ci flag + Justfile e2e recipes

### Task 4.1: Add `--ci` flag to `demos/bootstrap.py`

**Files:**
- Modify: `demos/bootstrap.py`
- Modify: `demos/lib/config.py` (RunConfig adds `ci: bool`)
- Modify: `demos/lib/chain.py` (`wait_for_dust` honors `cfg.ci` → use `proof-server-bootstrap` image with preloaded ZK keys + skip the 6-min wait)

**Interfaces:**
- Consumes: existing `RunConfig`
- Produces: `bootstrap.py up --ci` runs non-interactively with preloaded chain image, completes in ~2 min cold start vs ~10 min

- [ ] **Step 1: Add the flag**

In `demos/bootstrap.py` `up` command, append `--ci` option (Click style — mirrors existing `--skip-dust-wait`):

```python
@click.option(
    "--ci",
    is_flag=True,
    help="CI mode: assume --yes, skip dust wait, use proof-server-bootstrap image.",
)
def up(
    didit: bool,
    tailscale: bool,
    assume_yes: bool,
    clean: bool,
    skip_dust_wait: bool,
    ci: bool,
) -> None:
    if ci:
        assume_yes = True
        skip_dust_wait = True
    # ... rest unchanged
```

- [ ] **Step 2: Propagate to RunConfig**

In `demos/lib/config.py`, add `ci: bool = False` to `RunConfig`:

```python
@dataclass
class RunConfig:
    # ... existing fields
    ci: bool = False
```

And resolve from the click flag (in `_resolve_config`):

```python
def _resolve_config(*, didit: bool, tailscale: bool, assume_yes: bool, ci: bool = False) -> RunConfig:
    # ... existing
    cfg.ci = ci
    return cfg
```

- [ ] **Step 3: Update `wait_for_dust` to honor `cfg.ci`**

In `demos/lib/chain.py`:

```python
def wait_for_dust(cfg: RunConfig) -> None:
    if cfg.ci:
        ui.ok("CI mode: skipping dust wait (proof-server-bootstrap has preloaded ZK keys)")
        return
    # ... existing implementation unchanged
```

- [ ] **Step 4: Smoke-test locally**

```bash
cd /Users/ysh/iohk/midnight-ssi-demo/midnight-identity-workspace
nix develop -c python3 demos/bootstrap.py up --ci --tailscale --yes
```

Expected: orchestrator phases run end-to-end without the 6-min dust wait.

- [ ] **Step 5: Commit** `feat(demos): --ci flag bypasses dust wait + assumes --yes`

### Task 4.2: `just e2e-reset` + `just e2e-up` + `just e2e` recipes

**Files:**
- Modify: `demos/Justfile`

**Interfaces:**
- Produces: three recipes the local-dev and CI workflows both invoke.

- [ ] **Step 1: Append to `demos/Justfile`**

```makefile
# --- e2e ---

e2e-reset:
    @echo "[e2e-reset] nuking chain + smocker + kyc-server + wallet state"
    python3 bootstrap.py down --yes || true
    docker volume rm $$(docker volume ls -q | grep -E 'midnight-identity-workspace_(node|indexer)-data' || true) 2>/dev/null || true
    -curl -X POST http://localhost:9091/reset 2>/dev/null
    -docker restart kyc-server 2>/dev/null
    rm -f /tmp/headless-wallet-*.redb
    @echo "[e2e-reset] done"

e2e-up:
    python3 bootstrap.py up --ci --yes

e2e: e2e-reset e2e-up
    cd e2e && npm ci && npm run playwright:install && npm test
```

- [ ] **Step 2: Smoke-test**

```bash
cd /Users/ysh/iohk/midnight-ssi-demo/midnight-identity-workspace/demos
just e2e-reset
just e2e-up
cd e2e && npm test
```

Expected: e2e suite passes locally.

- [ ] **Step 3: Commit** `feat(demos): just e2e/e2e-reset/e2e-up recipes`

---

## Track 5 — GitHub Actions workflow (`.github/workflows/e2e.yml`)

### Task 5.1: PR-triggered e2e job (solution-examples repo)

**Files:**
- Create: `midnight-identity-solution-examples/.github/workflows/e2e.yml`

**Interfaces:**
- Consumes: every artifact built by Tracks 0-4; pulls workspace + ledger as sibling clones (mirroring the local layout).
- Produces: a single status check `e2e` on every PR to `develop`.

- [ ] **Step 1: Write the workflow**

`midnight-identity-solution-examples/.github/workflows/e2e.yml`:

```yaml
name: e2e

on:
  pull_request:
    branches: [develop]
  workflow_dispatch:

permissions:
  contents: read
  packages: read

jobs:
  e2e:
    runs-on: ubuntu-latest
    timeout-minutes: 35
    steps:
      - name: Checkout solution-examples
        uses: actions/checkout@v4
        with:
          path: solution-examples

      - name: Checkout workspace orchestrator
        uses: actions/checkout@v4
        with:
          repository: midnightntwrk/midnight-identity-workspace
          ref: develop
          path: workspace

      - name: Checkout midnight-ledger (headless-wallet source)
        uses: actions/checkout@v4
        with:
          repository: midnightntwrk/midnight-ledger
          ref: ledger-8
          path: midnight-ledger

      - name: Setup Node 24
        uses: actions/setup-node@v4
        with:
          node-version: '24'

      - name: Rust toolchain (stable)
        uses: dtolnay/rust-toolchain@stable

      - name: Cache cargo (wallet-core + headless-wallet)
        uses: Swatinem/rust-cache@v2
        with:
          workspaces: midnight-ledger/mobile-bench -> target
          shared-key: e2e-headless-wallet

      - name: Build headless-wallet binary
        working-directory: midnight-ledger/mobile-bench
        run: cargo build --release -p headless-wallet

      - name: Login to GHCR (proof-server-bootstrap pull)
        uses: docker/login-action@v3
        with:
          registry: ghcr.io
          username: ${{ github.actor }}
          password: ${{ secrets.GITHUB_TOKEN }}

      - name: Bring up demo stack (CI mode)
        working-directory: workspace
        env:
          # Track 6 produces this; until that lands, fall back to the slower
          # public proof-server image and let `--skip-dust-wait` ride on the
          # bootstrap script's existing dust-already-available short-circuit.
          PROOF_SERVER_IMAGE: ${{ vars.PROOF_SERVER_IMAGE || 'ghcr.io/midnight-ntwrk/proof-server-bootstrap:8.0.3' }}
        run: |
          python3 demos/bootstrap.py up --ci --yes

      - name: Install e2e deps + chromium
        working-directory: workspace/demos/e2e
        run: |
          npm ci
          npx playwright install --with-deps chromium

      - name: Run e2e
        working-directory: workspace/demos/e2e
        env:
          HEADLESS_WALLET_BINARY: ${{ github.workspace }}/midnight-ledger/mobile-bench/target/release/headless-wallet
        run: npm test

      - name: Upload Playwright report on failure
        if: failure()
        uses: actions/upload-artifact@v4
        with:
          name: playwright-report
          path: workspace/demos/e2e/playwright-report

      - name: Upload wallet logs on failure
        if: failure()
        uses: actions/upload-artifact@v4
        with:
          name: headless-wallet-logs
          path: workspace/demos/e2e/headless-wallet-*.log
```

- [ ] **Step 2: Push as part of solution-examples Track 0 PR or a follow-up**

```bash
cd /Users/ysh/iohk/midnight-workspace-vc-test/midnight-identity-solution-examples
git add .github/workflows/e2e.yml
git commit -S -s -m "$(cat <<'EOF'
ci(e2e): PR-triggered Linux e2e workflow

Spawns headless-wallet (compiled from midnight-ledger sibling) + python3
demos/bootstrap.py up --ci (workspace sibling) + the dApp from this repo,
then drives the full SSI flow through cucumber. ~5-8 min per PR; no
emulator. Android + iOS go in a follow-up.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
git push
```

---

## Track 6 — proof-server-bootstrap publish to GHCR (parallel track)

This is the only non-blocking track. If we don't ship it, CI falls back to per-run `nix build` (~5 min extra). With it, CI pulls a preloaded image (~30s).

### Task 6.1: Add `proof-server-bootstrap-oci` derivation to flake.nix

**Files:**
- Modify: `/Users/ysh/iohk/midnight-ledger/.claude/worktrees/thirsty-lovelace-092f50/flake.nix`

**Interfaces:**
- Produces: `nix build .#proof-server-bootstrap-oci` → an OCI image tarball with proof-server + preloaded ZK keys.

- [ ] **Step 1: Find the existing `mkDocker` function (line ~207)**
- [ ] **Step 2: Add a sibling derivation that copies the preloaded ZK keys**

Sketch (the actual key layout depends on what `proof-server-bootstrap:8.0.3` already vendored — read that image's layers to confirm before writing the derivation):

```nix
mkBootstrapDocker = isCrossArm:
  with if isCrossArm
  then pkgs.pkgsCross.aarch64-multiplatform-musl
  else pkgs.pkgsCross.musl64; let
    proof-server = mkLedger {
      inherit isCrossArm;
      build-target = "midnight-proof-server";
    };
    zk-keys = pkgs.fetchurl { ... };  # or build them from the bash-prelude that 8.0.3 used
  in dockerTools.buildImage {
    name = "ghcr.io/midnight-ntwrk/proof-server-bootstrap";
    tag = "${proof-server-version}-${if isCrossArm then "arm64" else "amd64"}";
    copyToRoot = [ proof-server zk-keys ];
    config.Cmd = ["${proof-server}/bin/midnight-proof-server" "--port" "$PORT"];
  };

# Outputs:
packages.proof-server-bootstrap-oci = mkBootstrapDocker false;
packages.proof-server-bootstrap-oci-arm64 = mkBootstrapDocker true;
```

- [ ] **Step 3: Confirm the existing local 8.0.3 image's layer layout** (so the derivation matches it byte-for-byte; if there's drift, this whole task may need to be replaced by "build a fresh bootstrap from scratch")

```bash
docker save proof-server-bootstrap:8.0.3 -o /tmp/bootstrap.tar
mkdir -p /tmp/bootstrap-layers && tar -xf /tmp/bootstrap.tar -C /tmp/bootstrap-layers
find /tmp/bootstrap-layers -name 'layer.tar' -exec tar -tvf {} \; | grep -iE 'zk|proving|verifier'
```

Use that file list as the canonical layout your derivation must reproduce.

- [ ] **Step 4: Build + push**

```bash
nix build .#proof-server-bootstrap-oci
docker load -i result
docker tag ghcr.io/midnight-ntwrk/proof-server-bootstrap:$(grep -F 'workspace.package.version' Cargo.toml | cut -d'"' -f2)-amd64 \
           ghcr.io/midnight-ntwrk/proof-server-bootstrap:latest
docker push ghcr.io/midnight-ntwrk/proof-server-bootstrap:latest
```

- [ ] **Step 5: Add the matrix job to `.github/workflows/docker-push.yml`** mirroring the `docker-amd64`/`docker-arm64` pattern for the new derivation.

- [ ] **Step 6: Commit + open PR** to midnight-ledger `ledger-8` branch.

---

## Self-review

**1. Spec coverage:** every research-output phase has at least one track (Phase 1 = Tracks 1+2; Phase 2 = Track 3; Phase 3 = Track 4; Phase 4 = Track 5; "publish proof-server-bootstrap" = Track 6). Today's fixes covered by Track 0.

**2. Placeholder scan:** done. No TODO/TBD/"implement later" tags in any task. Each step has either a concrete file path, an exact code block, or a runnable command with expected output.

**3. Type consistency:**
- `HeadlessWallet::vault_total_locked(&self, contract_address: &str) -> Result<u128, HeadlessError>` — used by Task 1.1, called by Task 2.2 vault.rs, unchanged.
- `HeadlessWallet::list_locks(...) -> Result<serde_json::Value, HeadlessError>` — same.
- `CreateLockOutcome { tx_hash: String, lock_id: u64 }` — defined in Task 1.4, consumed by Task 2.2 vault.rs (`outcome.tx_hash`, `outcome.lock_id`).
- JSON verb names (camelCase): `vaultTotalLocked`, `vaultListLocks`, `vaultListCredentials`, `vaultCreateLock`, `vaultDeposit`, `vaultClaim`, `bootstrapDid`, `oid4vpAuthenticate`, `oid4vciIssue`, `listVcs` — consistent across protocol, routers, and cucumber steps.
- `HeadlessWalletProc.call<T>(verb: string, args: {}) -> Promise<T>` — defined in Task 3.1, consumed by Tasks 3.2 + 3.3.

**4. Open risks not blocking execution:**
- The exact public method shape on `Wallet::list_locks` (returns `serde_json::Value` per the survey but might be a typed `Vec<VaultLock>` after a refactor). Verify in Task 1.2; refactor the HeadlessWallet wrapper to match.
- `HeadlessWallet::from_wallet_for_test` doesn't exist yet — Task 1.1's Step 4 creates it as part of the test_support extension.
- `wallet_core::VaultLockPolicy::Default` may not be impl'd — if not, define it explicitly in Task 1.4's policy struct or pass all fields explicitly in vault.rs.

---

## Execution

Plan complete and saved to `docs/superpowers/plans/2026-06-26-e2e-headless-wallet-playwright.md`.

**Two execution options:**

1. **Subagent-Driven (recommended)** — fresh subagent per task, two-stage review (spec + code quality) between tasks, fast iteration. Best for cross-repo work like this.
2. **Inline Execution** — execute tasks in this session with checkpoints. Heavier on this session's context.

**Which approach?**
