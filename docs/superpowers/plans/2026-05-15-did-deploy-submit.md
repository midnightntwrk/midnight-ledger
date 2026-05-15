# DID Deploy Submission Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Submit a real `ContractDeploy` for our DID contract from the desktop wallet — wait for the extrinsic to land in a block on the configured Midnight network, then surface the on-chain DID id + tx/block hashes in the UI.

**Architecture:** A new `wallet_core::tx` module composes/balances/proves the deploy and forwards it through a new `NodeClient::submit_deploy` that wraps subxt's `Midnight.send_mn_transaction`. A new `wallet_core::dust` module hydrates a `ledger::dust::DustLocalState<DefaultDB>` from the indexer's `dustLedgerEvents` subscription (whose payloads are scale-encoded `Event<D>` blobs) so the balancer can spend real DUST UTXOs. The UI replaces `CreateDidPanel` with a `CreateDidWizard` that consumes a `Stream<WizardStage>` from `Wallet::create_did()`.

**Tech Stack:** Rust 2024 · `tokio` async · `tokio-tungstenite` + the reusable `unshielded::transport::subscribe` (graphql-transport-ws) · `subxt 0.44` + `midnight-node-metadata` (tag `node-0.22.3`) · ledger crate's `Intent`/`StandardTransaction`/`Transaction` builders, `dust::DustLocalState::replay_events`, `prove::tx_prove` · `MidnightDataProvider` (`FetchMode::OnDemand`) for cached DUST proving keys.

**Spec:** `docs/superpowers/specs/2026-05-15-did-deploy-submit-design.md` (commit `1dcf1f93`). See "Spec divergences" at the bottom of this plan.

**Repository conventions:**
- `mobile-bench/wallet-core/src/lib.rs` has `#![deny(warnings)]`. Imports must be used; dead code triggers errors. Use `#[allow(dead_code)]` with a one-line comment for items that get reachable in a later task — same pattern as the unshielded slice.
- Re-exports go through `lib.rs` (`pub use unshielded::{…}` is the existing pattern).
- `pub(crate)` for internal helpers; `pub` only at re-export boundaries.
- Unit tests live in `#[cfg(test)] mod tests { … }` at the bottom of each module.
- All commits MUST use `git commit -S -s -m "…"` (GPG sign + DCO sign-off). After every commit, run `git log --format="%h %G? %s" -1` — must show `G`. On `B`/`N` re-sign once with `git commit --amend --no-edit -S`. Never amend otherwise.
- Run `bash ~/iohk/git-iohk.sh` once at the start of the session.

**Subagent execution note:** Each task closes with a signed commit; the SHA is required for the spec-compliance reviewer that follows. Tasks 8, 9, 10, 11 either touch the network or stitch together heavyweight ledger machinery — those are smoke-tested rather than fully unit-tested. Task 12's live integration test is the real proof point.

---

## File structure

| Path | Role | Status |
|---|---|---|
| `mobile-bench/wallet-core/src/artifacts/mod.rs` | Top-level `artifacts` module | **Create (Task 1)** |
| `mobile-bench/wallet-core/src/artifacts/dust.rs` | `dust_resolver()` factory wrapping `MidnightDataProvider` | **Create (Task 1)** |
| `mobile-bench/wallet-core/src/wallet.rs` | Add `Wallet::dust_secret_key()` + `dust_public_key_hex()` | **Modify (Task 2)** |
| `mobile-bench/wallet-core/src/dust/mod.rs` | `DustError` + re-export `DustLocalState` | **Create (Task 3)** |
| `mobile-bench/wallet-core/queries/midnight-indexer/dust_ledger_events.subscription.graphql` | Subscription document | **Create (Task 4)** |
| `mobile-bench/wallet-core/src/dust/snapshot.rs` | Decoder (`raw` hex → ledger `Event`) + `fold_events` + `snapshot` | **Create (Tasks 4 + 5)** |
| `mobile-bench/wallet-core/src/wallet.rs` | Add `Wallet::sync_dust() -> DustLocalState` | **Modify (Task 5)** |
| `mobile-bench/wallet-core/src/lib.rs` | New module decls + re-exports | **Modify (Tasks 1, 3, 5, 6, 10)** |
| `mobile-bench/wallet-core/src/tx/mod.rs` | `WizardStage`, `DeployOutcome`, `TxError` | **Create (Task 6)** |
| `mobile-bench/wallet-core/src/tx/scale.rs` | `scale_encode(&Transaction) -> Vec<u8>` | **Create (Task 6)** |
| `mobile-bench/wallet-core/src/tx/build.rs` | `build_deploy(...) -> UnprovenTx` | **Create (Task 7)** |
| `mobile-bench/wallet-core/src/tx/balance.rs` | `balance(unproven, &mut DustLocalState, ...)` | **Create (Task 8)** |
| `mobile-bench/wallet-core/src/tx/prove.rs` | `prove(balanced, dust_resolver) -> ProvenTx` | **Create (Task 9)** |
| `mobile-bench/wallet-core/src/node/signer.rs` | `subxt::tx::Signer<SubstrateConfig>` impl | **Modify (Task 10)** |
| `mobile-bench/wallet-core/src/node/client.rs` | `submit_deploy(bytes, &MidnightSigner) -> SubmitResult` | **Modify (Task 10)** |
| `mobile-bench/wallet-core/src/wallet.rs` | Replace `create_did` stub with stream pipeline | **Modify (Task 11)** |
| `mobile-bench/dioxus-wallet/src/app.rs` | Replace `CreateDidPanel` with `CreateDidWizard` | **Modify (Task 11)** |
| `mobile-bench/wallet-core/tests/deploy_undeployed_live.rs` | Live integration test (gated `network-tests`) | **Create (Task 12)** |

---

### Task 1: DUST proving-key resolver

**Files:**
- Create: `mobile-bench/wallet-core/src/artifacts/mod.rs`
- Create: `mobile-bench/wallet-core/src/artifacts/dust.rs`
- Modify: `mobile-bench/wallet-core/Cargo.toml` (verify `ledger` features)
- Modify: `mobile-bench/wallet-core/src/lib.rs`

The DUST spend `.prover/.verifier/.bzkir` aren't in the repo — only hashes are bundled (`DUST_EXPECTED_FILES`). `MidnightDataProvider` (`FetchMode::OnDemand`) fetches them from `srs.midnight.network` on first use and caches at `~/.cache/midnight/zk-params/`.

- [ ] **Step 1.1: Verify `ledger` features**

```
grep '^ledger' mobile-bench/wallet-core/Cargo.toml
```

If `features = ["proving"]` is missing, edit `mobile-bench/wallet-core/Cargo.toml` so the line reads:

```toml
ledger = { path = "../../ledger", package = "midnight-ledger", default-features = false, features = ["proving"] }
```

The `proving` feature gates `ledger::prove::tx_prove` (Task 9).

- [ ] **Step 1.2: Create `artifacts/mod.rs`**

```rust
//! Bundled / cached prover artifacts. See the per-protocol
//! submodule for the specific resolver factory.

pub(crate) mod dust;
```

- [ ] **Step 1.3: Create `artifacts/dust.rs`**

```rust
//! DUST spend prover artifacts.
//!
//! `MidnightDataProvider` ships per-artifact SHA-256 hashes baked
//! into `ledger::dust::DUST_EXPECTED_FILES`. The real bytes
//! (`spend.prover` / `spend.verifier` / `spend.bzkir`) get fetched
//! from `$MIDNIGHT_PARAM_SOURCE` (default
//! `https://srs.midnight.network/`) on first use and cached at
//! `$MIDNIGHT_PP` / `$XDG_CACHE_HOME/midnight/zk-params` /
//! `$HOME/.cache/midnight/zk-params`. The cache is shared with
//! every other midnight tool — dev machines that already ran the
//! upstream test suite have the artifacts in place.

use base_crypto::data_provider::{FetchMode, MidnightDataProvider, OutputMode};
use ledger::dust::{DUST_EXPECTED_FILES, DustResolver};

/// Build a `DustResolver` pointing at the standard cache dir.
/// First call on a fresh machine triggers the one-time download.
#[allow(dead_code)] // Wired by tx::prove in Task 9.
pub(crate) fn dust_resolver() -> std::io::Result<DustResolver> {
    let provider = MidnightDataProvider::new(
        FetchMode::OnDemand,
        OutputMode::Log,
        DUST_EXPECTED_FILES.to_owned(),
    )?;
    Ok(DustResolver(provider))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sanity: the resolver constructs without panicking.
    /// OnDemand mode only fetches when keys are actually resolved,
    /// so this stays offline.
    #[test]
    fn dust_resolver_constructs() {
        let r = dust_resolver().expect("constructs");
        let _ = format!("{:?}", r);
    }
}
```

- [ ] **Step 1.4: Wire into `lib.rs`**

In `mobile-bench/wallet-core/src/lib.rs`, after the existing `mod` block (alphabetically), insert:

```rust
mod artifacts;
```

No public re-export.

- [ ] **Step 1.5: Run the test + cargo check**

```
cargo test -p wallet-core --lib artifacts::dust::tests
cargo check -p wallet-core
```

Expected: 1 test passes. `cargo check` clean.

- [ ] **Step 1.6: Commit**

```bash
bash ~/iohk/git-iohk.sh
git add mobile-bench/wallet-core/Cargo.toml mobile-bench/wallet-core/src/artifacts/ mobile-bench/wallet-core/src/lib.rs
git commit -S -s -m "$(cat <<'EOF'
feat(wallet-core): artifacts::dust — resolver factory for fee proving

