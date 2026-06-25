# Mobile-Bench Backlog

Deferred work items that have been scoped and decided-against-for-now,
but should be picked up later. Each entry should include enough context
that a future session can act on it without re-doing the investigation.

---

## Path B — Full wallet sync to redb

**Status:** Phase 1 LANDED in commit `5f7df14f` (schema v6 +
`DUST_SYNC` table + `WalletStore` get/put/clear methods).
Phase 2/3/4 still to do — sketched at the bottom of this section.

Background on Path A: per-call live fetch of `LedgerParameters` +
`ZswapChainState` from the indexer landed via commit `57068c39` +
follow-ups, plus the HTTP-proof-server route in `0875368c`. That
unblocked the PreProd write demo but each write still pays a full
DUST event replay (~534k events on PreProd → 30–50 min per click).
Path B persists that snapshot so subsequent launches resume from
`last_id + 1`.

**Why this is the natural next step:**
Upstream `midnight-did-manager-service` persists 5 JSON files in
`~/.midnight-did/profiles/<network>/<profile>/wallet-state/<seedHash6>/`:

| File | Size | Contains |
|---|---|---|
| `meta.json` | 175 B | `walletSchema`, `profile`, `updatedAt` |
| `shielded.json` | ~3.5 KB | tagged-serialized `zswap-local-state[v6]`, `offset` checkpoint, `coinPublicKey`, `encryptionPublicKey` |
| `unshielded.json` | ~700 B | available UTXOs, `appliedId` checkpoint |
| `unshielded-history.json` | ~1.7 KB | tx history |
| `dust.json` | **~4.2 MB** | tagged-serialized `dust-local-state[v1]`, `offset` checkpoint |

The pattern: each file stores `{ state: <bytes>, offset: <event_id>, ... }`.
On startup, deserialize → catch up from `offset+1` via indexer
subscription → re-serialize back. The 10-15 min sync is only on
first run; subsequent launches are fast.

**Why we'd want it (long term):**

- Tx history view, balance trends, "recent activity" tab — all need
  persisted state Path A doesn't have.
- Mobile cold-start UX: one slow first-run, fast thereafter, vs Path
  A's per-action indexer spinner.
- Indexer outage resilience: read cached state, retry write later.
- Adding new contracts beyond DID reuses the same sync infrastructure
  instead of N independent per-call queries.
- Schema migration story stays consolidated in redb (already at v5;
  v6 adds sync tables).
- Architectural alignment with upstream — when their state model
  evolves, ours evolves the same way.

**Why Path A is fine for now:**

- ~30 lines vs ~600 for B. Real maintenance delta.
- Always fresh — no drift window.
- The "fresh fetch right before submit" hook stays useful even
  after B lands (upstream's `publicDataProvider.queryZSwapAndContractState`
  is exactly this pattern at submit time).

---

### Phase 1 — Schema v6 snapshot tables (~150 LOC) — DONE `5f7df14f`

Mirror upstream's 5 JSON files 1-to-1 in redb. One row per "state kind"
keeps it small and all-or-nothing replaceable.

```rust
// mobile-bench/wallet-core/src/store/mod.rs (or wherever schema lives)
const WALLET_SYNC_STATE: TableDefinition<&str, &[u8]>;
// keys:
//   "zswap"             -> tagged-serialized ZswapLocalState bytes
//   "dust"              -> tagged-serialized DustLocalState bytes
//   "unshielded"        -> tagged-serialized UnshieldedState bytes
//   "unshielded_history" -> serialized history blob

const WALLET_SYNC_OFFSETS: TableDefinition<&str, u64>;
// keys: "zswap_offset", "dust_offset", "unshielded_applied_id"

const WALLET_SYNC_META: TableDefinition<&str, &str>;
// "wallet_schema" -> "ledger8"
// "network"       -> "preprod"
// "updated_at"    -> ISO-8601 timestamp
```

Migration: follow the same pattern as v4→v5 (see existing migration
helpers). On open, if `META.get("wallet_schema") == None`, create
empty rows and write `{ "wallet_schema": "ledger8", "network": <net>,
"updated_at": now }`.

