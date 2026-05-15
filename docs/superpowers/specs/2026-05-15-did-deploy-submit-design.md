# DID deploy submission — end-to-end

Submit a `ContractDeploy` transaction for the DID contract from the
desktop wallet against the Midnight node, watch for inclusion, and
surface the resulting on-chain DID id in the UI. Subsystem B + D′
of the DID CRUD plan, collapsed into one slice because the pieces
are tightly coupled — none of them ships standalone.

Builds on:

- Subsystem A — `Wallet::sync_unshielded()` returns the unshielded
  NIGHT UTXO set (commit `2d284da4`, branch `mobile-bench/iteration-2`).
- Phase 3 — `wallet_core::did::deploy::compose_deploy` produces a
  deterministic `ContractDeploy<DefaultDB>` whose address matches
  the preview DID id.
- `MidnightSigner` — ECDSA over the wallet's secp256k1 secret,
  ready for the substrate envelope.
- `subxt 0.44` + `midnight-node-metadata` at tag `node-0.22.3`
  already pinned in `Cargo.toml`.

## Goal

User clicks **Create DID** in the desktop wallet and a real deploy
transaction lands in a block on the configured Midnight network.
The wizard reports composing → balancing → proving → submitting →
confirming → done, then shows the actual on-chain DID id and the
extrinsic hash.

## Non-goals

- **DID content customization.** The deploy publishes the initial
  state we already compose in Phase 3 — empty `verificationMethods`,
  empty relation sets, empty `services`, `controllerPublicKey` set
  to the wallet's HD-derived commitment. Verification-method and
  service editing is the next slice (write circuits).
- **Faucet automation.** On PreProd, the user is responsible for
  having funded their wallet's unshielded address and (if needed)
  registering for DUST generation. The wallet consumes whatever
  DUST UTXOs the indexer reports.
- **DUST registration UI.** The `DustRegistration` intent needs a
  Cardano reward address signature — orthogonal to the deploy flow
  and not in scope here.
- **Finalization waiting.** Watch stops at in-block. Finality is a
  cheap upgrade later (`wait_for_finalized_success`).
- **Tx history.** No persistent log of submitted txs; the wizard's
  result is forgotten on next deploy attempt.
- **Retry-on-rejection.** If the node rejects the tx (`MalformedTransaction`,
  insufficient DUST, etc.), surface the error verbatim and let the
  user re-click. No automatic retry.
- **Mainnet / Preview / QaNet / DevNet.** Same code path, but
  untested in this slice; PreProd is the proof point for "real
  network" semantics.

## Architecture

One async pipeline behind `Wallet::create_did()`:

```
[UI: Create DID button]
   │
   ▼
Wallet::create_did()                       ── stream of WizardStage events ──┐
   │                                                                          │
   ├── dust::snapshot()         → DustState              [stage: SyncingDust] │
   ├── tx::build_deploy(...)    → unproven Transaction   [stage: Composing]   │
   ├── tx::balance(...)         → balanced unproven tx   [stage: Balancing]   │
   ├── tx::prove(..., resolver) → proven Transaction     [stage: Proving]     │
   ├── tx::scale_encode(...)    → Vec<u8>                                     │
   ├── node::submit_deploy(...) → TxHash + in-block      [stage: Submitting,  │
   │                                                       Confirming]        │
   ▼                                                                          │
DidId on success / typed error on failure                                     │
                                                                              │
   [UI: CreateDidWizard reads the WizardStage stream and renders progress ────┘
        states; final state shows DidId + tx hash or the error message]
```

NIGHT sync (`Wallet::sync_unshielded()`) is **not** part of this pipeline — a pure-deploy transaction doesn't move unshielded tokens and the existing Connect flow already populates the NIGHT balance card. Forcing a NIGHT sync here would just add wizard latency for no observable effect.

### File layout