Adds the wallet_core::artifacts subdirectory module with a single
factory: artifacts::dust::dust_resolver() returning a DustResolver
backed by MidnightDataProvider in FetchMode::OnDemand.

The DUST spend prover/verifier/bzkir aren't in the repo — only
their hashes are, vendored in ledger::dust::DUST_EXPECTED_FILES.
The bytes get fetched from srs.midnight.network on first use and
cached at ~/.cache/midnight/zk-params/. The cache is shared with
every other midnight tool, so machines that have already run
ledger's test suite have the artifacts ready.

Also enables the ledger crate's `proving` feature (gates
tx_prove for Task 9).

Spec: docs/superpowers/specs/2026-05-15-did-deploy-submit-design.md

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
git log --format="%h %G? %s" -1
```

Expected: `G`.

---

### Task 2: DUST key derivation on `Wallet`

**Files:**
- Modify: `mobile-bench/wallet-core/src/wallet.rs`

Add two helpers: the `DustSecretKey` (consumed by `DustLocalState::replay_events`) and the hex-encoded public key (consumed as the `dustAddress: HexEncoded!` indexer subscription variable).

- [ ] **Step 2.1: Add a failing test in `wallet.rs`'s test module**

Append to the existing `#[cfg(test)] mod tests` at the bottom:

```rust
    #[test]
    fn dust_secret_key_is_deterministic_per_seed() {
        let a = Wallet::demo(Network::Undeployed).dust_secret_key().unwrap();
        let b = Wallet::demo(Network::Undeployed).dust_secret_key().unwrap();
        // DustSecretKey derives via PartialEq on its inner Fr.
        assert_eq!(a, b);
    }

    #[test]
    fn dust_public_key_hex_is_64_chars() {
        let hex = Wallet::demo(Network::Undeployed).dust_public_key_hex().unwrap();
        assert_eq!(hex.len(), 64, "expected 32-byte hex, got {hex}");
        // Must be lowercase hex with no 0x prefix.
        assert!(hex.chars().all(|c| c.is_ascii_hexdigit() && !c.is_uppercase()));
    }
```

- [ ] **Step 2.2: Run the test to verify it fails**

```
cargo test -p wallet-core --lib wallet::tests::dust_
```

Expected: compile error — `dust_secret_key`, `dust_public_key_hex` undefined.

- [ ] **Step 2.3: Add the helpers to `Wallet`**

In `mobile-bench/wallet-core/src/wallet.rs`, find the existing `unshielded_address` method and append directly after its closing brace:

```rust
    /// Derive the DUST secret key for this wallet.
    ///
    /// The seed feeding `ledger::dust::DustSecretKey::derive_secret_key`
    /// is the BIP44 child at `m/44'/2400'/0'/2/0` (account 0, role
    /// Dust, index 0) — same path the upstream wallet SDKs use.
    pub fn dust_secret_key(&self) -> Result<ledger::dust::DustSecretKey, WalletError> {
        let child = crate::hd::derive_child_priv(&self.seed_bytes, 0, crate::hd::Role::Dust, 0)
            .map_err(|e| WalletError::Address(format!("hd: {e}")))?;
        Ok(ledger::dust::DustSecretKey::derive_secret_key(
            &base_crypto::hash::HashOutput(child).into(),
        ))
    }

    /// Hex-encoded 32-byte DUST public key, ready to feed as the
    /// `dustAddress: HexEncoded!` variable of the indexer's
    /// `dustLedgerEvents`-by-owner subscription.
    pub fn dust_public_key_hex(&self) -> Result<String, WalletError> {
        let sk = self.dust_secret_key()?;
        let pk = ledger::dust::DustPublicKey::from(sk);
        let bytes = transient_crypto::fab::fr_to_bytes(pk.0);
        Ok(hex::encode(bytes))
    }
```

The exact conversion path between `[u8;32]` ↔ `Seed` ↔ `DustSecretKey` may differ slightly — `cargo check` will guide you. The minimum correct shape: convert the 32-byte HD child to whatever `derive_secret_key`'s parameter type is. Look at `ledger::dust::DustSecretKey::derive_secret_key`'s signature for the canonical conversion.

- [ ] **Step 2.4: Run tests + cargo check**

```
cargo test -p wallet-core --lib wallet::tests
cargo check -p wallet-core
```

Expected: all wallet tests pass (including the 2 new ones). `cargo check` clean.

- [ ] **Step 2.5: Commit**

```bash
git add mobile-bench/wallet-core/src/wallet.rs
git commit -S -s -m "$(cat <<'EOF'
feat(wallet-core): DUST key derivation on Wallet

Adds two helpers on Wallet:

- `dust_secret_key()` — DustSecretKey derived from the BIP44 child
  at m/44'/2400'/0'/2/0 fed into ledger::dust::DustSecretKey::
  derive_secret_key. Consumed by replay_events in Task 5 and the
  balance spend in Task 8.

- `dust_public_key_hex()` — 32-byte hex of the DustPublicKey,
  ready to feed as the `dustAddress: HexEncoded!` variable of the
  indexer's dust subscriptions.

