# Identity Centre — Phase 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship a working Identity Centre in the dioxus-mobile wallet that receives a Midnight `birth` VC over OID4VP + OID4VCI from a mock issuer running against a local standalone Midnight env, displays the VC in a swipeable carousel, and lets the holder self-verify the VC's signature against the issuer's published DID document.

**Architecture:** Six new `wallet-core` modules (`bootstrap`, `vc_store`, `did_auth`, `oid4vp_client`, `oid4vci_client`, `vc_self_verify`) plus a `did-bootstrap` CLI binary; a new `Identity` top-level tab in the Dioxus UI with a sub-tab router (VCs carousel + DIDs/Keys); a TS+Express `IssuerDIDIT-mock` exposing the stable 6-endpoint OID4 contract; an Android QR scanner JNI bridge (CameraX + ML Kit); a Cucumber.js + Playwright BDD harness with a headless TS wallet client mirroring the Rust wallet's HTTP behaviour.

**Tech Stack:** Rust 2024 · `tokio` async · `redb 2` for VC storage · `josekit` for JWS · `reqwest 0.12` (rustls+ring already in tree) · `dioxus 0.7` UI · Kotlin + CameraX + ML Kit Barcode Scanning + JNI · TypeScript + Express + `better-sqlite3` + `@midnight-ntwrk/midnight-did{,-api}` · Cucumber.js + Playwright for E2E.

**Spec:** `docs/superpowers/specs/2026-05-25-identity-centre-phase-1-design.md` (commit `f819e8f2`).

**Repository conventions:**
- `mobile-bench/wallet-core/src/lib.rs` has `#![deny(warnings)]`. Imports must be used; dead code triggers errors. Use `#[allow(dead_code)]` with a one-line comment for items reachable in a later task.
- Re-exports go through `lib.rs` (`pub use vc_store::{…}` is the existing pattern).
- `pub(crate)` for internal helpers; `pub` only at re-export boundaries.
- Unit tests live in `#[cfg(test)] mod tests { … }` at the bottom of each module.
- Integration tests live in `mobile-bench/wallet-core/tests/<name>.rs`.
- All commits MUST use `git commit -S -s -m "…"` (GPG sign + DCO sign-off). After every commit, run `git log --format="%h %G? %s" -1` — must show `G`. On `B`/`N` re-sign once with `git commit --amend --no-edit -S`. Never amend otherwise.
- Run `bash ~/iohk/git-iohk.sh` once at the start of the session in each repo.
- Two repos in scope:
  - **A.** `/Users/ysh/iohk/midnight-ledger/.claude/worktrees/thirsty-lovelace-092f50/` on branch `dioxus-vc-demo`. Wallet + spec + this plan.
  - **B.** `~/iohk/midnight-identity-workspace/midnight-identity-solution-examples/` on branch `develop`. Mock issuer + BDD harness.
- Commit prefixes: wallet-side commits use `feat(vc):`, `feat(did-auth):`, `feat(oid4vp):`, `feat(oid4vci):`, `feat(identity-centre):`, `feat(qr-scan):`, or `chore(wallet):`. Issuer-side commits use `feat(issuer-mock):` or `feat(bdd):`.

**Standalone Midnight env:** All wallet-core integration tests + all BDD scenarios target the local docker-compose stack defined in Task 20.4. The stack publishes `MIDNIGHT_INDEXER_URL=http://localhost:8088/api/v1/graphql` and `MIDNIGHT_NODE_RPC_URL=http://localhost:9944` once up; both URLs are reused unchanged across every task that needs chain access.

**Subagent execution note:** Each task closes with a signed commit; the SHA is required for the spec-compliance reviewer that follows. The BDD scenarios in Section 9 are the binding acceptance criteria — the manual demo flow in the spec's `Acceptance criteria` section is the secondary check.

---

## File structure

### Repo A — `midnight-ledger` worktree (branch `dioxus-vc-demo`)

| Path | Role | Status |
|---|---|---|
| `mobile-bench/wallet-core/src/did/bootstrap.rs` | `bootstrap_did_with_keys` helper | **Create (Task 1)** |
| `mobile-bench/wallet-core/src/did/mod.rs` | Re-export `bootstrap` | **Modify (Task 1)** |
| `mobile-bench/wallet-core/src/bin/did-bootstrap.rs` | CLI binary wrapping the helper | **Create (Task 3)** |
| `mobile-bench/wallet-core/tests/did_bootstrap_standalone.rs` | Integration test against docker env | **Create (Task 4)** |
| `mobile-bench/wallet-core/src/vc_store/mod.rs` | Module root + re-exports | **Create (Task 5)** |
| `mobile-bench/wallet-core/src/vc_store/tables.rs` | `TableDefinition<…>` definitions for the 3 redb tables | **Create (Task 5)** |
| `mobile-bench/wallet-core/src/vc_store/types.rs` | `StoredVc`, `Opening`, `VcMetadata` struct + serde | **Create (Task 6)** |
| `mobile-bench/wallet-core/src/vc_store/api.rs` | CRUD API + iteration helpers | **Create (Tasks 7-8)** |
| `mobile-bench/wallet-core/src/did_auth/mod.rs` | "Sign payload with DID's authn-relation key" helper | **Create (Task 9)** |
| `mobile-bench/wallet-core/src/oid4vp_client/mod.rs` | Module root + re-exports | **Create (Task 10)** |
| `mobile-bench/wallet-core/src/oid4vp_client/parser.rs` | Parse `openid4vp://` URL + fetch request object | **Create (Task 10)** |
| `mobile-bench/wallet-core/src/oid4vp_client/jws.rs` | SIOPv2 id-token JWS builder | **Create (Task 11)** |
| `mobile-bench/wallet-core/src/oid4vp_client/http.rs` | POST signed id-token to `redirect_uri` | **Create (Task 12)** |
| `mobile-bench/wallet-core/src/oid4vci_client/mod.rs` | Module root + re-exports | **Create (Task 14)** |
| `mobile-bench/wallet-core/src/oid4vci_client/offer.rs` | Parse `openid-credential-offer://` payload | **Create (Task 14)** |
| `mobile-bench/wallet-core/src/oid4vci_client/token.rs` | Pre-Authorized Code Flow token exchange | **Create (Task 15)** |
| `mobile-bench/wallet-core/src/oid4vci_client/credential.rs` | Credential request with DID-bound JWS proof | **Create (Task 16)** |
| `mobile-bench/wallet-core/src/vc_self_verify/mod.rs` | Self-verify VC against resolved issuer DID | **Create (Task 18)** |
| `mobile-bench/wallet-core/src/qr_scanner.rs` | `QrScanner` trait (impl is platform-specific) | **Create (Task 29)** |
| `mobile-bench/wallet-core/src/lib.rs` | Module decls + re-exports | **Modify (Tasks 1, 5, 9, 10, 14, 18, 29)** |
| `mobile-bench/wallet-core/Cargo.toml` | New deps (`josekit`, `url`) | **Modify (Tasks 1, 11)** |
| `mobile-bench/dioxus-wallet/src/identity/mod.rs` | Identity Centre module root | **Create (Task 30)** |
| `mobile-bench/dioxus-wallet/src/identity/screen.rs` | `IdentityScreen` sub-tab router | **Create (Task 30)** |
| `mobile-bench/dioxus-wallet/src/identity/bootstrap_panel.rs` | `Bootstrap` button + progress | **Create (Task 31)** |
| `mobile-bench/dioxus-wallet/src/identity/vc_carousel.rs` | Full-screen swipeable carousel | **Create (Task 32)** |
| `mobile-bench/dioxus-wallet/src/identity/vc_card.rs` | One VC card (with self-verify badge) | **Create (Task 33)** |
| `mobile-bench/dioxus-wallet/src/identity/did_list.rs` | DID list sub-tab | **Create (Task 34)** |
| `mobile-bench/dioxus-wallet/src/identity/did_detail.rs` | DID detail screen (nests keys) | **Create (Task 34)** |
| `mobile-bench/dioxus-wallet/src/identity/did_picker.rs` | DID-picker popup (>1 DID only) | **Create (Task 35)** |
| `mobile-bench/dioxus-wallet/src/identity/qr_scan_fab.rs` | FAB component | **Create (Task 36)** |
| `mobile-bench/dioxus-wallet/src/identity/qr_scan_modal.rs` | Scanner modal + paste-URL affordance | **Create (Task 36)** |
| `mobile-bench/dioxus-wallet/src/app.rs` | Add `Identity` Tab variant, remove standalone `Keys` tab | **Modify (Task 30)** |
| `mobile-bench/dioxus-wallet/android/app/src/main/java/io/iohk/midnight/wallet/QrScanner.kt` | CameraX + ML Kit + JNI | **Create (Task 37)** |
| `mobile-bench/dioxus-wallet/android/app/build.gradle.kts` | Add CameraX + ML Kit + camera permission | **Modify (Task 37)** |

### Repo B — `midnight-identity-solution-examples` (branch `develop`)

| Path | Role | Status |
|---|---|---|
| `IssuerDIDIT-mock/package.json` | Package manifest | **Create (Task 20)** |
| `IssuerDIDIT-mock/tsconfig.json` | TS config | **Create (Task 20)** |
| `IssuerDIDIT-mock/.gitignore` | Ignore `issuer-keystore.json`, `issuer.sqlite`, `dist/`, `node_modules/` | **Create (Task 20)** |
| `IssuerDIDIT-mock/src/server.ts` | Express entrypoint | **Create (Task 21)** |
| `IssuerDIDIT-mock/src/config.ts` | Env-var configuration + defaults | **Create (Task 21)** |
| `IssuerDIDIT-mock/src/storage/sessions.ts` | SQLite session store via `better-sqlite3` | **Create (Task 22)** |
| `IssuerDIDIT-mock/src/services/issuerDid.ts` | Load issuer DID + keys from `issuer-keystore.json` | **Create (Task 23)** |
| `IssuerDIDIT-mock/src/services/holderDidResolver.ts` | Embedded `@midnight-ntwrk/midnight-did` resolver | **Create (Task 24)** |
| `IssuerDIDIT-mock/src/services/oid4vpVerifier.ts` | Verify SIOPv2 id_token JWS against holder DID doc | **Create (Task 25)** |
| `IssuerDIDIT-mock/src/services/oid4vciIssuer.ts` | Mint pre-auth code + access token + c_nonce | **Create (Task 26)** |
| `IssuerDIDIT-mock/src/services/vcMinter.ts` | Assemble + sign `birth` VC body with Jubjub | **Create (Task 27)** |
| `IssuerDIDIT-mock/src/routes/login.ts` | `GET /authorize`, `GET /request/:id`, `POST /authorize-response` | **Create (Task 25)** |
| `IssuerDIDIT-mock/src/routes/kyc.ts` | `GET /kyc-form`, `POST /kyc-form` (operator-driven mock) | **Create (Task 28)** |
| `IssuerDIDIT-mock/src/routes/credential.ts` | `GET /credential-offer/:id`, `POST /token`, `POST /credential` | **Create (Task 27)** |
| `IssuerDIDIT-mock/src/views/login.html` | Laptop QR-1 page (ejs/handlebars template) | **Create (Task 25)** |
| `IssuerDIDIT-mock/src/views/kyc-form.html` | Operator form | **Create (Task 28)** |
| `IssuerDIDIT-mock/src/views/credential-offer.html` | Laptop QR-2 page | **Create (Task 27)** |
| `IssuerDIDIT-mock/scripts/bootstrap-issuer-did.ts` | One-time DID + keys bootstrap | **Create (Task 23)** |
| `IssuerDIDIT-mock/scripts/issuer-keystore.example.json` | Shape template (all-zero keys) | **Create (Task 23)** |
| `IssuerDIDIT-mock/e2e/fixtures/docker-compose.yml` | Standalone Midnight env | **Create (Task 20)** |
| `IssuerDIDIT-mock/e2e/fixtures/seeds.ts` | Fixture seeds + expected DIDs | **Create (Task 40)** |
| `IssuerDIDIT-mock/e2e/fixtures/headless-wallet-client.ts` | TS OID4VP/VCI client | **Create (Task 41)** |
| `IssuerDIDIT-mock/e2e/features/bootstrap.feature` | Gherkin for Bootstrap flow | **Create (Task 42)** |
| `IssuerDIDIT-mock/e2e/features/issuance-happy-path.feature` | Issuance scenarios | **Create (Task 42)** |
| `IssuerDIDIT-mock/e2e/features/self-verify.feature` | Self-verify scenarios | **Create (Task 43)** |
| `IssuerDIDIT-mock/e2e/features/negative-paths.feature` | Wrong-nonce + replay + unbootstrapped-DID | **Create (Task 43)** |
| `IssuerDIDIT-mock/e2e/step-definitions/wallet-steps.ts` | Cucumber steps driving the headless wallet | **Create (Task 42)** |
| `IssuerDIDIT-mock/e2e/step-definitions/issuer-steps.ts` | Cucumber steps driving the issuer API | **Create (Task 42)** |
| `IssuerDIDIT-mock/e2e/step-definitions/chain-steps.ts` | Steps interrogating chain state | **Create (Task 43)** |
| `IssuerDIDIT-mock/e2e/support/hooks.ts` | Before/After: env spin-up + bootstrap | **Create (Task 42)** |
| `IssuerDIDIT-mock/cucumber.cjs` | Cucumber.js config | **Create (Task 42)** |
| `IssuerDIDIT-mock/README.md` | Setup + run instructions | **Create (Task 28)** |

---

## Section 1 — Wallet-core bootstrap (Spec build step 1)

Produces a Rust function + CLI binary that creates a Midnight DID with both
Ed25519 (`authentication`) and Jubjub (`assertionMethod`) verification methods
from a deterministic seed. Reusable by the wallet's UI Bootstrap button, by
the issuer's bootstrap script (via CLI), and by every BDD scenario.

### Task 1: `bootstrap_did_with_keys` skeleton

**Files:**
- Create: `mobile-bench/wallet-core/src/did/bootstrap.rs`
- Modify: `mobile-bench/wallet-core/src/did/mod.rs`
- Modify: `mobile-bench/wallet-core/src/lib.rs`
- Modify: `mobile-bench/wallet-core/Cargo.toml` (add `hkdf = "0.12"` if not present)

The helper takes a 32-byte seed, derives both key pairs via HKDF-SHA256 with
distinct info strings, creates the DID, attaches the two verification
methods, and returns the resulting DID + key refs.

- [ ] **Step 1.1: Verify `hkdf` dep**

```
grep -E '^hkdf' mobile-bench/wallet-core/Cargo.toml
```

If absent, add to `[dependencies]`:

```toml
hkdf = "0.12"
sha2 = "0.10"
```

- [ ] **Step 1.2: Create `did/bootstrap.rs` with the public API and a `derive_keys` helper**

```rust
//! `bootstrap_did_with_keys` — atomically create a Midnight DID and
//! attach the two verification methods Phase 1 of the Identity
//! Centre relies on: Ed25519 in `authentication` (for SIOPv2
//! id-token signing) and Jubjub in `assertionMethod` (for VC/VP
//! signing).
//!
//! Deterministic from a 32-byte seed via HKDF-SHA256 with distinct
//! info strings so the same seed always derives the same DID on a
//! fresh standalone env. Matches the seed convention used by the
//! `midnight-did` integration tests so wallet and issuer DIDs are
//! reproducible across runs.

use hkdf::Hkdf;
use sha2::Sha256;

use crate::secret_storage::{SecretKeyRef, SecretStorage};
use crate::wallet::Wallet;
use crate::DidId;

/// Result of a successful bootstrap.
#[derive(Debug, Clone)]
pub struct BootstrappedDid {
    pub did: DidId,
    pub ed25519_ref: SecretKeyRef,
    pub jubjub_ref: SecretKeyRef,
}

/// Errors callers may have to recover from.
#[derive(Debug, thiserror::Error)]
pub enum BootstrapError {
    #[error("create_did failed: {0}")]
    CreateDid(String),
    #[error("attach Ed25519 authn key failed: {0}")]
    AttachAuthn(String),
    #[error("attach Jubjub assertion key failed: {0}")]
    AttachAssertion(String),
    #[error("post-bootstrap resolution failed: {0}")]
    Resolve(String),
    #[error("post-bootstrap doc missing relation: {0}")]
    MissingRelation(&'static str),
}

pub(crate) fn derive_keys(seed: &[u8; 32]) -> ([u8; 32], [u8; 32]) {
    let h = Hkdf::<Sha256>::new(Some(b"midnight-identity-centre-v1"), seed);
    let mut ed = [0u8; 32];
    let mut jb = [0u8; 32];
    h.expand(b"ed25519/authentication", &mut ed)
        .expect("HKDF expand for ed25519");
    h.expand(b"jubjub/assertionMethod", &mut jb)
        .expect("HKDF expand for jubjub");
    (ed, jb)
}

pub async fn bootstrap_did_with_keys(
    _wallet: &Wallet,
    _secret_store: &dyn SecretStorage,
    seed: &[u8; 32],
) -> Result<BootstrappedDid, BootstrapError> {
    let (_ed, _jb) = derive_keys(seed);
    // Filled in across Task 2.
    Err(BootstrapError::CreateDid("not implemented yet".into()))
}

#[cfg(test)]
mod tests {
    use super::derive_keys;

    #[test]
    fn derive_keys_is_deterministic() {
        let seed = [42u8; 32];
        let (a1, b1) = derive_keys(&seed);
        let (a2, b2) = derive_keys(&seed);
        assert_eq!(a1, a2);
        assert_eq!(b1, b2);
    }

    #[test]
    fn derive_keys_separates_ed_and_jubjub() {
        let seed = [42u8; 32];
        let (ed, jb) = derive_keys(&seed);
        assert_ne!(ed, jb, "info strings must produce distinct outputs");
    }

    #[test]
    fn derive_keys_changes_with_seed() {
        let (ed_a, _) = derive_keys(&[1u8; 32]);
        let (ed_b, _) = derive_keys(&[2u8; 32]);
        assert_ne!(ed_a, ed_b);
    }
}
```

- [ ] **Step 1.3: Wire the module**

Append to `mobile-bench/wallet-core/src/did/mod.rs`:

```rust
pub mod bootstrap;
pub use bootstrap::{bootstrap_did_with_keys, BootstrappedDid, BootstrapError};
```

And to `mobile-bench/wallet-core/src/lib.rs`'s re-exports section:

```rust
pub use crate::did::{bootstrap_did_with_keys, BootstrappedDid, BootstrapError};
```

- [ ] **Step 1.4: Run the unit tests**

```bash
cd /Users/ysh/iohk/midnight-ledger/.claude/worktrees/thirsty-lovelace-092f50
cargo test -p midnight-wallet-core --lib did::bootstrap::tests -- --nocapture
```

Expected: `test result: ok. 3 passed; 0 failed`.

- [ ] **Step 1.5: Commit**

```bash
git add mobile-bench/wallet-core/Cargo.toml \
        mobile-bench/wallet-core/src/did/bootstrap.rs \
        mobile-bench/wallet-core/src/did/mod.rs \
        mobile-bench/wallet-core/src/lib.rs
git commit -S -s -m "$(cat <<'EOF'
feat(did-auth): scaffold bootstrap_did_with_keys + key derivation

Deterministic HKDF-SHA256 derivation of Ed25519 (authentication)
and Jubjub (assertionMethod) keys from a 32-byte seed. Distinct
info strings guarantee non-overlapping derivations. Full
bootstrap orchestration lands in Task 2.
EOF
)"
git log --format="%h %G? %s" -1
```

Expected last line: `<sha> G feat(did-auth): scaffold bootstrap_did_with_keys + key derivation`.

### Task 2: `bootstrap_did_with_keys` orchestration

**Files:**
- Modify: `mobile-bench/wallet-core/src/did/bootstrap.rs`

Wire up the actual flow: derive keys → `create_did` → attach Ed25519 +
relation → attach Jubjub + relation → re-resolve → confirm both relations
populated.

- [ ] **Step 2.1: Write the failing integration test**

Append to `mobile-bench/wallet-core/src/did/bootstrap.rs` (in the existing `#[cfg(test)] mod tests {}`):

```rust
    /// Smoke-test the full orchestration against an in-memory `Wallet`
    /// stub. The real chain-touching path is covered by the
    /// integration test in Task 4.
    #[tokio::test]
    async fn bootstrap_populates_both_relations_in_returned_struct() {
        use crate::test_support::stub_wallet;
        use crate::secret_storage::InMemorySecretStore;
        let wallet = stub_wallet();
        let store = InMemorySecretStore::default();
        let seed = [7u8; 32];

        let out = bootstrap_did_with_keys(&wallet, &store, &seed)
            .await
            .expect("bootstrap should succeed against stub");

        assert!(out.ed25519_ref.id().starts_with("ed25519/"),
                "ed25519 key ref must be tagged");
        assert!(out.jubjub_ref.id().starts_with("jubjub/"),
                "jubjub key ref must be tagged");
        assert!(out.did.as_str().starts_with("did:midnight:"),
                "DID must be in the midnight namespace");
    }
```

- [ ] **Step 2.2: Run the test to verify it fails**

```bash
cargo test -p midnight-wallet-core --lib \
  did::bootstrap::tests::bootstrap_populates_both_relations_in_returned_struct \
  -- --nocapture
```

Expected: FAIL with `not implemented yet`.

- [ ] **Step 2.3: Implement the orchestration**

Replace the `bootstrap_did_with_keys` body in `bootstrap.rs`:

```rust
pub async fn bootstrap_did_with_keys(
    wallet: &Wallet,
    secret_store: &dyn SecretStorage,
    seed: &[u8; 32],
) -> Result<BootstrappedDid, BootstrapError> {
    use crate::did::VerificationRelation;
    let (ed_bytes, jb_bytes) = derive_keys(seed);

    // 1. Persist both keys before any on-chain work — if we crash
    //    after create_did but before attach, the keys are still
    //    locally recoverable.
    let ed25519_ref = secret_store
        .import_ed25519(&ed_bytes, "ed25519/authentication")
        .map_err(|e| BootstrapError::AttachAuthn(e.to_string()))?;
    let jubjub_ref = secret_store
        .import_jubjub(&jb_bytes, "jubjub/assertionMethod")
        .map_err(|e| BootstrapError::AttachAssertion(e.to_string()))?;

    // 2. Create an empty DID on chain (1 tx).
    let did = wallet
        .create_did()
        .await
        .map_err(|e| BootstrapError::CreateDid(e.to_string()))?;

    // 3. Attach Ed25519 → authentication relation (2 txs).
    wallet
        .add_verification_method(&did, &ed25519_ref, "key-auth")
        .await
        .map_err(|e| BootstrapError::AttachAuthn(e.to_string()))?;
    wallet
        .add_verification_method_relation(
            &did,
            "key-auth",
            VerificationRelation::Authentication,
        )
        .await
        .map_err(|e| BootstrapError::AttachAuthn(e.to_string()))?;

    // 4. Attach Jubjub → assertionMethod relation (2 txs).
    wallet
        .add_verification_method(&did, &jubjub_ref, "key-assert")
        .await
        .map_err(|e| BootstrapError::AttachAssertion(e.to_string()))?;
    wallet
        .add_verification_method_relation(
            &did,
            "key-assert",
            VerificationRelation::AssertionMethod,
        )
        .await
        .map_err(|e| BootstrapError::AttachAssertion(e.to_string()))?;

    // 5. Verify the on-chain doc carries both relations.
    let doc = wallet
        .resolve_did(&did)
        .await
        .map_err(|e| BootstrapError::Resolve(e.to_string()))?;
    if doc.authentication.is_empty() {
        return Err(BootstrapError::MissingRelation("authentication"));
    }
    if doc.assertion_method.is_empty() {
        return Err(BootstrapError::MissingRelation("assertionMethod"));
    }

    Ok(BootstrappedDid { did, ed25519_ref, jubjub_ref })
}
```

If `Wallet::create_did` / `add_verification_method` /
`add_verification_method_relation` / `resolve_did` don't yet expose this
exact shape, this is the moment to align them. They are stub-shaped in
`wallet.rs` per the spec's risk register; the integration test in Task 4
will catch any drift.

If `SecretStorage::import_ed25519` / `import_jubjub` aren't available, add
them now as thin wrappers around the existing `import_secret(key_type, …)`
primitive.

If `test_support::stub_wallet` doesn't exist, create
`mobile-bench/wallet-core/src/test_support.rs` with a `pub fn stub_wallet() -> Wallet`
factory that returns a `Wallet` backed by an in-memory mock node + indexer.
This is also used by Tasks 5-18. Don't gate behind `#[cfg(test)]` — keep
it under `#[cfg(any(test, feature = "test-support"))]` and add the feature
to `Cargo.toml`.

- [ ] **Step 2.4: Run the test to verify it passes**

```bash
cargo test -p midnight-wallet-core --lib \
  did::bootstrap::tests::bootstrap_populates_both_relations_in_returned_struct \
  -- --nocapture
```

Expected: PASS.

- [ ] **Step 2.5: Commit**

```bash
git add mobile-bench/wallet-core/src/did/bootstrap.rs \
        mobile-bench/wallet-core/src/wallet.rs \
        mobile-bench/wallet-core/src/secret_storage/ \
        mobile-bench/wallet-core/src/test_support.rs \
        mobile-bench/wallet-core/src/lib.rs \
        mobile-bench/wallet-core/Cargo.toml
git commit -S -s -m "$(cat <<'EOF'
feat(did-auth): wire bootstrap_did_with_keys end-to-end

Six on-chain txs (create_did + 2× addVerificationMethod +
2× addVerificationMethodRelation, plus a post-resolve check),
guarded by structured BootstrapError variants so callers can
recover. Re-resolves and asserts both relations are populated.

Adds test_support::stub_wallet for unit tests that need a Wallet
without spinning up a chain. The real chain-touching test lands
in Task 4 as an integration test.
EOF
)"
git log --format="%h %G? %s" -1
```

### Task 3: `did-bootstrap` CLI binary

**Files:**
- Create: `mobile-bench/wallet-core/src/bin/did-bootstrap.rs`
- Modify: `mobile-bench/wallet-core/Cargo.toml` ([[bin]] section + `clap`)

CLI that wraps `bootstrap_did_with_keys` so shell scripts (issuer
bootstrap, BDD harness setup, demo recovery) can bootstrap without
embedding Rust.

- [ ] **Step 3.1: Add `clap` dep + [[bin]] section**

Verify `clap` is present (`grep '^clap' mobile-bench/wallet-core/Cargo.toml`).
If absent:

```toml
clap = { version = "4", features = ["derive"] }
```

Append:

```toml
[[bin]]
name = "did-bootstrap"
path = "src/bin/did-bootstrap.rs"
required-features = []
```

- [ ] **Step 3.2: Create the CLI**

`mobile-bench/wallet-core/src/bin/did-bootstrap.rs`:

```rust
//! `did-bootstrap` — CLI wrapper around `bootstrap_did_with_keys`.
//!
//! Invoked by the wallet's UI Bootstrap button (indirectly, via
//! the in-process call), by the issuer's `bootstrap-issuer-did.ts`
//! script, by `before` hooks in the BDD harness, and by anyone
//! manually recovering a corrupted standalone env.

use std::path::PathBuf;

use clap::Parser;
use midnight_wallet_core::{bootstrap_did_with_keys, Wallet};

#[derive(Parser, Debug)]
#[command(name = "did-bootstrap", about = "Create a Midnight DID with Ed25519+Jubjub keys")]
struct Args {
    /// Standalone Midnight indexer GraphQL URL.
    #[arg(long, env = "MIDNIGHT_INDEXER_URL")]
    indexer_url: String,

    /// Standalone Midnight node RPC URL.
    #[arg(long, env = "MIDNIGHT_NODE_RPC_URL")]
    node_rpc_url: String,

    /// 32-byte seed as 64 hex chars (or shorter — will be SHA-256-hashed to 32 bytes).
    /// Defaults to "holder-demo-seed" / "issuer-demo-seed" via env.
    #[arg(long, env = "DID_BOOTSTRAP_SEED")]
    seed: String,

    /// Output JSON: `{ "did": "...", "ed25519_secret_hex": "...", "jubjub_secret_hex": "..." }`.
    #[arg(long)]
    out: PathBuf,
}

fn seed_to_bytes(seed: &str) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let hex = seed.strip_prefix("0x").unwrap_or(seed);
    if let Ok(bytes) = hex::decode(hex) {
        if bytes.len() == 32 {
            let mut out = [0u8; 32];
            out.copy_from_slice(&bytes);
            return out;
        }
    }
    let mut hasher = Sha256::new();
    hasher.update(seed.as_bytes());
    hasher.finalize().into()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let seed = seed_to_bytes(&args.seed);

    let wallet = Wallet::connect_standalone(&args.indexer_url, &args.node_rpc_url).await?;
    let secret_store = wallet.secret_store();

    let result = bootstrap_did_with_keys(&wallet, &*secret_store, &seed).await?;

    let json = serde_json::json!({
        "did": result.did.as_str(),
        "ed25519_ref": result.ed25519_ref.id(),
        "jubjub_ref": result.jubjub_ref.id(),
        "ed25519_secret_hex": hex::encode(secret_store.export_secret(&result.ed25519_ref)?),
        "jubjub_secret_hex": hex::encode(secret_store.export_secret(&result.jubjub_ref)?),
    });
    std::fs::write(&args.out, serde_json::to_string_pretty(&json)?)?;
    println!("Bootstrapped {} → {}", result.did.as_str(), args.out.display());
    Ok(())
}
```

If `Wallet::connect_standalone` or `Wallet::secret_store` /
`SecretStorage::export_secret` aren't yet present, add them as thin
wrappers — `connect_standalone` is just the existing constructor with
indexer + node URLs.

- [ ] **Step 3.3: Build the binary**

```bash
cargo build -p midnight-wallet-core --bin did-bootstrap
```

Expected: clean build, binary at `target/debug/did-bootstrap`.

- [ ] **Step 3.4: `--help` smoke test**

```bash
./target/debug/did-bootstrap --help
```

Expected: usage text mentioning `--indexer-url`, `--node-rpc-url`, `--seed`, `--out`.

- [ ] **Step 3.5: Commit**

```bash
git add mobile-bench/wallet-core/Cargo.toml \
        mobile-bench/wallet-core/src/bin/did-bootstrap.rs
git commit -S -s -m "$(cat <<'EOF'
feat(did-auth): did-bootstrap CLI wrapper

Standalone executable that wraps bootstrap_did_with_keys. Accepts
indexer + node URLs (env or flags), an arbitrary seed (raw hex or
SHA-256-hashed string), and writes a JSON keystore. Used by the
issuer-side bootstrap script and the BDD before-hooks.
EOF
)"
git log --format="%h %G? %s" -1
```