| Path | Role | Status |
|---|---|---|
| `mobile-bench/wallet-core/src/dust/mod.rs` | Public types (`DustOutput`, `DustState`, `DustError`) | **Create** |
| `mobile-bench/wallet-core/src/dust/snapshot.rs` | `Wallet::sync_dust()` — `dustGenerations` subscription, fold into `DustState` | **Create** |
| `mobile-bench/wallet-core/queries/midnight-indexer/dust_generations.subscription.graphql` | Subscription document | **Create** |
| `mobile-bench/wallet-core/src/tx/mod.rs` | Public types (`DeployRequest`, `WizardStage`, `TxError`) + entry point | **Create** |
| `mobile-bench/wallet-core/src/tx/build.rs` | Compose `Intent::new` + `add_deploy` → `Transaction::Standard` (unproven) | **Create** |
| `mobile-bench/wallet-core/src/tx/balance.rs` | Port simplified DUST-only balancer from `TestState::balance_tx` | **Create** |
| `mobile-bench/wallet-core/src/tx/prove.rs` | Wrap `ledger::prove::tx_prove` with our embedded `Resolver` | **Create** |
| `mobile-bench/wallet-core/src/tx/scale.rs` | `Transaction → Vec<u8>` via `serialize::Serializable` | **Create** |
| `mobile-bench/wallet-core/src/artifacts/dust.rs` | Bundle `dust/spend.{bzkir,prover,verifier}` via `include_bytes!`; build a `DustResolver` | **Create** |
| `mobile-bench/wallet-core/src/node/client.rs` | Add `submit_deploy(scale_bytes, signer) → SubmitResult` using subxt typed extrinsic | **Modify** |
| `mobile-bench/wallet-core/src/wallet.rs` | Replace `Wallet::create_did()` stub with the real pipeline | **Modify** |
| `mobile-bench/wallet-core/src/lib.rs` | New modules + re-exports | **Modify** |
| `mobile-bench/dioxus-wallet/src/app.rs` | Replace `CreateDidPanel` with `CreateDidWizard` (progress states) | **Modify** |

## Types

```rust
// In wallet-core/src/tx/mod.rs.

/// Inputs the user-facing deploy flow needs. Exposed so callers
/// (CreateDidWizard) can also use it for the preview.
pub struct DeployRequest {
    pub network: Network,
}

/// Result of a successful deploy: the resolved DID id and the
/// extrinsic hash that produced it.
pub struct DeployOutcome {
    pub did_id: DidId,
    pub tx_hash: [u8; 32],
    pub block_hash: [u8; 32],
}

/// Progress events emitted by the deploy pipeline. The wizard
/// renders the current stage; the order is fixed.
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

pub enum TxError {
    Unshielded(UnshieldedError),
    Dust(DustError),
    Compose(String),
    Balance(String),
    Prove(String),
    ScaleEncode(String),
    Submit(NodeError),
    Inclusion(String),
}
```

```rust
// In wallet-core/src/dust/mod.rs.

pub struct DustState {
    /// All live DUST UTXOs for the wallet, keyed by their ledger
    /// id. Spends in `tx::balance` consume from this set.
    utxos: Vec<DustOutput>,
}

impl DustState {
    pub fn total(&self) -> u128 { /* sum of live UTXO values */ }
    pub fn iter(&self) -> impl Iterator<Item = &DustOutput>;
}

pub enum DustError {
    WsConnect(String),
    Decode(String),
    StreamClosedEarly,
    InvalidDustAddress(String),
}
```

`DustOutput` is intentionally a thin wrapper around the indexer's
`DustOutput` type — same fields, hex-decoded.

## Component interfaces

### `Wallet::sync_dust(&self) → Result<DustState, DustError>`

Mirrors `sync_unshielded`: open a `dustGenerations(dustAddress, startIndex: 0, endIndex: <large>)` subscription, fold events, terminate on the final collapsed `DustGenerationsProgress` carrying `progress.endIndex`. Uses the same `transport::subscribe` we already have.