No bech32m form in this slice — the indexer takes raw hex; the
display surface (BalancesCard's DUST line) lands later.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
git log --format="%h %G? %s" -1
```

Expected: `G`.

---

### Task 3: `DustError` + module wiring

**Files:**
- Create: `mobile-bench/wallet-core/src/dust/mod.rs`
- Modify: `mobile-bench/wallet-core/src/lib.rs`

We use the ledger's `DustLocalState<DefaultDB>` directly — no custom wrapper. This task just adds the error enum, re-exports `DustLocalState`, and stubs the snapshot submodule for Tasks 4 + 5.

- [ ] **Step 3.1: Create `dust/mod.rs`**

```rust
//! DUST UTXO sync. Hydrates a `ledger::dust::DustLocalState` by
//! replaying the indexer's `dustLedgerEvents` stream into it via
//! `DustLocalState::replay_events`.
//!
//! Public entry point is `crate::Wallet::sync_dust()`. The
//! returned state is consumed by the fee balancer in
//! `crate::tx::balance`.

pub(crate) mod snapshot;

#[derive(Debug, thiserror::Error)]
pub enum DustError {
    #[error("ws connect failed: {0}")]
    WsConnect(String),
    #[error("graphql-transport-ws handshake failed: {0}")]
    WsHandshake(String),
    #[error("graphql error frame: {0}")]
    GqlError(String),
    #[error("unexpected ws frame: {0}")]
    UnexpectedFrame(String),
    #[error("decode error: {0}")]
    Decode(String),
    #[error("stream closed before final progress event")]
    StreamClosedEarly,
    #[error("invalid dust public key: {0}")]
    InvalidPublicKey(String),
    #[error("replay events: {0}")]
    Replay(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verifies DustError carries enough context to render. The
    /// real DUST-state ops live on `ledger::dust::DustLocalState`
    /// (re-exported via lib.rs) — no point retesting them here.
    #[test]
    fn error_variants_format() {
        let e = DustError::WsConnect("boom".into());
        assert!(format!("{e}").contains("boom"));
    }
}
```

- [ ] **Step 3.2: Create the snapshot stub**

```rust
//! DUST sync driver — filled in by Tasks 4 + 5.
```

at `mobile-bench/wallet-core/src/dust/snapshot.rs`.

- [ ] **Step 3.3: Wire into `lib.rs`**

In `mobile-bench/wallet-core/src/lib.rs`, add the module declaration (alphabetically after `mod did;`, before `mod hd;`):

```rust
mod dust;
```

Add a new re-export block:

```rust
pub use dust::DustError;
pub use ledger::dust::{DustLocalState, DustSecretKey, DustPublicKey};
```

- [ ] **Step 3.4: Run tests + cargo check**

```
cargo test -p wallet-core --lib dust::tests
cargo check -p wallet-core
```

Expected: 1 test passes. `cargo check` clean.

- [ ] **Step 3.5: Commit**

```bash
git add mobile-bench/wallet-core/src/dust/ mobile-bench/wallet-core/src/lib.rs
git commit -S -s -m "$(cat <<'EOF'
feat(wallet-core): dust module skeleton — DustError + ledger re-exports

Adds the wallet_core::dust subdirectory module with just the
DustError enum. The actual DUST UTXO type is the ledger crate's
DustLocalState<DefaultDB> — re-exported through lib.rs so
downstream tasks (tx::balance in Task 8) can consume it
directly. No custom wrapper, no parallel state-machine.

Submodule `snapshot` exists as a stub; Tasks 4 + 5 fill it in.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
git log --format="%h %G? %s" -1
```

Expected: `G`.

---

### Task 4: Subscription doc + raw-event decoder

**Files:**
- Create: `mobile-bench/wallet-core/queries/midnight-indexer/dust_ledger_events.subscription.graphql`
- Modify: `mobile-bench/wallet-core/src/dust/snapshot.rs` (replace stub)

The indexer's `dustLedgerEvents(id: Int)` subscription emits a `DustLedgerEvent!` for each event. Every variant carries a `raw: HexEncoded!` field — the scale-encoded `ledger::events::Event<D>` blob. We hex-decode and `tagged_deserialize` to get the ledger's concrete `Event<DefaultDB>` back.

- [ ] **Step 4.1: Write the subscription**

Create `mobile-bench/wallet-core/queries/midnight-indexer/dust_ledger_events.subscription.graphql`:

```graphql
subscription DustLedgerEvents($id: Int) {
  dustLedgerEvents(id: $id) {
    __typename
    id
    maxId
    raw
  }
}
```

We don't need the typed variant fields — `raw` is sufficient. `id` and `maxId` drive termination (we stop when we've seen an event whose `id == maxId`).

- [ ] **Step 4.2: Replace `dust/snapshot.rs` with the decoder skeleton**

```rust
//! Snapshot driver: subscribe to `dustLedgerEvents`, decode each
//! into a `ledger::events::Event<DefaultDB>` via the `raw` field,
//! collect them, then call `DustLocalState::replay_events` to
//! hydrate the wallet's DUST state. See the design spec for the
//! termination semantics.

use serde_json::Value;
use storage::DefaultDB;

use super::DustError;

#[allow(dead_code)] // Used by snapshot (Task 5) and tests.
pub(super) const DUST_LEDGER_EVENTS_QUERY: &str = include_str!(
    "../../queries/midnight-indexer/dust_ledger_events.subscription.graphql"
);

/// One decoded element of the subscription stream. The ledger's
/// `Event<D>` carries the actual variant — we keep our outer
/// envelope minimal (just the id we need for termination).
#[allow(dead_code)] // Used by snapshot (Task 5) and tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DecodedEvent {
    pub id: i64,
    pub max_id: i64,
    pub event: ledger::events::Event<DefaultDB>,
}

/// Decode one `next.payload.data.dustLedgerEvents` JSON value.
#[allow(dead_code)] // Used by snapshot (Task 5) and tests.
pub(super) fn decode_event(data: &Value) -> Result<DecodedEvent, DustError> {
    let obj = data
        .get("dustLedgerEvents")
        .ok_or_else(|| DustError::Decode("missing dustLedgerEvents".into()))?;
    let id = obj
        .get("id")
        .and_then(Value::as_i64)
        .ok_or_else(|| DustError::Decode("missing id".into()))?;
    let max_id = obj
        .get("maxId")
        .and_then(Value::as_i64)
        .ok_or_else(|| DustError::Decode("missing maxId".into()))?;
    let raw_hex = obj
        .get("raw")
        .and_then(Value::as_str)
        .ok_or_else(|| DustError::Decode("missing raw".into()))?;
    let raw_bytes = hex::decode(raw_hex.trim_start_matches("0x"))
        .map_err(|e| DustError::Decode(format!("raw hex: {e}")))?;
    let event: ledger::events::Event<DefaultDB> =
        serialize::tagged_deserialize(&raw_bytes[..])
            .map_err(|e| DustError::Decode(format!("raw tagged: {e}")))?;
    Ok(DecodedEvent { id, max_id, event })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// We can't easily synthesise a valid scale-encoded
    /// ledger::events::Event<D> in a unit test without dragging
    /// in the full ledger machinery, so the decoder is exercised
    /// end-to-end against the live indexer in Task 12. Here we
    /// only test the JSON-walking layer with bad payloads.

    #[test]
    fn decode_missing_root_errors() {
        let data = json!({});
        let err = decode_event(&data).unwrap_err();
        assert!(matches!(err, DustError::Decode(_)));
    }

    #[test]
    fn decode_missing_id_errors() {
        let data = json!({
            "dustLedgerEvents": {
                "__typename": "DustInitialUtxo",
                "maxId": 10,
                "raw": "00"
            }
        });
        let err = decode_event(&data).unwrap_err();
        match err {
            DustError::Decode(msg) => assert!(msg.contains("id"), "msg={msg}"),
            other => panic!("expected Decode, got {other:?}"),
        }
    }

    #[test]
    fn decode_bad_raw_hex_errors() {
        let data = json!({
            "dustLedgerEvents": {
                "__typename": "DustInitialUtxo",
                "id": 0,
                "maxId": 0,
                "raw": "not-hex"
            }
        });
        let err = decode_event(&data).unwrap_err();
        match err {
            DustError::Decode(msg) => assert!(msg.contains("raw hex"), "msg={msg}"),
            other => panic!("expected Decode, got {other:?}"),
        }
    }
}
```

- [ ] **Step 4.3: Run tests + cargo check**

```
cargo test -p wallet-core --lib dust::snapshot::tests
cargo check -p wallet-core
```

Expected: 3 tests pass. `cargo check` clean.

- [ ] **Step 4.4: Commit**

```bash
git add mobile-bench/wallet-core/queries/midnight-indexer/dust_ledger_events.subscription.graphql mobile-bench/wallet-core/src/dust/snapshot.rs
git commit -S -s -m "$(cat <<'EOF'
feat(wallet-core): dust sync — subscription doc + raw-event decoder

Adds the dustLedgerEvents subscription document (the variant-
agnostic version — every variant carries id/maxId/raw via the
DustLedgerEvent interface) and the JSON walker that turns each
event into a (id, max_id, ledger::events::Event<DefaultDB>) tuple.

Each event's `raw: HexEncoded!` is the scale-encoded form of the
ledger-side concrete Event type; we hex-decode and
tagged_deserialize to get back the typed enum. No duplication of
the ledger's variant encoding in the wallet.

Three unit tests cover negative paths (missing root, missing id,
bad raw hex). Happy-path decoding requires a real Event blob,
exercised end-to-end in Task 12.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
git log --format="%h %G? %s" -1
```

Expected: `G`.

---

### Task 5: `Wallet::sync_dust` driver

**Files:**
- Modify: `mobile-bench/wallet-core/src/dust/snapshot.rs` (append `fold_events` + `snapshot`)
- Modify: `mobile-bench/wallet-core/src/wallet.rs` (add `sync_dust` method)

Open the subscription, collect events in id-order until we've seen `id == max_id`, then call `DustLocalState::replay_events`.

- [ ] **Step 5.1: Append `fold_events` + `snapshot` to `dust/snapshot.rs`**

Add imports at the top:

```rust
use futures::{Stream, StreamExt};
use ledger::dust::DustLocalState;
use serde_json::json;

use crate::unshielded::transport;
```

Append (after the existing decoder code, before the `#[cfg(test)] mod tests` block):

```rust
const IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Fold the event stream into an ordered Vec<ledger::events::Event>,
/// then replay against an empty `DustLocalState` to produce the
/// hydrated state. Termination: stop after seeing an event with
/// `id == max_id` (the indexer's own "you're caught up" marker).
/// Idle timeout (5s) is the belt-and-braces backstop.
pub(super) async fn fold_events<S>(
    stream: S,
    sk: &ledger::dust::DustSecretKey,
    params: ledger::dust::DustParameters,
) -> Result<DustLocalState<storage::DefaultDB>, DustError>
where
    S: Stream<Item = Result<DecodedEvent, DustError>>,
{
    let mut events: Vec<ledger::events::Event<storage::DefaultDB>> = Vec::new();
    let mut last_id: i64 = -1;
    let mut target_max: Option<i64> = None;
    futures::pin_mut!(stream);

    loop {
        // Early exit on caught-up.
        if let Some(max) = target_max {
            if last_id >= max {
                break;
            }
        }

        let next = tokio::time::timeout(IDLE_TIMEOUT, stream.next()).await;
        match next {
            Ok(Some(item)) => {
                let DecodedEvent { id, max_id, event } = item?;
                events.push(event);
                last_id = id;
                target_max = Some(max_id);
            }
            Ok(None) => {
                if target_max.is_some() {
                    break;
                }
                return Err(DustError::StreamClosedEarly);
            }
            Err(_) => {
                if target_max.is_some() {
                    break;
                }
                return Err(DustError::StreamClosedEarly);
            }
        }
    }

    let state = DustLocalState::new(params);
    state
        .replay_events(sk, events.iter())
        .map_err(|e| DustError::Replay(e.to_string()))
}

/// Open a subscription, fold, hydrate. Caller supplies the dust
/// secret key + params.
pub(crate) async fn snapshot(
    ws_url: &str,
    sk: &ledger::dust::DustSecretKey,
    params: ledger::dust::DustParameters,
) -> Result<DustLocalState<storage::DefaultDB>, DustError> {
    let stream = transport::subscribe(
        ws_url,
        DUST_LEDGER_EVENTS_QUERY,
        json!({ "id": 0 }),
    )
    .await
    .map_err(translate_unshielded_error)?;

    let events = stream.map(|item| {
        item.map_err(translate_unshielded_error)
            .and_then(|v| decode_event(&v))
    });
    fold_events(events, sk, params).await
}

fn translate_unshielded_error(e: crate::unshielded::UnshieldedError) -> DustError {
    use crate::unshielded::UnshieldedError as U;
    match e {
        U::WsConnect(s) => DustError::WsConnect(s),
        U::WsHandshake(s) => DustError::WsHandshake(s),
        U::GqlError(s) => DustError::GqlError(s),
        U::UnexpectedFrame(s) => DustError::UnexpectedFrame(s),
        U::Decode(s) => DustError::Decode(s),
        U::StreamClosedEarly => DustError::StreamClosedEarly,
        U::InvalidAddress(s) => DustError::InvalidPublicKey(s),
    }
}
```

- [ ] **Step 5.2: Add a `fold_events` unit test**

In the existing `mod tests` block, append:

```rust
    use futures::stream;

    /// Hand-built stream of one Progress-like event; verify the
    /// fold terminates and returns an empty state. We can't feed
    /// a real ledger Event<D> here without major fixture setup,
    /// so use the empty-history path: id == max_id == -1 means
    /// caught up immediately.
    #[tokio::test]
    async fn fold_returns_empty_state_when_immediately_caught_up() {
        let events: Vec<Result<DecodedEvent, DustError>> = vec![];
        let mut rng = rand::rngs::OsRng;
        let sk = ledger::dust::DustSecretKey::sample(&mut rng);
        let params = ledger::structure::INITIAL_PARAMETERS.dust;
        let result = fold_events(stream::iter(events), &sk, params).await;
        // Empty stream + no progress observed = StreamClosedEarly.
        assert!(matches!(result, Err(DustError::StreamClosedEarly)));
    }
```

The happy-path fold against real events runs in Task 12's live integration test (we can't synthesise a real `Event<D>` here without dragging in `apply_system_tx`).