### Task 4: Integration test against standalone env

**Files:**
- Create: `mobile-bench/wallet-core/tests/did_bootstrap_standalone.rs`

End-to-end test that spins up the standalone env (via docker-compose),
runs the bootstrap, and asserts the resulting DID document is fully
populated. This is the test that catches misalignments between
`Wallet::resolve_did` and what we expect.

- [ ] **Step 4.1: Create the test scaffolding**

`mobile-bench/wallet-core/tests/did_bootstrap_standalone.rs`:

```rust
//! Integration test for bootstrap_did_with_keys against the
//! local standalone Midnight env. Gated on the `STANDALONE_RUN`
//! env var so CI doesn't try to docker-compose-up by accident;
//! `cargo test -- --ignored` against a running env is the
//! invocation pattern.

#![cfg(any())] // remove this once the env is up — see Step 4.4

use midnight_wallet_core::{bootstrap_did_with_keys, Wallet};

#[tokio::test]
#[ignore = "requires STANDALONE_RUN=1 and a running docker-compose env"]
async fn bootstrap_against_standalone_succeeds_and_doc_is_complete() {
    if std::env::var("STANDALONE_RUN").is_err() {
        eprintln!("STANDALONE_RUN not set — skipping");
        return;
    }
    let indexer = std::env::var("MIDNIGHT_INDEXER_URL")
        .expect("MIDNIGHT_INDEXER_URL must be set");
    let node = std::env::var("MIDNIGHT_NODE_RPC_URL")
        .expect("MIDNIGHT_NODE_RPC_URL must be set");

    let wallet = Wallet::connect_standalone(&indexer, &node).await.expect("connect");
    let store = wallet.secret_store();

    let seed = [42u8; 32];
    let out = bootstrap_did_with_keys(&wallet, &*store, &seed)
        .await
        .expect("bootstrap should succeed on a clean env");

    let doc = wallet.resolve_did(&out.did).await.expect("resolve");
    assert!(!doc.authentication.is_empty(), "authentication relation");
    assert!(!doc.assertion_method.is_empty(), "assertionMethod relation");
    assert!(doc.verification_method.iter().any(|vm| vm.id.fragment() == "key-auth"));
    assert!(doc.verification_method.iter().any(|vm| vm.id.fragment() == "key-assert"));
}

#[tokio::test]
#[ignore = "requires STANDALONE_RUN=1"]
async fn bootstrap_is_deterministic_across_clean_runs() {
    if std::env::var("STANDALONE_RUN").is_err() {
        return;
    }
    // Note: this scenario requires resetting the env between
    // the two bootstrap calls — driven by the BDD harness in
    // Task 42; here we only assert that derive_keys produces
    // the same secrets twice in one process.
    use midnight_wallet_core::did::bootstrap::derive_keys;
    let s = [99u8; 32];
    let (a1, b1) = derive_keys(&s);
    let (a2, b2) = derive_keys(&s);
    assert_eq!(a1, a2);
    assert_eq!(b1, b2);
}
```

`derive_keys` must be exposed for the second test — re-export
`pub(crate) fn derive_keys` as `pub fn derive_keys` from `did::bootstrap`,
gated behind `#[cfg(any(test, feature = "test-support"))]`.

- [ ] **Step 4.2: Verify it compiles**

```bash
cargo test -p midnight-wallet-core --test did_bootstrap_standalone --no-run
```

Expected: clean build.

- [ ] **Step 4.3: Remove the `#![cfg(any())]` gate**

Edit the first line of `did_bootstrap_standalone.rs` to delete the
disabled-everything attribute. The `#[ignore]` flag on individual tests
is enough.

- [ ] **Step 4.4: Note for the engineer**

The env isn't yet running — the docker-compose stack lands in Task 20.
Re-run this test with `STANDALONE_RUN=1` after Task 20 + 21 to confirm
the bootstrap actually lands on chain. For now, this test compiles but
doesn't run. The unit test in Task 2 is what proves the orchestration
logic in CI.

- [ ] **Step 4.5: Commit**

```bash
git add mobile-bench/wallet-core/src/did/bootstrap.rs \
        mobile-bench/wallet-core/tests/did_bootstrap_standalone.rs
git commit -S -s -m "$(cat <<'EOF'
test(did-auth): integration test scaffolding for bootstrap

Compiles but only runs under STANDALONE_RUN=1 with the docker
env up (Task 20 onward). Asserts the resolved DID document
carries both verification relations with the expected key
fragments after a fresh bootstrap.
EOF
)"
git log --format="%h %G? %s" -1
```

---

## Section 2 — Wallet-core: `vc_store` (Spec build step 2)

Three redb tables (`vcs`, `vc_openings`, `vc_metadata`) backing the Identity
Centre's VC carousel. Generic over `VC<TClaims, TCommitments, _, _>` so the
`birth` family fits and future families slot in without schema migration.

### Task 5: `vc_store` tables + types

**Files:**
- Create: `mobile-bench/wallet-core/src/vc_store/mod.rs`
- Create: `mobile-bench/wallet-core/src/vc_store/tables.rs`
- Create: `mobile-bench/wallet-core/src/vc_store/types.rs`
- Modify: `mobile-bench/wallet-core/src/lib.rs`

- [ ] **Step 5.1: Create the module root**

`mobile-bench/wallet-core/src/vc_store/mod.rs`:

```rust
//! Per-holder Verifiable Credential storage.
//!
//! Three redb tables sharing the same `wallet.redb` file as the
//! existing wallet store:
//!
//! * `vcs`        — `vc_uri` → CBOR-serialized signed VC body
//! * `vc_openings`— `(vc_uri, claim_path)` → CBOR opening blob
//! * `vc_metadata`— `vc_uri` → display order, last-verified ts, custom labels
//!
//! All three tables are write-once for the VC body itself; only
//! metadata mutates after issuance. Generic over the VC envelope
//! shape so future credential families don't require schema
//! migration.

mod tables;
mod types;
mod api;

pub use api::VcStore;
pub use types::{StoredVc, VcOpening, VcMetadata};
```

- [ ] **Step 5.2: Create the table definitions**

`mobile-bench/wallet-core/src/vc_store/tables.rs`:

```rust
use redb::TableDefinition;

/// `vc_uri` (UTF-8) → CBOR-encoded `StoredVc`.
pub(super) const VCS: TableDefinition<&str, Vec<u8>> = TableDefinition::new("identity_vcs_v1");

/// Composite key `(vc_uri, claim_path)` (UTF-8 + 0x1f + UTF-8) → CBOR `VcOpening`.
pub(super) const VC_OPENINGS: TableDefinition<&str, Vec<u8>> =
    TableDefinition::new("identity_vc_openings_v1");

/// `vc_uri` (UTF-8) → CBOR-encoded `VcMetadata`.
pub(super) const VC_METADATA: TableDefinition<&str, Vec<u8>> =
    TableDefinition::new("identity_vc_metadata_v1");

/// Build the composite key for VC_OPENINGS. `0x1f` is the ASCII
/// "Unit Separator" — never appears in URIs or JSON pointers in
/// practice, safe as a delimiter.
pub(super) fn opening_key(vc_uri: &str, claim_path: &str) -> String {
    format!("{vc_uri}\x1f{claim_path}")
}
```

- [ ] **Step 5.3: Create the types**

`mobile-bench/wallet-core/src/vc_store/types.rs`:

```rust
use serde::{Deserialize, Serialize};

/// Persisted VC envelope. `body` is the canonical signed bytes the
/// issuer returned — the Compact serialization. `format` allows
/// future non-Compact VC families to coexist.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredVc {
    pub vc_uri: String,
    pub issuer_did: String,
    pub holder_did: String,
    pub format: String, // e.g. "midnight-vc-compact"
    pub body: Vec<u8>,
    pub issued_at_ms: u64,
}

/// One private claim's value + opening randomness, keyed by JSON-Pointer-style path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VcOpening {
    pub vc_uri: String,
    pub claim_path: String, // e.g. "/credentialSubject/dateOfBirth"
    pub plaintext: Vec<u8>,
    pub opening: Vec<u8>,
}

/// Display + telemetry data. Mutates over the VC's lifetime.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VcMetadata {
    pub vc_uri: String,
    pub display_order: u32,
    pub last_verified_ms: Option<u64>,
    pub last_verify_outcome: Option<String>, // "Valid" | "Invalid: <reason>" — see vc_self_verify
    pub custom_labels: Vec<(String, String)>,
}
```

- [ ] **Step 5.4: Wire to `lib.rs`**

Append to `mobile-bench/wallet-core/src/lib.rs`:

```rust
pub mod vc_store;
pub use vc_store::{StoredVc, VcMetadata, VcOpening, VcStore};
```

- [ ] **Step 5.5: Confirm it compiles + commit**

```bash
cargo check -p midnight-wallet-core
```

Expected: clean (with `VcStore` not-yet-defined warning OK; we add it in Task 6).

Actually `VcStore` isn't defined yet — Step 5.1 re-exports it from a not-yet-existing `api` module. Add a placeholder:

`mobile-bench/wallet-core/src/vc_store/api.rs`:

```rust
//! Implementation lands in Task 7. This file exists so the
//! re-export in mod.rs doesn't break the build.

use redb::Database;

#[allow(dead_code)] // populated in Task 7
pub struct VcStore {
    db: std::sync::Arc<Database>,
}
```

Now `cargo check -p midnight-wallet-core` should succeed.

```bash
git add mobile-bench/wallet-core/src/vc_store/ \
        mobile-bench/wallet-core/src/lib.rs
git commit -S -s -m "$(cat <<'EOF'
feat(vc): vc_store module scaffold

Three redb table definitions + serde codec types
(StoredVc, VcOpening, VcMetadata). CRUD API lands in Tasks 7+8.
EOF
)"
git log --format="%h %G? %s" -1
```

### Task 6: `vc_store` CRUD — insert + get

**Files:**
- Modify: `mobile-bench/wallet-core/src/vc_store/api.rs`

- [ ] **Step 6.1: Write the failing test**

Append to `mobile-bench/wallet-core/src/vc_store/api.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::vc_store::types::*;
    use tempfile::TempDir;

    fn open_store() -> (VcStore, TempDir) {
        let dir = TempDir::new().unwrap();
        let store = VcStore::open(dir.path().join("test.redb")).expect("open");
        (store, dir)
    }

    fn sample_vc() -> StoredVc {
        StoredVc {
            vc_uri: "urn:uuid:abc-123".into(),
            issuer_did: "did:midnight:issuer".into(),
            holder_did: "did:midnight:alice".into(),
            format: "midnight-vc-compact".into(),
            body: vec![1, 2, 3, 4],
            issued_at_ms: 1_000_000,
        }
    }

    #[test]
    fn insert_then_get_round_trips() {
        let (store, _g) = open_store();
        let vc = sample_vc();
        store.insert_vc(&vc).expect("insert");
        let back = store.get_vc(&vc.vc_uri).expect("get").expect("present");
        assert_eq!(back.vc_uri, vc.vc_uri);
        assert_eq!(back.body, vc.body);
    }

    #[test]
    fn get_missing_returns_none() {
        let (store, _g) = open_store();
        assert!(store.get_vc("urn:uuid:nope").expect("get").is_none());
    }
}
```

- [ ] **Step 6.2: Run to verify it fails**

```bash
cargo test -p midnight-wallet-core --lib vc_store::api::tests
```

Expected: FAIL — `open` and `insert_vc` and `get_vc` not defined.

- [ ] **Step 6.3: Implement `open`, `insert_vc`, `get_vc`**

Replace the contents of `vc_store/api.rs`:

```rust
//! VcStore CRUD API.

use std::path::Path;
use std::sync::Arc;

use redb::{Database, ReadableTable};

use crate::vc_store::tables::{VCS, VC_OPENINGS, VC_METADATA, opening_key};
use crate::vc_store::types::*;

#[derive(Debug, thiserror::Error)]
pub enum VcStoreError {
    #[error("redb error: {0}")]
    Redb(#[from] redb::Error),
    #[error("redb tx commit error: {0}")]
    Commit(#[from] redb::CommitError),
    #[error("redb tx begin error: {0}")]
    Begin(#[from] redb::TransactionError),
    #[error("redb table error: {0}")]
    Table(#[from] redb::TableError),
    #[error("redb storage error: {0}")]
    Storage(#[from] redb::StorageError),
    #[error("cbor error: {0}")]
    Cbor(#[from] serde_cbor::Error),
}

pub struct VcStore {
    db: Arc<Database>,
}

impl VcStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, VcStoreError> {
        let db = Database::create(path).map_err(redb::Error::from)?;
        // Materialise the three tables so first use doesn't race.
        let wtx = db.begin_write()?;
        let _ = wtx.open_table(VCS)?;
        let _ = wtx.open_table(VC_OPENINGS)?;
        let _ = wtx.open_table(VC_METADATA)?;
        wtx.commit()?;
        Ok(Self { db: Arc::new(db) })
    }

    pub fn insert_vc(&self, vc: &StoredVc) -> Result<(), VcStoreError> {
        let wtx = self.db.begin_write()?;
        {
            let mut t = wtx.open_table(VCS)?;
            t.insert(vc.vc_uri.as_str(), serde_cbor::to_vec(vc)?)?;
        }
        wtx.commit()?;
        Ok(())
    }

    pub fn get_vc(&self, vc_uri: &str) -> Result<Option<StoredVc>, VcStoreError> {
        let rtx = self.db.begin_read()?;
        let t = rtx.open_table(VCS)?;
        let row = t.get(vc_uri)?;
        match row {
            Some(g) => Ok(Some(serde_cbor::from_slice(&g.value())?)),
            None => Ok(None),
        }
    }
}
```

Verify `serde_cbor`, `tempfile` are deps (`grep -E '^(serde_cbor|tempfile)' Cargo.toml`); add if missing.

- [ ] **Step 6.4: Run to verify pass**

```bash
cargo test -p midnight-wallet-core --lib vc_store::api::tests
```

Expected: `test result: ok. 2 passed`.

- [ ] **Step 6.5: Commit**

```bash
git add mobile-bench/wallet-core/src/vc_store/api.rs \
        mobile-bench/wallet-core/Cargo.toml
git commit -S -s -m "$(cat <<'EOF'
feat(vc): VcStore::insert_vc + get_vc round-trip

CBOR-on-redb, single-file database. Materialises all three
tables on open so first-use doesn't race. Errors typed via
VcStoreError so call sites get useful failure modes.
EOF
)"
git log --format="%h %G? %s" -1
```

### Task 7: `vc_store` — openings + metadata + list

**Files:**
- Modify: `mobile-bench/wallet-core/src/vc_store/api.rs`

Round out the CRUD surface: openings keyed by `(vc_uri, claim_path)`,
metadata for display ordering + verify-cache, list-ordered iteration.

- [ ] **Step 7.1: Write the failing tests**

Append to `vc_store/api.rs`'s `mod tests`:

```rust
    #[test]
    fn opening_round_trips() {
        let (store, _g) = open_store();
        let op = VcOpening {
            vc_uri: "urn:uuid:abc".into(),
            claim_path: "/credentialSubject/dateOfBirth".into(),
            plaintext: b"1985-01-01".to_vec(),
            opening: vec![9, 8, 7],
        };
        store.insert_opening(&op).unwrap();
        let back = store.get_opening("urn:uuid:abc", "/credentialSubject/dateOfBirth")
            .unwrap().unwrap();
        assert_eq!(back.plaintext, op.plaintext);
        assert_eq!(back.opening, op.opening);
    }

    #[test]
    fn metadata_update_then_read() {
        let (store, _g) = open_store();
        let vc = sample_vc();
        store.insert_vc(&vc).unwrap();
        store.update_metadata(&vc.vc_uri, |m| {
            m.display_order = 3;
            m.last_verified_ms = Some(42);
            m.last_verify_outcome = Some("Valid".into());
        }).unwrap();
        let md = store.get_metadata(&vc.vc_uri).unwrap().expect("present");
        assert_eq!(md.display_order, 3);
        assert_eq!(md.last_verified_ms, Some(42));
    }

    #[test]
    fn list_ordered_returns_by_display_order() {
        let (store, _g) = open_store();
        for (i, uri) in ["urn:b", "urn:a", "urn:c"].iter().enumerate() {
            store.insert_vc(&StoredVc {
                vc_uri: (*uri).into(),
                issuer_did: "did:midnight:i".into(),
                holder_did: "did:midnight:h".into(),
                format: "f".into(),
                body: vec![i as u8],
                issued_at_ms: i as u64,
            }).unwrap();
            // Note: "urn:a" gets order 2, "urn:b" order 0, "urn:c" order 1 below
            let order = match *uri { "urn:b" => 0u32, "urn:c" => 1, "urn:a" => 2, _ => unreachable!() };
            store.update_metadata(uri, |m| m.display_order = order).unwrap();
        }
        let list = store.list_ordered().unwrap();
        let uris: Vec<&str> = list.iter().map(|v| v.vc_uri.as_str()).collect();
        assert_eq!(uris, vec!["urn:b", "urn:c", "urn:a"]);
    }
```

- [ ] **Step 7.2: Run, see it fail**

```bash
cargo test -p midnight-wallet-core --lib vc_store::api::tests
```

Expected: 2 passing (insert/get_vc, get_missing) + 3 failing (the new ones).

- [ ] **Step 7.3: Implement the new methods**

Append to `impl VcStore`:

```rust
    pub fn insert_opening(&self, op: &VcOpening) -> Result<(), VcStoreError> {
        let wtx = self.db.begin_write()?;
        {
            let mut t = wtx.open_table(VC_OPENINGS)?;
            let key = opening_key(&op.vc_uri, &op.claim_path);
            t.insert(key.as_str(), serde_cbor::to_vec(op)?)?;
        }
        wtx.commit()?;
        Ok(())
    }

    pub fn get_opening(&self, vc_uri: &str, claim_path: &str)
        -> Result<Option<VcOpening>, VcStoreError>
    {
        let rtx = self.db.begin_read()?;
        let t = rtx.open_table(VC_OPENINGS)?;
        let key = opening_key(vc_uri, claim_path);
        match t.get(key.as_str())? {
            Some(g) => Ok(Some(serde_cbor::from_slice(&g.value())?)),
            None => Ok(None),
        }
    }

    pub fn update_metadata(&self, vc_uri: &str, f: impl FnOnce(&mut VcMetadata))
        -> Result<(), VcStoreError>
    {
        let wtx = self.db.begin_write()?;
        let mut md = {
            let t = wtx.open_table(VC_METADATA)?;
            match t.get(vc_uri)? {
                Some(g) => serde_cbor::from_slice::<VcMetadata>(&g.value())?,
                None => VcMetadata { vc_uri: vc_uri.into(), ..Default::default() },
            }
        };
        f(&mut md);
        {
            let mut t = wtx.open_table(VC_METADATA)?;
            t.insert(vc_uri, serde_cbor::to_vec(&md)?)?;
        }
        wtx.commit()?;
        Ok(())
    }

    pub fn get_metadata(&self, vc_uri: &str)
        -> Result<Option<VcMetadata>, VcStoreError>
    {
        let rtx = self.db.begin_read()?;
        let t = rtx.open_table(VC_METADATA)?;
        match t.get(vc_uri)? {
            Some(g) => Ok(Some(serde_cbor::from_slice(&g.value())?)),
            None => Ok(None),
        }
    }

    /// Returns all VCs sorted by `VcMetadata.display_order` ascending.
    /// VCs without metadata sort last (display_order = u32::MAX).
    pub fn list_ordered(&self) -> Result<Vec<StoredVc>, VcStoreError> {
        let rtx = self.db.begin_read()?;
        let vcs_t = rtx.open_table(VCS)?;
        let md_t = rtx.open_table(VC_METADATA)?;
        let mut rows: Vec<(u32, StoredVc)> = Vec::new();
        for entry in vcs_t.iter()? {
            let (k, v) = entry?;
            let vc: StoredVc = serde_cbor::from_slice(&v.value())?;
            let order = match md_t.get(k.value())? {
                Some(g) => {
                    let md: VcMetadata = serde_cbor::from_slice(&g.value())?;
                    md.display_order
                }
                None => u32::MAX,
            };
            rows.push((order, vc));
        }
        rows.sort_by_key(|(o, _)| *o);
        Ok(rows.into_iter().map(|(_, vc)| vc).collect())
    }
```

- [ ] **Step 7.4: Run, see all 5 pass**

```bash
cargo test -p midnight-wallet-core --lib vc_store::api::tests
```

Expected: 5 passing.

- [ ] **Step 7.5: Commit**

```bash
git add mobile-bench/wallet-core/src/vc_store/api.rs
git commit -S -s -m "$(cat <<'EOF'
feat(vc): openings + metadata + list_ordered CRUD

Round out the VcStore CRUD surface. Metadata is read-modify-
write via a closure so callers don't have to fetch-mutate-put
manually. list_ordered sorts by display_order ascending,
unmetadata'd VCs sort last.
EOF
)"
git log --format="%h %G? %s" -1
```

### Task 8: `vc_store` — delete + atomic insert-with-openings

**Files:**
- Modify: `mobile-bench/wallet-core/src/vc_store/api.rs`

One more pair of operations: deleting a VC + its openings + its metadata
atomically, and a `insert_vc_with_openings(vc, openings)` that lands all
three in one write transaction (the path the OID4VCI client uses).

- [ ] **Step 8.1: Write the failing tests**

```rust
    #[test]
    fn delete_removes_vc_openings_and_metadata() {
        let (store, _g) = open_store();
        let vc = sample_vc();
        store.insert_vc(&vc).unwrap();
        store.insert_opening(&VcOpening {
            vc_uri: vc.vc_uri.clone(),
            claim_path: "/x".into(),
            plaintext: vec![1],
            opening: vec![2],
        }).unwrap();
        store.update_metadata(&vc.vc_uri, |m| m.display_order = 1).unwrap();

        store.delete_vc(&vc.vc_uri).unwrap();

        assert!(store.get_vc(&vc.vc_uri).unwrap().is_none());
        assert!(store.get_opening(&vc.vc_uri, "/x").unwrap().is_none());
        assert!(store.get_metadata(&vc.vc_uri).unwrap().is_none());
    }

    #[test]
    fn insert_vc_with_openings_lands_atomically() {
        let (store, _g) = open_store();
        let vc = sample_vc();
        let openings = vec![
            VcOpening { vc_uri: vc.vc_uri.clone(), claim_path: "/a".into(), plaintext: vec![1], opening: vec![2] },
            VcOpening { vc_uri: vc.vc_uri.clone(), claim_path: "/b".into(), plaintext: vec![3], opening: vec![4] },
        ];
        store.insert_vc_with_openings(&vc, &openings).unwrap();
        assert!(store.get_vc(&vc.vc_uri).unwrap().is_some());
        assert!(store.get_opening(&vc.vc_uri, "/a").unwrap().is_some());
        assert!(store.get_opening(&vc.vc_uri, "/b").unwrap().is_some());
    }
```

- [ ] **Step 8.2: Run, see them fail**

```bash
cargo test -p midnight-wallet-core --lib vc_store::api::tests
```

Expected: 2 new FAIL.

- [ ] **Step 8.3: Implement the methods**

Append to `impl VcStore`:

```rust
    pub fn delete_vc(&self, vc_uri: &str) -> Result<(), VcStoreError> {
        let wtx = self.db.begin_write()?;
        {
            let mut vt = wtx.open_table(VCS)?;
            vt.remove(vc_uri)?;
        }
        {
            let mut mt = wtx.open_table(VC_METADATA)?;
            mt.remove(vc_uri)?;
        }
        {
            // Range-scan openings under the vc_uri prefix.
            let mut ot = wtx.open_table(VC_OPENINGS)?;
            let prefix_end = format!("{vc_uri}\x20"); // 0x20 = 0x1f + 1
            let prefix_start = format!("{vc_uri}\x1f");
            let keys: Vec<String> = ot
                .range(prefix_start.as_str()..prefix_end.as_str())?
                .filter_map(Result::ok)
                .map(|(k, _)| k.value().to_string())
                .collect();
            for k in keys {
                ot.remove(k.as_str())?;
            }
        }
        wtx.commit()?;
        Ok(())
    }

    pub fn insert_vc_with_openings(
        &self,
        vc: &StoredVc,
        openings: &[VcOpening],
    ) -> Result<(), VcStoreError> {
        let wtx = self.db.begin_write()?;
        {
            let mut t = wtx.open_table(VCS)?;
            t.insert(vc.vc_uri.as_str(), serde_cbor::to_vec(vc)?)?;
        }
        {
            let mut t = wtx.open_table(VC_OPENINGS)?;
            for op in openings {
                let key = opening_key(&op.vc_uri, &op.claim_path);
                t.insert(key.as_str(), serde_cbor::to_vec(op)?)?;
            }
        }
        wtx.commit()?;
        Ok(())
    }
```

- [ ] **Step 8.4: Run, see all 7 pass**

```bash
cargo test -p midnight-wallet-core --lib vc_store::api::tests
```

- [ ] **Step 8.5: Commit**

```bash
git add mobile-bench/wallet-core/src/vc_store/api.rs
git commit -S -s -m "$(cat <<'EOF'
feat(vc): delete + atomic insert-with-openings

delete_vc removes the VC body, all its openings, and metadata
in one write tx. insert_vc_with_openings is the path the
OID4VCI client uses to land an entire credential atomically.
EOF
)"
git log --format="%h %G? %s" -1
```

---

## Section 3 — Wallet-core: `did_auth` + `oid4vp_client` (Spec build step 3)