The wallet's DUST address is derived from `m/44'/2400'/0'/2/0` (account=0, role=Dust, index=0) — Phase 3 added the HD path already. We need to bech32m-encode it with HRP `mn_addr_dust_<networkId>` (analogous to the unshielded HRP), then submit it as the `DustAddress` scalar.

### `Wallet::create_did(&self) → impl Stream<Item = WizardStage>`

Returns an `async_stream::stream!` that:

1. Yields `SyncingDust`, calls `self.sync_dust()`. On error yields `Failed(...)` and stops.
2. Yields `Composing`, calls `tx::build_deploy(pk_commitment, network_id, ts_ms, nonce, rng)` returning an unproven `Transaction<Signature, ProofPreimageMarker, _, _>`.
3. Yields `Balancing`, calls `tx::balance(unproven, &mut dust_state, dust_key, params, time)` returning the balanced unproven tx.
4. Yields `Proving`, calls `tx::prove(balanced, &dust_resolver())` returning the proven `Transaction<Signature, ProofMarker, _, _>`.
5. Yields `Submitting`, calls `tx::scale_encode(proven)` then `node::submit_deploy(bytes, &signer)`.
6. Yields `Confirming`, awaits the subxt `submit_and_watch` future to `wait_for_in_block_success`.
7. Yields `Done(DeployOutcome { did_id, tx_hash, block_hash })`.

The `DidId` is derived client-side (not parsed from the indexer) — the deploy's `ContractDeploy::address()` is deterministic from the `initial_state + nonce` and matches the on-chain address.

### `tx::build_deploy`

Builds the unproven transaction. Decoupled from `Wallet` to keep
the function unit-testable with a hand-supplied controller pubkey
commitment, timestamp, and nonce:

```rust
pub(crate) fn build_deploy(
    pk_commitment: [u8; 32],
    network_id: &str,
    timestamp_ms: u64,
    nonce: [u8; 32],
    rng: &mut impl Rng,
    ttl: Timestamp,
) -> Result<UnprovenTx, TxError> {
    let deploy = crate::did::deploy::compose_deploy(pk_commitment, timestamp_ms, nonce);
    let mut intent: Intent<Schnorr, ProofPreimageMarker, PedersenRandomness, DefaultDB> =
        Intent::empty(rng, ttl);
    intent = intent.add_deploy(deploy);
    let intents = HashMap::new().insert(GUARANTEED_SEGMENT, intent);
    let tx = Transaction::Standard(StandardTransaction::new(
        network_id, intents, None, HashMap::new(),
    ));
    Ok(tx)
}
```

`network_id` is `"undeployed"` / `"preprod"` from `Network::config().network_id` — matches what the node expects in the tx envelope and what the test infrastructure uses (`"local-test"` in `ledger/tests/intent.rs:680`). `Wallet::create_did` is the caller that supplies the timestamp, nonce, RNG, and ttl.

### `tx::balance`

Port the DUST branch of `TestState::balance_tx`. The shielded-zswap branch is omitted (deploys don't use shielded coins). NIGHT balancing is also omitted for this slice — deploys don't move unshielded tokens.

```rust
pub(crate) async fn balance(
    mut tx: UnprovenTx,
    dust_state: &mut DustState,   // mutated: consumed UTXOs removed
    dust_key: &DustSecretKey,
    params: &LedgerParameters,
    time: Timestamp,
) -> Result<UnprovenTx, TxError>;
```

The function iterates `tx.balance(Some(tx.fees(params, false)?))` until the `(Dust, 0)` slot is non-negative, picking DUST UTXOs greedily by value (matching `UtxoSet::pick_for_amount`).

### `tx::prove`

Wrap `ledger::prove::tx_prove` (already exists, gated behind `feature = "proving"`). Pass a `Resolver` built from `artifacts::dust::dust_resolver()` — a `DustResolver` whose `MidnightDataProvider` is in `FetchMode::Synchronous` and serves from the bundled bytes.

For a deploy-only tx with DUST balance, the only proofs `tx_prove` will produce are:

- DUST spend proofs (one per consumed DUST UTXO) — handled by the bundled `DustResolver`.
- The `ContractDeploy` itself does NOT require a circuit proof (the deploy's payload is `(initial_state, nonce)`, not a witness-bearing call).

The `external_resolver` is a closure that returns `None` for every key location — we have no contract-call-specific proving keys in this slice (DID write circuits land later).

### `tx::scale_encode`

```rust
pub(crate) fn scale_encode(tx: &ProvenTx) -> Result<Vec<u8>, TxError> {
    let mut buf = Vec::new();
    serialize::serialize(tx, &mut buf)?;   // not `tagged_serialize` — the
                                            // node expects bare SCALE bytes,
                                            // matches `Midnight.send_mn_transaction`'s
                                            // parameter type per the metadata probe
    Ok(buf)
}
```

### `node::submit_deploy`

Wraps the subxt typed extrinsic:

```rust
impl NodeClient {
    pub async fn submit_deploy(
        &self,
        bytes: Vec<u8>,
        signer: &MidnightSigner,
    ) -> Result<SubmitResult, NodeError> {
        let tx = midnight_runtime::tx().midnight().send_mn_transaction(bytes);
        let progress = self.api
            .tx()
            .sign_and_submit_then_watch(&tx, signer)
            .await?;
        let in_block = progress.wait_for_in_block().await?;
        in_block.wait_for_success().await?;
        Ok(SubmitResult {
            tx_hash: in_block.extrinsic_hash().into(),
            block_hash: in_block.block_hash().into(),
        })
    }
}
```

`SubmitResult` carries `tx_hash` and `block_hash`. `MidnightSigner` already implements `subxt::tx::Signer` per `node/signer.rs`.

### `artifacts::dust`

```rust
const DUST_SPEND_BZKIR: &[u8] = include_bytes!("../../../../../ledger/static/dust/spend.bzkir");
const DUST_SPEND_PROVER: &[u8] = include_bytes!("../../../../../ledger/static/dust/spend.prover");
const DUST_SPEND_VERIFIER: &[u8] = include_bytes!("../../../../../ledger/static/dust/spend.verifier");