- [ ] **Step 5.3: Add `Wallet::sync_dust`**

In `mobile-bench/wallet-core/src/wallet.rs`, after the existing `sync_unshielded` method, append:

```rust
    /// Snapshot the wallet's DUST state by replaying the
    /// indexer's `dustLedgerEvents` stream into a fresh
    /// `DustLocalState`. The returned state is consumed by
    /// `tx::balance` to cover deploy/call fees.
    pub async fn sync_dust(&self) -> Result<crate::DustLocalState, crate::DustError> {
        let sk = self
            .dust_secret_key()
            .map_err(|e| crate::DustError::InvalidPublicKey(e.to_string()))?;
        let cfg = self.network.config();
        let params = ledger::structure::INITIAL_PARAMETERS.dust;
        crate::dust::snapshot::snapshot(cfg.indexer_ws_url, &sk, params).await
    }
```

- [ ] **Step 5.4: Run tests + cargo check**

```
cargo test -p wallet-core --lib dust
cargo check -p wallet-core
```

Expected: 5 dust tests pass (1 from Task 3 + 3 from Task 4 + 1 here). `cargo check` clean. Drop any `#[allow(dead_code)]` markers that are now unreachable (the snapshot chain is called from `Wallet::sync_dust`).

- [ ] **Step 5.5: Commit**

```bash
git add mobile-bench/wallet-core/src/dust/snapshot.rs mobile-bench/wallet-core/src/wallet.rs
git commit -S -s -m "$(cat <<'EOF'
feat(wallet-core): dust sync — replay_events driver + Wallet::sync_dust

Adds snapshot() + fold_events() in dust/snapshot.rs. Walks the
indexer's dustLedgerEvents subscription, collects each raw event
into a Vec<ledger::events::Event>, then calls
DustLocalState::replay_events to hydrate the wallet's local DUST
view.

Termination tracks last_id vs max_id (indexer's own caught-up
marker) with an idle-timeout backstop matching the unshielded
sync's belt-and-braces pattern.

Wallet::sync_dust() exposes it: derives the DustSecretKey, reads
the indexer URL from NetworkConfig, uses INITIAL_PARAMETERS.dust
for the chain's dust parameters (sufficient for Undeployed/PreProd
at this protocol version).

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
git log --format="%h %G? %s" -1
```

Expected: `G`.

---

### Task 6: `tx` module skeleton + SCALE encoder

**Files:**
- Create: `mobile-bench/wallet-core/src/tx/mod.rs`
- Create: `mobile-bench/wallet-core/src/tx/scale.rs`
- Modify: `mobile-bench/wallet-core/src/lib.rs`

- [ ] **Step 6.1: Create `tx/mod.rs`**

```rust
//! Transaction build + balance + prove + submit pipeline for
//! DID deploys.
//!
//!   `build`   → compose an unproven `Transaction::Standard`
//!   `balance` → cover DUST fees from a `DustLocalState`
//!   `prove`   → wrap `ledger::prove::tx_prove`
//!   `scale`   → `Transaction → Vec<u8>` for send_mn_transaction
//!
//! Public API is the `WizardStage` stream emitted by
//! `Wallet::create_did()` (Task 11).

pub(crate) mod scale;

use crate::DidId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeployOutcome {
    pub did_id: DidId,
    pub tx_hash: [u8; 32],
    pub block_hash: [u8; 32],
}

#[derive(Debug, Clone)]
pub enum WizardStage {
    SyncingDust,
    Composing,
    Balancing,
    Proving,
    Submitting,
    Confirming,
    Done(DeployOutcome),
    Failed(String),
}

#[derive(Debug, thiserror::Error)]
pub enum TxError {
    #[error("dust sync: {0}")]
    Dust(#[from] crate::DustError),
    #[error("compose: {0}")]
    Compose(String),
    #[error("balance: {0}")]
    Balance(String),
    #[error("prove: {0}")]
    Prove(String),
    #[error("scale encode: {0}")]
    ScaleEncode(String),
    #[error("submit: {0}")]
    Submit(String),
}
```

- [ ] **Step 6.2: Create `tx/scale.rs`**

```rust
//! SCALE-encode a fully-proven `Transaction` into the byte form
//! `Midnight.send_mn_transaction` expects. Uses ledger's
//! tagged_serialize — the same encoding the indexer surfaces
//! under `Transaction.raw: HexEncoded` in schema-v4.

use serialize::tagged_serialize;

use super::TxError;

#[allow(dead_code)] // Wired by Wallet::create_did in Task 11.
pub(crate) fn scale_encode<T: serde::Serialize + serialize::Tagged + std::fmt::Debug>(
    tx: &T,
) -> Result<Vec<u8>, TxError> {
    let mut buf = Vec::new();
    tagged_serialize(tx, &mut buf)
        .map_err(|e| TxError::ScaleEncode(e.to_string()))?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_a_contract_deploy() {
        use crate::did::deploy::compose_deploy;

        let pk = [0xabu8; 32];
        let ts = 1_777_840_000_000u64;
        let nonce = [0x99u8; 32];
        let deploy = compose_deploy(pk, ts, nonce);

        let bytes = scale_encode(&deploy).expect("encode");
        assert!(!bytes.is_empty(), "produced empty bytes");

        let back: ledger::structure::ContractDeploy<storage::DefaultDB> =
            serialize::tagged_deserialize(&bytes[..]).expect("round-trip");
        assert_eq!(deploy.address().0.0, back.address().0.0);
    }
}
```

- [ ] **Step 6.3: Wire into `lib.rs`**

Module decl (alphabetically after `mod probe;`):

```rust
mod tx;
```

Re-exports:

```rust
pub use tx::{DeployOutcome, TxError, WizardStage};
```

- [ ] **Step 6.4: Run tests + cargo check**

```
cargo test -p wallet-core --lib tx::scale::tests
cargo check -p wallet-core
```

Expected: 1 test passes. `cargo check` clean. If `WizardStage`/`DeployOutcome`/`TxError` flag as dead code, add `#[allow(dead_code)]` with `// Wired by Wallet::create_did in Task 11.`

- [ ] **Step 6.5: Commit**

```bash
git add mobile-bench/wallet-core/src/tx/ mobile-bench/wallet-core/src/lib.rs
git commit -S -s -m "$(cat <<'EOF'
feat(wallet-core): tx module skeleton + SCALE encoder

Adds wallet_core::tx with three public types (WizardStage,
DeployOutcome, TxError) and the scale_encode helper. Generic over
Tagged + Serialize so it'll encode both unit-test fixtures
(ContractDeploy) and the proven Transaction once Task 9 lands.
Round-trip via tagged_deserialize verifies the bytes parse back.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
git log --format="%h %G? %s" -1
```

Expected: `G`.

---

### Task 7: `tx::build_deploy`

**Files:**
- Create: `mobile-bench/wallet-core/src/tx/build.rs`
- Modify: `mobile-bench/wallet-core/src/tx/mod.rs`

- [ ] **Step 7.1: Create `tx/build.rs`**