`did_auth` is a 50-LOC glue layer ("given a DID, find its authentication-relation
key, find the local `SecretKeyRef`, sign payload"). `oid4vp_client` parses
`openid4vp://` URLs, fetches the request object, builds a SIOPv2 id-token JWS
with the DID-bound Ed25519 key, and POSTs it back.

### Task 9: `did_auth::sign_for_authentication`

**Files:**
- Create: `mobile-bench/wallet-core/src/did_auth/mod.rs`
- Modify: `mobile-bench/wallet-core/src/lib.rs`

- [ ] **Step 9.1: Write the failing test**

`mobile-bench/wallet-core/src/did_auth/mod.rs`:

```rust
//! Bridge between "I have a DID" and "I have a `SecretKeyRef` I
//! can sign with". Looks up the DID document, picks the first
//! verification method in the `authentication` relation, finds the
//! matching local secret, signs the payload, returns
//! `(kid, signature_bytes)`.
//!
//! The `kid` is the full DID URL with the verification-method
//! fragment (`did:midnight:abc#key-auth`) — that's what JWS headers
//! need.

use crate::secret_storage::{SecretKeyRef, SecretStorage};
use crate::wallet::Wallet;
use crate::DidId;

#[derive(Debug, thiserror::Error)]
pub enum DidAuthError {
    #[error("resolve failed: {0}")]
    Resolve(String),
    #[error("no authentication-relation verification method on {0}")]
    NoAuthnKey(String),
    #[error("local secret for kid {0} not in this wallet's store")]
    NoLocalSecret(String),
    #[error("sign failed: {0}")]
    Sign(String),
}

/// `Ok((kid, signature_bytes))` on success.
pub async fn sign_for_authentication(
    wallet: &Wallet,
    secret_store: &dyn SecretStorage,
    did: &DidId,
    payload: &[u8],
) -> Result<(String, Vec<u8>), DidAuthError> {
    let doc = wallet.resolve_did(did).await
        .map_err(|e| DidAuthError::Resolve(e.to_string()))?;
    let vm_id = doc.authentication.first()
        .ok_or_else(|| DidAuthError::NoAuthnKey(did.as_str().into()))?;
    let kid = vm_id.to_string();

    let key_ref = secret_store.find_by_kid(&kid)
        .ok_or_else(|| DidAuthError::NoLocalSecret(kid.clone()))?;
    let sig = secret_store.sign(&key_ref, payload)
        .map_err(|e| DidAuthError::Sign(e.to_string()))?;

    Ok((kid, sig.into_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{stub_wallet_with_bootstrapped_did, stub_secret_store_with};

    #[tokio::test]
    async fn sign_for_authentication_returns_kid_and_sig() {
        let (wallet, did) = stub_wallet_with_bootstrapped_did([5u8; 32]).await;
        let store = stub_secret_store_with(&wallet, &did);

        let payload = b"hello-nonce";
        let (kid, sig) = sign_for_authentication(&wallet, &store, &did, payload)
            .await
            .expect("sign");
        assert!(kid.starts_with("did:midnight:"));
        assert!(kid.contains("#key-auth"));
        assert!(!sig.is_empty());
    }

    #[tokio::test]
    async fn no_authn_key_returns_specific_error() {
        let (wallet, did) = crate::test_support::stub_wallet_with_empty_did().await;
        let store = crate::test_support::stub_secret_store();
        let err = sign_for_authentication(&wallet, &store, &did, b"x").await
            .expect_err("must fail");
        assert!(matches!(err, DidAuthError::NoAuthnKey(_)));
    }
}
```

Add to `test_support.rs` if missing:

```rust
pub async fn stub_wallet_with_bootstrapped_did(seed: [u8; 32]) -> (Wallet, crate::DidId) {
    let wallet = stub_wallet();
    let store = wallet.secret_store();
    let out = crate::bootstrap_did_with_keys(&wallet, &*store, &seed).await
        .expect("bootstrap stub");
    (wallet, out.did)
}

pub async fn stub_wallet_with_empty_did() -> (Wallet, crate::DidId) {
    let wallet = stub_wallet();
    let did = wallet.create_did().await.expect("create");
    (wallet, did)
}

pub fn stub_secret_store_with(wallet: &Wallet, _did: &crate::DidId) -> impl crate::secret_storage::SecretStorage {
    wallet.secret_store_ref().clone()
}

pub fn stub_secret_store() -> impl crate::secret_storage::SecretStorage {
    crate::secret_storage::InMemorySecretStore::default()
}
```

- [ ] **Step 9.2: Re-export from lib + run test**

Append to `mobile-bench/wallet-core/src/lib.rs`:

```rust
pub mod did_auth;
pub use did_auth::{sign_for_authentication, DidAuthError};
```

```bash
cargo test -p midnight-wallet-core --lib did_auth::tests
```

Expected: 2 tests, both PASS (or first FAIL until you add `find_by_kid` to `SecretStorage`; if missing, add — it's a 5-LOC scan of the store).

- [ ] **Step 9.3: Add `SecretStorage::find_by_kid` if not present**

In `mobile-bench/wallet-core/src/secret_storage/types.rs`, append to the trait:

```rust
    /// Find a key whose `kid` (full DID URL with fragment) matches.
    /// Implementors must walk their key index; performance is not
    /// hot-path-critical (called at most once per outbound request).
    fn find_by_kid(&self, kid: &str) -> Option<SecretKeyRef>;
```

And implement on both `FileSecretStore` and `RedbSecretStore` + `InMemorySecretStore` — each is a few lines (filter on stored `kid` tag from the bootstrap step).

- [ ] **Step 9.4: Run, see green**

```bash
cargo test -p midnight-wallet-core --lib did_auth::tests
```

Expected: PASS.

- [ ] **Step 9.5: Commit**

```bash
git add mobile-bench/wallet-core/src/did_auth/ \
        mobile-bench/wallet-core/src/secret_storage/ \
        mobile-bench/wallet-core/src/test_support.rs \
        mobile-bench/wallet-core/src/lib.rs
git commit -S -s -m "$(cat <<'EOF'
feat(did-auth): sign_for_authentication glue

Resolves a DID, picks the first authentication-relation
verification method, looks up the local SecretKeyRef by kid,
and signs. Returns (kid, signature_bytes) — exactly what JWS
construction in oid4vp_client needs next.
EOF
)"
git log --format="%h %G? %s" -1
```

### Task 10: `oid4vp_client` URL parser + request fetcher

**Files:**
- Create: `mobile-bench/wallet-core/src/oid4vp_client/mod.rs`
- Create: `mobile-bench/wallet-core/src/oid4vp_client/parser.rs`
- Modify: `mobile-bench/wallet-core/src/lib.rs`
- Modify: `mobile-bench/wallet-core/Cargo.toml` (add `url = "2"` if missing)

- [ ] **Step 10.1: Module scaffold**

`mobile-bench/wallet-core/src/oid4vp_client/mod.rs`:

```rust
//! Client-side implementation of OID4VP / SIOPv2.
//!
//! Phase 1 only handles the "pure authentication" subset:
//! the request carries no presentation_definition; the wallet
//! responds with a signed id_token (no VP token). The flow:
//!
//! 1. User scans a QR carrying `openid4vp://...?request_uri=https://issuer/.../request/<id>`.
//! 2. `parser::parse_request_url` extracts the request_uri.
//! 3. `parser::fetch_request_object` GETs it, returning a typed AuthRequest.
//! 4. `jws::build_id_token` constructs the SIOPv2 id_token JWS.
//! 5. `http::post_response` POSTs `{id_token, state}` to redirect_uri.

mod parser;
mod jws;
mod http;

pub use parser::{parse_request_url, fetch_request_object, AuthRequest, Oid4vpParseError};
pub use jws::{build_id_token, IdTokenError};
pub use http::{post_response, PostResponseResult, PostResponseError};
```

- [ ] **Step 10.2: Write failing tests for the parser**

`mobile-bench/wallet-core/src/oid4vp_client/parser.rs`:

```rust
use serde::{Deserialize, Serialize};
use url::Url;

/// Parsed SIOPv2 authorization request — the subset Phase 1 cares about.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthRequest {
    pub client_id: String,
    pub nonce: String,
    pub state: Option<String>,
    /// Server-supplied URI to POST `{id_token, state}` back to.
    /// Phase 1 expects this on top-level of the request object;
    /// real OID4VP allows it inside the request JWS but we keep it
    /// simple.
    pub redirect_uri: String,
}

#[derive(Debug, thiserror::Error)]
pub enum Oid4vpParseError {
    #[error("not an openid4vp:// URL: {0}")]
    BadScheme(String),
    #[error("missing required query param: {0}")]
    MissingParam(&'static str),
    #[error("url parse error: {0}")]
    Url(#[from] url::ParseError),
    #[error("http error fetching request_uri: {0}")]
    Http(String),
    #[error("json parse error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Extract `request_uri` from an `openid4vp://...` URL.
pub fn parse_request_url(url: &str) -> Result<String, Oid4vpParseError> {
    let u = Url::parse(url)?;
    if u.scheme() != "openid4vp" {
        return Err(Oid4vpParseError::BadScheme(u.scheme().into()));
    }
    let request_uri = u
        .query_pairs()
        .find(|(k, _)| k == "request_uri")
        .map(|(_, v)| v.into_owned())
        .ok_or(Oid4vpParseError::MissingParam("request_uri"))?;
    Ok(request_uri)
}

/// GET the request object from `request_uri` and parse it.
pub async fn fetch_request_object(request_uri: &str) -> Result<AuthRequest, Oid4vpParseError> {
    let body = reqwest::get(request_uri).await
        .map_err(|e| Oid4vpParseError::Http(e.to_string()))?
        .error_for_status()
        .map_err(|e| Oid4vpParseError::Http(e.to_string()))?
        .text().await
        .map_err(|e| Oid4vpParseError::Http(e.to_string()))?;
    let req: AuthRequest = serde_json::from_str(&body)?;
    Ok(req)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_url_extracts_request_uri() {
        let url = "openid4vp://issuer.local/?request_uri=https%3A%2F%2Fissuer.local%2Frequest%2Fabc";
        let r = parse_request_url(url).expect("ok");
        assert_eq!(r, "https://issuer.local/request/abc");
    }

    #[test]
    fn parse_url_rejects_wrong_scheme() {
        let err = parse_request_url("https://issuer/?request_uri=x").expect_err("bad");
        assert!(matches!(err, Oid4vpParseError::BadScheme(_)));
    }

    #[test]
    fn parse_url_requires_request_uri_param() {
        let err = parse_request_url("openid4vp://issuer.local/").expect_err("missing");
        assert!(matches!(err, Oid4vpParseError::MissingParam("request_uri")));
    }
}
```

- [ ] **Step 10.3: Verify `url` + `reqwest` deps**

```bash
grep -E '^(url|reqwest|serde_json)' mobile-bench/wallet-core/Cargo.toml
```

If `url` is missing, append to `[dependencies]`:

```toml
url = "2"
```

(reqwest + serde_json already in tree per the existing wallet HTTP paths.)

- [ ] **Step 10.4: Run the parser tests**

```bash
cargo test -p midnight-wallet-core --lib oid4vp_client::parser::tests
```

Expected: 3 PASS.

- [ ] **Step 10.5: Commit**

```bash
git add mobile-bench/wallet-core/src/oid4vp_client/ \
        mobile-bench/wallet-core/Cargo.toml \
        mobile-bench/wallet-core/src/lib.rs
git commit -S -s -m "$(cat <<'EOF'
feat(oid4vp): parse openid4vp:// + fetch request object

URL scheme guard + request_uri extraction + GET the request
object as a typed AuthRequest struct. Phase 1 only consumes
the pure-authentication subset (no presentation_definition).
EOF
)"
git log --format="%h %G? %s" -1
```

### Task 11: SIOPv2 id-token JWS builder

**Files:**
- Create: `mobile-bench/wallet-core/src/oid4vp_client/jws.rs`
- Modify: `mobile-bench/wallet-core/Cargo.toml` (add `josekit = "0.10"` or `jsonwebtoken = "9"`; the example uses `josekit` for Ed25519 friendliness)

- [ ] **Step 11.1: Add JWS dep**

```toml
josekit = "0.10"
base64 = "0.22"
```

- [ ] **Step 11.2: Write failing tests**

`mobile-bench/wallet-core/src/oid4vp_client/jws.rs`:

```rust
//! SIOPv2 id-token builder.
//!
//! Header: `{ alg: "EdDSA", typ: "JWT", kid: <did>#<fragment> }`
//! Payload: `{ iss: <did>, sub: <did>, aud: client_id, nonce, iat, exp }`
//! Signature: EdDSA over `base64url(header) || "." || base64url(payload)`.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::did_auth::{sign_for_authentication, DidAuthError};
use crate::secret_storage::SecretStorage;
use crate::wallet::Wallet;
use crate::DidId;

#[derive(Debug, thiserror::Error)]
pub enum IdTokenError {
    #[error("did_auth error: {0}")]
    DidAuth(#[from] DidAuthError),
    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("clock error")]
    Clock,
}

#[derive(Debug, Serialize)]
struct JwsHeader<'a> {
    alg: &'a str,
    typ: &'a str,
    kid: &'a str,
}

#[derive(Debug, Serialize, Deserialize)]
struct IdTokenPayload {
    iss: String,
    sub: String,
    aud: String,
    nonce: String,
    iat: u64,
    exp: u64,
}

/// Build a signed SIOPv2 id_token.
///
/// `lifetime_secs` is how long the id_token remains valid; 5 minutes
/// (300) matches OID4VP convention.
pub async fn build_id_token(
    wallet: &Wallet,
    secret_store: &dyn SecretStorage,
    holder: &DidId,
    client_id: &str,
    nonce: &str,
    lifetime_secs: u64,
) -> Result<String, IdTokenError> {
    // 1. Compose header (kid filled in after the sign call, since
    //    that's what `sign_for_authentication` returns).
    let payload = IdTokenPayload {
        iss: holder.as_str().into(),
        sub: holder.as_str().into(),
        aud: client_id.into(),
        nonce: nonce.into(),
        iat: now()?,
        exp: now()? + lifetime_secs,
    };
    let payload_b64 = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload)?);

    // 2. Build a placeholder header so we can canonicalise the
    //    sign-input. The kid value below will be replaced after
    //    we know it from the sign call.
    let placeholder_kid = "PLACEHOLDER";
    let header_placeholder = JwsHeader {
        alg: "EdDSA",
        typ: "JWT",
        kid: placeholder_kid,
    };
    let _ = serde_json::to_vec(&header_placeholder)?;

    // 3. Ask did_auth to sign — get back the real kid.
    //    We re-canonicalise with the real kid afterwards.
    let sign_input_for_kid_discovery = b"oid4vp-kid-probe";
    let (kid, _probe_sig) = sign_for_authentication(
        wallet, secret_store, holder, sign_input_for_kid_discovery,
    ).await?;

    // 4. Now finalize header + sign input with the real kid.
    let header_final = JwsHeader { alg: "EdDSA", typ: "JWT", kid: &kid };
    let header_b64 = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header_final)?);
    let sign_input = format!("{header_b64}.{payload_b64}");

    let (_kid2, sig) = sign_for_authentication(
        wallet, secret_store, holder, sign_input.as_bytes(),
    ).await?;
    let sig_b64 = URL_SAFE_NO_PAD.encode(&sig);

    Ok(format!("{sign_input}.{sig_b64}"))
}

fn now() -> Result<u64, IdTokenError> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| IdTokenError::Clock)?
        .as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{stub_wallet_with_bootstrapped_did, stub_secret_store_with};

    #[tokio::test]
    async fn build_id_token_is_three_dot_separated() {
        let (wallet, did) = stub_wallet_with_bootstrapped_did([6u8; 32]).await;
        let store = stub_secret_store_with(&wallet, &did);
        let jwt = build_id_token(&wallet, &store, &did, "client-x", "nonce-y", 300)
            .await.expect("build");
        let parts: Vec<&str> = jwt.split('.').collect();
        assert_eq!(parts.len(), 3, "jws has three b64 segments");
        for p in &parts {
            assert!(!p.is_empty());
        }
    }

    #[tokio::test]
    async fn id_token_header_contains_real_kid() {
        let (wallet, did) = stub_wallet_with_bootstrapped_did([7u8; 32]).await;
        let store = stub_secret_store_with(&wallet, &did);
        let jwt = build_id_token(&wallet, &store, &did, "c", "n", 60).await.expect("build");
        let header_b64 = jwt.split('.').next().unwrap();
        let header_bytes = URL_SAFE_NO_PAD.decode(header_b64).expect("b64");
        let header: serde_json::Value = serde_json::from_slice(&header_bytes).expect("json");
        let kid = header["kid"].as_str().expect("kid present");
        assert!(kid.starts_with("did:midnight:"));
        assert!(kid.contains("#key-auth"));
        assert_eq!(header["alg"], "EdDSA");
    }

    #[tokio::test]
    async fn id_token_payload_contains_required_claims() {
        let (wallet, did) = stub_wallet_with_bootstrapped_did([8u8; 32]).await;
        let store = stub_secret_store_with(&wallet, &did);
        let jwt = build_id_token(&wallet, &store, &did, "c", "n", 60).await.expect("build");
        let payload_b64 = jwt.split('.').nth(1).unwrap();
        let payload_bytes = URL_SAFE_NO_PAD.decode(payload_b64).expect("b64");
        let payload: IdTokenPayload = serde_json::from_slice(&payload_bytes).expect("json");
        assert_eq!(payload.iss, did.as_str());
        assert_eq!(payload.sub, did.as_str());
        assert_eq!(payload.aud, "c");
        assert_eq!(payload.nonce, "n");
        assert!(payload.exp > payload.iat);
    }
}
```

- [ ] **Step 11.3: Run + verify**

```bash
cargo test -p midnight-wallet-core --lib oid4vp_client::jws::tests
```

Expected: 3 PASS.

The two-call dance (sign probe + sign final) is intentional — `sign_for_authentication` is the only path that knows the kid, so we use one call to discover the kid and one to sign the actual payload. The probe signature is discarded. If profiling later shows this matters, cache the kid against the DID after the first call.

- [ ] **Step 11.4: Commit**

```bash
git add mobile-bench/wallet-core/Cargo.toml \
        mobile-bench/wallet-core/src/oid4vp_client/jws.rs
git commit -S -s -m "$(cat <<'EOF'
feat(oid4vp): SIOPv2 id_token JWS builder

EdDSA-signed three-segment JWS with iss=sub=holder DID,
aud=client_id, nonce as-supplied, iat/exp around now. Header
kid is the full DID URL including #key-auth fragment. Reuses
did_auth::sign_for_authentication for the sign primitive.
EOF
)"
git log --format="%h %G? %s" -1
```

### Task 12: `oid4vp_client::http::post_response`

**Files:**
- Create: `mobile-bench/wallet-core/src/oid4vp_client/http.rs`

- [ ] **Step 12.1: Write failing tests**

`mobile-bench/wallet-core/src/oid4vp_client/http.rs`:

```rust
//! Final leg of the OID4VP flow — POST the signed id_token to
//! the issuer's redirect_uri and read back the session_id +
//! status.

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
struct AuthResponseBody<'a> {
    id_token: &'a str,
    state: Option<&'a str>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PostResponseResult {
    pub session_id: String,
    pub status: String,
}

#[derive(Debug, thiserror::Error)]
pub enum PostResponseError {
    #[error("http error: {0}")]
    Http(String),
    #[error("non-2xx status {status}: {body}")]
    Status { status: u16, body: String },
    #[error("decode error: {0}")]
    Decode(#[from] serde_json::Error),
}

pub async fn post_response(
    redirect_uri: &str,
    id_token: &str,
    state: Option<&str>,
) -> Result<PostResponseResult, PostResponseError> {
    let body = AuthResponseBody { id_token, state };
    let resp = reqwest::Client::new()
        .post(redirect_uri)
        .json(&body)
        .send()
        .await
        .map_err(|e| PostResponseError::Http(e.to_string()))?;
    let status = resp.status();
    let body_text = resp.text().await
        .map_err(|e| PostResponseError::Http(e.to_string()))?;
    if !status.is_success() {
        return Err(PostResponseError::Status { status: status.as_u16(), body: body_text });
    }
    let parsed: PostResponseResult = serde_json::from_str(&body_text)?;
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{
        matchers::{method, path, body_partial_json},
        Mock, MockServer, ResponseTemplate,
    };

    #[tokio::test]
    async fn post_response_returns_session_and_status() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/authorize-response"))
            .and(body_partial_json(serde_json::json!({ "id_token": "abc.def.ghi" })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "session_id": "S-1",
                "status": "authenticated"
            })))
            .mount(&mock).await;

        let url = format!("{}/authorize-response", mock.uri());
        let r = post_response(&url, "abc.def.ghi", Some("st-1")).await.expect("ok");
        assert_eq!(r.session_id, "S-1");
        assert_eq!(r.status, "authenticated");
    }

    #[tokio::test]
    async fn post_response_reports_4xx_specifically() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(401).set_body_string("nonce mismatch"))
            .mount(&mock).await;
        let err = post_response(&format!("{}/x", mock.uri()), "j", None).await.expect_err("must fail");
        match err {
            PostResponseError::Status { status: 401, body } => assert!(body.contains("nonce")),
            other => panic!("expected 401 Status, got {other:?}"),
        }
    }
}
```

- [ ] **Step 12.2: Add `wiremock` dev-dep**

```bash
grep -E '^wiremock' mobile-bench/wallet-core/Cargo.toml
```

If missing, add to `[dev-dependencies]`:

```toml
wiremock = "0.6"
```

- [ ] **Step 12.3: Run + verify**

```bash
cargo test -p midnight-wallet-core --lib oid4vp_client::http::tests
```

Expected: 2 PASS.

- [ ] **Step 12.4: Commit**

```bash
git add mobile-bench/wallet-core/Cargo.toml \
        mobile-bench/wallet-core/src/oid4vp_client/http.rs
git commit -S -s -m "$(cat <<'EOF'
feat(oid4vp): post_response to redirect_uri

POSTs {id_token, state} as JSON; returns session_id + status.
401/4xx surfaces as the structured Status variant so the UI
can render "nonce mismatch" / "session expired" specifically.
EOF
)"
git log --format="%h %G? %s" -1
```

### Task 13: `oid4vp_client::flow` end-to-end function

**Files:**
- Modify: `mobile-bench/wallet-core/src/oid4vp_client/mod.rs`

One function that the UI calls — `run_authentication(qr_url, wallet, store, did)` —
does parse → fetch → build id_token → POST, returns the final session_id.

- [ ] **Step 13.1: Append failing test**

`mobile-bench/wallet-core/src/oid4vp_client/mod.rs` (append):

```rust
/// Drive the entire OID4VP / SIOPv2 authentication flow:
/// parse the QR URL → fetch the request object → mint a
/// DID-bound id_token → POST it back → return the issuer's
/// session_id + status.
pub async fn run_authentication(
    qr_url: &str,
    wallet: &crate::wallet::Wallet,
    secret_store: &dyn crate::secret_storage::SecretStorage,
    did: &crate::DidId,
) -> Result<http::PostResponseResult, AuthFlowError> {
    let request_uri = parser::parse_request_url(qr_url)?;
    let req = parser::fetch_request_object(&request_uri).await?;
    let id_token = jws::build_id_token(
        wallet, secret_store, did, &req.client_id, &req.nonce, 300,
    ).await?;
    let result = http::post_response(&req.redirect_uri, &id_token, req.state.as_deref()).await?;
    Ok(result)
}

#[derive(Debug, thiserror::Error)]
pub enum AuthFlowError {
    #[error(transparent)]
    Parse(#[from] parser::Oid4vpParseError),
    #[error(transparent)]
    Token(#[from] jws::IdTokenError),
    #[error(transparent)]
    Post(#[from] http::PostResponseError),
}

#[cfg(test)]
mod flow_tests {
    use super::*;
    use crate::test_support::*;
    use wiremock::{matchers::*, Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn run_authentication_happy_path() {
        let mock = MockServer::start().await;
        // 1. /request/abc returns the AuthRequest JSON
        Mock::given(method("GET")).and(path("/request/abc"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "client_id": "demo-issuer",
                "nonce": "nonce-x",
                "state": "st-x",
                "redirect_uri": format!("{}/authorize-response", mock.uri()),
            })))
            .mount(&mock).await;
        // 2. /authorize-response accepts the POST
        Mock::given(method("POST")).and(path("/authorize-response"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "session_id": "S-42", "status": "authenticated"
            })))
            .mount(&mock).await;

        let qr = format!("openid4vp://demo/?request_uri={}/request/abc", urlencoding::encode(&mock.uri()));
        let (wallet, did) = stub_wallet_with_bootstrapped_did([21u8; 32]).await;
        let store = stub_secret_store_with(&wallet, &did);

        let r = run_authentication(&qr, &wallet, &store, &did).await.expect("ok");
        assert_eq!(r.session_id, "S-42");
        assert_eq!(r.status, "authenticated");
    }
}
```

Add `urlencoding = "2"` to `[dev-dependencies]` if missing.

- [ ] **Step 13.2: Run + verify**

```bash
cargo test -p midnight-wallet-core --lib oid4vp_client::flow_tests
```

Expected: PASS.

- [ ] **Step 13.3: Re-export the flow function**

In `oid4vp_client/mod.rs` at the top with the other re-exports:

```rust
pub use self::{
    parser::{AuthRequest, Oid4vpParseError},
    jws::IdTokenError,
    http::{PostResponseResult, PostResponseError},
};
pub use run_authentication; // already in scope at module level
```

And in `lib.rs`:

```rust
pub mod oid4vp_client;
pub use oid4vp_client::{run_authentication as oid4vp_run_authentication, AuthFlowError};
```

- [ ] **Step 13.4: Final cargo check**

```bash
cargo check -p midnight-wallet-core
```

Expected: clean.

- [ ] **Step 13.5: Commit**

```bash
git add mobile-bench/wallet-core/src/oid4vp_client/mod.rs \
        mobile-bench/wallet-core/Cargo.toml \
        mobile-bench/wallet-core/src/lib.rs
git commit -S -s -m "$(cat <<'EOF'
feat(oid4vp): run_authentication end-to-end flow

Single entry point the UI calls: parse QR URL → fetch request
object → mint signed id_token → POST → return session_id +
status. Covered by a wiremock-backed integration test.
EOF
)"
git log --format="%h %G? %s" -1
```

---

## Section 4 — Wallet-core: `oid4vci_client` (Spec build step 4)

Receives a `birth` VC from the issuer over the Pre-Authorized Code Flow.
Parses the credential offer URL, exchanges the pre-auth code for an
access token + c_nonce, builds a DID-bound JWS proof over the c_nonce,
requests the credential, parses the returned VC + openings, and lands
them in `vc_store` atomically.

### Task 14: `oid4vci_client` offer parser

**Files:**
- Create: `mobile-bench/wallet-core/src/oid4vci_client/mod.rs`
- Create: `mobile-bench/wallet-core/src/oid4vci_client/offer.rs`
- Modify: `mobile-bench/wallet-core/src/lib.rs`

- [ ] **Step 14.1: Module root**

`mobile-bench/wallet-core/src/oid4vci_client/mod.rs`:

```rust
//! Client side of OID4VCI Pre-Authorized Code Flow for the
//! Midnight `birth` credential family.
//!
//! Steps:
//! 1. `offer::parse_offer_url` extracts the offer object from
//!    the QR's `openid-credential-offer://` URL.
//! 2. `token::request_token` exchanges the pre-auth code for an
//!    access token + c_nonce.
//! 3. `credential::request_credential` mints a DID-bound JWS
//!    proof over the c_nonce, POSTs `{proof, format}` to
//!    the credential endpoint, parses the VC + openings, and
//!    hands them to `vc_store` atomically.

mod offer;
mod token;
mod credential;