pub(crate) fn dust_resolver() -> DustResolver {
    let provider = MidnightDataProvider::new_with_static_bytes(
        DUST_EXPECTED_FILES,
        &[
            ("dust/spend.bzkir",    DUST_SPEND_BZKIR),
            ("dust/spend.prover",   DUST_SPEND_PROVER),
            ("dust/spend.verifier", DUST_SPEND_VERIFIER),
        ],
    );
    DustResolver(provider)
}
```

The `MidnightDataProvider` has an in-memory constructor that doesn't fetch over the network. If the existing API requires a path-based provider, we'll vendor the artifacts into `target`/`OUT_DIR` at build time and point the provider there.

## Data flow

```
sync_unshielded() ─┐
                   ├──> tx::build_deploy ──> Transaction::Standard (unproven)
sync_dust()       ─┤                                      │
                   │                                      ▼
                   │                          tx::balance(dust UTXOs) ──> balanced
                   │                                      │
                   ▼                                      ▼
              DustResolver ──────────────> tx::prove ──> proven
              (bundled keys)                              │
                                                          ▼
                                                  tx::scale_encode
                                                          │
                                                          ▼
                                              node::submit_deploy
                                              (subxt + MidnightSigner)
                                                          │
                                                          ▼
                                                  in-block hash + tx hash
                                                          │
                                                          ▼
                                                  DeployOutcome
                                                          │
                                                          ▼
                                                  WizardStage::Done