```rust
//! Build an unproven deploy transaction. Pure function — no I/O,
//! decoupled from Wallet so the caller (Wallet::create_did in
//! Task 11) supplies the inputs.

use base_crypto::signatures::Schnorr;
use base_crypto::time::Timestamp;
use ledger::construct::Intent;
use ledger::structure::{StandardTransaction, Transaction};
use rand::{CryptoRng, Rng};
use storage::DefaultDB;
use storage::storage::HashMap;
use transient_crypto::commitment::PedersenRandomness;
use transient_crypto::proofs::ProofPreimageMarker;

use crate::did::deploy::compose_deploy;
use super::TxError;

/// Segment id for the guaranteed (always-applied) portion of the
/// transaction. Matches `ledger::construct::GUARANTEED_SEGMENT`.
const GUARANTEED_SEGMENT: u16 = 0;

pub(crate) type UnprovenTx = Transaction<
    Schnorr,
    ProofPreimageMarker,
    PedersenRandomness,
    DefaultDB,
>;

#[allow(dead_code)] // Wired by Wallet::create_did in Task 11.
pub(crate) fn build_deploy<R: Rng + CryptoRng>(
    pk_commitment: [u8; 32],
    network_id: &str,
    timestamp_ms: u64,
    nonce: [u8; 32],
    ttl: Timestamp,
    rng: &mut R,
) -> Result<UnprovenTx, TxError> {
    let deploy = compose_deploy(pk_commitment, timestamp_ms, nonce);

    let intent: Intent<Schnorr, ProofPreimageMarker, PedersenRandomness, DefaultDB> =
        Intent::empty(rng, ttl);
    let intent = intent.add_deploy(deploy);

    let mut intents = HashMap::new();
    intents = intents.insert(GUARANTEED_SEGMENT, intent);

    let stx = StandardTransaction::new(network_id, intents, None, HashMap::new());
    Ok(Transaction::Standard(stx))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand_chacha::ChaCha20Rng;

    #[test]
    fn builds_a_standard_transaction_with_one_intent() {
        let mut rng = ChaCha20Rng::seed_from_u64(0x42);
        let pk = [0xabu8; 32];
        let now = 1_777_840_000_000u64;
        let nonce = [0x99u8; 32];
        let ttl = Timestamp::from_secs(now / 1000 + 3600);

        let tx = build_deploy(pk, "undeployed", now, nonce, ttl, &mut rng)
            .expect("build");

        match &tx {
            Transaction::Standard(stx) => {
                assert_eq!(stx.intents().count(), 1);
                assert_eq!(stx.network_id.as_str(), "undeployed");
                let deploys: Vec<_> = stx
                    .intents()
                    .flat_map(|(_, intent)| intent.deploys().collect::<Vec<_>>())
                    .collect();
                assert_eq!(deploys.len(), 1);
            }
            _ => panic!("expected Transaction::Standard"),
        }
    }

    #[test]
    fn build_is_deterministic_per_inputs() {
        let mut rng_a = ChaCha20Rng::seed_from_u64(0x42);
        let mut rng_b = ChaCha20Rng::seed_from_u64(0x42);
        let pk = [0xabu8; 32];
        let now = 1_777_840_000_000u64;
        let nonce = [0x99u8; 32];
        let ttl = Timestamp::from_secs(now / 1000 + 3600);

        let a = build_deploy(pk, "undeployed", now, nonce, ttl, &mut rng_a).unwrap();
        let b = build_deploy(pk, "undeployed", now, nonce, ttl, &mut rng_b).unwrap();

        let mut ba = Vec::new();
        let mut bb = Vec::new();
        serialize::tagged_serialize(&a, &mut ba).unwrap();
        serialize::tagged_serialize(&b, &mut bb).unwrap();
        assert_eq!(ba, bb);
    }
}
```

- [ ] **Step 7.2: Wire into `tx/mod.rs`**

After the existing `pub(crate) mod scale;` line:

```rust
pub(crate) mod build;
```

- [ ] **Step 7.3: Run tests + cargo check**

```
cargo test -p wallet-core --lib tx::build::tests
cargo check -p wallet-core
```

Expected: 2 tests pass. `cargo check` clean. If the type-alias imports (`Schnorr`, `Intent`, etc.) trip — compile error tells you the path. Adjust mechanically.

- [ ] **Step 7.4: Commit**

```bash
git add mobile-bench/wallet-core/src/tx/build.rs mobile-bench/wallet-core/src/tx/mod.rs
git commit -S -s -m "$(cat <<'EOF'
feat(wallet-core): tx::build_deploy

Composes an unproven Transaction::Standard from the controller-
pubkey commitment, network id, timestamp, nonce, and ttl. Wraps
Phase 3's compose_deploy() output in a single Intent under the
guaranteed (segment-0) slot. No fallible section, no shielded
coins, no contract calls.

Decoupled from Wallet — tests feed synthetic inputs. Two tests:
happy path + determinism (same seed + same inputs → same SCALE
bytes).

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
git log --format="%h %G? %s" -1
```

Expected: `G`.

---

### Task 8: `tx::balance` — port DUST balancer

**Files:**
- Create: `mobile-bench/wallet-core/src/tx/balance.rs`
- Modify: `mobile-bench/wallet-core/src/tx/mod.rs`

Iteratively spend DUST UTXOs from a caller-supplied `DustLocalState` until the tx's `(Dust, 0)` slot is non-negative. Ported from `TestState::balance_tx`'s DUST branch (`ledger/src/test_utilities.rs:572-643`), stripped of zswap/shielded paths.

- [ ] **Step 8.1: Create `tx/balance.rs`**

```rust
//! Cover the DUST fees of an unproven deploy by spending UTXOs
//! from the wallet's DustLocalState. Ported from
//! `ledger::test_utilities::TestState::balance_tx`'s DUST branch
//! (test_utilities.rs:572-643), simplified to the deploy case:
//! no shielded coins, no fallible segments.

use base_crypto::signatures::Schnorr;
use base_crypto::time::Timestamp;
use coin_structure::coin::TokenType;
use ledger::construct::Intent;
use ledger::dust::{DustActions, DustLocalState, DustSecretKey};
use ledger::structure::{LedgerParameters, StandardTransaction, Transaction};
use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;
use storage::DefaultDB;
use storage::arena::Sp;
use storage::storage::{Array, HashMap};
use transient_crypto::commitment::PedersenRandomness;
use transient_crypto::proofs::ProofPreimageMarker;

use super::TxError;
use super::build::UnprovenTx;

const GUARANTEED_SEGMENT: u16 = 0;

pub(crate) struct BalanceCtx<'a> {
    pub dust_state: &'a mut DustLocalState<DefaultDB>,
    pub dust_key: &'a DustSecretKey,
    pub params: &'a LedgerParameters<DefaultDB>,
    pub time: Timestamp,
    pub network_id: &'a str,
}

#[allow(dead_code)] // Wired by Wallet::create_did in Task 11.
pub(crate) fn balance(
    mut tx: UnprovenTx,
    ctx: &mut BalanceCtx<'_>,
) -> Result<UnprovenTx, TxError> {
    let mut rng = ChaCha20Rng::seed_from_u64(0);
    let mut last_dust: u128 = 0;

    loop {
        let fees = tx
            .fees(ctx.params, false)
            .map_err(|e| TxError::Balance(format!("fees: {e}")))?;
        let balance_map = tx
            .balance(Some(fees))
            .map_err(|e| TxError::Balance(format!("balance: {e}")))?;
        let dust_short = balance_map
            .get(&(TokenType::Dust, 0))
            .and_then(|v| (*v < 0).then_some((-*v) as u128))
            .unwrap_or(0);
        if dust_short == 0 {
            return Ok(tx);
        }

        let dust_to_cover = dust_short + last_dust;
        last_dust = dust_to_cover;

        let mut spends = Array::new();
        let utxos: Vec<_> = ctx.dust_state.utxos().collect();
        let mut remaining = dust_to_cover;
        for qdo in utxos {
            if remaining == 0 {
                break;
            }
            let gen_info = ctx
                .dust_state
                .generation_info(&qdo)
                .ok_or_else(|| TxError::Balance("missing generation info".into()))?;
            let current_value = ledger::dust::DustOutput::from(qdo.clone()).updated_value(
                &gen_info,
                ctx.time,
                &ctx.params.dust,
            );
            let v = u128::min(current_value, remaining);
            remaining = remaining.saturating_sub(current_value);
            let (next_state, spend) = ctx
                .dust_state
                .clone()
                .spend(ctx.dust_key, &qdo, v, ctx.time)
                .map_err(|e| TxError::Balance(format!("dust spend: {e}")))?;
            *ctx.dust_state = next_state;
            spends = spends.push(spend);
        }
        if remaining > 0 {
            return Err(TxError::Balance(format!(
                "insufficient DUST: short by {remaining} atomic units"
            )));
        }

        let mut intent: Intent<Schnorr, ProofPreimageMarker, PedersenRandomness, DefaultDB> =
            Intent::empty(&mut rng, ctx.time);
        intent.dust_actions = Some(Sp::new(DustActions {
            spends,
            registrations: Array::new(),
            ctime: ctx.time,
        }));
        let mut intents = HashMap::new();
        intents = intents.insert(GUARANTEED_SEGMENT, intent);
        let merge_with = Transaction::Standard(StandardTransaction::new(
            ctx.network_id,
            intents,
            None,
            HashMap::new(),
        ));
        tx = tx
            .merge(&merge_with)
            .map_err(|e| TxError::Balance(format!("merge dust intent: {e}")))?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Compile-only typecheck. The real exercise is Task 12's
    /// live integration test — synthesising a populated
    /// DustLocalState fixture isn't worth the code at this layer.
    #[test]
    fn signature_typechecks() {
        let _: fn(UnprovenTx, &mut BalanceCtx<'_>) -> Result<UnprovenTx, TxError> = balance;
    }
}
```