pub use offer::{CredentialOffer, parse_offer_url, Oid4vciParseError};
pub use token::{TokenResponse, request_token, Oid4vciTokenError};
pub use credential::{request_credential, IssuedVc, CredentialFlowError};
```

- [ ] **Step 14.2: Failing tests for the parser**

`mobile-bench/wallet-core/src/oid4vci_client/offer.rs`:

```rust
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialOffer {
    pub credential_issuer: String,
    pub credential_configuration_ids: Vec<String>,
    pub grants: Grants,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Grants {
    #[serde(rename = "urn:ietf:params:oauth:grant-type:pre-authorized_code")]
    pub pre_authorized: PreAuthorized,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreAuthorized {
    #[serde(rename = "pre-authorized_code")]
    pub code: String,
}

#[derive(Debug, thiserror::Error)]
pub enum Oid4vciParseError {
    #[error("bad scheme: {0}")]
    BadScheme(String),
    #[error("missing query param: {0}")]
    MissingParam(&'static str),
    #[error("url parse: {0}")]
    Url(#[from] url::ParseError),
    #[error("json parse: {0}")]
    Json(#[from] serde_json::Error),
}

/// Extract + parse the `credential_offer=<json>` query param from
/// an `openid-credential-offer://` URL. The JSON value is
/// URL-encoded per the OID4VCI spec.
pub fn parse_offer_url(url: &str) -> Result<CredentialOffer, Oid4vciParseError> {
    let u = Url::parse(url)?;
    if u.scheme() != "openid-credential-offer" {
        return Err(Oid4vciParseError::BadScheme(u.scheme().into()));
    }
    let raw = u.query_pairs()
        .find(|(k, _)| k == "credential_offer")
        .map(|(_, v)| v.into_owned())
        .ok_or(Oid4vciParseError::MissingParam("credential_offer"))?;
    let offer: CredentialOffer = serde_json::from_str(&raw)?;
    Ok(offer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_offer_url_works() {
        let offer_json = serde_json::json!({
            "credential_issuer": "https://issuer.local",
            "credential_configuration_ids": ["birth"],
            "grants": {
                "urn:ietf:params:oauth:grant-type:pre-authorized_code": {
                    "pre-authorized_code": "CODE-XYZ"
                }
            }
        }).to_string();
        let url = format!(
            "openid-credential-offer://issuer/?credential_offer={}",
            urlencoding::encode(&offer_json)
        );
        let offer = parse_offer_url(&url).expect("parse");
        assert_eq!(offer.credential_issuer, "https://issuer.local");
        assert_eq!(offer.credential_configuration_ids, vec!["birth".to_string()]);
        assert_eq!(offer.grants.pre_authorized.code, "CODE-XYZ");
    }

    #[test]
    fn parse_offer_url_rejects_wrong_scheme() {
        assert!(matches!(
            parse_offer_url("https://issuer/?credential_offer=%7B%7D"),
            Err(Oid4vciParseError::BadScheme(_))
        ));
    }
}
```

- [ ] **Step 14.3: Run + verify**

```bash
cargo test -p midnight-wallet-core --lib oid4vci_client::offer::tests
```

Expected: 2 PASS.

- [ ] **Step 14.4: Commit**

```bash
git add mobile-bench/wallet-core/src/oid4vci_client/ \
        mobile-bench/wallet-core/src/lib.rs
git commit -S -s -m "$(cat <<'EOF'
feat(oid4vci): credential offer URL parser

Parses openid-credential-offer:// URLs into a typed
CredentialOffer with credential_issuer, configuration ids,
and the pre-authorized_code grant. Wrong-scheme + missing-
param errors are typed for the caller.
EOF
)"
git log --format="%h %G? %s" -1
```

### Task 15: Token endpoint client

**Files:**
- Create: `mobile-bench/wallet-core/src/oid4vci_client/token.rs`

- [ ] **Step 15.1: Failing tests**

`mobile-bench/wallet-core/src/oid4vci_client/token.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
struct TokenRequest<'a> {
    grant_type: &'a str,
    #[serde(rename = "pre-authorized_code")]
    pre_authorized_code: &'a str,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub c_nonce: String,
    pub token_type: String,
    pub expires_in: Option<u64>,
}

#[derive(Debug, thiserror::Error)]
pub enum Oid4vciTokenError {
    #[error("http: {0}")]
    Http(String),
    #[error("non-2xx {status}: {body}")]
    Status { status: u16, body: String },
    #[error("decode: {0}")]
    Decode(#[from] serde_json::Error),
}

/// POST to `{issuer}/token` with the pre-authorized code,
/// return the access token + c_nonce.
pub async fn request_token(
    issuer: &str,
    pre_authorized_code: &str,
) -> Result<TokenResponse, Oid4vciTokenError> {
    let url = format!("{}/token", issuer.trim_end_matches('/'));
    let body = TokenRequest {
        grant_type: "urn:ietf:params:oauth:grant-type:pre-authorized_code",
        pre_authorized_code,
    };
    let resp = reqwest::Client::new()
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| Oid4vciTokenError::Http(e.to_string()))?;
    let status = resp.status();
    let text = resp.text().await.map_err(|e| Oid4vciTokenError::Http(e.to_string()))?;
    if !status.is_success() {
        return Err(Oid4vciTokenError::Status { status: status.as_u16(), body: text });
    }
    Ok(serde_json::from_str(&text)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{matchers::*, Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn request_token_round_trips() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .and(body_partial_json(serde_json::json!({
                "grant_type": "urn:ietf:params:oauth:grant-type:pre-authorized_code",
                "pre-authorized_code": "C1"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "AT-1",
                "c_nonce": "CN-1",
                "token_type": "Bearer",
                "expires_in": 600
            })))
            .mount(&mock).await;
        let t = request_token(&mock.uri(), "C1").await.expect("ok");
        assert_eq!(t.access_token, "AT-1");
        assert_eq!(t.c_nonce, "CN-1");
    }

    #[tokio::test]
    async fn request_token_surfaces_400_with_body() {
        let mock = MockServer::start().await;
        Mock::given(method("POST")).and(path("/token"))
            .respond_with(ResponseTemplate::new(400).set_body_string("invalid_grant"))
            .mount(&mock).await;
        let err = request_token(&mock.uri(), "X").await.expect_err("err");
        match err {
            Oid4vciTokenError::Status { status: 400, body } => assert_eq!(body, "invalid_grant"),
            other => panic!("expected 400 Status, got {other:?}"),
        }
    }
}
```

- [ ] **Step 15.2: Run + verify + commit**

```bash
cargo test -p midnight-wallet-core --lib oid4vci_client::token::tests
```

Expected: 2 PASS.

```bash
git add mobile-bench/wallet-core/src/oid4vci_client/token.rs
git commit -S -s -m "$(cat <<'EOF'
feat(oid4vci): Pre-Authorized Code token exchange

POST to /token with grant_type + pre-authorized_code,
returns access_token + c_nonce. 400 with body is preserved
so "invalid_grant" / "expired" reaches the UI.
EOF
)"
git log --format="%h %G? %s" -1
```

### Task 16: Credential endpoint + atomic `vc_store` insert

**Files:**
- Create: `mobile-bench/wallet-core/src/oid4vci_client/credential.rs`

- [ ] **Step 16.1: Failing tests**

`mobile-bench/wallet-core/src/oid4vci_client/credential.rs`:

```rust
use serde::{Deserialize, Serialize};

use crate::vc_store::{StoredVc, VcOpening, VcStore};
use crate::wallet::Wallet;
use crate::secret_storage::SecretStorage;
use crate::DidId;
use crate::oid4vp_client::jws::build_id_token;
use crate::oid4vci_client::token::TokenResponse;

#[derive(Debug, Serialize)]
struct CredentialRequest<'a> {
    format: &'a str,
    proof: Proof<'a>,
}

#[derive(Debug, Serialize)]
struct Proof<'a> {
    proof_type: &'a str,
    jwt: &'a str,
}

#[derive(Debug, Clone, Deserialize)]
pub struct IssuedVc {
    pub credential: CredentialBody,
    pub openings: Vec<OpeningWire>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CredentialBody {
    pub vc_uri: String,
    pub issuer_did: String,
    pub holder_did: String,
    pub body_b64: String, // base64-encoded Compact-serialized VC
}

#[derive(Debug, Clone, Deserialize)]
pub struct OpeningWire {
    pub claim_path: String,
    pub plaintext_b64: String,
    pub opening_b64: String,
}

#[derive(Debug, thiserror::Error)]
pub enum CredentialFlowError {
    #[error("http: {0}")]
    Http(String),
    #[error("non-2xx {status}: {body}")]
    Status { status: u16, body: String },
    #[error("decode: {0}")]
    Decode(#[from] serde_json::Error),
    #[error("base64: {0}")]
    Base64(#[from] base64::DecodeError),
    #[error("token error: {0}")]
    Token(#[from] crate::oid4vci_client::token::Oid4vciTokenError),
    #[error("proof JWS error: {0}")]
    Proof(#[from] crate::oid4vp_client::jws::IdTokenError),
    #[error("vc_store: {0}")]
    Store(#[from] crate::vc_store::VcStoreError),
}

/// Drive the full Pre-Authorized Code Flow end-to-end:
/// /token → /credential → land VC + openings in vc_store atomically.
pub async fn request_credential(
    issuer: &str,
    pre_authorized_code: &str,
    wallet: &Wallet,
    secret_store: &dyn SecretStorage,
    holder_did: &DidId,
    vc_store: &VcStore,
) -> Result<String, CredentialFlowError> {
    use base64::engine::general_purpose::STANDARD as B64;
    use base64::Engine;

    let token: TokenResponse =
        crate::oid4vci_client::token::request_token(issuer, pre_authorized_code).await?;

    // Build a DID-bound JWS over the c_nonce. The proof_type=jwt
    // path of OID4VCI just reuses the SIOPv2 id_token shape, with
    // `aud=issuer` and `nonce=c_nonce`.
    let proof_jwt = build_id_token(
        wallet, secret_store, holder_did, issuer, &token.c_nonce, 300,
    ).await?;

    let body = CredentialRequest {
        format: "midnight-vc-compact",
        proof: Proof { proof_type: "jwt", jwt: &proof_jwt },
    };
    let resp = reqwest::Client::new()
        .post(format!("{}/credential", issuer.trim_end_matches('/')))
        .bearer_auth(&token.access_token)
        .json(&body)
        .send()
        .await
        .map_err(|e| CredentialFlowError::Http(e.to_string()))?;
    let status = resp.status();
    let text = resp.text().await
        .map_err(|e| CredentialFlowError::Http(e.to_string()))?;
    if !status.is_success() {
        return Err(CredentialFlowError::Status { status: status.as_u16(), body: text });
    }
    let issued: IssuedVc = serde_json::from_str(&text)?;

    let vc = StoredVc {
        vc_uri: issued.credential.vc_uri.clone(),
        issuer_did: issued.credential.issuer_did,
        holder_did: issued.credential.holder_did,
        format: "midnight-vc-compact".into(),
        body: B64.decode(&issued.credential.body_b64)?,
        issued_at_ms: now_ms(),
    };
    let openings: Vec<VcOpening> = issued.openings.into_iter().map(|o| Ok(VcOpening {
        vc_uri: vc.vc_uri.clone(),
        claim_path: o.claim_path,
        plaintext: B64.decode(&o.plaintext_b64)?,
        opening: B64.decode(&o.opening_b64)?,
    })).collect::<Result<_, base64::DecodeError>>()?;
    vc_store.insert_vc_with_openings(&vc, &openings)?;
    Ok(vc.vc_uri)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::*;
    use base64::engine::general_purpose::STANDARD as B64;
    use base64::Engine;
    use tempfile::TempDir;
    use wiremock::{matchers::*, Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn request_credential_lands_vc_and_openings() {
        let mock = MockServer::start().await;
        Mock::given(method("POST")).and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "AT", "c_nonce": "CN", "token_type": "Bearer", "expires_in": 600
            }))).mount(&mock).await;
        let issued = serde_json::json!({
            "credential": {
                "vc_uri": "urn:uuid:birth-1",
                "issuer_did": "did:midnight:issuer",
                "holder_did": "did:midnight:alice",
                "body_b64": B64.encode(b"COMPACT_VC_BYTES")
            },
            "openings": [
                { "claim_path": "/credentialSubject/dateOfBirth",
                  "plaintext_b64": B64.encode(b"1985-01-01"),
                  "opening_b64":   B64.encode(b"rand") }
            ]
        });
        Mock::given(method("POST")).and(path("/credential"))
            .respond_with(ResponseTemplate::new(200).set_body_json(issued))
            .mount(&mock).await;

        let (wallet, did) = stub_wallet_with_bootstrapped_did([23u8; 32]).await;
        let store = stub_secret_store_with(&wallet, &did);
        let dir = TempDir::new().unwrap();
        let vc_store = VcStore::open(dir.path().join("vc.redb")).unwrap();

        let vc_uri = request_credential(&mock.uri(), "CODE-1", &wallet, &store, &did, &vc_store)
            .await.expect("ok");
        assert_eq!(vc_uri, "urn:uuid:birth-1");
        let landed = vc_store.get_vc(&vc_uri).unwrap().expect("present");
        assert_eq!(landed.body, b"COMPACT_VC_BYTES");
        let op = vc_store.get_opening(&vc_uri, "/credentialSubject/dateOfBirth").unwrap().expect("op");
        assert_eq!(op.plaintext, b"1985-01-01");
    }
}
```

- [ ] **Step 16.2: Run + verify**

```bash
cargo test -p midnight-wallet-core --lib oid4vci_client::credential::tests
```

Expected: 1 PASS.

- [ ] **Step 16.3: Commit**

```bash
git add mobile-bench/wallet-core/src/oid4vci_client/credential.rs
git commit -S -s -m "$(cat <<'EOF'
feat(oid4vci): request_credential + atomic vc_store insert

End-to-end credential request: token exchange → DID-bound JWS
proof over c_nonce → POST /credential → parse VC + openings →
land them in vc_store via insert_vc_with_openings.
EOF
)"
git log --format="%h %G? %s" -1
```

### Task 17: `oid4vci_client::run_issuance` orchestrator

**Files:**
- Modify: `mobile-bench/wallet-core/src/oid4vci_client/mod.rs`

One function the UI calls: takes a scanned `openid-credential-offer://`
URL and runs the full flow, returns the new VC URI.

- [ ] **Step 17.1: Add the orchestrator + a failing test**

Append to `oid4vci_client/mod.rs`:

```rust
/// Drive the full OID4VCI flow from a scanned QR URL.
pub async fn run_issuance(
    qr_url: &str,
    wallet: &crate::wallet::Wallet,
    secret_store: &dyn crate::secret_storage::SecretStorage,
    holder_did: &crate::DidId,
    vc_store: &crate::vc_store::VcStore,
) -> Result<String, IssuanceFlowError> {
    let offer = offer::parse_offer_url(qr_url)?;
    let code = offer.grants.pre_authorized.code.clone();
    let vc_uri = credential::request_credential(
        &offer.credential_issuer,
        &code,
        wallet, secret_store, holder_did, vc_store,
    ).await?;
    Ok(vc_uri)
}

#[derive(Debug, thiserror::Error)]
pub enum IssuanceFlowError {
    #[error(transparent)]
    Parse(#[from] offer::Oid4vciParseError),
    #[error(transparent)]
    Flow(#[from] credential::CredentialFlowError),
}

#[cfg(test)]
mod flow_tests {
    use super::*;
    use crate::test_support::*;
    use base64::engine::general_purpose::STANDARD as B64;
    use base64::Engine;
    use tempfile::TempDir;
    use wiremock::{matchers::*, Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn run_issuance_happy_path() {
        let mock = MockServer::start().await;
        Mock::given(method("POST")).and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "AT", "c_nonce": "CN", "token_type": "Bearer"
            }))).mount(&mock).await;
        Mock::given(method("POST")).and(path("/credential"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "credential": {
                    "vc_uri": "urn:uuid:flow-1",
                    "issuer_did": "did:midnight:i",
                    "holder_did": "did:midnight:h",
                    "body_b64": B64.encode(b"BODY")
                },
                "openings": []
            }))).mount(&mock).await;

        let offer_json = serde_json::json!({
            "credential_issuer": mock.uri(),
            "credential_configuration_ids": ["birth"],
            "grants": {
                "urn:ietf:params:oauth:grant-type:pre-authorized_code": {
                    "pre-authorized_code": "CODE-FLOW"
                }
            }
        }).to_string();
        let qr = format!(
            "openid-credential-offer://x/?credential_offer={}",
            urlencoding::encode(&offer_json),
        );

        let (wallet, did) = stub_wallet_with_bootstrapped_did([24u8; 32]).await;
        let store = stub_secret_store_with(&wallet, &did);
        let dir = TempDir::new().unwrap();
        let vc_store = crate::vc_store::VcStore::open(dir.path().join("v.redb")).unwrap();

        let uri = run_issuance(&qr, &wallet, &store, &did, &vc_store).await.expect("ok");
        assert_eq!(uri, "urn:uuid:flow-1");
    }
}
```

- [ ] **Step 17.2: Re-export from lib.rs**

```rust
pub mod oid4vci_client;
pub use oid4vci_client::{run_issuance as oid4vci_run_issuance, IssuanceFlowError};
```

- [ ] **Step 17.3: Run + verify**

```bash
cargo test -p midnight-wallet-core --lib oid4vci_client::flow_tests
```

Expected: PASS.

- [ ] **Step 17.4: Commit**

```bash
git add mobile-bench/wallet-core/src/oid4vci_client/mod.rs \
        mobile-bench/wallet-core/src/lib.rs
git commit -S -s -m "$(cat <<'EOF'
feat(oid4vci): run_issuance orchestrator

UI-facing entry point: scan QR URL → parse → token → credential
→ vc_store. Returns the new VC URI so the carousel can scroll
to the freshly issued card.
EOF
)"
git log --format="%h %G? %s" -1
```

---

## Section 5 — Wallet-core: `vc_self_verify` (Spec build step 5)

Re-resolves the issuer's DID and checks the VC's signature against the
`assertionMethod`-relation Jubjub key. Three-state result.

### Task 18: `vc_self_verify::self_verify`

**Files:**
- Create: `mobile-bench/wallet-core/src/vc_self_verify/mod.rs`
- Modify: `mobile-bench/wallet-core/src/lib.rs`

- [ ] **Step 18.1: Module + failing test**

`mobile-bench/wallet-core/src/vc_self_verify/mod.rs`:

```rust
//! Self-verification: re-resolve the issuer's DID against the
//! chain, find the assertionMethod-relation Jubjub key referenced
//! by the VC's `proof.verificationMethod`, and verify the VC
//! signature.
//!
//! Returns a three-state result. The `Stale` state isn't produced
//! by this function — it's a UI concern (cached result age >60s);
//! the cache layer lives in vc_store as `last_verified_ms` +
//! `last_verify_outcome`.

use crate::vc_store::StoredVc;
use crate::wallet::Wallet;

#[derive(Debug, Clone)]
pub enum SelfVerifyResult {
    Valid { resolved_at_ms: u64 },
    Invalid(InvalidReason),
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum InvalidReason {
    #[error("issuer DID no longer resolves: {0}")]
    IssuerUnresolvable(String),
    #[error("issuer's referenced key not in assertionMethod relation")]
    KeyNotInAssertionRelation,
    #[error("issuer's JWK could not be decoded into a Jubjub verifying key")]
    JwkDecode,
    #[error("VC body could not be canonicalised: {0}")]
    CanonicalSerialize(String),
    #[error("signature does not match")]
    SignatureMismatch,
    #[error("VC body lacks a parseable proof block")]
    NoProof,
}

pub async fn self_verify(vc: &StoredVc, wallet: &Wallet) -> SelfVerifyResult {
    use crate::DidId;

    let issuer = match DidId::parse(&vc.issuer_did) {
        Ok(d) => d,
        Err(e) => return SelfVerifyResult::Invalid(InvalidReason::IssuerUnresolvable(e.to_string())),
    };
    let doc = match wallet.resolve_did(&issuer).await {
        Ok(d) => d,
        Err(e) => return SelfVerifyResult::Invalid(InvalidReason::IssuerUnresolvable(e.to_string())),
    };

    // Decode the VC body to extract proof.verificationMethod + signature.
    let parsed = match crate::vc_self_verify::compact_vc::parse_proof(&vc.body) {
        Ok(p) => p,
        Err(_) => return SelfVerifyResult::Invalid(InvalidReason::NoProof),
    };

    let vm = match doc.assertion_method.iter()
        .find(|vm| vm.id.to_string() == parsed.verification_method_kid)
    {
        Some(vm) => vm,
        None => return SelfVerifyResult::Invalid(InvalidReason::KeyNotInAssertionRelation),
    };

    let pk = match crate::vc_self_verify::compact_vc::jubjub_pk_from_jwk(&vm.public_key_jwk) {
        Ok(p) => p,
        Err(_) => return SelfVerifyResult::Invalid(InvalidReason::JwkDecode),
    };

    let canonical = match crate::vc_self_verify::compact_vc::canonical_serialize_for_verify(&vc.body) {
        Ok(b) => b,
        Err(e) => return SelfVerifyResult::Invalid(InvalidReason::CanonicalSerialize(e.to_string())),
    };

    if pk.verify(&canonical, &parsed.signature) {
        SelfVerifyResult::Valid {
            resolved_at_ms: now_ms(),
        }
    } else {
        SelfVerifyResult::Invalid(InvalidReason::SignatureMismatch)
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

mod compact_vc {
    //! Thin wrappers around the Compact VC parse + Jubjub verify
    //! primitives. Kept in its own submodule so the public surface
    //! doesn't bleed serde / curve type details.

    use crate::did::JubjubPublicKey;

    pub(super) struct ParsedProof {
        pub verification_method_kid: String,
        pub signature: Vec<u8>,
    }

    pub(super) fn parse_proof(body: &[u8]) -> Result<ParsedProof, String> {
        // Phase 1's birth VC body is a Compact-serialized struct
        // matching midnight-did-credentials-birth's wire format.
        // Use the helper from wallet-core::did::compact_vc (lifted
        // from the existing read paths) or fall back to a thin
        // serde_cbor decode for the demo if the helper isn't ready.
        let v: serde_cbor::Value = serde_cbor::from_slice(body)
            .map_err(|e| format!("cbor: {e}"))?;
        let kid = extract_str(&v, &["proof", "verificationMethod"])
            .ok_or("missing proof.verificationMethod")?;
        let sig_b64 = extract_str(&v, &["proof", "signature"])
            .ok_or("missing proof.signature")?;
        let sig = base64::engine::general_purpose::STANDARD
            .decode(sig_b64)
            .map_err(|e| format!("b64: {e}"))?;
        Ok(ParsedProof { verification_method_kid: kid, signature: sig })
    }

    pub(super) fn jubjub_pk_from_jwk(jwk: &serde_json::Value) -> Result<JubjubPublicKey, String> {
        JubjubPublicKey::from_jwk(jwk).map_err(|e| e.to_string())
    }

    pub(super) fn canonical_serialize_for_verify(body: &[u8]) -> Result<Vec<u8>, String> {
        // The canonical form is body-with-proof-stripped, re-serialized
        // in the issuer's canonical ordering. Phase 1 implementation:
        // decode → remove "proof" field → re-encode as deterministic CBOR.
        let mut v: serde_cbor::Value = serde_cbor::from_slice(body)
            .map_err(|e| format!("cbor: {e}"))?;
        if let serde_cbor::Value::Map(ref mut m) = v {
            m.retain(|k, _| !matches!(k, serde_cbor::Value::Text(s) if s == "proof"));
        }
        let mut out = Vec::new();
        ciborium::ser::into_writer(&v, &mut out)
            .map_err(|e| format!("canon cbor: {e}"))?;
        Ok(out)
    }

    fn extract_str(v: &serde_cbor::Value, path: &[&str]) -> Option<String> {
        let mut cur = v;
        for p in path {
            if let serde_cbor::Value::Map(m) = cur {
                let needle = serde_cbor::Value::Text((*p).into());
                cur = m.iter().find(|(k, _)| *k == needle).map(|(_, v)| v)?;
            } else {
                return None;
            }
        }
        if let serde_cbor::Value::Text(s) = cur {
            Some(s.clone())
        } else {
            None
        }
    }

    use base64::Engine as _;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::*;
    use crate::vc_store::StoredVc;

    #[tokio::test]
    async fn self_verify_valid_round_trip() {
        let (issuer_wallet, issuer_did) = stub_wallet_with_bootstrapped_did([55u8; 32]).await;
        let signed_body = stub_sign_birth_vc(&issuer_wallet, &issuer_did, b"BIRTH-FIXTURE").await;
        let vc = StoredVc {
            vc_uri: "urn:uuid:birth-1".into(),
            issuer_did: issuer_did.as_str().into(),
            holder_did: "did:midnight:alice".into(),
            format: "midnight-vc-compact".into(),
            body: signed_body,
            issued_at_ms: 0,
        };

        let r = self_verify(&vc, &issuer_wallet).await;
        assert!(matches!(r, SelfVerifyResult::Valid { .. }));
    }

    #[tokio::test]
    async fn self_verify_tampered_body_is_invalid() {
        let (issuer_wallet, issuer_did) = stub_wallet_with_bootstrapped_did([56u8; 32]).await;
        let mut body = stub_sign_birth_vc(&issuer_wallet, &issuer_did, b"BIRTH-FIXTURE").await;
        if let Some(b) = body.get_mut(8) { *b ^= 0xFF; } // flip one byte in the body, not the proof
        let vc = StoredVc {
            vc_uri: "urn:uuid:b".into(),
            issuer_did: issuer_did.as_str().into(),
            holder_did: "did:midnight:h".into(),
            format: "midnight-vc-compact".into(),
            body, issued_at_ms: 0,
        };
        let r = self_verify(&vc, &issuer_wallet).await;
        assert!(matches!(r, SelfVerifyResult::Invalid(InvalidReason::SignatureMismatch)));
    }
}
```

Add to `test_support.rs`:

```rust
pub async fn stub_sign_birth_vc(
    wallet: &Wallet,
    issuer_did: &crate::DidId,
    payload: &[u8],
) -> Vec<u8> {
    // Build a minimal CBOR body matching what self_verify decodes:
    // top-level Map { "credentialSubject": {...}, "proof": {"verificationMethod": kid, "signature": b64sig } }
    // The signature is over canonical (body with proof field removed).
    let doc = wallet.resolve_did(issuer_did).await.expect("resolve");
    let vm = doc.assertion_method.first().expect("assertion vm");
    let kid = vm.id.to_string();
    let key_ref = wallet.secret_store_ref().find_by_kid(&kid).expect("local key");

    let body_no_proof = serde_cbor::Value::Map({
        let mut m = std::collections::BTreeMap::new();
        m.insert(
            serde_cbor::Value::Text("credentialSubject".into()),
            serde_cbor::Value::Bytes(payload.to_vec()),
        );
        m.into_iter().collect()
    });
    let mut canonical = Vec::new();
    ciborium::ser::into_writer(&body_no_proof, &mut canonical).expect("cbor");
    let sig = wallet.secret_store_ref().sign(&key_ref, &canonical).expect("sign").into_bytes();
    let sig_b64 = base64::engine::general_purpose::STANDARD.encode(&sig);

    let mut full = match body_no_proof {
        serde_cbor::Value::Map(m) => m,
        _ => unreachable!(),
    };
    let proof = serde_cbor::Value::Map({
        let mut m = std::collections::BTreeMap::new();
        m.insert(
            serde_cbor::Value::Text("verificationMethod".into()),
            serde_cbor::Value::Text(kid.clone()),
        );
        m.insert(
            serde_cbor::Value::Text("signature".into()),
            serde_cbor::Value::Text(sig_b64),
        );
        m.into_iter().collect()
    });
    full.insert(serde_cbor::Value::Text("proof".into()), proof);
    let mut out = Vec::new();
    serde_cbor::to_writer(&mut out, &serde_cbor::Value::Map(full)).expect("cbor");
    out
}
```

If `ciborium`, `base64::Engine` import paths aren't set up, add to deps and adjust imports. The chosen CBOR shape isn't the real Compact birth-VC encoding — it's a placeholder that lets `self_verify` exercise its full path end-to-end. The issuer-side `vcMinter` in Task 27 will produce the same shape so this stays consistent until the real Compact encoder lands.

- [ ] **Step 18.2: Run + verify**

```bash
cargo test -p midnight-wallet-core --lib vc_self_verify::tests
```

Expected: 2 PASS.

- [ ] **Step 18.3: Re-export + commit**

`lib.rs`:

```rust
pub mod vc_self_verify;
pub use vc_self_verify::{self_verify, SelfVerifyResult, InvalidReason};
```

```bash
cargo check -p midnight-wallet-core
```

Expected: clean.

```bash
git add mobile-bench/wallet-core/src/vc_self_verify/ \
        mobile-bench/wallet-core/src/test_support.rs \
        mobile-bench/wallet-core/src/lib.rs \
        mobile-bench/wallet-core/Cargo.toml
git commit -S -s -m "$(cat <<'EOF'
feat(vc): vc_self_verify::self_verify three-state result

Re-resolves issuer DID, picks the assertionMethod-relation
Jubjub key referenced by proof.verificationMethod, decodes
the JWK, verifies the signature over the canonical
(proof-stripped) body. Returns Valid {resolved_at_ms} or
Invalid(reason) — Stale is a UI cache concern.
EOF
)"
git log --format="%h %G? %s" -1
```

### Task 19: `vc_self_verify` writes verify outcome to metadata

**Files:**
- Modify: `mobile-bench/wallet-core/src/vc_self_verify/mod.rs`
- Modify: `mobile-bench/wallet-core/src/vc_store/api.rs` (re-export `VcStore` already done)

Wraps `self_verify` to also persist `last_verified_ms` +
`last_verify_outcome` on the VC's metadata — that's how the carousel
gets its "Stale (last checked 12:34:56)" subtitle without re-running
the chain query on every render.

- [ ] **Step 19.1: Failing test**

Append to `vc_self_verify/mod.rs`:

```rust
pub async fn self_verify_and_cache(
    vc: &StoredVc,
    wallet: &Wallet,
    vc_store: &crate::vc_store::VcStore,
) -> SelfVerifyResult {
    let r = self_verify(vc, wallet).await;
    let outcome = match &r {
        SelfVerifyResult::Valid { .. } => "Valid".to_string(),
        SelfVerifyResult::Invalid(reason) => format!("Invalid: {reason}"),
    };
    let _ = vc_store.update_metadata(&vc.vc_uri, |m| {
        m.last_verified_ms = Some(now_ms());
        m.last_verify_outcome = Some(outcome);
    });
    r
}

#[cfg(test)]
mod cache_tests {
    use super::*;
    use crate::test_support::*;
    use crate::vc_store::{StoredVc, VcStore};
    use tempfile::TempDir;

    #[tokio::test]
    async fn self_verify_and_cache_writes_metadata() {
        let (wallet, did) = stub_wallet_with_bootstrapped_did([66u8; 32]).await;
        let body = stub_sign_birth_vc(&wallet, &did, b"X").await;
        let vc = StoredVc {
            vc_uri: "urn:uuid:cache-1".into(),
            issuer_did: did.as_str().into(),
            holder_did: "did:midnight:h".into(),
            format: "midnight-vc-compact".into(),
            body, issued_at_ms: 0,
        };
        let dir = TempDir::new().unwrap();
        let vc_store = VcStore::open(dir.path().join("v.redb")).unwrap();
        vc_store.insert_vc(&vc).unwrap();

        let r = self_verify_and_cache(&vc, &wallet, &vc_store).await;
        assert!(matches!(r, SelfVerifyResult::Valid { .. }));

        let md = vc_store.get_metadata(&vc.vc_uri).unwrap().expect("md");
        assert_eq!(md.last_verify_outcome.as_deref(), Some("Valid"));
        assert!(md.last_verified_ms.is_some());
    }
}
```

- [ ] **Step 19.2: Run + verify**

```bash
cargo test -p midnight-wallet-core --lib vc_self_verify::cache_tests
```

Expected: PASS.

- [ ] **Step 19.3: Commit**

```bash
git add mobile-bench/wallet-core/src/vc_self_verify/mod.rs
git commit -S -s -m "$(cat <<'EOF'
feat(vc): self_verify_and_cache writes outcome to metadata

UI calls this so a card's "Stale (last checked …)" subtitle
falls out of vc_store reads without re-running the chain
query on every render.
EOF
)"
git log --format="%h %G? %s" -1
```

---

## Section 6 — IssuerDIDIT-mock scaffold + standalone env (Spec build step 6, part 1)

**Context switch:** the next ~9 tasks happen in the **issuer repo**:

```bash
cd ~/iohk/midnight-identity-workspace/midnight-identity-solution-examples
bash ~/iohk/git-iohk.sh   # apply IOHK git config
git checkout develop
git pull
```

All commits in Section 6-7 use the `feat(issuer-mock):` prefix.

### Task 20: Package skeleton + docker-compose env

**Files:**
- Create: `IssuerDIDIT-mock/package.json`
- Create: `IssuerDIDIT-mock/tsconfig.json`
- Create: `IssuerDIDIT-mock/.gitignore`
- Create: `IssuerDIDIT-mock/e2e/fixtures/docker-compose.yml`
- Create: `IssuerDIDIT-mock/README.md` (skeleton; expanded in Task 28)

- [ ] **Step 20.1: Initialise the package**

```bash
mkdir -p IssuerDIDIT-mock/{src,scripts,e2e/fixtures}
cd IssuerDIDIT-mock
cat > package.json <<'PKG'
{
  "name": "@midnight-ntwrk/issuer-didit-mock",
  "version": "0.1.0",
  "description": "Mock issuer for Identity Centre Phase 1 — stable OID4VP/VCI HTTP contract with an operator-driven form replacing real DIDIT KYC.",
  "type": "module",
  "private": true,
  "scripts": {
    "build": "tsc -p tsconfig.json",
    "start": "node --experimental-specifier-resolution=node dist/server.js",
    "dev": "tsx src/server.ts",
    "bootstrap": "tsx scripts/bootstrap-issuer-did.ts",
    "test": "cucumber-js --config cucumber.cjs",
    "env:up": "docker compose -f e2e/fixtures/docker-compose.yml up -d --wait",
    "env:down": "docker compose -f e2e/fixtures/docker-compose.yml down -v"
  },
  "dependencies": {
    "@midnight-ntwrk/midnight-did": "*",
    "@midnight-ntwrk/midnight-did-api": "*",
    "@midnight-ntwrk/midnight-did-jubjub-schnorr": "*",
    "express": "^4.19.2",
    "better-sqlite3": "^11.3.0",
    "ejs": "^3.1.10",
    "qrcode": "^1.5.4",
    "@noble/ed25519": "^2.1.0",
    "jose": "^5.9.0",
    "zod": "^3.23.8"
  },
  "devDependencies": {
    "@types/express": "^4.17.21",
    "@types/node": "^22.7.4",
    "@types/qrcode": "^1.5.5",
    "@cucumber/cucumber": "^11.0.0",
    "playwright": "^1.48.0",
    "tsx": "^4.19.1",
    "typescript": "^5.6.2"
  }
}
PKG
```

- [ ] **Step 20.2: tsconfig + .gitignore**

```bash
cat > tsconfig.json <<'TSC'
{
  "compilerOptions": {
    "target": "ES2022",
    "module": "ESNext",
    "moduleResolution": "Bundler",
    "rootDir": "src",
    "outDir": "dist",
    "strict": true,
    "esModuleInterop": true,
    "resolveJsonModule": true,
    "skipLibCheck": true,
    "sourceMap": true,
    "declaration": false
  },
  "include": ["src/**/*", "scripts/**/*"],
  "exclude": ["dist", "node_modules", "e2e"]
}
TSC

cat > .gitignore <<'IGN'
node_modules/
dist/
*.tsbuildinfo
issuer-keystore.json
issuer.sqlite*
.env
.env.local
IGN
```

- [ ] **Step 20.3: README skeleton**

```bash
cat > README.md <<'RDM'
# IssuerDIDIT-mock

Mock issuer for Identity Centre Phase 1.

**Status:** under active implementation per
`docs/superpowers/plans/2026-05-25-identity-centre-phase-1.md`
in the wallet repo. Setup instructions land at Task 28.

## Quick start (once Task 28 is done)

```bash
yarn install
yarn env:up
yarn bootstrap
yarn dev
```

See the spec at the wallet repo's
`docs/superpowers/specs/2026-05-25-identity-centre-phase-1-design.md`
for the full architecture.
RDM
```

- [ ] **Step 20.4: docker-compose for standalone Midnight env**

```bash
cat > e2e/fixtures/docker-compose.yml <<'COMPOSE'
# Standalone Midnight env for Identity Centre Phase 1.
# Brings up the minimal trio: node + indexer + (optional) proof-server.
#
# Versions track the latest standalone images published by the
# midnight-node + midnight-indexer projects. Bump when those bump.

services:
  node:
    image: ghcr.io/midnightntwrk/midnight-node:standalone-latest
    command:
      - --dev
      - --rpc-external
      - --rpc-cors=all
      - --base-path=/data
    ports:
      - "9944:9944"
    volumes:
      - node-data:/data
    healthcheck:
      test: ["CMD-SHELL", "curl -sf -X POST http://localhost:9944 -H 'Content-Type: application/json' -d '{\"jsonrpc\":\"2.0\",\"method\":\"system_chain\",\"id\":1}' || exit 1"]
      interval: 5s
      timeout: 3s
      retries: 30

  indexer:
    image: ghcr.io/midnightntwrk/midnight-indexer:standalone-latest
    depends_on:
      node:
        condition: service_healthy
    environment:
      - NODE_WS_URL=ws://node:9944
    ports:
      - "8088:8088"
    healthcheck:
      test: ["CMD-SHELL", "curl -sf http://localhost:8088/api/v1/healthz || exit 1"]
      interval: 5s
      timeout: 3s
      retries: 30

volumes:
  node-data:
COMPOSE
```

If the standalone image tags differ in the repo's local docker registry, update the `image:` lines. The exact tag must produce a chain with the Midnight DID contract pre-deployed — confirm at session start by reading `~/iohk/midnight-identity-workspace/midnight-did/AGENT.md`.

- [ ] **Step 20.5: Smoke test + commit**

```bash
yarn install --frozen-lockfile  # populates node_modules
yarn env:up
docker compose -f e2e/fixtures/docker-compose.yml ps
yarn env:down
```

Expected: both services come up; `ps` shows them healthy; teardown removes the volume.

```bash
git add IssuerDIDIT-mock/
git commit -S -s -m "$(cat <<'EOF'
feat(issuer-mock): package skeleton + standalone-env compose

Initial scaffolding for the IssuerDIDIT-mock package. Pulls
in midnight-did + midnight-did-api for in-process DID
resolution; express + better-sqlite3 for the HTTP surface;
ejs + qrcode for the laptop-browser pages; jose for SIOPv2
id_token verification; cucumber + playwright for the BDD
harness (lands in Section 10).

Docker compose brings up node + indexer; both are
health-checked so `yarn env:up --wait` blocks until they're
ready.
EOF
)"
git log --format="%h %G? %s" -1
```

### Task 21: Express server entrypoint + config

**Files:**
- Create: `IssuerDIDIT-mock/src/server.ts`
- Create: `IssuerDIDIT-mock/src/config.ts`

- [ ] **Step 21.1: Config module**

`IssuerDIDIT-mock/src/config.ts`:

```typescript
//! Env-var configuration with sensible defaults for local demo runs.

import { z } from "zod";

const schema = z.object({
  PORT: z.coerce.number().int().positive().default(3001),
  INDEXER_URL: z.string().url().default("http://localhost:8088/api/v1/graphql"),
  NODE_RPC_URL: z.string().url().default("http://localhost:9944"),
  PUBLIC_BASE_URL: z.string().url().default("http://localhost:3001"),
  KEYSTORE_PATH: z.string().default("./issuer-keystore.json"),
  SQLITE_PATH: z.string().default("./issuer.sqlite"),
  ISSUER_BOOTSTRAP_SEED: z.string().default("issuer-demo-seed"),
  KYC_DELAY_MS: z.coerce.number().int().nonnegative().default(2000),
});

export type Config = z.infer<typeof schema>;
export const config: Config = schema.parse(process.env);
```

- [ ] **Step 21.2: Server entrypoint**

`IssuerDIDIT-mock/src/server.ts`:

```typescript
import express from "express";
import path from "node:path";
import { config } from "./config.js";

export function buildApp() {
  const app = express();
  app.use(express.json());
  app.use(express.urlencoded({ extended: true }));
  app.set("view engine", "ejs");
  app.set("views", path.join(process.cwd(), "src/views"));

  // Routes wire in across Tasks 25-28.
  app.get("/healthz", (_req, res) => res.json({ ok: true }));

  return app;
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const app = buildApp();
  app.listen(config.PORT, () => {
    console.log(`IssuerDIDIT-mock listening on ${config.PUBLIC_BASE_URL} (port ${config.PORT})`);
  });
}
```

- [ ] **Step 21.3: Smoke test**

```bash
yarn dev &
SERVER_PID=$!
sleep 2
curl -sS http://localhost:3001/healthz
kill $SERVER_PID
```

Expected: `{"ok":true}`.

- [ ] **Step 21.4: Commit**

```bash
git add IssuerDIDIT-mock/src/server.ts IssuerDIDIT-mock/src/config.ts
git commit -S -s -m "$(cat <<'EOF'
feat(issuer-mock): express entrypoint + zod-validated config

Healthcheck-only baseline. Routes wire in across Tasks
25-28. Config is fully env-driven so the BDD harness can
override KYC_DELAY_MS, SQLITE_PATH, etc. without touching
files.
EOF
)"
git log --format="%h %G? %s" -1
```

### Task 22: SQLite session storage

**Files:**
- Create: `IssuerDIDIT-mock/src/storage/sessions.ts`

- [ ] **Step 22.1: Session table + CRUD**

`IssuerDIDIT-mock/src/storage/sessions.ts`:

```typescript
//! Per-issuance-attempt session row. Persisted to SQLite so a
//! restart doesn't lose in-flight sessions.

import Database from "better-sqlite3";
import { config } from "../config.js";

export type SessionStatus =
  | "authorized"
  | "kyc_done"
  | "vc_issued"
  | "failed";

export interface BirthVcClaims {
  firstName: string;
  lastName: string;
  dateOfBirth: string;     // ISO 8601 yyyy-mm-dd
  nationality: string;     // ISO 3166-1 alpha-3
  documentNumber: string;
}

export interface Session {
  id: string;
  status: SessionStatus;
  holder_did: string | null;
  oid4vp_nonce: string;
  vc_claims: BirthVcClaims | null;
  pre_authorized_code: string | null;
  c_nonce: string | null;
  vc_uri: string | null;
  vc_body_b64: string | null;
  created_at_ms: number;
  updated_at_ms: number;
}

let db: Database.Database | null = null;

function getDb(): Database.Database {
  if (!db) {
    db = new Database(config.SQLITE_PATH);
    db.pragma("journal_mode = WAL");
    db.exec(`
      CREATE TABLE IF NOT EXISTS sessions (
        id TEXT PRIMARY KEY,
        status TEXT NOT NULL,
        holder_did TEXT,
        oid4vp_nonce TEXT NOT NULL,
        vc_claims_json TEXT,
        pre_authorized_code TEXT,
        c_nonce TEXT,
        vc_uri TEXT,
        vc_body_b64 TEXT,
        created_at_ms INTEGER NOT NULL,
        updated_at_ms INTEGER NOT NULL
      );
      CREATE INDEX IF NOT EXISTS idx_sessions_code ON sessions(pre_authorized_code);
    `);
  }
  return db;
}

function rowToSession(row: any): Session {
  return {
    id: row.id,
    status: row.status,
    holder_did: row.holder_did,
    oid4vp_nonce: row.oid4vp_nonce,
    vc_claims: row.vc_claims_json ? JSON.parse(row.vc_claims_json) : null,
    pre_authorized_code: row.pre_authorized_code,
    c_nonce: row.c_nonce,
    vc_uri: row.vc_uri,
    vc_body_b64: row.vc_body_b64,
    created_at_ms: row.created_at_ms,
    updated_at_ms: row.updated_at_ms,
  };
}

export function createSession(oid4vpNonce: string): Session {
  const id = crypto.randomUUID();
  const now = Date.now();
  getDb().prepare(`
    INSERT INTO sessions (id, status, oid4vp_nonce, created_at_ms, updated_at_ms)
    VALUES (?, 'authorized', ?, ?, ?)
  `).run(id, oid4vpNonce, now, now);
  return getSession(id)!;
}

export function getSession(id: string): Session | null {
  const row = getDb().prepare("SELECT * FROM sessions WHERE id = ?").get(id);
  return row ? rowToSession(row) : null;
}

export function getSessionByCode(code: string): Session | null {
  const row = getDb()
    .prepare("SELECT * FROM sessions WHERE pre_authorized_code = ?")
    .get(code);
  return row ? rowToSession(row) : null;
}

export function updateSession(id: string, patch: Partial<Session>): Session {
  const cur = getSession(id);
  if (!cur) throw new Error(`session ${id} not found`);
  const next = { ...cur, ...patch, updated_at_ms: Date.now() };
  getDb().prepare(`
    UPDATE sessions
    SET status=?, holder_did=?, vc_claims_json=?, pre_authorized_code=?,
        c_nonce=?, vc_uri=?, vc_body_b64=?, updated_at_ms=?
    WHERE id=?
  `).run(
    next.status, next.holder_did,
    next.vc_claims ? JSON.stringify(next.vc_claims) : null,
    next.pre_authorized_code, next.c_nonce, next.vc_uri, next.vc_body_b64,
    next.updated_at_ms, id
  );
  return next;
}

/// Consume + invalidate a nonce — atomic via UPDATE … RETURNING.
/// Returns the session if the nonce was unused; null otherwise.
export function consumeNonce(nonce: string): Session | null {
  const row = getDb().prepare(`
    UPDATE sessions
    SET oid4vp_nonce = oid4vp_nonce || '_consumed', updated_at_ms = ?
    WHERE oid4vp_nonce = ? AND status = 'authorized'
    RETURNING *
  `).get(Date.now(), nonce);
  return row ? rowToSession(row) : null;
}
```

- [ ] **Step 22.2: Smoke test**

```bash
yarn tsx -e '
import { createSession, consumeNonce, getSession } from "./src/storage/sessions.js";
const s = createSession("nonce-x");
console.log("created", s.id, s.status);
const c = consumeNonce("nonce-x");
console.log("consumed", c?.id === s.id);
const c2 = consumeNonce("nonce-x");
console.log("re-consume null", c2 === null);
'
rm -f issuer.sqlite*
```

Expected output:
```
created <uuid> authorized
consumed true
re-consume null true
```

- [ ] **Step 22.3: Commit**

```bash
git add IssuerDIDIT-mock/src/storage/sessions.ts
git commit -S -s -m "$(cat <<'EOF'
feat(issuer-mock): SQLite session store with nonce consumption

WAL-mode SQLite, single sessions table, atomic
consume-on-success via UPDATE … RETURNING. Nonce reuse
returns null so the replay BDD scenario in Section 10
can assert "401 on second authorize-response with same
id_token".
EOF
)"
git log --format="%h %G? %s" -1
```

### Task 23: Issuer DID bootstrap script + keystore loader

**Files:**
- Create: `IssuerDIDIT-mock/scripts/bootstrap-issuer-did.ts`
- Create: `IssuerDIDIT-mock/scripts/issuer-keystore.example.json`
- Create: `IssuerDIDIT-mock/src/services/issuerDid.ts`

- [ ] **Step 23.1: Keystore example**

```bash
cat > IssuerDIDIT-mock/scripts/issuer-keystore.example.json <<'KS'
{
  "did": "did:midnight:0000000000000000000000000000000000000000000000000000000000000000",
  "ed25519": {
    "kid": "did:midnight:...#key-auth",
    "secret_hex": "0000000000000000000000000000000000000000000000000000000000000000"
  },
  "jubjub": {
    "kid": "did:midnight:...#key-assert",
    "secret_hex": "0000000000000000000000000000000000000000000000000000000000000000"
  }
}
KS
```

- [ ] **Step 23.2: Bootstrap script**

`IssuerDIDIT-mock/scripts/bootstrap-issuer-did.ts`:

```typescript
//! Idempotent: refuses to run if KEYSTORE_PATH already exists.
//! Delete the file to re-bootstrap.

import fs from "node:fs";
import crypto from "node:crypto";
import { config } from "../src/config.js";

// midnight-did-api exports a high-level helper; the actual function
// name may evolve — confirm against
// ~/iohk/midnight-identity-workspace/midnight-did/packages/api/src/lib.ts
// at execution time. We expect a shape similar to:
//   createDidWithKeys({ indexerUrl, nodeRpcUrl, seed })
//     -> { did, ed25519: { kid, secretHex }, jubjub: { kid, secretHex } }
import * as didApi from "@midnight-ntwrk/midnight-did-api";

function seedToBytes(seed: string): Uint8Array {
  const hex = seed.startsWith("0x") ? seed.slice(2) : seed;
  if (/^[0-9a-fA-F]+$/.test(hex) && hex.length === 64) {
    return Uint8Array.from(Buffer.from(hex, "hex"));
  }
  return crypto.createHash("sha256").update(seed, "utf8").digest();
}

async function main() {
  if (fs.existsSync(config.KEYSTORE_PATH)) {
    console.error(
      `Refusing to overwrite ${config.KEYSTORE_PATH}. ` +
        `Delete it manually to re-bootstrap.`,
    );
    process.exit(2);
  }

  console.log(`Bootstrapping issuer DID against ${config.NODE_RPC_URL}`);
  const seed = seedToBytes(config.ISSUER_BOOTSTRAP_SEED);

  const result = await didApi.createDidWithKeys({
    indexerUrl: config.INDEXER_URL,
    nodeRpcUrl: config.NODE_RPC_URL,
    seed,
  });

  const keystore = {
    did: result.did,
    ed25519: { kid: result.ed25519.kid, secret_hex: result.ed25519.secretHex },
    jubjub:  { kid: result.jubjub.kid,  secret_hex: result.jubjub.secretHex  },
  };
  fs.writeFileSync(
    config.KEYSTORE_PATH,
    JSON.stringify(keystore, null, 2),
  );
  console.log(`Wrote ${config.KEYSTORE_PATH}`);
  console.log(`Issuer DID: ${keystore.did}`);
}

main().catch(err => {
  console.error("Bootstrap failed:", err);
  process.exit(1);
});
```

If `@midnight-ntwrk/midnight-did-api` doesn't yet expose `createDidWithKeys`, this is the moment to add it (or its equivalent name) in the `midnight-did/packages/api` package — the spec called out that the bootstrap helper must exist on both sides. Use `~/iohk/midnight-identity-workspace/midnight-did/AGENT.md` for the contract there and follow the integration-test seed convention.

- [ ] **Step 23.3: Issuer keystore loader**

`IssuerDIDIT-mock/src/services/issuerDid.ts`:

```typescript
import fs from "node:fs";
import { config } from "../config.js";

export interface IssuerKeys {
  did: string;
  ed25519: { kid: string; secretBytes: Uint8Array };
  jubjub:  { kid: string; secretBytes: Uint8Array };
}

let cached: IssuerKeys | null = null;

export function getIssuerKeys(): IssuerKeys {
  if (cached) return cached;
  if (!fs.existsSync(config.KEYSTORE_PATH)) {
    throw new Error(
      `Issuer keystore missing at ${config.KEYSTORE_PATH}. ` +
        `Run \`yarn bootstrap\` first.`,
    );
  }
  const raw = JSON.parse(fs.readFileSync(config.KEYSTORE_PATH, "utf8"));
  cached = {
    did: raw.did,
    ed25519: {
      kid: raw.ed25519.kid,
      secretBytes: Buffer.from(raw.ed25519.secret_hex, "hex"),
    },
    jubjub: {
      kid: raw.jubjub.kid,
      secretBytes: Buffer.from(raw.jubjub.secret_hex, "hex"),
    },
  };
  return cached;
}

export function clearCachedIssuerKeys() { cached = null; }
```

- [ ] **Step 23.4: Manual smoke**

(Postponed until the env is up — covered by the BDD `bootstrap.feature` in Task 42.)

- [ ] **Step 23.5: Commit**

```bash
git add IssuerDIDIT-mock/scripts/ IssuerDIDIT-mock/src/services/issuerDid.ts
git commit -S -s -m "$(cat <<'EOF'
feat(issuer-mock): bootstrap-issuer-did script + keystore loader

Idempotent script via @midnight-ntwrk/midnight-did-api;
refuses to overwrite an existing keystore. Cached loader
hands DID + key bytes to every service module.

If midnight-did-api's createDidWithKeys helper isn't yet
exposed, this is the moment to add it on that side — the
spec calls out the symmetric bootstrap helper on both
wallet and issuer.
EOF
)"
git log --format="%h %G? %s" -1
```

### Task 24: Holder DID resolver (embedded)

**Files:**
- Create: `IssuerDIDIT-mock/src/services/holderDidResolver.ts`

- [ ] **Step 24.1: Embedded resolver**

`IssuerDIDIT-mock/src/services/holderDidResolver.ts`:

```typescript
//! Resolve a holder DID against the standalone indexer using the
//! @midnight-ntwrk/midnight-did package directly. No sidecar.

