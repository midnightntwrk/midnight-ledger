# Midnight DID — Rust-native plan

Drafted 2026-04-30. Status: **plan**, implementation starting now.

## Decision

We **port midnight-did's user-facing API to Rust** rather than calling
the upstream TS/JS package from inside the Dioxus WebView.

The previous direction (vendor the npm tree, serve via Wry custom
protocol, load via import map, call from `<script type="module">`)
proved technically viable — we got `await
import("@midnight-ntwrk/midnight-did-contract")` to instantiate
real WebAssembly inside WKWebView through a `mn-pkg://` handler — but
shipping it long-term means:

- Maintaining a JS toolchain (esbuild + polyfills + vendor pipeline)
  inside a Rust workspace.
- Tracking a moving npm target (5+ Midnight packages, dozens of
  transitive deps) for every upstream release.
- Re-implementing each missing browser polyfill (`fs`, `crypto`,
  `level`, etc.) when Node-only paths reach for them.
- Living with the WebView lifecycle and Mobile-vs-Desktop drift on
  every platform.

The TS layer adds *no* domain logic the Rust workspace doesn't
already have — it's a thin orchestrator over the same crates we
already link (`ledger`, `zswap`, `transient-crypto`, `coin-structure`,
`onchain-runtime`, `zkir`). We get a smaller, faster, native API by
porting the orchestration to Rust directly.

## What we keep, what we retire

### Keep
- **wallet-core**: `Network`, `Wallet`, BIP32 HD, bech32m address
  codec, indexer GraphQL client (read), node JSON-RPC client.
- **prover-core**: ZK prover wrapper with the local HTTP server. Same
  proof-server protocol; we drive it from Rust instead of via TS.
- **dioxus-wallet UI shell**: dark theme, mobile-sized window,
  network selector, status pill, address pill, balances card,
  Advanced disclosure.
- **JSON-RPC bridge** (bridge.rs): kept for UI ↔ Rust signal flow
  (clipboard, dialogs, etc.) — not for contract logic.

### Retire (default-off behind `js-bridge` cargo feature)
- `mobile-bench/dioxus-wallet/web/` — esbuild bundle, vendor.mjs,
  polyfill stack, ws-shim, unsupported-stub.
- `mobile-bench/dioxus-wallet/src/protocol.rs` — Wry `mn-pkg://`
  custom-protocol handler.
- Head injection that pasted the import map + bundle into the
  WebView.
- The vendored `assets/web/pkg/` tree.

These stay in the source tree but are gated behind
`--features js-bridge`. If someone ever needs to load an unported
TS package (research demo, one-off), they can flip the feature on
and the existing harness still works. The `web/` dir is ignored by
the default build path and doesn't pull npm into normal CI.

## API surface (Rust)

Mirrors `midnight-did-domain` + a subset of `midnight-did-api`. Types
are `Clone + Debug + Serialize + Deserialize` end-to-end.

```rust
// wallet_core::did

/// `did:midnight:<network>:<contract_address>` (64-hex contract
/// address). Parses both `mn_did_*` bech32m form (the human-readable
/// alias gsd-wallet uses) and the canonical did-string form.
pub struct DidId {
    pub network: Network,
    pub contract_address: ContractAddress, // 32 bytes
}

impl DidId {
    pub fn parse(s: &str) -> Result<Self, DidIdError>;
    pub fn to_did_string(&self) -> String;       // "did:midnight:..."
    pub fn to_bech32m(&self) -> String;          // "mn_did_*..."
}

/// DID Core 1.0 document, restricted to Midnight's verification-method
/// + service shape (Ed25519 / P-256 / Jubjub).
pub struct DidDocument {
    pub id: DidId,
    pub controller: Option<DidId>,
    pub also_known_as: Vec<String>,
    pub verification_methods: Vec<VerificationMethod>,
    pub authentication: Vec<VerificationMethodRef>,
    pub assertion_method: Vec<VerificationMethodRef>,
    pub key_agreement: Vec<VerificationMethodRef>,
    pub capability_invocation: Vec<VerificationMethodRef>,
    pub capability_delegation: Vec<VerificationMethodRef>,
    pub services: Vec<Service>,
    pub deactivated: bool,
    pub created: SystemTime,
    pub updated: SystemTime,
    pub version: u64,
}

pub struct VerificationMethod {
    pub id: String,                              // "<did>#<fragment>"
    pub typ: VerificationMethodType,             // JsonWebKey
    pub controller: DidId,
    pub public_key: PublicKeyJwk,                // OKP/Ed25519 | EC/P-256 | EC/Jubjub
}

pub enum CurveType { Ed25519, P256, Jubjub }
pub enum KeyType   { OKP, EC }

/// Read-only resolution. Phase 1.
impl Wallet {
    pub async fn resolve_did(&self, id: &DidId) -> Result<DidDocument, DidError>;
}

/// Write surface. Phase 2.
impl Wallet {
    pub async fn create_did(&self, request: CreateDidRequest) -> Result<DidId, DidError>;
    pub async fn add_verification_method(&self, did: &DidId, vm: VerificationMethod) -> Result<(), DidError>;
    pub async fn remove_verification_method(&self, did: &DidId, vm_id: &str) -> Result<(), DidError>;
    pub async fn add_service(&self, did: &DidId, svc: Service) -> Result<(), DidError>;
    pub async fn deactivate_did(&self, did: &DidId) -> Result<(), DidError>;
    // ... mirroring midnight-did-api::lib.ts
}
```