- [ ] **Step 8.2: Wire into `tx/mod.rs`**

```rust
pub(crate) mod balance;
```

- [ ] **Step 8.3: Run + commit**

```
cargo test -p wallet-core --lib tx::balance::tests
cargo check -p wallet-core
```

Expected: 1 test passes, clean.

```bash
git add mobile-bench/wallet-core/src/tx/balance.rs mobile-bench/wallet-core/src/tx/mod.rs
git commit -S -s -m "$(cat <<'EOF'
feat(wallet-core): tx::balance — port DUST balancer from test_utilities

Ports the DUST branch of TestState::balance_tx (ledger/src/
test_utilities.rs:572-643) into a wallet-side balance() that
takes a BalanceCtx pointing at a real DustLocalState. Iteratively
spends UTXOs until the tx's (Dust, 0) imbalance is covered;
errors out cleanly when DUST is exhausted.

Stripped of the shielded-zswap branch (deploys don't use zswap),
the TestProcessingMode debug paths, and unwraps.

Typecheck-only test — synthesising a populated DustLocalState
fixture at unit-test layer is expensive and Task 12's live
integration test exercises this path end-to-end.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
git log --format="%h %G? %s" -1
```

Expected: `G`.

---

### Task 9: `tx::prove` — wrap `tx_prove`

**Files:**
- Create: `mobile-bench/wallet-core/src/tx/prove.rs`
- Modify: `mobile-bench/wallet-core/src/tx/mod.rs`

- [ ] **Step 9.1: Create `tx/prove.rs`**

```rust
//! Generate ZK proofs for the DUST spend offers added during
//! balancing. The deploy itself carries no proof preimages —
//! ContractDeploy's payload is `(initial_state, nonce)` — but
//! each DUST spend the balancer added is a ProofPreimage that
//! must become a Proof before SCALE encoding.

use base_crypto::rng::SplittableRng;
use base_crypto::signatures::Schnorr;
use ledger::prove::{Resolver, tx_prove};
use ledger::structure::Transaction;
use rand::{CryptoRng, Rng};
use storage::DefaultDB;
use transient_crypto::commitment::Pedersen;
use transient_crypto::proofs::ProofMarker;

use super::TxError;
use super::build::UnprovenTx;

pub(crate) type ProvenTx = Transaction<Schnorr, ProofMarker, Pedersen, DefaultDB>;

#[allow(dead_code)] // Wired by Wallet::create_did in Task 11.
pub(crate) async fn prove<R: Rng + CryptoRng + SplittableRng>(
    tx: UnprovenTx,
    rng: R,
) -> Result<ProvenTx, TxError> {
    let resolver = Resolver::new(
        transient_crypto::proofs::PARAMS_PROVER_PROVIDER.clone(),
        crate::artifacts::dust::dust_resolver()
            .map_err(|e| TxError::Prove(format!("dust resolver: {e}")))?,
        Box::new(|_loc| Box::pin(std::future::ready(Ok(None)))),
    );
    tx_prove(rng, &tx, &resolver)
        .await
        .map_err(|e| TxError::Prove(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_typechecks() {
        fn _check<R: Rng + CryptoRng + SplittableRng>() {
            let _ = prove::<R>;
        }
    }
}
```

The `PARAMS_PROVER_PROVIDER` path is the workspace's public-params provider. If the symbol is at a different path (search with `grep -rn 'PARAMS_PROVER_PROVIDER\|fn params_prover' transient-crypto/`), adjust accordingly.

- [ ] **Step 9.2: Wire into `tx/mod.rs`**

```rust
pub(crate) mod prove;
```

- [ ] **Step 9.3: Run + commit**

```
cargo test -p wallet-core --lib tx::prove::tests
cargo check -p wallet-core
```

Expected: 1 test passes. `cargo check` clean. If `PARAMS_PROVER_PROVIDER`'s path differs, the compile error points to the fix.

```bash
git add mobile-bench/wallet-core/src/tx/prove.rs mobile-bench/wallet-core/src/tx/mod.rs
git commit -S -s -m "$(cat <<'EOF'
feat(wallet-core): tx::prove — wrap tx_prove with bundled DUST resolver

Wraps ledger::prove::tx_prove with our artifacts::dust::dust_resolver()
factory. The deploy-only flow's only proof preimages come from
DUST spend offers added by tx::balance; the external_resolver
returns None for every key location since DID write circuits
(which would need their own proving keys) are out of scope.

First call on a fresh machine triggers the one-time download of
DUST spend artifacts via MidnightDataProvider's OnDemand mode.
Subsequent calls hit the local cache.

Typecheck-only test; live integration in Task 12 is the proof
point.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
git log --format="%h %G? %s" -1
```

Expected: `G`.

---

### Task 10: subxt `Signer` impl + `submit_deploy`

**Files:**
- Modify: `mobile-bench/wallet-core/src/node/signer.rs`
- Modify: `mobile-bench/wallet-core/src/node/client.rs`
- Modify: `mobile-bench/wallet-core/src/lib.rs`

- [ ] **Step 10.1: Add `subxt::tx::Signer` impl to `signer.rs`**

In `mobile-bench/wallet-core/src/node/signer.rs`, add the imports near the top:

```rust
use subxt::tx::Signer;
use subxt::{Config, SubstrateConfig};
```

After the existing `impl MidnightSigner { … }` block, append:

```rust
impl Signer<SubstrateConfig> for MidnightSigner {
    fn account_id(&self) -> <SubstrateConfig as Config>::AccountId {
        subxt::utils::AccountId32(self.account_id_bytes)
    }

    fn address(&self) -> <SubstrateConfig as Config>::Address {
        self.account_id().into()
    }

    fn sign(&self, payload: &[u8]) -> <SubstrateConfig as Config>::Signature {
        let sig_bytes = self.sign_envelope(payload);
        let ecdsa_sig = subxt::ext::sp_core::ecdsa::Signature::from_raw(sig_bytes);
        subxt::ext::sp_runtime::MultiSignature::Ecdsa(ecdsa_sig)
    }
}
```

The exact paths under `subxt::ext::sp_runtime` / `subxt::utils` / `subxt::ext::sp_core` may differ slightly in 0.44 — `cargo check` will guide. Adjust mechanically.

- [ ] **Step 10.2: Verify with a small test**

Append to the `tests` module in `signer.rs`:

```rust
    #[test]
    fn signer_impls_subxt_signer_for_substrate_config() {
        use subxt::tx::Signer as _;
        let s = signer_from_demo(Network::Undeployed);
        let _: subxt::utils::AccountId32 = s.account_id();
        let sig = s.sign(b"hello");
        let _ = format!("{:?}", sig);
    }
```

- [ ] **Step 10.3: Add `submit_deploy` to `NodeClient`**

In `mobile-bench/wallet-core/src/node/client.rs`, find `pub enum NodeError` and add:

```rust
    #[error("submit: {0}")]
    Submit(String),
```

After the existing `impl NodeClient { … }` methods, append:

```rust
/// Outcome of a successful in-block submission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmitResult {
    pub tx_hash: [u8; 32],
    pub block_hash: [u8; 32],
}

impl NodeClient {
    /// Submit a SCALE-encoded transaction via the runtime call
    /// `Midnight.send_mn_transaction(bytes)`. Waits for in-block
    /// inclusion (not finality — design choice).
    #[allow(dead_code)] // Wired by Wallet::create_did in Task 11.
    pub async fn submit_deploy(
        &self,
        bytes: Vec<u8>,
        signer: &crate::MidnightSigner,
    ) -> Result<SubmitResult, NodeError> {
        use midnight_node_metadata as runtime;

        let api = self.subxt_client.as_ref().ok_or_else(|| {
            NodeError::Submit("subxt client not initialised".into())
        })?;
        let call = runtime::tx().midnight().send_mn_transaction(bytes);
        let progress = api
            .tx()
            .sign_and_submit_then_watch_default(&call, signer)
            .await
            .map_err(|e| NodeError::Submit(e.to_string()))?;
        let in_block = progress
            .wait_for_in_block()
            .await
            .map_err(|e| NodeError::Submit(format!("wait_for_in_block: {e}")))?;
        in_block
            .wait_for_success()
            .await
            .map_err(|e| NodeError::Submit(format!("wait_for_success: {e}")))?;

        Ok(SubmitResult {
            tx_hash: in_block.extrinsic_hash().0,
            block_hash: in_block.block_hash().0,
        })
    }
}
```

If `NodeClient` doesn't yet store a `subxt::OnlineClient<SubstrateConfig>`, add a field for it. Update `NodeClient::connect` to populate it via `subxt::OnlineClient::<SubstrateConfig>::from_url(cfg.node_ws_url)`. Look at the existing `connect` implementation to see the right place to extend.

- [ ] **Step 10.4: Re-export `SubmitResult`**

In `mobile-bench/wallet-core/src/lib.rs`, find the line:

```rust
pub use node::{MidnightSigner, NodeClient, NodeError, NodeHealth, NodeStatus, SignerError};
```

Replace with:

```rust
pub use node::{
    MidnightSigner, NodeClient, NodeError, NodeHealth, NodeStatus, SignerError, SubmitResult,
};
```

- [ ] **Step 10.5: Run + commit**

```
cargo test -p wallet-core --lib node::signer
cargo check -p wallet-core
```

Expected: existing signer tests pass + the new typecheck. `cargo check` clean.

If subxt's `Signer` trait requires methods we haven't implemented (e.g. `account_index`, `genesis_hash`), the compile error tells you which. Default `SubstrateConfig` impls typically suffice.

```bash
git add mobile-bench/wallet-core/src/node/signer.rs mobile-bench/wallet-core/src/node/client.rs mobile-bench/wallet-core/src/lib.rs
git commit -S -s -m "$(cat <<'EOF'
feat(wallet-core): subxt Signer impl + NodeClient::submit_deploy

Two coupled changes:

1. MidnightSigner now implements subxt::tx::Signer<SubstrateConfig>
   via its existing sign_envelope() (raw 65-byte ECDSA) wrapped in
   MultiSignature::Ecdsa. Account id is the same 32-byte blob we
   already cached.

2. NodeClient gains submit_deploy(scale_bytes, &signer) ->
   SubmitResult. Wraps the metadata-generated
   Midnight.send_mn_transaction call, kicks off
   sign_and_submit_then_watch_default, awaits in-block + success.

Watch model is wait-for-inclusion (not finality) per the design
spec. SubmitResult carries the extrinsic hash + block hash for
the wizard UI to surface.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
git log --format="%h %G? %s" -1
```

Expected: `G`.

---

### Task 11: `Wallet::create_did` pipeline + `CreateDidWizard` UI

**Files:**
- Modify: `mobile-bench/wallet-core/src/wallet.rs`
- Modify: `mobile-bench/dioxus-wallet/src/app.rs`

- [ ] **Step 11.1: Replace `create_did` with the streaming pipeline**

In `mobile-bench/wallet-core/src/wallet.rs`, find the existing `create_did` method (around line 269 — the `WriteNotImplemented` stub) and replace it with:

```rust
    /// Submit a real deploy of the DID contract. Returns a
    /// `Stream<WizardStage>` so the UI renders progress.
    /// See docs/superpowers/specs/2026-05-15-did-deploy-submit-design.md.
    pub fn create_did(
        &self,
    ) -> impl futures::Stream<Item = crate::WizardStage> + Send + 'static {
        let network = self.network;
        let seed_bytes = self.seed_bytes;
        async_stream::stream! {
            // 1. SyncingDust
            yield crate::WizardStage::SyncingDust;
            let wallet = Wallet { network, keys: crate::crypto::derive_keys(&seed_bytes), seed_bytes };
            let mut dust_state = match wallet.sync_dust().await {
                Ok(s) => s,
                Err(e) => { yield crate::WizardStage::Failed(format!("sync dust: {e}")); return; }
            };
            let dust_key = match wallet.dust_secret_key() {
                Ok(k) => k,
                Err(e) => { yield crate::WizardStage::Failed(format!("dust key: {e}")); return; }
            };

            // 2. Composing
            yield crate::WizardStage::Composing;
            let pk_commitment = match wallet.did_controller_public_key() {
                Ok(pk) => pk,
                Err(e) => { yield crate::WizardStage::Failed(format!("controller pk: {e}")); return; }
            };
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            let ttl = base_crypto::time::Timestamp::from_secs(now_ms / 1000 + 3600);
            let mut rng = rand::thread_rng();
            let nonce: [u8; 32] = rand::Rng::r#gen(&mut rng);
            let net_id = network.config().network_id;
            let unproven = match crate::tx::build::build_deploy(
                pk_commitment, net_id, now_ms, nonce, ttl, &mut rng,
            ) {
                Ok(t) => t,
                Err(e) => { yield crate::WizardStage::Failed(format!("compose: {e}")); return; }
            };

            // Preview DID id from the exact inputs we just used.
            let preview_id = match wallet.create_did_preview_with(pk_commitment, now_ms, nonce) {
                Ok(id) => id,
                Err(e) => { yield crate::WizardStage::Failed(format!("preview id: {e}")); return; }
            };

            // 3. Balancing
            yield crate::WizardStage::Balancing;
            let params = ledger::structure::INITIAL_PARAMETERS.clone();
            let mut ctx = crate::tx::balance::BalanceCtx {
                dust_state: &mut dust_state,
                dust_key: &dust_key,
                params: &params,
                time: base_crypto::time::Timestamp::from_secs(now_ms / 1000),
                network_id: net_id,
            };
            let balanced = match crate::tx::balance::balance(unproven, &mut ctx) {
                Ok(b) => b,
                Err(e) => { yield crate::WizardStage::Failed(format!("balance: {e}")); return; }
            };

            // 4. Proving
            yield crate::WizardStage::Proving;
            let prove_rng = rand_chacha::ChaCha20Rng::from_entropy();
            let proven = match crate::tx::prove::prove(balanced, prove_rng).await {
                Ok(p) => p,
                Err(e) => { yield crate::WizardStage::Failed(format!("prove: {e}")); return; }
            };

            // 5. Submitting
            yield crate::WizardStage::Submitting;
            let bytes = match crate::tx::scale::scale_encode(&proven) {
                Ok(b) => b,
                Err(e) => { yield crate::WizardStage::Failed(format!("encode: {e}")); return; }
            };
            let signer = match wallet.midnight_signer() {
                Ok(s) => s,
                Err(e) => { yield crate::WizardStage::Failed(format!("signer: {e}")); return; }
            };
            let node = match crate::NodeClient::connect(network).await {
                Ok(n) => n,
                Err(e) => { yield crate::WizardStage::Failed(format!("node connect: {e}")); return; }
            };

            // 6. Confirming
            yield crate::WizardStage::Confirming;
            let submit = match node.submit_deploy(bytes, &signer).await {
                Ok(r) => r,
                Err(e) => { yield crate::WizardStage::Failed(format!("submit: {e}")); return; }
            };

            yield crate::WizardStage::Done(crate::DeployOutcome {
                did_id: preview_id,
                tx_hash: submit.tx_hash,
                block_hash: submit.block_hash,
            });
        }
    }

    /// Like `create_did_preview` but with caller-supplied
    /// pk-commitment, timestamp, and nonce — lets the pipeline
    /// compute the on-chain DID id from the *exact* inputs fed
    /// to `tx::build_deploy`.
    pub fn create_did_preview_with(
        &self,
        pk_commitment: [u8; 32],
        timestamp_ms: u64,
        nonce: [u8; 32],
    ) -> Result<crate::DidId, crate::DidError> {
        let deploy = crate::did::deploy::compose_deploy(pk_commitment, timestamp_ms, nonce);
        let bytes: crate::ContractAddressBytes = deploy.address().0.0;
        Ok(crate::DidId::new(self.network, bytes))
    }
```

If `crate::crypto::derive_keys` has a different name, look at the existing `Wallet::from_*` constructors in `wallet.rs` and mirror whatever they call. The point is: rebuild a `Wallet` inside the `async move` block from the captured `seed_bytes + network` so we don't hold a `&self` across the await boundary.

- [ ] **Step 11.2: Replace `CreateDidPanel` with `CreateDidWizard` in `app.rs`**

Find the existing `CreateDidPanel` definition (around line ~291) and replace its body:

```rust
#[component]
fn CreateDidWizard(network: Network) -> Element {
    use wallet_core::WizardStage;

    let mut stages = use_signal::<Vec<WizardStage>>(Vec::new);
    let mut running = use_signal(|| false);

    let start = move |_| {
        if *running.read() {
            return;
        }
        running.set(true);
        stages.set(Vec::new());
        spawn(async move {
            use futures::StreamExt;
            let w = Wallet::demo(network);
            let mut stream = std::pin::pin!(w.create_did());
            while let Some(stage) = stream.next().await {
                let mut current = stages.read().clone();
                current.push(stage);
                stages.set(current);
            }
            running.set(false);
        });
    };

    rsx! {
        div { class: "row", "Create DID" }
        div { class: "row",
            button {
                disabled: *running.read(),
                onclick: start,
                {if *running.read() { "Submitting…" } else { "Create DID" }}
            }
        }
        for stage in stages.read().iter() {
            div { class: "row",
                {render_stage(stage)}
            }
        }
    }
}

fn render_stage(s: &wallet_core::WizardStage) -> String {
    use wallet_core::WizardStage as W;
    match s {
        W::SyncingDust => "• syncing dust…".to_string(),
        W::Composing => "• composing…".to_string(),
        W::Balancing => "• balancing fees…".to_string(),
        W::Proving => "• proving…".to_string(),
        W::Submitting => "• submitting…".to_string(),
        W::Confirming => "• waiting for inclusion…".to_string(),
        W::Done(o) => format!(
            "✓ done\n  did:    {}\n  tx:     0x{}\n  block:  0x{}",
            o.did_id.to_did_string(),
            hex::encode(o.tx_hash),
            hex::encode(o.block_hash),
        ),
        W::Failed(e) => format!("✗ failed: {e}"),
    }
}
```