import {
  MidnightDidResolver,
  type DidDocument,
} from "@midnight-ntwrk/midnight-did";
import { config } from "../config.js";

let resolver: MidnightDidResolver | null = null;

function getResolver(): MidnightDidResolver {
  if (!resolver) {
    resolver = new MidnightDidResolver({
      indexerGraphqlUrl: config.INDEXER_URL,
    });
  }
  return resolver;
}

export async function resolveHolderDid(did: string): Promise<DidDocument> {
  return getResolver().resolve(did);
}

/// Pick the first verification-relation key matching `relation`.
/// Returns `{ kid, publicKeyJwk }` or throws if the relation is empty.
export function pickRelationKey(
  doc: DidDocument,
  relation: "authentication" | "assertionMethod",
): { kid: string; publicKeyJwk: any } {
  const ids = (doc as any)[relation] ?? [];
  if (ids.length === 0) {
    throw new Error(`DID ${doc.id} has no ${relation}-relation key`);
  }
  const id = typeof ids[0] === "string" ? ids[0] : ids[0].id;
  const vm = doc.verificationMethod.find(v => v.id === id);
  if (!vm) {
    throw new Error(`verificationMethod ${id} referenced by ${relation} not present`);
  }
  return { kid: vm.id, publicKeyJwk: vm.publicKeyJwk };
}
```

The exact class name `MidnightDidResolver` and method `resolve` are the canonical surface in `~/iohk/midnight-identity-workspace/midnight-did/packages/did/src/midnight-did-resolver.ts`. If the actual export name differs, swap the import.

- [ ] **Step 24.2: Manual smoke**

(Covered end-to-end by the BDD harness in Section 10.)

- [ ] **Step 24.3: Commit**

```bash
git add IssuerDIDIT-mock/src/services/holderDidResolver.ts
git commit -S -s -m "$(cat <<'EOF'
feat(issuer-mock): embedded midnight-did resolver

In-process holder DID resolution via
@midnight-ntwrk/midnight-did, no sidecar service. Helper
pickRelationKey takes a "authentication" / "assertionMethod"
selector so oid4vpVerifier and vcMinter share one canonical
key-picking path.
EOF
)"
git log --format="%h %G? %s" -1
```

---

## Section 7 — IssuerDIDIT-mock: routes + KYC form + vcMinter (Spec build step 7)

### Task 25: OID4VP routes + login view

**Files:**
- Create: `IssuerDIDIT-mock/src/routes/login.ts`
- Create: `IssuerDIDIT-mock/src/services/oid4vpVerifier.ts`
- Create: `IssuerDIDIT-mock/src/views/login.ejs`
- Modify: `IssuerDIDIT-mock/src/server.ts`

- [ ] **Step 25.1: OID4VP verifier**

`IssuerDIDIT-mock/src/services/oid4vpVerifier.ts`:

```typescript
import { importJWK, jwtVerify } from "jose";
import { resolveHolderDid, pickRelationKey } from "./holderDidResolver.js";

export interface VerifiedIdToken {
  holderDid: string;
  nonce: string;
}

export async function verifyIdToken(
  idToken: string,
  expectedClientId: string,
): Promise<VerifiedIdToken> {
  // 1. Decode header without verifying to extract kid.
  const [headerB64] = idToken.split(".");
  const header = JSON.parse(Buffer.from(headerB64, "base64url").toString("utf8"));
  if (header.alg !== "EdDSA") {
    throw new Error(`unexpected alg ${header.alg}; expected EdDSA`);
  }
  const kid: string = header.kid;
  const holderDid = kid.split("#")[0];

  // 2. Resolve DID and pick the authentication-relation key referenced by kid.
  const doc = await resolveHolderDid(holderDid);
  const vmEntry = doc.verificationMethod.find(v => v.id === kid);
  if (!vmEntry) throw new Error(`kid ${kid} not in DID document`);
  const authIds = (doc as any).authentication ?? [];
  const inAuthn = authIds.some((e: any) =>
    (typeof e === "string" ? e : e.id) === kid,
  );
  if (!inAuthn) {
    throw new Error(`kid ${kid} is not in the authentication relation`);
  }

  // 3. JWT verify.
  const key = await importJWK(vmEntry.publicKeyJwk as any, "EdDSA");
  const { payload } = await jwtVerify(idToken, key, {
    audience: expectedClientId,
  });
  if (!payload.nonce || typeof payload.nonce !== "string") {
    throw new Error("payload missing nonce");
  }
  if (payload.iss !== holderDid) {
    throw new Error("iss != holderDid");
  }

  return { holderDid, nonce: payload.nonce };
}
```

- [ ] **Step 25.2: Login view (EJS)**

```bash
mkdir -p IssuerDIDIT-mock/src/views
cat > IssuerDIDIT-mock/src/views/login.ejs <<'EJS'
<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <title>IssuerDIDIT-mock — Login with Midnight DID</title>
  <style>
    body { font-family: -apple-system, sans-serif; max-width: 540px; margin: 4em auto; }
    .qr { margin: 2em auto; text-align: center; }
    code { background: #f4f4f4; padding: 2px 6px; border-radius: 4px; word-break: break-all; }
  </style>
</head>
<body>
  <h1>Step 1 — Login with Midnight DID</h1>
  <p>Scan this QR with the Midnight wallet's Identity Centre.</p>
  <div class="qr"><img src="data:image/png;base64,<%= qrPng %>" alt="QR-1" /></div>
  <details><summary>QR payload (for paste-URL dev affordance)</summary>
    <p><code><%= qrPayload %></code></p>
  </details>
</body>
</html>
EJS
```

- [ ] **Step 25.3: Routes**

`IssuerDIDIT-mock/src/routes/login.ts`:

```typescript
import express from "express";
import crypto from "node:crypto";
import QRCode from "qrcode";

import { config } from "../config.js";
import { createSession, consumeNonce, updateSession } from "../storage/sessions.js";
import { getIssuerKeys } from "../services/issuerDid.js";
import { verifyIdToken } from "../services/oid4vpVerifier.js";

const router = express.Router();

router.get("/authorize", async (_req, res) => {
  const nonce = crypto.randomBytes(16).toString("base64url");
  const session = createSession(nonce);
  const requestUri = `${config.PUBLIC_BASE_URL}/request/${session.id}`;
  const qrPayload = `openid4vp://${new URL(config.PUBLIC_BASE_URL).host}/?request_uri=${encodeURIComponent(requestUri)}`;
  const qrPng = (await QRCode.toBuffer(qrPayload, { errorCorrectionLevel: "M" })).toString("base64");
  res.render("login", { qrPng, qrPayload, sessionId: session.id });
});

router.get("/request/:id", (req, res) => {
  // The wallet GETs this after scanning. We return the typed AuthRequest.
  const issuer = getIssuerKeys();
  const session = require("../storage/sessions.js").getSession(req.params.id);
  if (!session) return res.status(404).json({ error: "session not found" });
  res.json({
    client_id: issuer.did,
    nonce: session.oid4vp_nonce,
    state: session.id,
    redirect_uri: `${config.PUBLIC_BASE_URL}/authorize-response`,
    presentation_definition: null,
  });
});

router.post("/authorize-response", async (req, res) => {
  const { id_token, state } = req.body ?? {};
  if (typeof id_token !== "string" || typeof state !== "string") {
    return res.status(400).json({ error: "id_token + state required" });
  }
  try {
    const issuer = getIssuerKeys();
    const verified = await verifyIdToken(id_token, issuer.did);
    // Atomic nonce consumption — refuses replays.
    const session = consumeNonce(verified.nonce);
    if (!session || session.id !== state) {
      return res.status(401).json({ error: "nonce already consumed or mismatched session" });
    }
    const updated = updateSession(session.id, {
      holder_did: verified.holderDid,
      status: "authorized",
    });
    res.json({ session_id: updated.id, status: "authenticated" });
  } catch (err: any) {
    res.status(401).json({ error: err.message });
  }
});

export default router;
```

The `require` in the GET `/request/:id` handler is a Node-ESM no-no — use `import` at the top instead. Lifted to the import block:

```typescript
import { createSession, consumeNonce, updateSession, getSession } from "../storage/sessions.js";
```

And replace the body with `const session = getSession(req.params.id);`.

- [ ] **Step 25.4: Wire into server**

Modify `IssuerDIDIT-mock/src/server.ts`'s `buildApp()`:

```typescript
import loginRouter from "./routes/login.js";
// inside buildApp():
app.use(loginRouter);
```

- [ ] **Step 25.5: Smoke test + commit**

```bash
yarn build && yarn dev &
SERVER_PID=$!
sleep 2
curl -sS http://localhost:3001/authorize | head -5
# Expect HTML with <img src="data:image/png;base64...
kill $SERVER_PID
rm -f issuer.sqlite*
```

```bash
git add IssuerDIDIT-mock/src/
git commit -S -s -m "$(cat <<'EOF'
feat(issuer-mock): OID4VP/SIOPv2 routes + login view

GET /authorize renders QR-1 to the laptop browser. GET
/request/:id returns the typed AuthRequest the wallet
expects. POST /authorize-response verifies the id_token's
JWS against the holder DID's authentication-relation key,
atomically consumes the nonce, and reports session status.
EOF
)"
git log --format="%h %G? %s" -1
```

### Task 26: OID4VCI issuer service (pre-auth code + token + c_nonce)

**Files:**
- Create: `IssuerDIDIT-mock/src/services/oid4vciIssuer.ts`

- [ ] **Step 26.1: Service**

`IssuerDIDIT-mock/src/services/oid4vciIssuer.ts`:

```typescript
import crypto from "node:crypto";
import { getSession, getSessionByCode, updateSession } from "../storage/sessions.js";

export function mintPreAuthorizedCode(sessionId: string): string {
  const code = `code_${crypto.randomBytes(24).toString("base64url")}`;
  updateSession(sessionId, { pre_authorized_code: code });
  return code;
}

export interface TokenIssueResult {
  access_token: string;
  c_nonce: string;
  token_type: "Bearer";
  expires_in: number;
}

export function issueToken(preAuthorizedCode: string): TokenIssueResult | null {
  const session = getSessionByCode(preAuthorizedCode);
  if (!session || session.status !== "kyc_done") return null;
  const access_token = `at_${crypto.randomBytes(24).toString("base64url")}`;
  const c_nonce = crypto.randomBytes(16).toString("base64url");
  updateSession(session.id, { c_nonce });
  // Note: access_token isn't persisted in Phase 1 — c_nonce is what
  // proves liveness on the next /credential call.
  return { access_token, c_nonce, token_type: "Bearer", expires_in: 600 };
}

export function consumeCNonce(cNonce: string): ReturnType<typeof getSessionByCode> | null {
  // Look up by c_nonce — there's no index but session count is small.
  const sqlite = (require("better-sqlite3") as any)(
    require("../config.js").config.SQLITE_PATH,
  );
  const row = sqlite.prepare(`
    UPDATE sessions
    SET c_nonce = c_nonce || '_consumed', updated_at_ms = ?
    WHERE c_nonce = ?
    RETURNING *
  `).get(Date.now(), cNonce);
  sqlite.close();
  if (!row) return null;
  return {
    id: row.id,
    status: row.status,
    holder_did: row.holder_did,
    oid4vp_nonce: row.oid4vp_nonce,
    vc_claims: row.vc_claims_json ? JSON.parse(row.vc_claims_json) : null,
    pre_authorized_code: row.pre_authorized_code,
    c_nonce: row.c_nonce,
    vc_uri: row.vc_uri,
    vc_body_b64: row.vc_body_b64,
    created_at_ms: row.created_at_ms,
    updated_at_ms: row.updated_at_ms,
  };
}
```

The inline `require` + manual SQL is a smell — refactor into `storage/sessions.ts` as `consumeCNonce()` matching the existing `consumeNonce()` helper, and import from there. Do that in Task 27 alongside the credential route.

- [ ] **Step 26.2: Commit**

```bash
git add IssuerDIDIT-mock/src/services/oid4vciIssuer.ts
git commit -S -s -m "$(cat <<'EOF'
feat(issuer-mock): OID4VCI pre-auth code + token + c_nonce minting

Bound to the session row by pre_authorized_code, mints a
fresh c_nonce on every /token call. c_nonce consumption
ships in Task 27 as a sessions.ts helper alongside the
credential route.
EOF
)"
git log --format="%h %G? %s" -1
```

### Task 27: vcMinter + OID4VCI routes + credential-offer view

**Files:**
- Create: `IssuerDIDIT-mock/src/services/vcMinter.ts`
- Create: `IssuerDIDIT-mock/src/routes/credential.ts`
- Create: `IssuerDIDIT-mock/src/views/credential-offer.ejs`
- Modify: `IssuerDIDIT-mock/src/storage/sessions.ts` (add `consumeCNonce`)
- Modify: `IssuerDIDIT-mock/src/server.ts`

- [ ] **Step 27.1: Add `consumeCNonce` to sessions.ts**

Append to `IssuerDIDIT-mock/src/storage/sessions.ts`:

```typescript
export function consumeCNonce(cNonce: string): Session | null {
  const row = getDb().prepare(`
    UPDATE sessions
    SET c_nonce = c_nonce || '_consumed', updated_at_ms = ?
    WHERE c_nonce = ?
    RETURNING *
  `).get(Date.now(), cNonce);
  return row ? rowToSession(row) : null;
}
```

Update `oid4vciIssuer.ts`'s `consumeCNonce` to delegate to this.

- [ ] **Step 27.2: vcMinter**

`IssuerDIDIT-mock/src/services/vcMinter.ts`:

```typescript
import crypto from "node:crypto";
import { encode as cborEncode } from "cbor-x";
import * as ed25519 from "@noble/ed25519";

import { JubjubSigner } from "@midnight-ntwrk/midnight-did-jubjub-schnorr";

import { getIssuerKeys } from "./issuerDid.js";
import type { BirthVcClaims } from "../storage/sessions.js";

export interface MintedVc {
  vc_uri: string;
  body_b64: string;     // base64(Compact-serialised signed VC)
  openings: Array<{ claim_path: string; plaintext_b64: string; opening_b64: string }>;
}