## Phases

### Phase 1 — types, codec, resolver shell (this week)

- [ ] Add `js-bridge` cargo feature to `dioxus-wallet`. Gate
      `protocol.rs` + the head injection + the `web/` build pipeline
      behind it. Default off.
- [ ] `wallet-core::did::types` — port `midnight-did-domain` Zod
      schemas to Rust structs, all serde-friendly.
- [ ] `wallet-core::did::id` — `DidId` parser + bech32m codec, both
      did-string and `mn_did_*` formats. Exhaustive unit tests on
      mainnet / testnet / undeployed prefixes.
- [ ] `Wallet::resolve_did` returning a stub `IndexerNotImplemented`
      error with a clear "not yet wired" message — provides the
      callsite for the UI button before we wire the indexer queries.
- [ ] First UI surface: a "Resolve DID" textfield in the Dioxus
      app's Advanced section that calls `Wallet::resolve_did` and
      renders the result (or the stub error).

### Phase 2 — real resolution (next iteration)

- [ ] Vendor the contract artifacts into
      `mobile-bench/wallet-core/contracts/midnight-did/`:
  - `did.compact` (source — for documentation only)
  - `keys/*.{prover,verifier}` (one pair per circuit)
  - `zkir/*.{zkir,bzkir}` (one per circuit)
  - `contract/index.{js,d.ts}` source as reference for state layout
- [ ] `wallet-core::did::contract::DidLedgerState` — Rust struct
      mirroring `did.compact`'s on-chain state. SCALE-decoded from
      the indexer contract-state payload.
- [ ] `wallet-core::did::contract::ledger_to_domain` — port of
      `midnight-did/did/src/ledger-to-domain.ts`. Pure function:
      `(DidLedgerState, NetworkId, ContractAddress) -> DidDocument`.
- [ ] Indexer query: get latest contract state by address. Decide
      between (a) replay every `ContractAction` since deploy, (b) a
      direct `block.contractAction(address).state` query if v4
      schema exposes one. Add the necessary GraphQL operation to
      `wallet-core/queries/`.
- [ ] Wire `Wallet::resolve_did` to actually fetch + decode + map.

### Phase 3 — first write circuit, contract deploy (multi-day)

- [ ] `wallet-core::did::contract::compile` — single function that
      loads PKM + IR for a circuit name from the vendored artifacts
      (`include_bytes!` so they ship with the binary).
- [ ] `Wallet::create_did(request)`:
  - Builds the `addVerificationMethod` initial-state input.
  - Hands `(ProofPreimage, ProvingKeyMaterial)` to
    `prover-core::ProverCore`.
  - Wraps the resulting proof in a deploy extrinsic.
  - Submits via subxt + `midnight-node-metadata` (git dep).
  - Watches finalisation, returns the contract address as `DidId`.
- [ ] All necessary glue between the wallet's keys + balances + the
      contract call. **Depends on the wallet's unshielded sync**
      (Phase B of `WALLET_PLAN.md`) for fee balancing.
- [ ] First end-to-end smoke: click "Create DID" in the UI →
      contract deploys against preprod → resolver returns the new
      document.

### Phase 4 — full midnight-did-api parity

Add one circuit at a time, each in its own commit:
`addVerificationMethod`, `updateVerificationMethod`,
`removeVerificationMethod`, `addVerificationMethodRelation`,
`removeVerificationMethodRelation`, `addService`, `updateService`,
`removeService`, `addAlsoKnownAs`, `removeAlsoKnownAs`, `deactivate`.
Each follows the same recipe from Phase 3 — input encoding, prove,
submit, decode result. Mostly mechanical once the deploy flow
works.

## Open questions / things to check before Phase 2

1. **Indexer contract-state query**: does v4 expose a single-call
   "give me current contract state for address X"? The schema we
   vendored shows `block.transactions[].contractActions[]` filterable,
   but a direct state read would simplify the resolver. Survey
   `indexer-tests/e2e.graphql` for an example.
2. **`did:midnight:<network>:<address>` HRP convention**: confirm
   exact network-id strings from
   `midnight-wallet/packages/abstractions/src/NetworkId.ts`. Earlier
   audit showed `mainnet | testnet | preprod | preview | qanet | devnet | undeployed`
   — check no surprise renames in `midnight-did/domain/src/midnight.ts`.
3. **DID URL fragment normalisation**: midnight-did-domain has a
   `normalizeBoundFragmentId` function. Pure-string logic; decide
   whether to port or to rely on a downstream did-resolver crate
   from the broader Rust ecosystem.
4. **Witness data**: `midnight-did/contract/src/witnesses.ts`
   provides off-chain inputs the circuits read. For Rust, port to a
   small `Witnesses` struct. Should be ~50 LoC.

## Reference index

- TS source we're porting:
  - <https://github.com/midnightntwrk/midnight-did/tree/develop/domain> — types
  - <https://github.com/midnightntwrk/midnight-did/tree/develop/did> — resolver + ledger-to-domain mapping
  - <https://github.com/midnightntwrk/midnight-did/tree/develop/api> — write API
  - <https://github.com/midnightntwrk/midnight-did/tree/develop/contract> — Compact source + compiled artifacts
- Compiled artifacts on disk: `~/iohk/midnight-did/contract/dist/managed/did/`
- Local survey results: see `mobile-bench/AGENT.md` "midnight-did".