Find the mount line in App (around line 285) and rename:

```rust
                CreateDidWizard { network: *network.read() }
```

- [ ] **Step 11.3: Compile both crates**

```
cargo check -p wallet-core
cargo check -p dioxus-wallet
```

Both clean. Fix `Wallet { network, keys, seed_bytes }` constructor reference if the struct fields differ (look at the existing `Wallet::demo` or any constructor for the right pattern; if construction requires going through a helper, call that helper instead).

- [ ] **Step 11.4: Commit**

```bash
git add mobile-bench/wallet-core/src/wallet.rs mobile-bench/dioxus-wallet/src/app.rs
git commit -S -s -m "$(cat <<'EOF'
feat: Wallet::create_did real pipeline + CreateDidWizard UI

Replaces the WriteNotImplemented stub with an async_stream!
pipeline yielding WizardStage events:
SyncingDust → Composing → Balancing → Proving → Submitting →
Confirming → Done(DeployOutcome) | Failed(String).

The wizard derives the on-chain DID id locally via the new
create_did_preview_with helper using the exact inputs (pk, ts,
nonce) fed to tx::build_deploy — so preview_did = on-chain_did
by construction.

UI replaces CreateDidPanel with CreateDidWizard: button click
spawns the stream, each yielded stage appends to a Vec<WizardStage>
signal that re-renders into a status list. Done shows the DID id,
tx hash, and block hash; Failed shows the error verbatim.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
git log --format="%h %G? %s" -1
```

Expected: `G`.

---

### Task 12: Live integration test

**Files:**
- Create: `mobile-bench/wallet-core/tests/deploy_undeployed_live.rs`

- [ ] **Step 12.1: Create the test**

```rust
//! Live integration test for `Wallet::create_did()` against the
//! local standalone Midnight stack. Gated behind `network-tests`.
//!
//! Run with:
//!   docker compose -f mobile-bench/scripts/standalone.yml up -d node indexer
//!   cargo test -p wallet-core --features network-tests \
//!     --test deploy_undeployed_live -- --nocapture

#![cfg(feature = "network-tests")]

use futures::StreamExt;
use wallet_core::{Network, Wallet, WizardStage};

#[tokio::test]
async fn deploy_did_on_undeployed_lands_in_block() {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let w = Wallet::demo(Network::Undeployed);
    println!("unshielded address: {}", w.unshielded_address().unwrap());
    println!("dust public key:    {}", w.dust_public_key_hex().unwrap());

    let stream = w.create_did();
    let mut stream = std::pin::pin!(stream);

    let mut outcome = None;
    while let Some(stage) = stream.next().await {
        match &stage {
            WizardStage::Done(o) => {
                println!(
                    "done: did={} tx=0x{} block=0x{}",
                    o.did_id.to_did_string(),
                    hex::encode(o.tx_hash),
                    hex::encode(o.block_hash),
                );
                outcome = Some(o.clone());
                break;
            }
            WizardStage::Failed(e) => panic!("pipeline failed: {e}"),
            other => println!("stage: {other:?}"),
        }
    }

    let outcome = outcome.expect("pipeline yielded Done");
    let did_string = outcome.did_id.to_did_string();
    assert!(did_string.starts_with("did:midnight:undeployed:"));
    let parsed = wallet_core::DidId::parse(&did_string).expect("parse did string");
    assert_eq!(parsed, outcome.did_id);
}
```

- [ ] **Step 12.2: Build check (no network)**

```
cargo build -p wallet-core --features network-tests --tests
```

Expected: clean.

- [ ] **Step 12.3: Manual run**

```
docker compose -f mobile-bench/scripts/standalone.yml up -d node indexer
cargo test -p wallet-core --features network-tests --test deploy_undeployed_live -- --nocapture
```

Likely first-failure modes and fixes (from the spec's open questions):

- `sync dust: stream closed early` → `dustLedgerEvents` doesn't accept `id: 0` cleanly. Try `id: null` (omit the variable) or `id: -1`. Adjust the subscription.
- `prove: dust resolver: …` → `srs.midnight.network` unreachable. Pre-warm cache by running `cargo test -p midnight-ledger --features proving` once on a network-connected machine, OR set `MIDNIGHT_PARAM_SOURCE` to an internal mirror.
- `submit: …` → subxt typed-call API drift. The metadata at `node-0.22.3` should match; if the runtime call shape is different, regenerate the metadata.
- `balance: insufficient DUST` → fix dust public key derivation; the genesis DustInitialUtxos must be for *our* address. Cross-check `dust_public_key_hex` against the genesis-block dust UTXOs.

Fix in place, re-run.

- [ ] **Step 12.4: Commit**

```bash
git add mobile-bench/wallet-core/tests/deploy_undeployed_live.rs
git commit -S -s -m "$(cat <<'EOF'
test(wallet-core): live DID deploy integration test

Gated behind --features network-tests. Runs the full
Wallet::create_did() pipeline against a local standalone stack,
asserts Done arrives, and round-trips the DidId through parse().

Manual prerequisites: bring up
mobile-bench/scripts/standalone.yml's node + indexer first, then
cargo test --features network-tests --test deploy_undeployed_live
-- --nocapture.

This is the proof point that resolves the spec's open questions
(DUST public-key derivation format, DataProvider reachability,
ttl type, PreProd specifics).

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
git log --format="%h %G? %s" -1
```

Expected: `G`.

---

## Final integration check

After all 12 tasks land:

```
cargo test -p wallet-core --lib
cargo check -p wallet-core
cargo check -p dioxus-wallet
docker compose -f mobile-bench/scripts/standalone.yml up -d node indexer
cargo test -p wallet-core --features network-tests --test deploy_undeployed_live -- --nocapture
git log --format="%h %G? %s" -15
```

Expected:
- All lib tests pass.
- Both `cargo check` clean.
- Live test prints stages and ends with `done: did=…`.
- 12+ G-signed commits since the design-spec commit `1dcf1f93`.

---

## Spec divergences

This plan diverges from the original spec in three places — flagging for the user to ratify or push back:

1. **No custom `DustState` / `DustOutput` types.** We use `ledger::dust::DustLocalState<DefaultDB>` directly. The original spec proposed thin wrappers; in practice they'd duplicate the ledger's encoding work and prevent the balancer from using `replay_events` cleanly.

2. **No bech32m DUST address (`mn_dust_<networkId>1…`).** The indexer's `dustLedgerEvents` subscription doesn't take a DUST address at all — it streams ALL events globally and the wallet filters by its own pubkey at replay time. The spec's `Wallet::dust_address` method is replaced with `Wallet::dust_public_key_hex()` (raw 32-byte hex of the DustPublicKey) for future UI display use. The bech32m form is deferred to a follow-up if/when the wallet wants a human-readable form.

3. **Switched indexer subscription from `dustGenerations` to `dustLedgerEvents`.** The former is keyed by address but returns shaped data we'd have to re-encode for `replay_events`. The latter exposes each event's `raw: HexEncoded!` (scale-encoded `ledger::events::Event<D>`) directly — much less decode work and matches the ledger's native API.

All three changes preserve the spec's goal and non-goals. The pipeline shape, error contract, UI flow, and watch model are unchanged.

---

## Spec coverage check

| Spec section | Task(s) | Notes |
|---|---|---|
| Goal: `Wallet::create_did()` returns `Stream<WizardStage>` | Task 11 | ✓ |
| `WizardStage` 6-stage enum + Done/Failed | Task 6 | ✓ |
| DUST state types | Task 3 | ✓ — ledger's `DustLocalState` directly (divergence #1) |
| `Wallet::sync_dust()` | Tasks 4 + 5 | ✓ via `dustLedgerEvents` + replay_events (divergence #3) |
| `tx::build_deploy` (decoupled from Wallet) | Task 7 | ✓ |
| `tx::balance` (DUST-only port of TestState::balance_tx) | Task 8 | ✓ |
| `tx::prove` (wraps tx_prove + bundled DUST resolver) | Task 9 | ✓ |
| `tx::scale_encode` | Task 6 | ✓ |
| `artifacts::dust::dust_resolver()` | Task 1 | ✓ via OnDemand cache (spec already amended) |
| DUST key derivation | Task 2 | ✓ — hex public key, no bech32m (divergence #2) |
| `NodeClient::submit_deploy` + subxt typed-call | Task 10 | ✓ |
| `MidnightSigner: subxt::tx::Signer` | Task 10 | ✓ |
| `CreateDidWizard` UI | Task 11 | ✓ |
| Live integration test | Task 12 | ✓ |
| Open question #1 (DataProvider) | Task 1 + Task 12 | ✓ |
| Open question #2 (DUST address format) | resolved in plan | DUST input is hex, not bech32m |
| Open question #3 (Intent ttl type) | Task 7 / Task 11 | use `Timestamp::from_secs` |
| Open question #4 (PreProd specifics) | follow-up | Undeployed-first |
| Non-goals (DID content edit, faucet, DUST reg UI, finality, retry, history) | n/a | preserved |