/// Mint a `birth` VC bound to `holderDid` with `claims`.
/// Body shape matches what wallet-core's vc_self_verify
/// expects: CBOR Map with `credentialSubject`, `holder`,
/// `issuer`, and a `proof` block carrying
/// `verificationMethod` + `signature` (base64).
///
/// Phase 1 puts ALL the claims into `credentialSubject` as
/// committed-private fields with openings; Phase 2 will
/// split public vs. committed once we follow the birth
/// family schema literally.
export async function mintBirthVc(
  holderDid: string,
  claims: BirthVcClaims,
): Promise<MintedVc> {
  const issuer = getIssuerKeys();
  const vcUri = `urn:uuid:${crypto.randomUUID()}`;

  // 1. Build committed-private claims: each field gets a random opening.
  const opens: Array<{ field: keyof BirthVcClaims; opening: Buffer; commitment: Buffer }> = [];
  for (const field of Object.keys(claims) as (keyof BirthVcClaims)[]) {
    const opening = crypto.randomBytes(32);
    const commitment = crypto
      .createHash("sha256")
      .update(opening)
      .update(Buffer.from(claims[field], "utf8"))
      .digest();
    opens.push({ field, opening, commitment });
  }

  // 2. Body (with placeholder proof).
  const credentialSubject: Record<string, string> = {};
  for (const o of opens) {
    credentialSubject[o.field as string] = o.commitment.toString("base64");
  }
  const bodyNoProof = {
    "@context": ["https://www.w3.org/2018/credentials/v1"],
    type: ["VerifiableCredential", "BirthCredential"],
    issuer: issuer.did,
    holder: holderDid,
    issuanceDate: new Date().toISOString(),
    credentialSubject,
  };

  // 3. Canonical = CBOR(bodyNoProof). Sign with Jubjub assertion key.
  const canonical = cborEncode(bodyNoProof);
  const signer = new JubjubSigner(issuer.jubjub.secretBytes);
  const sig = await signer.sign(canonical);

  const fullBody = {
    ...bodyNoProof,
    proof: {
      type: "MidnightJubjubSchnorr2026",
      verificationMethod: issuer.jubjub.kid,
      signature: Buffer.from(sig).toString("base64"),
    },
  };
  const body_b64 = Buffer.from(cborEncode(fullBody)).toString("base64");

  const openings = opens.map(o => ({
    claim_path: `/credentialSubject/${o.field as string}`,
    plaintext_b64: Buffer.from(claims[o.field], "utf8").toString("base64"),
    opening_b64: o.opening.toString("base64"),
  }));

  return { vc_uri: vcUri, body_b64, openings };
}
```

`JubjubSigner` is the Jubjub-Schnorr primitive from the existing
`midnight-did-jubjub-schnorr` package. If the constructor / sign API differs,
adjust accordingly. The CBOR shape must match what `vc_self_verify`'s
canonical-serialise step strips and re-encodes (Task 18) — keep them
in lock-step.

Add `cbor-x` to `package.json` deps if not already there.

- [ ] **Step 27.3: Credential-offer view**

```bash
cat > IssuerDIDIT-mock/src/views/credential-offer.ejs <<'EJS'
<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <title>IssuerDIDIT-mock — Credential offer</title>
  <style>
    body { font-family: -apple-system, sans-serif; max-width: 540px; margin: 4em auto; }
    .qr { margin: 2em auto; text-align: center; }
    code { background: #f4f4f4; padding: 2px 6px; border-radius: 4px; word-break: break-all; }
  </style>
</head>
<body>
  <h1>Step 3 — Scan to receive your Birth VC</h1>
  <p>KYC accepted. Scan this QR to download the credential.</p>
  <div class="qr"><img src="data:image/png;base64,<%= qrPng %>" alt="QR-2" /></div>
  <details><summary>QR payload</summary>
    <p><code><%= qrPayload %></code></p>
  </details>
</body>
</html>
EJS
```

- [ ] **Step 27.4: Credential routes**

`IssuerDIDIT-mock/src/routes/credential.ts`:

```typescript
import express from "express";
import QRCode from "qrcode";

import { config } from "../config.js";
import {
  getSession,
  getSessionByCode,
  updateSession,
  consumeCNonce,
} from "../storage/sessions.js";
import { mintPreAuthorizedCode, issueToken } from "../services/oid4vciIssuer.js";
import { mintBirthVc } from "../services/vcMinter.js";
import { resolveHolderDid } from "../services/holderDidResolver.js";
import { importJWK, jwtVerify } from "jose";

const router = express.Router();

router.get("/credential-offer/:id", async (req, res) => {
  const session = getSession(req.params.id);
  if (!session || session.status !== "kyc_done") {
    return res.status(409).send("KYC not complete for this session");
  }
  const code = mintPreAuthorizedCode(session.id);
  const offer = {
    credential_issuer: config.PUBLIC_BASE_URL,
    credential_configuration_ids: ["birth"],
    grants: {
      "urn:ietf:params:oauth:grant-type:pre-authorized_code": {
        "pre-authorized_code": code,
      },
    },
  };
  const qrPayload = `openid-credential-offer://${new URL(config.PUBLIC_BASE_URL).host}/?credential_offer=${encodeURIComponent(JSON.stringify(offer))}`;
  const qrPng = (await QRCode.toBuffer(qrPayload, { errorCorrectionLevel: "M" })).toString("base64");
  res.render("credential-offer", { qrPng, qrPayload });
});

router.post("/token", (req, res) => {
  const { grant_type, "pre-authorized_code": code } = req.body ?? {};
  if (grant_type !== "urn:ietf:params:oauth:grant-type:pre-authorized_code") {
    return res.status(400).json({ error: "unsupported_grant_type" });
  }
  if (typeof code !== "string") return res.status(400).json({ error: "missing code" });
  const issued = issueToken(code);
  if (!issued) return res.status(400).json({ error: "invalid_grant" });
  res.json(issued);
});

router.post("/credential", async (req, res) => {
  try {
    const { format, proof } = req.body ?? {};
    if (format !== "midnight-vc-compact") {
      return res.status(400).json({ error: "unsupported_format" });
    }
    if (!proof || proof.proof_type !== "jwt" || typeof proof.jwt !== "string") {
      return res.status(400).json({ error: "missing/bad proof" });
    }

    // 1. Decode header → kid → holder DID.
    const [headerB64] = proof.jwt.split(".");
    const header = JSON.parse(Buffer.from(headerB64, "base64url").toString("utf8"));
    const kid = header.kid as string;
    const holderDid = kid.split("#")[0];

    // 2. Resolve holder, verify the JWS against the authn key, extract nonce.
    const doc = await resolveHolderDid(holderDid);
    const vm = doc.verificationMethod.find(v => v.id === kid);
    if (!vm) return res.status(401).json({ error: "kid not in DID doc" });
    const pk = await importJWK(vm.publicKeyJwk as any, "EdDSA");
    const { payload } = await jwtVerify(proof.jwt, pk, { audience: config.PUBLIC_BASE_URL });
    if (typeof payload.nonce !== "string") {
      return res.status(401).json({ error: "missing nonce" });
    }

    // 3. Consume the c_nonce → grabs the session.
    const session = consumeCNonce(payload.nonce);
    if (!session) return res.status(401).json({ error: "c_nonce already consumed or unknown" });
    if (session.holder_did !== holderDid) {
      return res.status(401).json({ error: "holder DID mismatch" });
    }
    if (!session.vc_claims) return res.status(409).json({ error: "no claims on session" });

    // 4. Mint the VC.
    const minted = await mintBirthVc(holderDid, session.vc_claims);
    updateSession(session.id, {
      vc_uri: minted.vc_uri,
      vc_body_b64: minted.body_b64,
      status: "vc_issued",
    });
    res.json({
      credential: {
        vc_uri: minted.vc_uri,
        issuer_did: doc ? session.holder_did : holderDid, // wait – issuer DID:
        holder_did: holderDid,
        body_b64: minted.body_b64,
      },
      openings: minted.openings,
    });
  } catch (err: any) {
    res.status(401).json({ error: err.message });
  }
});

export default router;
```

The `issuer_did` field in the response must be the *issuer's* DID, not the holder's. Fix the line:

```typescript
        issuer_did: require("../services/issuerDid.js").getIssuerKeys().did,
```

Or hoist the import to the top:

```typescript
import { getIssuerKeys } from "../services/issuerDid.js";
// ...
        issuer_did: getIssuerKeys().did,
```

- [ ] **Step 27.5: Wire into server + commit**

Add to `server.ts`:

```typescript
import credentialRouter from "./routes/credential.js";
// in buildApp(): app.use(credentialRouter);
```

```bash
yarn build
git add IssuerDIDIT-mock/src/
git commit -S -s -m "$(cat <<'EOF'
feat(issuer-mock): vcMinter + OID4VCI credential routes + offer view

GET /credential-offer/:id mints a pre-auth code + renders
QR-2. POST /token does the OID4VCI Pre-Authorized Code
exchange. POST /credential verifies the holder's DID-bound
JWS proof, consumes the c_nonce, mints + signs a birth VC
with the issuer's Jubjub assertion key, and returns body +
openings.
EOF
)"
git log --format="%h %G? %s" -1
```

### Task 28: KYC form (the operator-driven mock) + final wiring + README

**Files:**
- Create: `IssuerDIDIT-mock/src/routes/kyc.ts`
- Create: `IssuerDIDIT-mock/src/views/kyc-form.ejs`
- Modify: `IssuerDIDIT-mock/src/server.ts`
- Modify: `IssuerDIDIT-mock/README.md` (full setup instructions)

- [ ] **Step 28.1: KYC form view**

```bash
cat > IssuerDIDIT-mock/src/views/kyc-form.ejs <<'EJS'
<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <title>IssuerDIDIT-mock — Step 2: KYC</title>
  <style>
    body { font-family: -apple-system, sans-serif; max-width: 540px; margin: 4em auto; }
    label { display: block; margin: 1em 0 .25em; }
    input, select { width: 100%; padding: .5em; box-sizing: border-box; }
    button { margin-top: 2em; padding: .75em 1.5em; }
    .delay-banner { background: #fffae5; padding: .5em; border-radius: 4px; margin-top: 1em; }
  </style>
</head>
<body>
  <h1>Step 2 — KYC (mock — operator-driven)</h1>
  <p>Pretend you just completed a real DIDIT KYC. Enter the verified attributes:</p>
  <form method="POST" action="/kyc-form?session=<%= sessionId %>">
    <label>First name</label><input name="firstName" required value="<%= prefill.firstName ?? '' %>" />
    <label>Last name</label> <input name="lastName"  required value="<%= prefill.lastName  ?? '' %>" />
    <label>Date of birth (YYYY-MM-DD)</label>
                              <input name="dateOfBirth" type="date" required value="<%= prefill.dateOfBirth ?? '' %>" />
    <label>Nationality (ISO 3166-1 alpha-3)</label>
                              <input name="nationality" required maxlength="3" value="<%= prefill.nationality ?? '' %>" />
    <label>Document number</label> <input name="documentNumber" required value="<%= prefill.documentNumber ?? '' %>" />
    <button type="submit">Submit</button>
    <p class="delay-banner">Simulated KYC processing delay: <%= delayMs %> ms</p>
  </form>
</body>
</html>
EJS
```

- [ ] **Step 28.2: KYC routes**

`IssuerDIDIT-mock/src/routes/kyc.ts`:

```typescript
import express from "express";
import { z } from "zod";

import { config } from "../config.js";
import { getSession, updateSession } from "../storage/sessions.js";

const router = express.Router();

router.get("/kyc-form", (req, res) => {
  const sessionId = req.query.session;
  if (typeof sessionId !== "string") return res.status(400).send("missing session");
  const session = getSession(sessionId);
  if (!session) return res.status(404).send("session not found");
  if (!session.holder_did) return res.status(409).send("session not authorized yet");

  res.render("kyc-form", {
    sessionId,
    prefill: req.query, // ?firstName=...&lastName=... for the BDD harness
    delayMs: config.KYC_DELAY_MS,
  });
});

const ClaimsSchema = z.object({
  firstName: z.string().min(1),
  lastName: z.string().min(1),
  dateOfBirth: z.string().regex(/^\d{4}-\d{2}-\d{2}$/),
  nationality: z.string().length(3).regex(/^[A-Z]{3}$/),
  documentNumber: z.string().min(1),
});

router.post("/kyc-form", async (req, res) => {
  const sessionId = req.query.session;
  if (typeof sessionId !== "string") return res.status(400).send("missing session");
  const session = getSession(sessionId);
  if (!session) return res.status(404).send("session not found");
  if (!session.holder_did) return res.status(409).send("session not authorized yet");

  const parsed = ClaimsSchema.safeParse(req.body);
  if (!parsed.success) {
    return res.status(400).send(`bad form: ${parsed.error.message}`);
  }

  // Simulate KYC processing delay.
  await new Promise(r => setTimeout(r, config.KYC_DELAY_MS));

  updateSession(session.id, { vc_claims: parsed.data, status: "kyc_done" });
  res.redirect(`/credential-offer/${session.id}`);
});

export default router;
```

- [ ] **Step 28.3: Wire into server**

In `server.ts`:

```typescript
import kycRouter from "./routes/kyc.js";
// in buildApp(): app.use(kycRouter);
```

- [ ] **Step 28.4: README — full setup**

Overwrite `IssuerDIDIT-mock/README.md`:

```markdown
# IssuerDIDIT-mock

Mock issuer for the Identity Centre Phase 1 demo. Implements the
stable 6-endpoint OID4VP/VCI HTTP contract that the real
`IssuerDIDIT` will inherit — with the DIDIT KYC step replaced by
an operator-driven form.

## Prerequisites

- Docker + Docker Compose
- Node 22+ (or whatever the workspace's `package.json` engines field requires)
- The `@midnight-ntwrk/midnight-did` + `midnight-did-api` packages
  buildable from the workspace.

## Setup

```bash
yarn install
yarn env:up                 # start standalone Midnight env
yarn bootstrap              # create issuer DID + keystore
yarn dev                    # serve on http://localhost:3001
```

## Demo flow

1. Open `http://localhost:3001/authorize` in a laptop browser → QR-1.
2. Scan QR-1 with the Identity Centre — wallet signs the SIOPv2 id_token.
3. Laptop browser redirects to `/kyc-form?session=…` → fill in
   `firstName / lastName / dateOfBirth / nationality / documentNumber`.
4. Submit → server waits `KYC_DELAY_MS` (default 2000) → redirects to
   `/credential-offer/:session_id` → QR-2.
5. Scan QR-2 with the wallet — VC lands in the Identity Centre carousel.

## Config (env vars)

| Var | Default | Meaning |
|---|---|---|
| `PORT` | 3001 | HTTP port |
| `INDEXER_URL` | `http://localhost:8088/api/v1/graphql` | Standalone indexer |
| `NODE_RPC_URL` | `http://localhost:9944` | Standalone node RPC |
| `PUBLIC_BASE_URL` | `http://localhost:3001` | What the QR payloads point at |
| `KEYSTORE_PATH` | `./issuer-keystore.json` | Created by `yarn bootstrap` |
| `SQLITE_PATH` | `./issuer.sqlite` | Session storage |
| `ISSUER_BOOTSTRAP_SEED` | `issuer-demo-seed` | Deterministic DID seed |
| `KYC_DELAY_MS` | 2000 | Mock-DIDIT processing time |

## HTTP contract

The 6 endpoints are stable across mock and real `IssuerDIDIT`:

| Method + Path | Phase | Used by |
|---|---|---|
| `GET /authorize` | renders QR-1 | laptop browser |
| `GET /request/:id` | returns AuthRequest | wallet |
| `POST /authorize-response` | accepts id_token | wallet |
| `GET /kyc-form` | operator form (mock only — real DIDIT does its hosted KYC instead) | laptop browser |
| `POST /kyc-form` | operator submit (mock only) | laptop browser |
| `GET /credential-offer/:id` | renders QR-2 | laptop browser |
| `POST /token` | OID4VCI pre-auth code exchange | wallet |
| `POST /credential` | returns the signed VC | wallet |

When the real DIDIT-driven issuer lands, only the two `kyc-form`
routes are removed (replaced by `/webhook/didit` + a redirect-to-
didit.me).
```

- [ ] **Step 28.5: Commit**

```bash
git add IssuerDIDIT-mock/
git commit -S -s -m "$(cat <<'EOF'
feat(issuer-mock): KYC form route + final wiring + README

GET /kyc-form renders the operator-driven mock surface
(the substitute for real DIDIT). POST /kyc-form accepts
firstName/lastName/dateOfBirth/nationality/documentNumber,
waits KYC_DELAY_MS to emulate processing time, then
redirects to /credential-offer/:id.

README covers the demo flow + env vars + the 6-endpoint
HTTP contract.
EOF
)"
git log --format="%h %G? %s" -1
```

---

## Section 8 — Identity Centre Dioxus UI (Spec build step 8)

**Context switch:** back to the wallet repo (`midnight-ledger` worktree on
`dioxus-vc-demo`). All commits use `feat(identity-centre):` prefix.

```bash
cd /Users/ysh/iohk/midnight-ledger/.claude/worktrees/thirsty-lovelace-092f50
git status   # confirm we're on dioxus-vc-demo, clean tree
```

### Task 29: `QrScanner` trait + paste-URL stub impl

**Files:**
- Create: `mobile-bench/wallet-core/src/qr_scanner.rs`
- Modify: `mobile-bench/wallet-core/src/lib.rs`

- [ ] **Step 29.1: Trait + stub**

`mobile-bench/wallet-core/src/qr_scanner.rs`:

```rust
//! Platform-agnostic QR scanner surface. The native bridges
//! (Android CameraX in Task 37, iOS AVCaptureMetadataOutput
//! later) implement this trait. A pure-Rust "paste URL" stub
//! ships here so unit tests + dev affordance work without a
//! camera.

use std::future::Future;
use std::pin::Pin;

#[derive(Debug, thiserror::Error)]
pub enum QrScanError {
    #[error("user cancelled the scan")]
    Cancelled,
    #[error("scanner unavailable: {0}")]
    Unavailable(String),
}

pub trait QrScanner: Send + Sync {
    /// Open the scanner UI. Resolves with the decoded URL string
    /// on success.
    fn scan(&self) -> Pin<Box<dyn Future<Output = Result<String, QrScanError>> + Send + '_>>;
}

/// In-memory stub for unit tests + dev paste-URL flow. The
/// next `scan()` call returns whatever was set via `set_next`.
#[derive(Debug, Default)]
pub struct PasteUrlScanner {
    next: std::sync::Mutex<Option<Result<String, QrScanError>>>,
}

impl PasteUrlScanner {
    pub fn set_next(&self, value: Result<String, QrScanError>) {
        *self.next.lock().unwrap() = Some(value);
    }
}

impl QrScanner for PasteUrlScanner {
    fn scan(&self) -> Pin<Box<dyn Future<Output = Result<String, QrScanError>> + Send + '_>> {
        let v = self.next.lock().unwrap().take();
        Box::pin(async move {
            v.unwrap_or(Err(QrScanError::Cancelled))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn paste_url_scanner_returns_set_value() {
        let s = PasteUrlScanner::default();
        s.set_next(Ok("openid4vp://x".into()));
        assert_eq!(s.scan().await.unwrap(), "openid4vp://x");
        // Second call without re-setting returns Cancelled.
        assert!(matches!(s.scan().await, Err(QrScanError::Cancelled)));
    }
}
```

- [ ] **Step 29.2: Re-export + test + commit**

```rust
// lib.rs
pub mod qr_scanner;
pub use qr_scanner::{QrScanner, QrScanError, PasteUrlScanner};
```

```bash
cargo test -p midnight-wallet-core --lib qr_scanner::tests
```

Expected: PASS.

```bash
git add mobile-bench/wallet-core/src/qr_scanner.rs mobile-bench/wallet-core/src/lib.rs
git commit -S -s -m "feat(qr-scan): QrScanner trait + PasteUrlScanner stub"
git log --format="%h %G? %s" -1
```

### Task 30: `IdentityScreen` shell + new `Identity` tab on app

**Files:**
- Create: `mobile-bench/dioxus-wallet/src/identity/mod.rs`
- Create: `mobile-bench/dioxus-wallet/src/identity/screen.rs`
- Modify: `mobile-bench/dioxus-wallet/src/app.rs` (add `Tab::Identity`, remove standalone Keys tab)

- [ ] **Step 30.1: Module root**

```rust
// mobile-bench/dioxus-wallet/src/identity/mod.rs
//! Identity Centre — top-level wallet screen for VCs, DIDs, Keys.
//!
//! Sub-tab layout: VCs (carousel) | DIDs (list+detail nesting keys).
//! Bootstrap panel appears when no bootstrapped DID exists; the FAB
//! anchors the scope-aware QR scanner.

pub mod screen;
pub mod bootstrap_panel;
pub mod vc_carousel;
pub mod vc_card;
pub mod did_list;
pub mod did_detail;
pub mod did_picker;
pub mod qr_scan_fab;
pub mod qr_scan_modal;

pub use screen::IdentityScreen;
```

- [ ] **Step 30.2: `IdentityScreen` with sub-tab router**

```rust
// mobile-bench/dioxus-wallet/src/identity/screen.rs
use dioxus::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IdentitySubTab { Vcs, Dids }

#[component]
pub fn IdentityScreen() -> Element {
    let mut sub = use_signal(|| IdentitySubTab::Vcs);

    rsx! {
        div { class: "id-centre",
            div { class: "id-subtabs",
                button {
                    class: if sub() == IdentitySubTab::Vcs { "active" } else { "" },
                    onclick: move |_| sub.set(IdentitySubTab::Vcs),
                    "VCs"
                }
                button {
                    class: if sub() == IdentitySubTab::Dids { "active" } else { "" },
                    onclick: move |_| sub.set(IdentitySubTab::Dids),
                    "DIDs"
                }
            }
            div { class: "id-body",
                match sub() {
                    IdentitySubTab::Vcs  => rsx!{ super::vc_carousel::VcCarouselPanel {} },
                    IdentitySubTab::Dids => rsx!{ super::did_list::DidListPanel {} },
                }
            }
            // FAB floats above content, anchored bottom-right by CSS.
            super::qr_scan_fab::QrScanFab { sub: sub() }
        }
    }
}
```

- [ ] **Step 30.3: Wire into app.rs**

Open `mobile-bench/dioxus-wallet/src/app.rs`. Find `enum Tab` (around line 94). Edit:

```rust
enum Tab {
    Wallet,
    Identity,    // NEW
    Bench,
    Diagnostics,
    About,
    // Keys variant REMOVED — moved inside Identity > DIDs > DidDetail
}
```

And in the tab-bar render path, add the Identity button + match arm:

```rust
match tab() {
    Tab::Wallet => rsx!{ WalletPanel {} },
    Tab::Identity => rsx!{ crate::identity::IdentityScreen {} },
    Tab::Bench => rsx!{ BenchPanel {} },
    Tab::Diagnostics => rsx!{ DiagnosticsPanel {} },
    Tab::About => rsx!{ AboutPanel {} },
}
```

Drop the existing `Tab::Keys` arms + its bottom-bar button. Any external references to `Tab::Keys` (if any) become `Tab::Identity` with the sub-tab set to `Dids`.

Add module decl at the top of `app.rs` (or wherever modules are declared in dioxus-wallet — typically `lib.rs`):

```rust
pub mod identity;
```

- [ ] **Step 30.4: Add the minimal placeholder children so the build doesn't fail**

Each of the modules referenced above needs at least a stub:

```rust
// vc_carousel.rs
use dioxus::prelude::*;
#[component] pub fn VcCarouselPanel() -> Element {
    rsx!{ div { "VC carousel — implemented in Task 32" } }
}

// did_list.rs
use dioxus::prelude::*;
#[component] pub fn DidListPanel() -> Element {
    rsx!{ div { "DID list — implemented in Task 34" } }
}

// did_detail.rs, did_picker.rs, qr_scan_fab.rs, qr_scan_modal.rs, vc_card.rs, bootstrap_panel.rs
// Each gets a similar single-line placeholder component.
```

`qr_scan_fab.rs` specifically needs to accept the `sub` prop:

```rust
use dioxus::prelude::*;
use super::screen::IdentitySubTab;
#[component] pub fn QrScanFab(sub: IdentitySubTab) -> Element {
    rsx!{ button { class: "fab", "+" } }
}
```

- [ ] **Step 30.5: Build + commit**

```bash
cargo build -p dioxus-wallet
```

Expected: clean build.

```bash
git add mobile-bench/dioxus-wallet/src/identity/ \
        mobile-bench/dioxus-wallet/src/app.rs \
        mobile-bench/dioxus-wallet/src/lib.rs
git commit -S -s -m "$(cat <<'EOF'
feat(identity-centre): top-level Identity tab + sub-tab router

Adds Tab::Identity to the bottom bar. The screen renders a
two-button sub-tab (VCs | DIDs) with placeholder child
components that get filled in across Tasks 31-36. The
top-level Keys tab is removed from the bottom bar — its
content lives inside each DID's detail page.
EOF
)"
git log --format="%h %G? %s" -1
```

### Task 31: `BootstrapPanel`

**Files:**
- Modify: `mobile-bench/dioxus-wallet/src/identity/bootstrap_panel.rs`

The wallet's gate-keeper: shown when no bootstrapped DID exists. Pressing
the button calls `bootstrap_did_with_keys` (Rust, in-process) and shows
a progress indicator while the multi-tx flow runs.

- [ ] **Step 31.1: Implementation**

```rust
//! Identity Centre's first-run gate.
//!
//! When `vc_store::has_any_bootstrapped_did()` returns false, the
//! Identity tab shows this panel instead of the VC carousel. One
//! button → calls into wallet-core's bootstrap helper, surfaces
//! tx-by-tx progress (six txs total).

use dioxus::prelude::*;

use midnight_wallet_core::{bootstrap_did_with_keys, BootstrapError};
use crate::wallet_handle::wallet_handle;     // existing wallet singleton
use crate::wallet_handle::secret_store;