```

## UI — `CreateDidWizard`

Replaces the current `CreateDidPanel`. Same mount point in `app.rs`, larger card. Three states:

| State | Render |
|---|---|
| Idle | "Create DID" button + the controller pubkey hex (preview info) |
| Running | Each `WizardStage` becomes a row with `[●] Stage name…` (current) or `[✓] Stage name` (past). Current row shows a spinner. |
| Done | The DID id (clickable; opens indexer URL), the tx hash, the block hash, and a "Create another" button to reset. |
| Failed | The error message in red, the last-completed stage above it, and a "Try again" button. |

The wizard subscribes to the `WizardStage` stream via `spawn(async move { while let Some(stage) = stream.next().await { signal.set(stage); } })`. Dioxus re-renders each time the signal changes.

## Error handling

| Failure | Surface |
|---|---|
| `sync_unshielded` fails | `Failed("syncing NIGHT: <UnshieldedError>")` |
| `sync_dust` fails | `Failed("syncing DUST: <DustError>")` |
| Composition (no DUST in wallet, time-of-day skew, etc.) | `Failed("compose: <reason>")` |
| Balance loop overshoots / can't cover fees | `Failed("balance: insufficient DUST (have X, need Y)")` |
| Proof generation throws | `Failed("prove: <error>")` |
| SCALE encode fails | `Failed("encode: <reason>")` |
| `send_mn_transaction` returns `BadOrigin` / `ImmediateError` | `Failed("submit: <subxt error>")` |
| Tx makes it on-chain but reverts | `Failed("inclusion: <ExtrinsicFailed reason>")` |

No automatic retries. User clicks "Try again" → restart from `SyncingNight`.

## Testing

| Layer | Test | Lives in |
|---|---|---|
| Unit | `DustState::total`, iteration, basic invariants | `dust/mod.rs::tests` |
| Unit | `dust::snapshot::fold_events` against hand-built event stream — termination, error propagation | `dust/snapshot.rs::tests` |
| Unit | `tx::build_deploy` produces a `Transaction::Standard` with one `Intent` containing the expected `ContractDeploy` (hash matches the Phase 3 preview) | `tx/build.rs::tests` |
| Unit | `tx::balance` against a hand-built `DustState` covers the fee; returns `Insufficient` when DUST is exhausted | `tx/balance.rs::tests` |
| Unit | `tx::scale_encode` round-trips through `tagged_deserialize` to confirm the bytes are well-formed | `tx/scale.rs::tests` |
| Integration | Live deploy against the standalone stack — full pipeline runs to `Done`, asserts that `DidId` parses and matches the preview | `tests/deploy_undeployed_live.rs`, gated by `--features network-tests` |

The integration test is the proof point. It exercises the bundled DUST resolver, the actual indexer subscriptions, the actual node submission. Same gating as `unshielded_live`.

## Open questions to verify during implementation

1. **`MidnightDataProvider` static-bytes constructor.** The reference in `test_resolver` uses `FetchMode::OnDemand` from a file path. We need a static-bytes / in-memory mode. If it doesn't exist, vendor the artifacts into a tempdir at startup and use `FetchMode::Synchronous`. Either path works; the question is which is less code.

2. **DUST address format.** Phase 3 derives the secret at `m/44'/2400'/0'/2/0`, but the bech32m HRP and payload format for DUST is presumed to be `mn_addr_dust_<networkId>` + SHA-256 of the public key. Confirm against the indexer schema — if the `DustAddress` scalar wants raw hex, we encode differently.

3. **`Intent::empty(rng, ttl)`'s `ttl` argument.** `TestState::balance_tx` uses `state.time`. We need a sensible default for "now" — `SystemTime::now()` converted to whatever `Timestamp` the ledger expects. Confirm the type + units.

4. **PreProd specifics.** Does the deploy go through cleanly without any per-network adjustment beyond `network_id` string and the URL flip? Test on PreProd once Undeployed is green; surface any gotchas as follow-up tasks rather than blocking on speculation.

## Out-of-scope follow-ups

- DID payload customization (verification methods, services, also-known-as) — needs write circuits.
- DUST registration UI for PreProd-without-pre-funded-seed users.
- Finalization waiting (`wait_for_finalized_success`) — one-line change once inclusion is shipped.
- Tx history persistence.
- Multi-account / multi-wallet support.
- Mainnet readiness review.