The 4.2 MB dust blob is fine: redb stores it as a single value,
atomic transaction, `Database::compact()` reclaims churn explicitly
when we ask.

---

### Phase 2 — `WalletSyncer` module (~200 LOC) — TODO

User wants:
- A **"Sync DUST" button** on the wallet page that triggers a full
  catch-up with a visible progress bar (initial cold sync UX).
- Subsequent CRUD operations do a **quick incremental resync** of
  the delta since the last persist — transparent, no spinner.

Sketch:

```rust
// mobile-bench/wallet-core/src/dust/syncer.rs (new)
pub struct DustSyncer {
    network: Network,
    store: Arc<WalletStore>,
    dust_key: Arc<DustSecretKey>,
}

pub struct SyncProgress {
    pub current_id: i64,
    pub max_id: i64,
}

impl DustSyncer {
    /// Load the persisted snapshot if any, subscribe from
    /// `last_id + 1`, fold events, persist on a debounce window.
    /// Returns a stream that emits progress updates so the UI
    /// can render a progress bar.
    pub fn sync(&self) -> impl Stream<Item = SyncProgress>;

    /// Read the current cached state without going to the network.
    /// Returns the most recent persisted snapshot. Pairs with
    /// `Wallet::call_did_circuit` which then needs only a tiny
    /// delta sync (current_id..tip).
    pub fn cached_state(&self) -> Option<DustLocalState<DefaultDB>>;
}
```

Reuse the `fold_events` pattern from
`mobile-bench/wallet-core/src/dust/snapshot.rs:64-112` — same
indexer subscription, just persists every N events / M seconds.

### Phase 3 — Wallet integration (~100 LOC) — TODO

Current `Wallet::sync_dust()` does a full replay every call. Make
it consult the cache first via the `DustSyncer`:

1. `Wallet` grows an `Option<Arc<DustSyncer>>` field (clean: trait
   abstraction so wallet-core doesn't bind to the store concretely).
2. `with_dust_syncer(syncer)` builder, called by the App at wallet
   construction.
3. `sync_dust()` checks the syncer's cached state; if missing,
   falls back to today's full-replay path.
4. Internal `Wallet::from_seed(seed_bytes, network)` calls inside
   `call_did_circuit` / `create_did` / `load_did_circuit` thread
   the syncer Arc the same way they currently propagate
   `proof_server_url`.

### Phase 4 — UI sync button + progress (~100 LOC) — TODO

- New row on the wallet page: "DUST sync: last synced 2026-05-20 16:30, last_id=534,302"
- "Sync now" button → kicks `DustSyncer::sync()`, renders progress
  bar driven by the `SyncProgress` stream.
- Progress format: `Indexing… 234,567 / 534,302 (43%)`.
- Auto-trigger an incremental sync on every CRUD submit (small,
  fast).

### Original Phase 2 — Sync loop per state kind (~250 LOC, deferred)

Reuse the exact `fold_events` pattern from
`mobile-bench/wallet-core/src/dust/snapshot.rs:64-112`. That code
already handles the indexer's "you're caught up" marker (`max_id`)
and the idle-timeout backstop.

```rust
// mobile-bench/wallet-core/src/wallet_sync/mod.rs (new module)
pub struct WalletSyncer { store: Arc<WalletStore>, network: Network, ... }

impl WalletSyncer {
    pub async fn cold_start(&mut self) -> Result<(), SyncError> {
        // 1. Load offsets from redb
        let offsets = self.store.load_offsets()?;
        // 2. Hydrate state from redb
        let mut zswap = self.store.load_zswap_state()?.unwrap_or_default();
        let mut dust  = self.store.load_dust_state()?.unwrap_or_default();
        let mut unsh  = self.store.load_unshielded_state()?.unwrap_or_default();
        // 3. Catch up from offset+1
        self.catchup_zswap(&mut zswap, offsets.zswap_offset).await?;
        self.catchup_dust(&mut dust, offsets.dust_offset).await?;
        self.catchup_unshielded(&mut unsh, offsets.unshielded_applied_id).await?;
        // 4. Persist atomically
        self.store.persist_all(&zswap, &dust, &unsh, &offsets)?;
        Ok(())
    }
}
```

Subscriptions (one per kind):
- `zswapLedgerEvents` — chain-wide Zswap event stream (use indexer
  WebSocket subscription, same `transport::subscribe` we use for
  dust)
- `dustLedgerEvents` — already wired in `dust/snapshot.rs`
- `unshieldedTransactions` — wallet-specific stream, takes the
  wallet's address as filter

Fold semantics:
- zswap: `ZswapLocalState::apply_event(event)`
- dust: `DustLocalState::apply_event(event)` (we already do this in
  fold_events; just persist instead of replay-from-zero)
- unshielded: maintain `availableUtxos` and `pendingUtxos` sets

Persist on every N events or every M seconds (whichever first), in
one redb transaction so all kinds stay consistent.

---

### Phase 3 — Read-through cache + fresh-fetch hooks (~100 LOC)

**Reads** (resolve, balance display, history): serve from redb
cache. Always fast.

**Writes** (`call_did_circuit`, `create_did`, `load_did_circuit`):
do a fresh fetch of `(ContractState, ZswapChainState, LedgerParameters)`
right before composing the unproven tx. This is what upstream's
`publicDataProvider.queryZSwapAndContractState` does at submit time
and is the pattern Path A already implements. Keep it.

The split:
- `WalletSyncer::get_state_for_read() -> CachedState` — fast, may be slightly stale
- `WalletSyncer::get_state_for_submit() -> FreshState` — slow, indexer round-trip, guaranteed current

---

### Acceptance criteria

- [ ] Schema v6 migration round-trips cleanly on a v5 store
- [ ] Cold start hydrates `(zswap, dust, unshielded)` from redb in <100ms
- [ ] First-run catch-up against PreProd completes (expect ~10-15 min
      based on upstream manager benchmarks)
- [ ] Subsequent launches resume from `offset` and catch up the delta
      in <30s on a current chain
- [ ] `preprod_add_also_known_as` still passes after Phase 3 — fresh
      fetch at submit time preserves Path A's correctness
- [ ] redb file size after a full preprod sync is bounded (verify with
      a real run; expect ~5 MB based on upstream's JSON sizes)
- [ ] `Database::compact()` reclaims at least 30% of churn after a
      thousand updates

### Test plan

1. Unit: round-trip each state kind through redb (encode → store →
   load → decode → assert equal)
2. Integration: replay a recorded stream of indexer events and assert
   the final state matches a known checkpoint
3. Live (`network-tests` feature): cold-start against PreProd, verify
   resume on second launch processes only the delta
4. Regression: re-run the byte-diff test in `preprod_decode_diff` and
   confirm the partition stays in `guaranteed_transcript`

### Open questions for Phase 1 kickoff

- Subscribe per-kind in parallel (3 concurrent streams) or sequential?
  Upstream's manager runs them concurrently — start there.
- Where does `WalletSyncer` live in the binary's lifecycle? Probably
  the same place that opens the redb store today. Needs care around
  shutdown to avoid torn writes (redb handles atomicity per-txn but
  we should drain in-flight events before close).
- Does Compaction belong in a background task on app idle? Or
  user-triggered? Decision: user-triggered via a Diagnostics-tab
  button (matches the user's "we have control over it" framing).

---

## SSI demo — Track 2: P0/P1 codebase fixes from the 2026-06-25 audit

**Status:** scoped, deferred. Quick wins — could ride on PR #4
(wallet) and PR #54 (dApp).

Audit identified after the SSI demo shipped. Each one is a silent
fall-through that masks a config bug; we hit two of them during the
demo (DEFAULT_VAULT_CONTRACT_ADDRESS Android arm; dApp shim dropping
`contractAddress`). The rest are still live.

### P0 — fix before next demo cycle

- **`DEFAULT_DAPP_URL` hardcoded to one tailnet IP** —
  `mobile-bench/dioxus-wallet/src/app.rs:1048,1050`. Breaks any other
  operator's phone demo. Fix: `option_env!("MIDNIGHT_DAPP_URL")`
  fallback (read at build via `--env` or at runtime via wallet config
  if we can persist it).
- **`getTotalLocked()` silently returns 0n on error** —
  `apps/dapp/lib/vault.ts:76-80`. Masks "wrong vault address"
  config bugs (which the user can't see). Fix: return
  `{ ok, value?, error? }`, render the error in `VaultCard`.
- **Issuer `ISSUER_URL/REDIRECT_URL` default to localhost** —
  `midnight-passport-issuer/src/config/index.ts:77,94`. We patched in
  the demo bootstrap script (PR #23 phase 8). Fix at source: read
  from a `PUBLIC_HOST` env var, default to `host.docker.internal:8080`
  inside container (works for sim + emu without tailnet override).

### P1 — same PR family

- **`vaultListCredentials` has no error fallback** —
  `apps/dapp/lib/vault.ts:99-101`. Compare to `listLocks()` which
  silently returns `[]`. Currently crashes the UI on disconnect.
- **Vault address duplicated in 3 places** — `bridge.rs:549-553`,
  `apps/dapp/lib/vault.ts:21-23`, `apps/dapp/.env.local:2`. Every
  redeploy needs 3 manual edits + APK rebuild. Fix candidates:
  fetch from indexer's contract registry at startup, or read from a
  single config file the demo bootstrap writes.
- **`vaultListCredentials` typed to accept `contractAddress` but
  ignores it** — misleading API. Remove from the type, OR thread it
  through `mobile-bench-host.ts` shim + wallet bridge.

**Effort:** ~3–4 hours total. Best landed as one follow-up PR on each
repo.

---

## SSI demo — Track 3: `/setup` page port (verifier admin in dApp)

**Status:** scoped, deferred. Needs design discussion before
implementation.

Today the verifier admin runs 7 `npm` scripts from `apps/cli`:
`deploy` → `make-issuer-anchor` → `set-trusted-issuer` →
`create-lock` → `deposit` → `claim` → `show-state`. The user wants
this in a dApp `/setup` page so non-engineers can configure a
verifier.

**Implementation surface:**

| Script | dApp surface |
|---|---|
| `deploy` | "Deploy Vault" button. Needs verifier wallet seed (admin key) — **not** the end-user's wallet. |
| `make-issuer-anchor` + `set-trusted-issuer` | "Trust Issuer" form: paste DID URL + Jubjub secret hex, generate anchor + submit rotation. |
| `create-lock` | "Create Lock" form: min-age, max-claim, issuing-state/document gates. |
| `deposit` | "Top up Lock" form: lockId + amount. |
| `claim` | already in `VaultCard` (end-user flow). |
| `show-state` | already in `VaultCard` (locks list, total locked). |

**Hard problem — admin secret handling:**
The verifier's signing key is NOT end-user-owned. Today it's in
`apps/cli/.env` (`VAULT_ADMIN_SEED_HEX`). A public dApp form is
unsafe. Three options:

1. **Out-of-band ceremony** — admin runs a one-time setup CLI that
   produces an encrypted key file the dApp loads via file picker.
2. **Hardware-wallet style** — admin's wallet exposes a "sign as
   vault operator" verb; dApp prompts the wallet to sign each
   admin tx (cleanest, but needs wallet support).
3. **Time-boxed in-memory secret** — admin pastes seed once, dApp
   holds it in JS memory only, expires on tab close. Acceptable for
   demo + lab, NOT for production.

**Recommended path:** start with option 3 (gated behind a "demo
mode" flag), upgrade to option 2 as wallet matures.

**Effort:** 3–4 days for option 3 / demo-mode. Design discussion
before code.

**Files to touch:**
- New: `apps/dapp/app/setup/page.tsx`, `apps/dapp/components/SetupCard.tsx`
- Extend: `apps/dapp/lib/vault.ts` with `deployVault`, `rotateTrustedIssuer`, etc.
- Extend: `apps/dapp/lib/midnight/connector.ts` with admin verbs (or expose via `getProvingProvider` + standard contract-call API)

---

## SSI demo — Track 4: Simulator support (Android emu + iOS sim)

**Status:** scoped, deferred. Network bridging is the only real work.

The "phone-driven" demo works against a real device over tailscale.
Teammates without a phone (or without tailscale setup) currently
can't reproduce. Goal: same demo against:

- **Android emulator** — host is reachable via `10.0.2.2` (Android
  emu's magic gateway). The wallet's `MIDNIGHT_INDEXER_HTTP_URL` etc.
  need to point there instead of tailnet IP.
- **iOS simulator** — host is reachable via `host.docker.internal`
  inside Docker, and `127.0.0.1` from the iOS sim (it shares the
  host's netstack). Probably easier than Android emu.

**Implementation:**

1. Wallet: extend `startup_network()` and `vault_contract_address()`
   resolution to read a JSON config (`/sdcard/midnight-demo.json`?)
   instead of compiled-in const. Lets the demo bootstrap script
   inject the right URLs per platform.
2. Demo bootstrap: in `demos/bootstrap.py` add `--platform` flag
   (`real-phone | android-emu | ios-sim`). Picks the right host IP +
   pushes the config file via `adb push` / `xcrun simctl`.
3. APK build path: still needed for new contract addresses **unless**
   the runtime config above lands first. Once it lands, ONE APK
   serves all three platforms.

**Effort:** 1–2 days once the config-file pattern lands. The
config-file refactor is itself the long pole — see Track 2 P1
"Vault address duplicated in 3 places."

**Files to touch:**
- `mobile-bench/dioxus-wallet/src/app.rs` (startup_network, vault default)
- `mobile-bench/dioxus-wallet/src/bridge.rs` (DEFAULT_VAULT_CONTRACT_ADDRESS)
- `demos/bootstrap.py` + `demos/lib/net.py` in midnight-identity-workspace
- New: `demos/lib/platform.py` (per-platform IP + push logic)

---

## SSI demo — Track 5: Demo bootstrap follow-ups (PR #23)

**Status:** PR #23 (`feat(demos): one-command demo bootstrap orchestrator`)
landed with 5 commits, verified end-to-end live. Follow-ups the
agent flagged:

- **Container-name compatibility** — script accepts both
  `fixtures-node-1` and `midnight-node`. Safer to set
  `container_name:` in the compose files so naming is deterministic
  regardless of which compose project the operator runs from.
- **Submodule structure** — `demos/` resolves `apps/cli` + `apps/dapp`
  from either the in-tree submodule or sibling `midnight-workspace-vc-test`
  checkout (env-driven). Canonical fix: land `apps/cli + apps/dapp`
  in the in-tree submodule so there's one source of truth.
- **APK rebuild path** — intentionally out of scope of the bootstrap
  today. Becomes irrelevant once Track 4's config-file pattern lands.

---

## Verifier (`vault.compact`) — Track 6: contract review follow-ups

**Status:** general correctness pass DONE (2026-06-25 sub-agent
review). No P0/P1 findings on the cryptographic side — `trustedIssuer`
stored as `persistentHash<JubjubPoint>` is correct by design (refuted
the "should be `Field` or `Point`" hypothesis).

Minor follow-up:
- **Explicit subgroup-membership check** — contract trusts the prover
  to supply a valid Jubjub point in `credentialProof.publicKey`. The
  Schnorr signature verification implicitly catches off-curve / small-
  subgroup points, but a redundant check would be belt-and-braces.
  Cite: `vault/src/passport-vault.compact:435-439` +
  `vault/src/vendored/credentials/types.compact:135-141`.

---

## ZK pipeline — Track 7: Jolt-inspired optimization candidates

**Status:** investigation done (2026-06-25 sub-agent). Five
candidates surfaced ranked by ROI. Not blocking anything; pick up
when there's prover-perf appetite.

### 7.1 Profiling harness (PREREQ — do this first)

**Effort:** ~1 week. **Confidence:** high.

Stand up `tracing` + flamegraph + heap profiling on the existing
`log_phase(...)` markers in `proofs/src/plonk/prover.rs:522,538,552,945+`.
Capture flamegraphs on three real SSI demo proofs (passport-VC
issuance, zswap transfer, dust). One-page report ranked by % wall-
clock + peak RSS. **This data tells us which of 7.2–7.5 is worth
funding** — don't start any of them without it.

### 7.2 LogUp / LogUp-GKR replacing classical plookup

**Effort:** 6–10 weeks. **Confidence:** high (multiple halo2-derivs
have done it).

Replace `proofs/src/plonk/lookup/prover.rs:152-260`'s grand-product
fraction-form lookup with logarithmic-derivative lookups
(Haböck 2022) or LogUp-GKR (Papini–Haböck 2023). Savings: ~30–40%
fewer commits per lookup. Compounds because zk_stdlib's heavy chips
(sha256 with 4 lookup tables, keccak, blake2b, automaton) each use
multiple lookup arguments. Needs verifier-key wire format bump +
re-keygen for the test corpus.

### 7.3 Sparse-aware quotient computation

**Effort:** 3–4 weeks. **Confidence:** medium (wins are
circuit-shape dependent).

`proofs/src/plonk/prover.rs:862-980` `compute_h_poly` multiplies
through dead rows. The disk-spill machinery at `:389` is a tacit
admission that extended-domain FFTs are the prover's RAM bottleneck.
Track per-region sparsity in `ProvingKey`, skip MSM/FFT chunks where
all involved columns are zero. Circuits with many disjoint chips
(passport, claim verification) likely win 20–40%; dense circuits
win nothing.

### 7.4 Streamed witness generation

**Effort:** 2–3 weeks. **Confidence:** medium (mobile-specific win).

Today `transient-crypto/src/proofs.rs:742-770` `ProofPreimage::prove`
holds the full `Preprocessed` (`zkir-v3/src/ir_vm.rs:62`: full memory
map + PI vector) in RAM before the prover starts. Split into
sink/source, ring-buffer between IR-evaluator and prover. Reduces
peak RAM materially — relevant on the on-device wallet prover
(already an issue per memory: `block_in_place panics on current-
thread runtime`).

### 7.5 GKR sum-check for hash chips (speculative)

**Effort:** 3–6 months, research-grade. **Confidence:** low.

Re-implement Poseidon / SHA-256 verification inside the PLONK proof
using a GKR sub-proof (sum-check over a layered hash circuit) +
wrapper verifier. Pattern works (Nexus, RISC0 Continuations, Jolt-
Atlas). Pay-off: 5–10× on hash-heavy circuits if it works. Risk:
the wrapper-verifier may eat the savings.

### Things that DON'T transfer from Jolt

- **RISC-V opcode lookups (Lasso)** — midnight proves app circuits,
  not VM execution.
- **Twist + Shout verbatim** — designed for uniform RAM model
  midnight doesn't have.
- **Multilinear commitments / dropping KZG** — incompatible with
  midnight's KZG-onchain story, `aggregator/` IPA tail, public
  BLS12-381 SRS supply chain, and Jubjub embedding.
- **Binary field arithmetic** — requires moving off BLS12-381;
  breaks Jubjub embedding which is load-bearing across the wallet.
- **BN254 / 256-bit field tradeoff** — Jolt's choice is folding-
  recursion-friendly; midnight's BLS12-381 is locked by onchain
  verifier compatibility.

### Bench coverage gap (orthogonal)

`zk_stdlib/` and `aggregator/` have **no criterion benches** today.
`proofs/`, `curves/`, `circuits/` do. CI has no prover-time
regression gate. Adding even one bench per crate would catch
performance regressions during the 7.x work. ~2 days.

### Test coverage gap (orthogonal)

`zk_stdlib/lib.rs` is 1600+ lines, only 3 files have `#[test]`. The
chip-level tests in `circuits/` cover the units, but the integration
surface is under-tested. ~1 week to bring it up to par.

---

## Index of repo locations

| Track | Primary repo | Branch / PR |
|---|---|---|
| Path B (full sync) | midnight-ledger | future PR |
| Track 2 (P0/P1 audit) | midnight-ledger + identity-solution-examples | extend PR #4 / #54 |
| Track 3 (/setup page) | identity-solution-examples | new feature branch |
| Track 4 (simulator) | midnight-ledger + midnight-ssi-demo | new feature branch |
| Track 5 (bootstrap follow-ups) | midnight-ssi-demo | follow-up commits on PR #23 |
| Track 6 (contract review) | identity-solution-examples (vault contract) | new feature branch |
| Track 7 (ZK optimizations) | midnight-ledger + midnight-zk | gated on 7.1 profiling |