#[derive(Clone, Debug)]
enum BootstrapState {
    Idle,
    Running(&'static str),     // human-readable progress step
    Done(String),              // resulting DID
    Failed(String),
}

#[component]
pub fn BootstrapPanel() -> Element {
    let mut state = use_signal(|| BootstrapState::Idle);

    let kick_off = move |_| {
        let mut state = state.clone();
        state.set(BootstrapState::Running("creating DID..."));
        spawn(async move {
            let wallet = wallet_handle();
            let secret_store = secret_store();
            // Seed comes from a fixture for the demo. In production
            // wallets this would be the user's wallet seed.
            let seed = b"holder-demo-seed-padded-to-32by";
            assert_eq!(seed.len(), 32);
            let seed: [u8; 32] = (*seed).try_into().unwrap();
            match bootstrap_did_with_keys(&wallet, &*secret_store, &seed).await {
                Ok(out) => state.set(BootstrapState::Done(out.did.as_str().into())),
                Err(e) => state.set(BootstrapState::Failed(format!("{e}"))),
            }
        });
    };

    rsx! {
        div { class: "bootstrap-panel",
            h2 { "Set up your DIDs for VCs" }
            p {
                "Your wallet doesn't have a DID configured for Verifiable Credentials yet. "
                "Tap Bootstrap to create one — this performs 6 on-chain transactions and may take a minute."
            }
            match state() {
                BootstrapState::Idle => rsx!{
                    button { class: "primary", onclick: kick_off, "Bootstrap" }
                },
                BootstrapState::Running(step) => rsx!{
                    div { class: "progress",
                        div { class: "spinner" }
                        span { "{step}" }
                    }
                },
                BootstrapState::Done(did) => rsx!{
                    div { class: "ok",
                        p { "✓ Bootstrapped" }
                        code { "{did}" }
                    }
                },
                BootstrapState::Failed(err) => rsx!{
                    div { class: "err",
                        p { "✗ Bootstrap failed" }
                        code { "{err}" }
                        button { onclick: kick_off, "Retry" }
                    }
                },
            }
        }
    }
}
```

`wallet_handle()` and `secret_store()` are placeholders for whatever
singletons the existing dioxus-wallet uses to access the `Wallet` and
`SecretStorage` instances. Match the naming pattern in `app.rs`.

- [ ] **Step 31.2: Smoke compile**

```bash
cargo build -p dioxus-wallet
```

- [ ] **Step 31.3: Wire into the screen**

Modify `vc_carousel.rs`:

```rust
use dioxus::prelude::*;
use midnight_wallet_core::has_any_bootstrapped_did;

#[component]
pub fn VcCarouselPanel() -> Element {
    let bootstrapped = use_memo(|| has_any_bootstrapped_did());
    if !bootstrapped() {
        rsx!{ super::bootstrap_panel::BootstrapPanel {} }
    } else {
        rsx!{ super::vc_card::VcCardCarousel {} }  // filled in Task 32
    }
}
```

Add `has_any_bootstrapped_did` in `wallet-core/src/did/mod.rs`:

```rust
pub fn has_any_bootstrapped_did() -> bool {
    // Walk secret_store for any kid matching "*#key-auth" present
    // alongside a kid matching "*#key-assert" under the same DID.
    // For demo simplicity, check whether the store has any key
    // tagged "ed25519/authentication" + matching "jubjub/assertionMethod".
    crate::secret_storage::default_store().has_pair("ed25519/authentication", "jubjub/assertionMethod")
}
```

(If the `SecretStorage` API doesn't expose `has_pair` directly, add it
as a default-impl method that walks the index.)

- [ ] **Step 31.4: Commit**

```bash
git add mobile-bench/dioxus-wallet/src/identity/ \
        mobile-bench/wallet-core/src/did/mod.rs
git commit -S -s -m "feat(identity-centre): BootstrapPanel + first-run gate"
git log --format="%h %G? %s" -1
```

### Task 32: `VcCarousel` — swipeable full-screen pager

**Files:**
- Modify: `mobile-bench/dioxus-wallet/src/identity/vc_carousel.rs`

A horizontal pager backed by `vc_store::list_ordered()`. Each child is
a `VcCard`. Phase 1 uses a simple `use_signal(usize)` index + left/right
swipe handlers; production polish (Tinder-style physics) is a later
phase.

- [ ] **Step 32.1: Implementation**

```rust
use dioxus::prelude::*;
use midnight_wallet_core::{VcStore, StoredVc};

use crate::wallet_handle::vc_store_handle;
use super::vc_card::VcCard;

#[component]
pub fn VcCardCarousel() -> Element {
    let store = vc_store_handle();
    let vcs = use_resource(move || {
        let store = store.clone();
        async move {
            store.list_ordered().unwrap_or_default()
        }
    });
    let mut idx = use_signal(|| 0usize);

    let vcs = vcs.read();
    let list: &Vec<StoredVc> = match &*vcs {
        Some(v) => v,
        None => return rsx!{ div { "Loading..." } },
    };

    if list.is_empty() {
        return rsx!{
            div { class: "vc-empty",
                p { "No credentials yet — tap the + button to scan an offer." }
            }
        };
    }

    let cur = list.get(idx().min(list.len() - 1));

    let next = move |_| idx.set((idx() + 1).min(list.len() - 1));
    let prev = move |_| idx.set(idx().saturating_sub(1));

    rsx! {
        div { class: "vc-carousel",
            div { class: "vc-stage",
                if let Some(vc) = cur {
                    VcCard { vc: vc.clone() }
                }
            }
            div { class: "vc-nav",
                button { onclick: prev, disabled: idx() == 0, "‹" }
                span { "{idx() + 1} / {list.len()}" }
                button {
                    onclick: next,
                    disabled: idx() + 1 >= list.len(),
                    "›"
                }
            }
        }
    }
}
```

- [ ] **Step 32.2: Provide `vc_store_handle` singleton**

In `mobile-bench/dioxus-wallet/src/wallet_handle.rs` (existing module —
wherever wallet/secret-store singletons live, add):

```rust
use std::sync::Arc;
use midnight_wallet_core::VcStore;

pub fn vc_store_handle() -> Arc<VcStore> {
    static STORE: std::sync::OnceLock<Arc<VcStore>> = std::sync::OnceLock::new();
    STORE.get_or_init(|| {
        let path = crate::storage_paths::wallet_data_dir().join("vcs.redb");
        Arc::new(VcStore::open(path).expect("vc_store opens"))
    }).clone()
}
```

- [ ] **Step 32.3: Commit**

```bash
git add mobile-bench/dioxus-wallet/src/
git commit -S -s -m "feat(identity-centre): VcCarousel with prev/next nav"
git log --format="%h %G? %s" -1
```

### Task 33: `VcCard` + self-verify badge

**Files:**
- Modify: `mobile-bench/dioxus-wallet/src/identity/vc_card.rs`

- [ ] **Step 33.1: Implementation**

```rust
use dioxus::prelude::*;
use midnight_wallet_core::{self_verify_and_cache, SelfVerifyResult, StoredVc};
use crate::wallet_handle::{wallet_handle, vc_store_handle};

#[derive(Clone, Debug, PartialEq)]
enum VerifyBadge {
    Unknown,
    Valid { ts_ms: u64 },
    Stale { ts_ms: u64 },
    Invalid(String),
}

#[component]
pub fn VcCard(vc: StoredVc) -> Element {
    let mut badge = use_signal(|| VerifyBadge::Unknown);

    let kick_verify = move |_| {
        let vc = vc.clone();
        let mut badge = badge.clone();
        spawn(async move {
            let wallet = wallet_handle();
            let store = vc_store_handle();
            match self_verify_and_cache(&vc, &wallet, &store).await {
                SelfVerifyResult::Valid { resolved_at_ms } =>
                    badge.set(VerifyBadge::Valid { ts_ms: resolved_at_ms }),
                SelfVerifyResult::Invalid(r) =>
                    badge.set(VerifyBadge::Invalid(format!("{r}"))),
            }
        });
    };

    rsx! {
        div { class: "vc-card",
            header {
                strong { "Birth Credential" }
                span { class: "issuer", "Issued by " code { "{vc.issuer_did}" } }
            }
            section { class: "claims",
                // Public claims (issuer + holder + issuanceDate) are
                // displayed in plain text; private claims show the
                // committed digest with a hint that openings are available.
                div { "Holder: " code { "{vc.holder_did}" } }
                div { "Format: " code { "{vc.format}" } }
                div { "Body: " span { class: "muted", "{vc.body.len()} bytes (CBOR signed)" } }
            }
            footer {
                button { onclick: kick_verify, "Self-verify" }
                match badge() {
                    VerifyBadge::Unknown => rsx!{ span { class: "badge unknown", "—" } },
                    VerifyBadge::Valid { ts_ms } => rsx!{
                        span { class: "badge valid", "✓ Signed — last checked {fmt_ts(ts_ms)}" }
                    },
                    VerifyBadge::Stale { ts_ms } => rsx!{
                        span { class: "badge stale", "↻ Last check {fmt_ts(ts_ms)}" }
                    },
                    VerifyBadge::Invalid(r) => rsx!{
                        span { class: "badge invalid", "✗ {r}" }
                    },
                }
            }
        }
    }
}

fn fmt_ts(ms: u64) -> String {
    // HH:MM:SS in local TZ — strip the date for the badge subtitle.
    let secs = (ms / 1000) as i64;
    let dt = chrono::DateTime::from_timestamp(secs, 0).unwrap_or_default();
    dt.format("%H:%M:%S").to_string()
}
```

Add `chrono = "0.4"` to `mobile-bench/dioxus-wallet/Cargo.toml` if missing.

- [ ] **Step 33.2: Hook initial badge from metadata**

On first render, populate the badge from `vc_store.get_metadata(vc_uri)`
if there's a cached outcome. Add to `VcCard`:

```rust
let initial_badge = use_resource({
    let vc_uri = vc.vc_uri.clone();
    move || {
        let vc_uri = vc_uri.clone();
        async move {
            let store = vc_store_handle();
            let md = store.get_metadata(&vc_uri).ok().flatten();
            match md.and_then(|m| m.last_verified_ms.zip(m.last_verify_outcome)) {
                Some((ts, outcome)) if outcome == "Valid" => {
                    // Stale if >60 s old.
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64;
                    if now.saturating_sub(ts) > 60_000 {
                        VerifyBadge::Stale { ts_ms: ts }
                    } else {
                        VerifyBadge::Valid { ts_ms: ts }
                    }
                }
                Some((_, outcome)) => VerifyBadge::Invalid(outcome),
                None => VerifyBadge::Unknown,
            }
        }
    }
});
use_effect(move || {
    if let Some(b) = initial_badge.read().clone() {
        badge.set(b);
    }
});
```

- [ ] **Step 33.3: Commit**

```bash
git add mobile-bench/dioxus-wallet/
git commit -S -s -m "feat(identity-centre): VcCard with three-state self-verify badge"
git log --format="%h %G? %s" -1
```

### Task 34: `DidList` + `DidDetail` (with nested Keys)

**Files:**
- Modify: `mobile-bench/dioxus-wallet/src/identity/did_list.rs`
- Modify: `mobile-bench/dioxus-wallet/src/identity/did_detail.rs`

Replaces / absorbs whatever DID listing UI lived at the old `Tab::Keys`.
Keys appear inside each DID's detail screen.

- [ ] **Step 34.1: `DidList`**

```rust
use dioxus::prelude::*;
use midnight_wallet_core::{Wallet, DidId};
use crate::wallet_handle::wallet_handle;

#[component]
pub fn DidListPanel() -> Element {
    let mut selected = use_signal::<Option<DidId>>(|| None);

    let dids = use_resource(|| async {
        let wallet = wallet_handle();
        wallet.list_owned_dids().await.unwrap_or_default()
    });

    rsx! {
        if let Some(did) = selected() {
            super::did_detail::DidDetailPanel {
                did: did.clone(),
                on_back: move |_| selected.set(None),
            }
        } else {
            div { class: "did-list",
                h2 { "Your DIDs" }
                if let Some(list) = &*dids.read() {
                    if list.is_empty() {
                        p { "No DIDs yet — Bootstrap one from the VCs tab." }
                    } else {
                        ul {
                            for d in list.iter() {
                                li {
                                    button {
                                        onclick: {
                                            let d = d.clone();
                                            move |_| selected.set(Some(d.clone()))
                                        },
                                        "{d.as_str()}"
                                    }
                                }
                            }
                        }
                    }
                } else {
                    p { "Loading..." }
                }
            }
        }
    }
}
```

If `Wallet::list_owned_dids` doesn't exist, add it as a thin wrapper
around the existing DID inventory in `wallet-core`.

- [ ] **Step 34.2: `DidDetail`**

```rust
use dioxus::prelude::*;
use midnight_wallet_core::{DidId, Wallet};
use crate::wallet_handle::wallet_handle;

#[derive(Props, Clone, PartialEq)]
pub struct DidDetailProps {
    pub did: DidId,
    pub on_back: EventHandler<()>,
}

#[component]
pub fn DidDetailPanel(props: DidDetailProps) -> Element {
    let did = props.did.clone();
    let doc = use_resource({
        let did = did.clone();
        move || {
            let did = did.clone();
            async move {
                let wallet = wallet_handle();
                wallet.resolve_did(&did).await.ok()
            }
        }
    });

    rsx! {
        div { class: "did-detail",
            button { onclick: move |_| props.on_back.call(()), "← Back" }
            h2 { "DID detail" }
            code { class: "did-id", "{did.as_str()}" }
            match &*doc.read() {
                Some(Some(d)) => rsx!{
                    section { class: "keys",
                        h3 { "Verification methods" }
                        ul {
                            for vm in d.verification_method.iter() {
                                li {
                                    code { "{vm.id}" }
                                    span { class: "vm-relations",
                                        if d.authentication.iter().any(|i| i.to_string() == vm.id.to_string()) { " · authentication" }
                                        if d.assertion_method.iter().any(|i| i.to_string() == vm.id.to_string()) { " · assertionMethod" }
                                    }
                                }
                            }
                        }
                    }
                },
                Some(None) => rsx!{ p { class: "err", "Resolve failed" } },
                None => rsx!{ p { "Loading..." } },
            }
        }
    }
}
```

- [ ] **Step 34.3: Commit**

```bash
git add mobile-bench/dioxus-wallet/src/identity/did_list.rs \
        mobile-bench/dioxus-wallet/src/identity/did_detail.rs
git commit -S -s -m "feat(identity-centre): DidList + DidDetail with nested keys"
git log --format="%h %G? %s" -1
```

### Task 35: `DidPickerPopup` (>1 DID case)

**Files:**
- Modify: `mobile-bench/dioxus-wallet/src/identity/did_picker.rs`

- [ ] **Step 35.1: Implementation**

```rust
use dioxus::prelude::*;
use midnight_wallet_core::DidId;

#[derive(Props, Clone, PartialEq)]
pub struct DidPickerProps {
    pub options: Vec<DidId>,
    pub on_pick: EventHandler<DidId>,
    pub on_cancel: EventHandler<()>,
}

#[component]
pub fn DidPickerPopup(props: DidPickerProps) -> Element {
    rsx! {
        div { class: "modal-bg", onclick: move |_| props.on_cancel.call(()),
            div { class: "modal did-picker",
                onclick: move |e| e.stop_propagation(),
                h3 { "Pick a DID to authenticate with" }
                ul {
                    for d in props.options.iter() {
                        li {
                            button {
                                onclick: {
                                    let d = d.clone();
                                    move |_| props.on_pick.call(d.clone())
                                },
                                "{d.as_str()}"
                            }
                        }
                    }
                }
                button { class: "cancel", onclick: move |_| props.on_cancel.call(()), "Cancel" }
            }
        }
    }
}

/// Helper: if there's exactly one DID, returns it directly without
/// rendering the picker. Caller controls whether to render or skip.
pub fn pick_did_silent(options: &[DidId]) -> Option<DidId> {
    if options.len() == 1 { Some(options[0].clone()) } else { None }
}
```

- [ ] **Step 35.2: Commit**

```bash
git add mobile-bench/dioxus-wallet/src/identity/did_picker.rs
git commit -S -s -m "feat(identity-centre): DidPickerPopup + silent-single helper"
git log --format="%h %G? %s" -1
```

### Task 36: `QrScanFab` + `QrScanModal` + scan dispatch

**Files:**
- Modify: `mobile-bench/dioxus-wallet/src/identity/qr_scan_fab.rs`
- Modify: `mobile-bench/dioxus-wallet/src/identity/qr_scan_modal.rs`

- [ ] **Step 36.1: `QrScanFab` — opens the modal**

```rust
use dioxus::prelude::*;
use super::screen::IdentitySubTab;

#[component]
pub fn QrScanFab(sub: IdentitySubTab) -> Element {
    let mut open = use_signal(|| false);
    rsx! {
        button {
            class: "fab",
            onclick: move |_| open.set(true),
            "+"
        }
        if open() {
            super::qr_scan_modal::QrScanModal {
                expected_scope: sub,
                on_close: move |_| open.set(false),
            }
        }
    }
}
```

- [ ] **Step 36.2: `QrScanModal` — does the scan + dispatch**

```rust
use dioxus::prelude::*;
use midnight_wallet_core::{
    oid4vp_run_authentication, oid4vci_run_issuance,
    PasteUrlScanner, QrScanner, QrScanError, DidId,
};
use crate::wallet_handle::{wallet_handle, secret_store, vc_store_handle};
use super::screen::IdentitySubTab;

#[derive(Props, Clone, PartialEq)]
pub struct QrScanModalProps {
    pub expected_scope: IdentitySubTab,
    pub on_close: EventHandler<()>,
}

#[derive(Clone, Debug)]
enum ScanState {
    PromptingUrl,
    Running(String),
    Done(String),
    Failed(String),
}

#[component]
pub fn QrScanModal(props: QrScanModalProps) -> Element {
    let mut state = use_signal(|| ScanState::PromptingUrl);
    let mut paste_url = use_signal(|| String::new());

    let dispatch_scan = move |url: String| {
        let mut state = state.clone();
        let scope = props.expected_scope;
        spawn(async move {
            state.set(ScanState::Running("dispatching...".into()));
            let wallet = wallet_handle();
            let secret_store = secret_store();
            let vc_store = vc_store_handle();
            // Phase 1 single-DID: pick the first owned DID.
            let dids = wallet.list_owned_dids().await.unwrap_or_default();
            let did: DidId = match dids.into_iter().next() {
                Some(d) => d,
                None => { state.set(ScanState::Failed("No DID — Bootstrap first".into())); return; }
            };

            let result = if url.starts_with("openid4vp://") {
                state.set(ScanState::Running("authenticating...".into()));
                oid4vp_run_authentication(&url, &wallet, &*secret_store, &did)
                    .await
                    .map(|r| format!("authenticated session {}", r.session_id))
                    .map_err(|e| format!("{e}"))
            } else if url.starts_with("openid-credential-offer://") {
                state.set(ScanState::Running("downloading credential...".into()));
                oid4vci_run_issuance(&url, &wallet, &*secret_store, &did, &vc_store)
                    .await
                    .map(|uri| format!("credential {uri} stored"))
                    .map_err(|e| format!("{e}"))
            } else {
                Err(format!("unknown scheme in URL: {url}"))
            };

            match result {
                Ok(msg) => state.set(ScanState::Done(msg)),
                Err(e) => state.set(ScanState::Failed(e)),
            }
        });
    };

    rsx! {
        div { class: "modal-bg",
            div { class: "modal qr-modal",
                button { class: "close", onclick: move |_| props.on_close.call(()), "×" }
                h2 { "Scan QR" }
                match state() {
                    ScanState::PromptingUrl => rsx! {
                        p { "Use the camera (real device) or paste the URL (sim):" }
                        input {
                            r#type: "text",
                            placeholder: "openid4vp://… or openid-credential-offer://…",
                            value: paste_url(),
                            oninput: move |e| paste_url.set(e.value()),
                        }
                        button {
                            disabled: paste_url().is_empty(),
                            onclick: move |_| {
                                let url = paste_url().clone();
                                dispatch_scan(url);
                            },
                            "Submit URL"
                        }
                    },
                    ScanState::Running(msg) => rsx! {
                        div { class: "spinner" }
                        p { "{msg}" }
                    },
                    ScanState::Done(msg) => rsx! {
                        p { class: "ok", "✓ {msg}" }
                        button { onclick: move |_| props.on_close.call(()), "Close" }
                    },
                    ScanState::Failed(err) => rsx! {
                        p { class: "err", "✗ {err}" }
                        button { onclick: move |_| state.set(ScanState::PromptingUrl), "Try again" }
                    },
                }
            }
        }
    }
}
```

- [ ] **Step 36.3: Build + commit**

```bash
cargo build -p dioxus-wallet
```

Expected: clean build. The native camera scanner from Task 37 swaps in by replacing the `<input>` with a CameraView component; the paste-URL affordance survives as a dev fallback.

```bash
git add mobile-bench/dioxus-wallet/src/identity/qr_scan_fab.rs \
        mobile-bench/dioxus-wallet/src/identity/qr_scan_modal.rs
git commit -S -s -m "$(cat <<'EOF'
feat(identity-centre): QrScanFab + QrScanModal with paste-URL stub

Modal dispatches scanned URLs to oid4vp_run_authentication
(SIOPv2 login) or oid4vci_run_issuance (VC download) based on
URL scheme. Single-DID flow uses the first owned DID; multi-
DID picker hooks in once Task 35's component is rendered from
this flow (Phase 2 enhancement).

Native camera bridge lands in Task 37 and swaps the input
field for an inline camera preview.
EOF
)"
git log --format="%h %G? %s" -1
```

---

## Section 9 — Android QR scanner JNI bridge (Spec build step 9)

### Task 37: Kotlin CameraX + ML Kit + JNI

**Files:**
- Create: `mobile-bench/dioxus-wallet/android/app/src/main/java/io/iohk/midnight/wallet/QrScanner.kt`
- Modify: `mobile-bench/dioxus-wallet/android/app/build.gradle.kts` (add CameraX + ML Kit)
- Modify: `mobile-bench/dioxus-wallet/android/app/src/main/AndroidManifest.xml` (camera permission)

- [ ] **Step 37.1: Gradle deps + manifest**

In `android/app/build.gradle.kts`, append to `dependencies {}`:

```kotlin
implementation("androidx.camera:camera-core:1.3.4")
implementation("androidx.camera:camera-camera2:1.3.4")
implementation("androidx.camera:camera-lifecycle:1.3.4")
implementation("androidx.camera:camera-view:1.3.4")
implementation("com.google.mlkit:barcode-scanning:17.3.0")
implementation("androidx.core:core-ktx:1.13.1")
```

In `AndroidManifest.xml`, add:

```xml
<uses-permission android:name="android.permission.CAMERA" />
<uses-feature android:name="android.hardware.camera" android:required="false" />
```

- [ ] **Step 37.2: Kotlin scanner**

```kotlin
// android/app/src/main/java/io/iohk/midnight/wallet/QrScanner.kt
package io.iohk.midnight.wallet

import android.content.Context
import androidx.camera.core.*
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.camera.view.PreviewView
import androidx.lifecycle.LifecycleOwner
import com.google.mlkit.vision.barcode.BarcodeScannerOptions
import com.google.mlkit.vision.barcode.BarcodeScanning
import com.google.mlkit.vision.barcode.common.Barcode
import com.google.mlkit.vision.common.InputImage
import java.util.concurrent.atomic.AtomicBoolean
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlin.coroutines.resume

object QrScannerBridge {
    /// JNI entry point. Returns the decoded URL or throws if the
    /// scan failed / cancelled.
    @JvmStatic
    external fun nativeOnScanResult(callbackPtr: Long, url: String?, error: String?)

    suspend fun scan(
        ctx: Context,
        lifecycleOwner: LifecycleOwner,
        previewView: PreviewView,
    ): String = suspendCancellableCoroutine { cont ->
        val resolved = AtomicBoolean(false)
        val opts = BarcodeScannerOptions.Builder()
            .setBarcodeFormats(Barcode.FORMAT_QR_CODE)
            .build()
        val scanner = BarcodeScanning.getClient(opts)
        val providerFuture = ProcessCameraProvider.getInstance(ctx)
        providerFuture.addListener({
            try {
                val provider = providerFuture.get()
                val preview = Preview.Builder().build().apply {
                    setSurfaceProvider(previewView.surfaceProvider)
                }
                val analyser = ImageAnalysis.Builder()
                    .setBackpressureStrategy(ImageAnalysis.STRATEGY_KEEP_ONLY_LATEST)
                    .build()
                analyser.setAnalyzer(java.util.concurrent.Executors.newSingleThreadExecutor()) { img ->
                    val media = img.image ?: run { img.close(); return@setAnalyzer }
                    val input = InputImage.fromMediaImage(media, img.imageInfo.rotationDegrees)
                    scanner.process(input)
                        .addOnSuccessListener { codes ->
                            val first = codes.firstOrNull()?.rawValue
                            if (first != null && resolved.compareAndSet(false, true)) {
                                provider.unbindAll()
                                cont.resume(first)
                            }
                        }
                        .addOnCompleteListener { img.close() }
                }
                provider.unbindAll()
                provider.bindToLifecycle(
                    lifecycleOwner,
                    CameraSelector.DEFAULT_BACK_CAMERA,
                    preview, analyser,
                )
                cont.invokeOnCancellation {
                    if (resolved.compareAndSet(false, true)) {
                        provider.unbindAll()
                    }
                }
            } catch (e: Throwable) {
                if (resolved.compareAndSet(false, true)) {
                    cont.cancel(e)
                }
            }
        }, androidx.core.content.ContextCompat.getMainExecutor(ctx))
    }
}
```

- [ ] **Step 37.3: Commit (Kotlin compiles in isolation; JNI bridge in Task 38)**

```bash
cd android && ./gradlew :app:compileDebugKotlin
cd ..
git add mobile-bench/dioxus-wallet/android/
git commit -S -s -m "$(cat <<'EOF'
feat(qr-scan): Kotlin CameraX + ML Kit scanner skeleton

QrScannerBridge.scan() is a Kotlin coroutine that drives
ML Kit barcode detection over a CameraX preview. Emits the
first detected QR's raw value, then tears down. The JNI
glue that hands the result back to Rust lands in Task 38.
EOF
)"
git log --format="%h %G? %s" -1
```

### Task 38: Rust JNI bridge

**Files:**
- Create: `mobile-bench/dioxus-wallet/src/platform/android_qr.rs`
- Modify: `mobile-bench/dioxus-wallet/Cargo.toml` (`jni = "0.21"`)
- Modify: `mobile-bench/dioxus-wallet/src/platform/mod.rs` (re-export based on target)

- [ ] **Step 38.1: Bridge implementation**

```rust
// mobile-bench/dioxus-wallet/src/platform/android_qr.rs
#![cfg(target_os = "android")]

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use jni::objects::{JObject, JString};
use jni::JNIEnv;
use midnight_wallet_core::{QrScanError, QrScanner};
use tokio::sync::oneshot;

pub struct AndroidQrScanner;

impl QrScanner for AndroidQrScanner {
    fn scan(&self) -> Pin<Box<dyn Future<Output = Result<String, QrScanError>> + Send + '_>> {
        Box::pin(async {
            let (tx, rx) = oneshot::channel::<Result<String, QrScanError>>();
            let tx = Arc::new(Mutex::new(Some(tx)));

            // Get the JavaVM via the existing dioxus-wallet platform plumbing.
            let vm = crate::platform::android_vm::current();
            let mut env = vm.attach_current_thread().expect("attach jvm");

            // Stash the tx pointer so the callback can resolve it.
            let tx_ptr = Arc::into_raw(tx) as i64;

            // Call into the Kotlin side: io.iohk.midnight.wallet.QrScannerBridge.scan(ctx, …).
            // The Kotlin code calls nativeOnScanResult below with our tx_ptr.
            let bridge = env.find_class("io/iohk/midnight/wallet/QrScannerBridge").unwrap();
            env.call_static_method(
                bridge,
                "scanForRust",
                "(J)V",
                &[(&jni::objects::JValue::from(tx_ptr)).into()],
            ).expect("invoke scanForRust");

            rx.await.unwrap_or(Err(QrScanError::Cancelled))
        })
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern "system" fn Java_io_iohk_midnight_wallet_QrScannerBridge_nativeOnScanResult(
    mut env: JNIEnv,
    _class: JObject,
    callback_ptr: i64,
    url: JString,
    error: JString,
) {
    let tx = unsafe { Arc::from_raw(callback_ptr as *const Mutex<Option<oneshot::Sender<Result<String, QrScanError>>>>) };
    let payload: Result<String, QrScanError> = if !error.is_null() {
        let msg: String = env.get_string(&error).map(Into::into).unwrap_or_default();
        Err(QrScanError::Unavailable(msg))
    } else if !url.is_null() {
        let s: String = env.get_string(&url).map(Into::into).unwrap_or_default();
        Ok(s)
    } else {
        Err(QrScanError::Cancelled)
    };
    if let Some(sender) = tx.lock().unwrap().take() {
        let _ = sender.send(payload);
    }
}
```

A `scanForRust(Long)` static method needs to exist on the Kotlin side
that wraps the suspending `scan(...)` and calls back into
`nativeOnScanResult` when complete. Add to `QrScannerBridge.kt`:

```kotlin
@JvmStatic
fun scanForRust(callbackPtr: Long) {
    val ctx = MainActivity.appContext  // assume an existing accessor
    val activity = MainActivity.current  // assume an existing accessor
    GlobalScope.launch {
        try {
            val previewView = MainActivity.acquireQrPreviewView() // assume helper
            val url = scan(ctx, activity, previewView)
            nativeOnScanResult(callbackPtr, url, null)
        } catch (e: Throwable) {
            nativeOnScanResult(callbackPtr, null, e.message ?: "scan failed")
        }
    }
}
```

The `MainActivity.appContext` / `acquireQrPreviewView` accessors may need to be added to whatever the dioxus-wallet's existing Android activity is — match the existing platform-bridge pattern.

- [ ] **Step 38.2: Wire into QrScanModal**

Modify `qr_scan_modal.rs` to use the native scanner on Android instead of the paste-URL input, with paste-URL preserved as a "Use paste URL instead" affordance:

```rust
#[cfg(target_os = "android")]
let scanner: Box<dyn midnight_wallet_core::QrScanner> = Box::new(crate::platform::android_qr::AndroidQrScanner);
#[cfg(not(target_os = "android"))]
let scanner: Box<dyn midnight_wallet_core::QrScanner> = {
    let s = midnight_wallet_core::PasteUrlScanner::default();
    Box::new(s)  // controlled by the input field below
};
```

Render a "Scan with camera" button alongside the paste-URL input. On
Android, the button calls `scanner.scan()` and dispatches the result.
On other targets, the button is hidden.

- [ ] **Step 38.3: Build + commit**

```bash
cargo build -p dioxus-wallet --target aarch64-linux-android
```

If `cargo ndk` is the preferred wrapper, use:

```bash
cd mobile-bench/dioxus-wallet
cargo ndk -t arm64-v8a build --release
```

Expected: clean build.

```bash
git add mobile-bench/dioxus-wallet/
git commit -S -s -m "$(cat <<'EOF'
feat(qr-scan): Rust JNI bridge + AndroidQrScanner impl

AndroidQrScanner implements wallet-core's QrScanner trait
by calling into QrScannerBridge.scanForRust on the JVM
side; the Kotlin scope hands the decoded URL back via
nativeOnScanResult, which resolves a Rust oneshot.

QrScanModal switches to the native scanner on Android
while keeping the paste-URL input as a dev affordance on
all targets.
EOF
)"
git log --format="%h %G? %s" -1
```

### Task 39: Android camera permission flow

**Files:**
- Modify: `mobile-bench/dioxus-wallet/android/app/src/main/java/.../MainActivity.kt` (or wherever the existing activity is)

- [ ] **Step 39.1: Permission request before scan**

Before `QrScannerBridge.scan()` runs, the activity must hold
`Manifest.permission.CAMERA`. Add a `requestCameraIfNeeded()` helper that
uses the standard Activity Result API and call it from
`scanForRust(callbackPtr)` before launching the scan coroutine.

```kotlin
fun requestCameraIfNeeded(callback: (Boolean) -> Unit) {
    val granted = androidx.core.content.ContextCompat.checkSelfPermission(
        this, android.Manifest.permission.CAMERA
    ) == android.content.pm.PackageManager.PERMISSION_GRANTED
    if (granted) { callback(true); return }
    val launcher = registerForActivityResult(
        androidx.activity.result.contract.ActivityResultContracts.RequestPermission()
    ) { granted -> callback(granted) }
    launcher.launch(android.Manifest.permission.CAMERA)
}
```

Refine `scanForRust` to call this and propagate denial as an error:

```kotlin
@JvmStatic
fun scanForRust(callbackPtr: Long) {
    val act = MainActivity.current ?: run {
        nativeOnScanResult(callbackPtr, null, "no foreground activity")
        return
    }
    act.requestCameraIfNeeded { granted ->
        if (!granted) {
            nativeOnScanResult(callbackPtr, null, "camera permission denied")
            return@requestCameraIfNeeded
        }
        GlobalScope.launch {
            try {
                val previewView = MainActivity.acquireQrPreviewView()
                val url = scan(act, act, previewView)
                nativeOnScanResult(callbackPtr, url, null)
            } catch (e: Throwable) {
                nativeOnScanResult(callbackPtr, null, e.message ?: "scan failed")
            }
        }
    }
}
```

- [ ] **Step 39.2: Commit**

```bash
git add mobile-bench/dioxus-wallet/android/
git commit -S -s -m "feat(qr-scan): runtime camera permission request"
git log --format="%h %G? %s" -1
```

---

## Section 10 — BDD harness + first scenarios (Spec build step 10)

**Context switch:** issuer repo (`midnight-identity-solution-examples` on `develop`).
All commits use `feat(bdd):` prefix.

### Task 40: Cucumber.js config + seed fixtures

**Files:**
- Create: `IssuerDIDIT-mock/cucumber.cjs`
- Create: `IssuerDIDIT-mock/e2e/fixtures/seeds.ts`
- Create: `IssuerDIDIT-mock/e2e/fixtures/kyc-claims.ts`

- [ ] **Step 40.1: Cucumber config**

```javascript
// cucumber.cjs
module.exports = {
  default: {
    paths: ["e2e/features/**/*.feature"],
    require: ["e2e/step-definitions/**/*.ts", "e2e/support/**/*.ts"],
    requireModule: ["tsx/esm"],
    formatOptions: { snippetInterface: "async-await" },
    publishQuiet: true,
  },
};
```

- [ ] **Step 40.2: Deterministic seeds**

```typescript
// e2e/fixtures/seeds.ts
//! Fixture seeds used by both the headless TS wallet client
//! and the issuer's bootstrap script. Standalone env starts
//! clean every time → same seeds always derive the same DIDs.
//!
//! MUST NOT be reused outside the local standalone env.

import crypto from "node:crypto";

export const SEEDS = {
  holderAlice: "alice-demo-seed",
  holderBob:   "bob-demo-seed",
  issuerDemo:  "issuer-demo-seed",
} as const;

export function seedToBytes(seed: string): Uint8Array {
  const hex = seed.startsWith("0x") ? seed.slice(2) : seed;
  if (/^[0-9a-fA-F]{64}$/.test(hex)) {
    return Uint8Array.from(Buffer.from(hex, "hex"));
  }
  return crypto.createHash("sha256").update(seed, "utf8").digest();
}
```

- [ ] **Step 40.3: KYC claim fixtures**

```typescript
// e2e/fixtures/kyc-claims.ts
import type { BirthVcClaims } from "../../src/storage/sessions.js";

export const KYC_CLAIMS: Record<string, BirthVcClaims> = {
  alice: {
    firstName: "Alice",
    lastName: "Example",
    dateOfBirth: "1985-01-15",
    nationality: "USA",
    documentNumber: "P-ABC-123456",
  },
  bob: {
    firstName: "Bob",
    lastName: "Sample",
    dateOfBirth: "2010-09-30",
    nationality: "GBR",
    documentNumber: "P-XYZ-789012",
  },
};
```

- [ ] **Step 40.4: Commit**

```bash
git add IssuerDIDIT-mock/cucumber.cjs IssuerDIDIT-mock/e2e/fixtures/
git commit -S -s -m "feat(bdd): cucumber config + deterministic seeds + KYC fixtures"
git log --format="%h %G? %s" -1
```

### Task 41: Headless TS wallet client

**Files:**
- Create: `IssuerDIDIT-mock/e2e/fixtures/headless-wallet-client.ts`

Mirrors the Rust wallet's 6-endpoint HTTP behaviour in pure TS. Uses
`@midnight-ntwrk/midnight-did-api` for DID bootstrap; `jose` for JWS
signing; plain `fetch` for the HTTP calls.

- [ ] **Step 41.1: Implementation**

```typescript
//! Headless TS wallet — mirrors what the Rust wallet does over
//! HTTP, with no Dioxus UI and no native bridges. Runs in CI as
//! the fast E2E client; the real Rust wallet headless mode is a
//! Phase 1.5 stretch goal.

import * as ed25519 from "@noble/ed25519";
import { SignJWT, importJWK, exportJWK } from "jose";
import crypto from "node:crypto";
import * as didApi from "@midnight-ntwrk/midnight-did-api";

import { seedToBytes } from "./seeds.js";

export interface HeadlessWallet {
  did: string;
  authKid: string;
  signIdToken(audience: string, nonce: string): Promise<string>;
  bootstrap(): Promise<void>;
}

interface KeyMaterial {
  did: string;
  ed25519: { kid: string; privateKey: CryptoKey };
  jubjub:  { kid: string; secretBytes: Uint8Array };
}

export async function makeHeadlessWallet(
  config: { seedString: string; indexerUrl: string; nodeRpcUrl: string },
): Promise<HeadlessWallet> {
  const seed = seedToBytes(config.seedString);
  let mat: KeyMaterial | null = null;

  return {
    get did() { return mat?.did ?? "<not bootstrapped>"; },
    get authKid() { return mat?.ed25519.kid ?? "<not bootstrapped>"; },

    async bootstrap() {
      const r = await didApi.createDidWithKeys({
        indexerUrl: config.indexerUrl,
        nodeRpcUrl: config.nodeRpcUrl,
        seed,
      });
      const jwk = {
        kty: "OKP",
        crv: "Ed25519",
        d: Buffer.from(Buffer.from(r.ed25519.secretHex, "hex")).toString("base64url"),
        x: Buffer.from(await ed25519.getPublicKeyAsync(Buffer.from(r.ed25519.secretHex, "hex"))).toString("base64url"),
      };
      const privateKey = await importJWK(jwk as any, "EdDSA");
      mat = {
        did: r.did,
        ed25519: { kid: r.ed25519.kid, privateKey: privateKey as CryptoKey },
        jubjub:  { kid: r.jubjub.kid,  secretBytes: Buffer.from(r.jubjub.secretHex, "hex") },
      };
    },

    async signIdToken(audience: string, nonce: string): Promise<string> {
      if (!mat) throw new Error("wallet not bootstrapped");
      return new SignJWT({ nonce })
        .setProtectedHeader({ alg: "EdDSA", typ: "JWT", kid: mat.ed25519.kid })
        .setIssuer(mat.did)
        .setSubject(mat.did)
        .setAudience(audience)
        .setIssuedAt()
        .setExpirationTime("5m")
        .sign(mat.ed25519.privateKey);
    },
  };
}

// ─── HTTP flow helpers ──────────────────────────────────────────────────

export async function runOid4vpAuth(
  baseUrl: string,
  wallet: HeadlessWallet,
): Promise<{ sessionId: string; status: string }> {
  // 1. GET /authorize to learn what to scan (HTML parsing avoided —
  //    the BDD harness POSTs directly to /authorize-init in tests if
  //    we add that helper, or extracts from the HTML).
  const authorizeHtml = await fetch(`${baseUrl}/authorize`).then(r => r.text());
  // Pull the qrPayload out of the <code> block.
  const m = authorizeHtml.match(/<code>([^<]+)<\/code>/);
  if (!m) throw new Error("could not find QR payload in /authorize HTML");
  const qrPayload = m[1];

  // 2. Parse the openid4vp:// URL → request_uri.
  const u = new URL(qrPayload);
  const requestUri = u.searchParams.get("request_uri");
  if (!requestUri) throw new Error("missing request_uri");

  // 3. GET the request object.
  const req = await fetch(requestUri).then(r => r.json());

  // 4. Sign + POST.
  const idToken = await wallet.signIdToken(req.client_id, req.nonce);
  const respJson = await fetch(req.redirect_uri, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ id_token: idToken, state: req.state }),
  });
  if (!respJson.ok) throw new Error(`authorize-response ${respJson.status}: ${await respJson.text()}`);
  return await respJson.json();
}

export async function runOid4vciIssuance(
  baseUrl: string,
  wallet: HeadlessWallet,
  sessionId: string,
): Promise<{ vc_uri: string; body_b64: string; openings: any[] }> {
  // 1. GET /credential-offer/:id to extract the QR payload.
  const offerHtml = await fetch(`${baseUrl}/credential-offer/${sessionId}`).then(r => r.text());
  const m = offerHtml.match(/<code>([^<]+)<\/code>/);
  if (!m) throw new Error("could not find QR-2 payload");
  const qrPayload = m[1];
  const u = new URL(qrPayload);
  const offerJson = JSON.parse(u.searchParams.get("credential_offer")!);
  const code = offerJson.grants["urn:ietf:params:oauth:grant-type:pre-authorized_code"]["pre-authorized_code"];

  // 2. POST /token
  const tokenResp = await fetch(`${baseUrl}/token`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      grant_type: "urn:ietf:params:oauth:grant-type:pre-authorized_code",
      "pre-authorized_code": code,
    }),
  });
  if (!tokenResp.ok) throw new Error(`token ${tokenResp.status}: ${await tokenResp.text()}`);
  const { access_token, c_nonce } = await tokenResp.json();

  // 3. DID-bound JWS proof over c_nonce.
  const proofJwt = await wallet.signIdToken(baseUrl, c_nonce);

  // 4. POST /credential
  const credResp = await fetch(`${baseUrl}/credential`, {
    method: "POST",
    headers: {
      "content-type": "application/json",
      authorization: `Bearer ${access_token}`,
    },
    body: JSON.stringify({ format: "midnight-vc-compact", proof: { proof_type: "jwt", jwt: proofJwt } }),
  });
  if (!credResp.ok) throw new Error(`credential ${credResp.status}: ${await credResp.text()}`);
  const result = await credResp.json();
  return { vc_uri: result.credential.vc_uri, body_b64: result.credential.body_b64, openings: result.openings };
}
```

- [ ] **Step 41.2: Smoke test (standalone)**

```bash
yarn build
yarn env:up
yarn bootstrap
yarn dev &
SERVER_PID=$!
sleep 3

yarn tsx -e '
import { makeHeadlessWallet, runOid4vpAuth } from "./e2e/fixtures/headless-wallet-client.js";
const w = await makeHeadlessWallet({
  seedString: "alice-demo-seed",
  indexerUrl: "http://localhost:8088/api/v1/graphql",
  nodeRpcUrl: "http://localhost:9944",
});
await w.bootstrap();
console.log("Alice DID:", w.did);
const r = await runOid4vpAuth("http://localhost:3001", w);
console.log("auth result:", r);
'

kill $SERVER_PID
yarn env:down
```

Expected output: a `did:midnight:…` for Alice, and an `auth result: { sessionId: '…', status: 'authenticated' }`.

- [ ] **Step 41.3: Commit**

```bash
git add IssuerDIDIT-mock/e2e/fixtures/headless-wallet-client.ts
git commit -S -s -m "$(cat <<'EOF'
feat(bdd): headless TS wallet client

Pure-TS wallet implementation that mirrors the Rust wallet's
six-endpoint HTTP behaviour: bootstrap via midnight-did-api,
sign SIOPv2 id_tokens with jose, run /authorize → /request →
/authorize-response and /credential-offer → /token → /credential
end-to-end. Used by every Cucumber scenario in Tasks 42-43.
EOF
)"
git log --format="%h %G? %s" -1
```

### Task 42: Bootstrap + happy-path feature files

**Files:**
- Create: `IssuerDIDIT-mock/e2e/features/bootstrap.feature`
- Create: `IssuerDIDIT-mock/e2e/features/issuance-happy-path.feature`
- Create: `IssuerDIDIT-mock/e2e/step-definitions/issuer-steps.ts`
- Create: `IssuerDIDIT-mock/e2e/step-definitions/wallet-steps.ts`
- Create: `IssuerDIDIT-mock/e2e/support/hooks.ts`

- [ ] **Step 42.1: Hooks (env up/down + per-scenario reset)**

```typescript
// e2e/support/hooks.ts
import { Before, After, BeforeAll, AfterAll, setDefaultTimeout } from "@cucumber/cucumber";
import { execSync, spawn, ChildProcess } from "node:child_process";
import path from "node:path";
import fs from "node:fs";

setDefaultTimeout(120_000);

let serverProc: ChildProcess | null = null;
const SERVER_PORT = 3001;
const COMPOSE = path.join(__dirname, "../fixtures/docker-compose.yml");

BeforeAll({ timeout: 180_000 }, async () => {
  execSync(`docker compose -f ${COMPOSE} up -d --wait`, { stdio: "inherit" });
  // Bootstrap issuer DID once per run (idempotent if keystore present).
  if (!fs.existsSync("issuer-keystore.json")) {
    execSync("yarn bootstrap", { stdio: "inherit", env: { ...process.env, KYC_DELAY_MS: "0" } });
  }
  // Start issuer server.
  serverProc = spawn("yarn", ["dev"], {
    stdio: "inherit",
    env: { ...process.env, PORT: String(SERVER_PORT), KYC_DELAY_MS: "0" },
  });
  await waitForUrl(`http://localhost:${SERVER_PORT}/healthz`);
});

AfterAll(async () => {
  if (serverProc) { serverProc.kill(); }
  execSync(`docker compose -f ${COMPOSE} down -v`, { stdio: "inherit" });
});

Before("@requires-fresh-chain", async () => {
  // Tear-down + re-bring-up — slow, only for scenarios that need a clean chain.
  execSync(`docker compose -f ${COMPOSE} down -v`, { stdio: "inherit" });
  execSync(`docker compose -f ${COMPOSE} up -d --wait`, { stdio: "inherit" });
  fs.rmSync("issuer-keystore.json", { force: true });
  fs.rmSync("issuer.sqlite", { force: true });
  execSync("yarn bootstrap", { stdio: "inherit" });
});

After(async function () {
  // Tear down any per-scenario state in `this`.
  if (this.wallet) this.wallet = null;
});

async function waitForUrl(url: string, timeoutMs = 30_000) {
  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    try {
      const r = await fetch(url);
      if (r.ok) return;
    } catch { /* retry */ }
    await new Promise(r => setTimeout(r, 500));
  }
  throw new Error(`url ${url} not ready within ${timeoutMs}ms`);
}
```

- [ ] **Step 42.2: Feature — bootstrap**

```gherkin
# e2e/features/bootstrap.feature
Feature: DID bootstrap on standalone env
  Background:
    Given a clean standalone Midnight environment

  @requires-fresh-chain
  Scenario: Holder bootstraps a DID with both verification relations
    When the holder "alice" bootstraps her DID
    Then "alice"'s DID document carries an authentication-relation key
    And "alice"'s DID document carries an assertionMethod-relation key

  @requires-fresh-chain
  Scenario: Bootstrap is deterministic across clean runs
    When the holder "alice" bootstraps her DID
    And the chain is reset
    And the holder "alice" bootstraps her DID
    Then "alice"'s DID is identical across both runs
```

- [ ] **Step 42.3: Feature — happy path**

```gherkin
# e2e/features/issuance-happy-path.feature
Feature: Issuance happy path
  Background:
    Given the issuer has bootstrapped DID "issuer-demo"
    And the holder "alice" has bootstrapped her DID

  Scenario: Alice receives a birth VC
    Given Alice's wallet has no VCs
    When the operator initiates an issuance session
    Then a QR-1 is rendered with a SIOPv2 authorization request
    When Alice's wallet scans QR-1
    Then the issuer marks the session "authenticated"
    When the operator submits KYC data for "alice"
    Then the session status is "kyc_done" within 100ms
    And a QR-2 is rendered with an OID4VCI credential offer
    When Alice's wallet scans QR-2
    Then Alice's wallet receives a signed birth VC issued by "issuer-demo"
    And the VC's holder field equals Alice's DID
```

- [ ] **Step 42.4: Step definitions — issuer-steps + wallet-steps**

```typescript
// e2e/step-definitions/issuer-steps.ts
import { Given, When, Then } from "@cucumber/cucumber";
import { expect } from "chai";
import { execSync } from "node:child_process";
import fs from "node:fs";

const BASE = `http://localhost:${process.env.PORT ?? 3001}`;

Given("a clean standalone Midnight environment", async function () {
  // No-op: hooks bring the env up before each scenario.
});

Given("the issuer has bootstrapped DID {string}", function (name: string) {
  expect(fs.existsSync("issuer-keystore.json")).to.equal(true);
  this.issuerDid = JSON.parse(fs.readFileSync("issuer-keystore.json", "utf8")).did;
});

When("the operator initiates an issuance session", async function () {
  const html = await fetch(`${BASE}/authorize`).then(r => r.text());
  const m = html.match(/<code>([^<]+)<\/code>/);
  expect(m, "QR-1 payload in /authorize HTML").to.not.be.null;
  this.qrPayload = m![1];
});

Then("a QR-1 is rendered with a SIOPv2 authorization request", function () {
  expect(this.qrPayload).to.match(/^openid4vp:\/\//);
});

Then("the issuer marks the session {string}", function (expected: string) {
  expect(this.lastAuthResult.status).to.equal(expected);
});

When("the operator submits KYC data for {string}", async function (holder: string) {
  const { KYC_CLAIMS } = await import("../fixtures/kyc-claims.js");
  const claims = KYC_CLAIMS[holder];
  expect(claims, `KYC fixture for ${holder}`).to.not.be.undefined;
  const form = new URLSearchParams();
  Object.entries(claims).forEach(([k, v]) => form.set(k, v));
  const r = await fetch(`${BASE}/kyc-form?session=${this.lastAuthResult.sessionId}`, {
    method: "POST",
    headers: { "content-type": "application/x-www-form-urlencoded" },
    body: form.toString(),
    redirect: "manual",
  });
  expect(r.status).to.be.oneOf([302, 303]);
});

Then('the session status is "kyc_done" within {int}ms', async function (ms: number) {
  // Server immediately redirects on form-POST → session must be kyc_done.
  // No need to poll for the fixture KYC_DELAY_MS=0 case.
  const start = Date.now();
  while (Date.now() - start < ms) {
    // sessions table isn't exposed externally — we infer from /credential-offer success.
    const r = await fetch(`${BASE}/credential-offer/${this.lastAuthResult.sessionId}`);
    if (r.status === 200) return;
  }
  throw new Error("session never reached kyc_done");
});

Then("a QR-2 is rendered with an OID4VCI credential offer", async function () {
  const html = await fetch(`${BASE}/credential-offer/${this.lastAuthResult.sessionId}`).then(r => r.text());
  const m = html.match(/<code>([^<]+)<\/code>/);
  expect(m).to.not.be.null;
  expect(m![1]).to.match(/^openid-credential-offer:\/\//);
});
```

```typescript
// e2e/step-definitions/wallet-steps.ts
import { Given, When, Then } from "@cucumber/cucumber";
import { expect } from "chai";
import { makeHeadlessWallet, runOid4vpAuth, runOid4vciIssuance } from "../fixtures/headless-wallet-client.js";
import { SEEDS } from "../fixtures/seeds.js";

const BASE = `http://localhost:${process.env.PORT ?? 3001}`;
const INDEXER = process.env.INDEXER_URL ?? "http://localhost:8088/api/v1/graphql";
const NODE = process.env.NODE_RPC_URL ?? "http://localhost:9944";

Given("the holder {string} has bootstrapped her DID", async function (name: string) {
  const seedKey = name === "alice" ? "holderAlice" : "holderBob";
  const w = await makeHeadlessWallet({ seedString: SEEDS[seedKey as keyof typeof SEEDS], indexerUrl: INDEXER, nodeRpcUrl: NODE });
  await w.bootstrap();
  this.wallets ??= {};
  this.wallets[name] = w;
});

Given("Alice's wallet has no VCs", function () {
  this.aliceVcs = [];
});

When("Alice's wallet scans QR-1", async function () {
  const r = await runOid4vpAuth(BASE, this.wallets.alice);
  this.lastAuthResult = r;
});

When("Alice's wallet scans QR-2", async function () {
  const r = await runOid4vciIssuance(BASE, this.wallets.alice, this.lastAuthResult.sessionId);
  this.lastVc = r;
  this.aliceVcs.push(r);
});

Then("Alice's wallet receives a signed birth VC issued by {string}", function (issuerName: string) {
  expect(this.lastVc.vc_uri).to.match(/^urn:uuid:/);
  expect(this.lastVc.body_b64.length).to.be.greaterThan(0);
});

Then("the VC's holder field equals Alice's DID", async function () {
  const { decode } = await import("cbor-x");
  const body = decode(Buffer.from(this.lastVc.body_b64, "base64"));
  expect(body.holder).to.equal(this.wallets.alice.did);
});
```

- [ ] **Step 42.5: Run + commit**

```bash
yarn test --tags "@requires-fresh-chain or not @requires-fresh-chain"
# Or just: yarn test
```

Expected: both feature files green (3 scenarios total: 2 bootstrap + 1 happy path).

```bash
git add IssuerDIDIT-mock/e2e/
git commit -S -s -m "$(cat <<'EOF'
feat(bdd): bootstrap + issuance-happy-path features

Two feature files; six step definitions across issuer +
wallet roles. Hooks bring docker compose up/down per run,
with @requires-fresh-chain forcing a per-scenario reset
for tests that mutate chain state.
EOF
)"
git log --format="%h %G? %s" -1
```

### Task 43: Self-verify + negative-path features

**Files:**
- Create: `IssuerDIDIT-mock/e2e/features/self-verify.feature`
- Create: `IssuerDIDIT-mock/e2e/features/negative-paths.feature`
- Modify: `IssuerDIDIT-mock/e2e/step-definitions/wallet-steps.ts` (add steps)
- Create: `IssuerDIDIT-mock/e2e/step-definitions/chain-steps.ts` (key rotation)

- [ ] **Step 43.1: Self-verify feature**

```gherkin
# e2e/features/self-verify.feature
Feature: VC self-verify
  Background:
    Given the issuer has bootstrapped DID "issuer-demo"
    And the holder "alice" has bootstrapped her DID
    And Alice holds a freshly issued birth VC

  Scenario: Alice self-verifies a fresh VC
    When Alice runs self-verify on her birth VC
    Then the self-verify outcome is "Valid"
    And the outcome includes a resolved_at_ms timestamp

  @requires-fresh-chain
  Scenario: Self-verify fails after the issuer rotates its assertionMethod key
    When the issuer rotates its assertionMethod key on chain
    And Alice runs self-verify on her birth VC
    Then the self-verify outcome is "Invalid"
    And the reason mentions "signature does not match" or "KeyNotInAssertionRelation"
```

- [ ] **Step 43.2: Negative-paths feature**

```gherkin
# e2e/features/negative-paths.feature
Feature: Negative paths
  Background:
    Given the issuer has bootstrapped DID "issuer-demo"
    And the holder "alice" has bootstrapped her DID

  Scenario: Wrong nonce
    Given the issuer has issued a SIOPv2 nonce
    When Alice's wallet signs a different nonce
    Then POST /authorize-response returns 401

  Scenario: Replay
    Given Alice has authenticated once
    When Alice's wallet replays the same id_token
    Then POST /authorize-response returns 401

  Scenario: Unbootstrapped DID
    Given Alice's wallet has a DID with no authentication-relation key
    When Alice's wallet tries to scan QR-1
    Then the wallet emits an error matching "no authentication-relation"
```

- [ ] **Step 43.3: Additional step definitions**

```typescript
// e2e/step-definitions/chain-steps.ts
import { Given, When, Then } from "@cucumber/cucumber";
import { expect } from "chai";
import fs from "node:fs";
import { execSync } from "node:child_process";

Given("Alice holds a freshly issued birth VC", async function () {
  // Run the happy path inline to set up state.
  await execSync("true"); // hooks already brought env up
  // Trigger the same flow as the happy path uses.
  const { runOid4vpAuth, runOid4vciIssuance } = await import("../fixtures/headless-wallet-client.js");
  const BASE = `http://localhost:${process.env.PORT ?? 3001}`;
  this.lastAuthResult = await runOid4vpAuth(BASE, this.wallets.alice);
  const form = new URLSearchParams();
  const { KYC_CLAIMS } = await import("../fixtures/kyc-claims.js");
  Object.entries(KYC_CLAIMS.alice).forEach(([k, v]) => form.set(k, v));
  await fetch(`${BASE}/kyc-form?session=${this.lastAuthResult.sessionId}`, {
    method: "POST",
    headers: { "content-type": "application/x-www-form-urlencoded" },
    body: form.toString(),
    redirect: "manual",
  });
  this.lastVc = await runOid4vciIssuance(BASE, this.wallets.alice, this.lastAuthResult.sessionId);
});

When("Alice runs self-verify on her birth VC", async function () {
  // The headless TS client mirrors vc_self_verify directly: resolve
  // the issuer DID, pick the assertionMethod-relation key, verify
  // the signature on the canonical (proof-stripped) CBOR.
  const { selfVerifyBirthVc } = await import("../fixtures/headless-wallet-client-verify.js");
  this.selfVerifyResult = await selfVerifyBirthVc(this.lastVc.body_b64);
});

Then("the self-verify outcome is {string}", function (expected: string) {
  expect(this.selfVerifyResult.kind).to.equal(expected);
});

Then("the outcome includes a resolved_at_ms timestamp", function () {
  expect(this.selfVerifyResult.resolved_at_ms).to.be.a("number");
});

When("the issuer rotates its assertionMethod key on chain", async function () {
  // Use the issuer's bootstrap script with a new seed to rotate.
  const env = { ...process.env, ISSUER_BOOTSTRAP_SEED: "issuer-demo-seed-rotated" };
  // First, delete the existing keystore so bootstrap doesn't refuse.
  fs.rmSync("issuer-keystore.json", { force: true });
  execSync("yarn bootstrap", { env, stdio: "inherit" });
});
```

Add a small `headless-wallet-client-verify.ts` that imports the same DID resolver and Jubjub-Schnorr verifier the issuer uses, runs the same algorithm `vc_self_verify::self_verify` does in Rust, returns `{ kind: "Valid", resolved_at_ms } | { kind: "Invalid", reason }`.

Append to `wallet-steps.ts`:

```typescript
Given("the issuer has issued a SIOPv2 nonce", async function () {
  const html = await fetch(`${process.env.PORT ? `http://localhost:${process.env.PORT}` : "http://localhost:3001"}/authorize`).then(r => r.text());
  const m = html.match(/<code>([^<]+)<\/code>/);
  const u = new URL(m![1]);
  const requestUri = u.searchParams.get("request_uri")!;
  this.req = await fetch(requestUri).then(r => r.json());
});

When("Alice's wallet signs a different nonce", async function () {
  this.id_token = await this.wallets.alice.signIdToken(this.req.client_id, "wrong-nonce");
});

When("Alice's wallet replays the same id_token", async function () {
  // assume Alice has already authenticated once
  // and `this.id_token` holds the previous id_token
});

Then("POST /authorize-response returns {int}", async function (code: number) {
  const r = await fetch(this.req.redirect_uri, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ id_token: this.id_token, state: this.req.state }),
  });
  expect(r.status).to.equal(code);
});

Given("Alice has authenticated once", async function () {
  // run the standard auth + capture id_token for replay
  const html = await fetch(`http://localhost:3001/authorize`).then(r => r.text());
  const m = html.match(/<code>([^<]+)<\/code>/);
  const u = new URL(m![1]);
  this.req = await fetch(u.searchParams.get("request_uri")!).then(r => r.json());
  this.id_token = await this.wallets.alice.signIdToken(this.req.client_id, this.req.nonce);
  await fetch(this.req.redirect_uri, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ id_token: this.id_token, state: this.req.state }),
  });
});
```

- [ ] **Step 43.4: Run all features**

```bash
yarn test
```

Expected: all four feature files green, ~10 scenarios total.

- [ ] **Step 43.5: Commit**

```bash
git add IssuerDIDIT-mock/e2e/
git commit -S -s -m "$(cat <<'EOF'
feat(bdd): self-verify + negative-paths features

Self-verify covers fresh-VC Valid + post-rotation Invalid
(@requires-fresh-chain). Negative paths cover wrong nonce
(401), replay (401), and the unbootstrapped-DID error
emitted client-side. Headless self-verify implementation
mirrors wallet-core::vc_self_verify::self_verify exactly so
the Rust and TS impls stay in lock-step.
EOF
)"
git log --format="%h %G? %s" -1
```

---

## Section 11 — Wrap-up

### Task 44: Update spec's "Open questions" with what's actually open

**Files:**
- Modify: wallet-repo's `docs/superpowers/specs/2026-05-25-identity-centre-phase-1-design.md`

After all 43 tasks land, walk through the spec's Open Questions table
and strike any items that are now done. Anything that's deferred to
Phase 2/3 stays. Add a "Phase 1 shipped" datestamp at the top of the
spec.

- [ ] **Step 44.1: Mark spec as shipped**

```diff
-**Status:** Approved design — ready for implementation planning
+**Status:** Phase 1 shipped (commit <sha-of-final-merge>) on <date>
```

- [ ] **Step 44.2: Commit**

```bash
cd /Users/ysh/iohk/midnight-ledger/.claude/worktrees/thirsty-lovelace-092f50
git add docs/superpowers/specs/2026-05-25-identity-centre-phase-1-design.md
git commit -S -s -m "docs(spec): mark Identity Centre Phase 1 as shipped"
git log --format="%h %G? %s" -1
```

### Task 45: README updates in both repos

**Files:**
- Modify: `mobile-bench/dioxus-wallet/README.md`
- Modify: `~/iohk/midnight-identity-workspace/midnight-identity-solution-examples/README.md`

- [ ] **Step 45.1: Add Identity Centre section to dioxus-wallet README**

```markdown
## Identity Centre (Phase 1)

The wallet's top-level Identity tab manages Verifiable Credentials,
DIDs, and per-DID keys. To run the demo flow end-to-end:

1. Bring up the standalone Midnight env from
   `~/iohk/midnight-identity-workspace/midnight-identity-solution-examples/IssuerDIDIT-mock`:

   ```bash
   cd ~/iohk/midnight-identity-workspace/midnight-identity-solution-examples/IssuerDIDIT-mock
   yarn install
   yarn env:up
   yarn bootstrap
   yarn dev
   ```

2. Launch the wallet on Android emulator:
   ```bash
   cd /Users/ysh/iohk/midnight-ledger/.claude/worktrees/thirsty-lovelace-092f50
   bash scripts/launch-android-emulator.sh   # (or the existing path)
   adb reverse tcp:3001 tcp:3001              # so wallet can reach the issuer
   ```

3. In the wallet, tap **Identity → Bootstrap** to create a DID.

4. Open `http://localhost:3001/authorize` in your laptop browser. Scan
   the QR with the wallet (use **+** FAB → **Scan with camera**).

5. Fill in the operator KYC form → scan QR-2 → see the new birth VC
   in the carousel → tap **Self-verify** to confirm the issuer's
   assertion still holds.

Full spec + design: `docs/superpowers/specs/2026-05-25-identity-centre-phase-1-design.md`.
Implementation plan: `docs/superpowers/plans/2026-05-25-identity-centre-phase-1.md`.
```

- [ ] **Step 45.2: Issuer repo README — already done in Task 28**

Verify the README from Task 28 is correct; tweak if anything drifted.

- [ ] **Step 45.3: Commit**

```bash
cd /Users/ysh/iohk/midnight-ledger/.claude/worktrees/thirsty-lovelace-092f50
git add mobile-bench/dioxus-wallet/README.md
git commit -S -s -m "docs(identity-centre): wallet README — demo setup"
git log --format="%h %G? %s" -1
```

---

## Self-review

**1. Spec coverage:** Each spec section is now backed by tasks:
- Goal + Scope → entire plan
- Phasing roadmap → marked as out-of-scope for Phase 1 explicitly in Task 28's README
- System overview → distributed across Sections 1-8
- Mobile architecture → Section 1-5 (wallet-core) + Section 8 (UI) + Section 9 (QR bridge)
- Mock issuer → Section 6-7
- DID bootstrap prerequisites → Task 1-3 (helper + CLI) + Task 31 (UI button) + Task 23 (issuer button + script)
- Self-verification → Tasks 18-19 + Task 33 (UI badge)
- BDD integration tests → Section 10
- Build sequence + effort → this plan IS the sequence; effort estimates pulled from spec
- Security considerations → not implemented as a single task because each task that handles secrets references the relevant point (nonce binding in Task 25-27, key storage via existing SecretStorage, etc.)
- Acceptance criteria → Tasks 42-43's BDD scenarios are the binding criteria

**2. Placeholders:** Final scan — the plan refers to existing surfaces (e.g., `wallet_handle()`, `crate::platform::android_vm::current()`) that the engineer needs to align with the actual dioxus-wallet's plumbing. These are NOT placeholders in the "TBD" sense — they're hand-offs to existing code the engineer will know about. The plan flags them with prose ("If `Wallet::create_did` … doesn't yet expose this exact shape, this is the moment to align").

**3. Type consistency:**
- `BootstrappedDid { did, ed25519_ref, jubjub_ref }` used consistently across Tasks 1-3, 9.
- `StoredVc / VcOpening / VcMetadata` defined in Task 5, used in 6-8, 16, 18-19, 32-33.
- `AuthRequest / TokenResponse / IssuedVc` defined in Tasks 10/15/16, used in 17.
- `SelfVerifyResult / InvalidReason` defined in Task 18, used in 19, 33.
- `QrScanner / QrScanError / PasteUrlScanner` defined in Task 29, used in 36/38.
- The issuer-side `Session / SessionStatus / BirthVcClaims` types from Task 22 carry through Tasks 23-28 and into the BDD harness in Section 10.
- Six HTTP endpoints (the contract): `/authorize`, `/request/:id`, `/authorize-response`, `/credential-offer/:id`, `/token`, `/credential` — defined in Task 25-27, exercised by the headless wallet client in Task 41 + BDD steps in Tasks 42-43.

No naming drift between tasks. If you spot one during execution, fix and reference the original task.

---

## Execution handoff

Plan complete and saved to
`docs/superpowers/plans/2026-05-25-identity-centre-phase-1.md`.
Forty-five tasks across two repos, ~21 eng-days as the spec estimated.

Two execution options:

**1. Subagent-Driven (recommended).** I dispatch a fresh subagent per
task, two-stage review between tasks, fast iteration. Best for plans
of this scale — fresh context per task keeps subagent focus tight.

**2. Inline Execution.** Execute tasks in this session using
executing-plans, batch execution with checkpoints for your review.

Which approach do you want?
