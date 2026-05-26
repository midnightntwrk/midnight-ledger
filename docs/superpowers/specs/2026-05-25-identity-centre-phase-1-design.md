# Identity Centre — Phase 1 (issuance + self-verify)

**Date:** 2026-05-25
**Branch:** `dioxus-vc-demo` (off `mobile-prototype`)
**Status:** Approved design — ready for implementation planning
**Author:** Yurii Shynbuiev with Claude

## Goal

Ship a working **Identity Centre** in the dioxus-mobile wallet that lets a
holder receive a Midnight `birth` Verifiable Credential from a mock issuer
over standard OID4VP + OID4VCI flows, view the credential as a card in a
swipeable carousel, and tap to **self-verify** the credential against the
issuer's published DID document — all running against a local standalone
Midnight environment, all driven by Gherkin-style integration tests, and
with no dependency on the real DIDIT KYC vendor (which is mocked by an
operator-driven form in the issuer app for this phase).

This is the first of three phases. Phase 1 covers issuance + holder
self-inspection. Phase 2 adds a separate verifier app doing plain
selective-disclosure verification. Phase 3 adds predicate proofs via
Compact smart-contract circuits.

## Scope

### In scope for Phase 1

- **Mobile wallet** (`mobile-bench/dioxus-wallet`, Rust + native Dioxus UI,
  Android-first):
  - New top-level `Identity` tab replacing/absorbing the existing DID and
    Keys surfaces.
  - `VCs` sub-tab with a full-screen swipeable carousel of credential cards.
  - `DIDs` sub-tab with the existing DID list + detail; per-DID keys nested
    inside DID detail.
  - Floating-action-button on the Identity tab that opens a native QR
    scanner (Android: CameraX + ML Kit via JNI; iOS sim: dev paste-URL
    affordance only).
  - `Bootstrap` button on the Identity tab that calls
    `wallet-core::bootstrap_did_with_keys` to create a holder DID with both
    Ed25519 (authentication relation) and Jubjub (assertionMethod relation)
    verification methods.
  - DID-picker popup that appears during QR-scan flow when >1 bootstrapped
    DID exists.
- **Mock issuer** (`midnight-identity-solution-examples/IssuerDIDIT-mock`,
  TypeScript + Express):
  - Six HTTP endpoints matching OID4VP + OID4VCI shape (the contract is
    stable across the mock and the real `IssuerDIDIT` that another
    engineer will deliver).
  - Server-rendered laptop-browser pages for QR-1 (auth) and QR-2 (VC
    offer).
  - Operator-driven KYC form replacing the real DIDIT flow. Field set
    matches what DIDIT's `id_verifications[]` would return: `firstName`,
    `lastName`, `dateOfBirth`, `nationality`, `documentNumber`.
  - `Bootstrap` button on the issuer's web UI to create its DID with
    Jubjub assertion key.
  - SQLite storage for sessions + issued VCs.
- **Wallet-core extensions** (`mobile-bench/wallet-core`, Rust):
  - `vc_store` module (three redb tables — `vcs`, `vc_openings`,
    `vc_metadata`).
  - `did_auth` glue module — "given a DID, find its authentication-relation
    key, sign payload, return (kid, signature)".
  - `oid4vp_client` module — parse `openid4vp://` payload, build SIOPv2
    id-token JWS, POST to authorization endpoint.
  - `oid4vci_client` module — parse `openid-credential-offer://` payload,
    run Pre-Authorized Code Flow, request credential with DID-bound JWS
    proof, hand parsed VC to `vc_store`.
  - `vc_self_verify` module — re-resolve issuer DID, check VC signature
    against `assertionMethod` key, return three-state result
    (`Valid { resolved_at }` / `Stale { age_seconds }` / `Invalid(reason)`).
  - `bootstrap_did_with_keys` helper + `did-bootstrap` CLI binary.
- **Standalone Midnight environment**:
  - `docker-compose.yml` spec under `IssuerDIDIT-mock/e2e/fixtures/`
    bringing up node + indexer + (in-process) proof-server.
  - Bootstrap scripts that produce reproducible DIDs from deterministic
    seeds, idempotent across reruns of a clean env.
- **BDD integration tests**:
  - Cucumber.js + TypeScript + Playwright harness in
    `IssuerDIDIT-mock/e2e/`.
  - Headless TS wallet client (`headless-wallet-client.ts`) mirroring the
    Rust wallet's HTTP behaviour.
  - Feature files for bootstrap, issuance happy path, self-verify, and the
    three high-value negative paths (wrong nonce, replay, unbootstrapped
    DID).

### Out of scope for Phase 1 (deferred to later phases)

- **Verifier app** (Phase 2 — `B`).
- **Predicate proofs via Compact circuits** (Phase 3 — `D` ultimate).
- **Real DIDIT integration**. The real `IssuerDIDIT` lands in a parallel
  workstream by another engineer; we ship against the stable HTTP contract
  defined here.
- **iOS support**. The Android-only QR scanner bridge is built in this
  phase; the iOS `AVCaptureMetadataOutput` bridge ships later.
- **Mobile WebView for KYC**. The didit.me flow always runs in the laptop
  browser for Phase 1 (Sim has no camera; production deployment can move
  KYC into a mobile WebView in a later phase).
- **PreProd or any non-local chain**. Standalone env only.
- **Multi-issuer trust UI**. Issuer DID is whichever the QR carries; no
  policy beyond "is it a valid Midnight DID document".
- **Revocation** for `birth` VCs in this demo.
- **DID creation UI flow** beyond the single `Bootstrap` button. No
  multi-key, multi-relation editing surface in Phase 1.
- **Wallet-side WebView changes**. The existing `mn-pkg://` WebView used
  by the contract layer is untouched.

## Phasing roadmap

| Phase | Letter | What lands | Why this phase |
|---|---|---|---|
| **1 — now** | `C` | Issuance + holder self-verify. No verifier app. | Fastest path to a demoable end-to-end trust chain. |
| **2** | `B` | Plain-selective-disclosure verifier app over OID4VP. Wallet generates a VP that reveals committed claims with their openings; verifier checks. | Closes the issue→verify loop without smart-contract complexity. |
| **3 — ultimate** | `D` | Predicate proofs via Compact verifier-contract circuits ("prove DOB < 2007-01-01" without revealing DOB). | Production-grade ZK-backed verification. The actual point of building on Midnight. |

## System overview

Three actors that never share memory and only talk over HTTPS or the chain:

```
                      adb reverse/HTTPS                  HTTPS (mocked in Phase 1)
   ┌──────────────┐ ◄────────────► ┌──────────────┐   X    ┌──────────────┐
   │ dioxus-wallet│  OID4VP/VCI    │  IssuerDIDIT │ ─ ─ ─→ │   didit.me   │
   │  (Android    │                │     -mock    │        │ (NOT in our  │
   │   emulator,  │                │ (TS+Express, │        │  Phase 1     │
   │   later real │                │  serves HTML │        │  build —     │
   │   devices)   │                │  to laptop)  │        │  operator    │
   └──────┬───────┘                └──────┬───────┘        │  form does   │
          │                               │                │  its job     │
          │ Compact tx + DID resolve      │ resolve holder │  instead)    │
          ▼                               ▼ DID            └──────────────┘
   ┌──────────────────────────────────────────┐
   │  Midnight Standalone Env (local docker)  │
   │  node + indexer + proof-server (in-proc) │
   └──────────────────────────────────────────┘
```

### Role split

| Actor | Responsibility | Secrets held |
|---|---|---|
| **dioxus-wallet** | Stores holder DIDs, keys, VCs + openings. Scans QR-1/QR-2. Signs SIOPv2 id-tokens with Ed25519. Receives VC over OID4VCI. Self-verifies VCs against resolved issuer DIDs. | Holder Ed25519 + Jubjub keys |
| **IssuerDIDIT-mock** | Renders QR-1 + QR-2 to laptop browser. Validates OID4VP id-tokens. Drives operator KYC form (substituting for real DIDIT). Mints a Midnight `birth` VC after form submit. Serves the VC over OID4VCI. | Issuer DID + Jubjub assertion key |
| **Midnight standalone env** | Hosts the chain (node + indexer + in-process proof-server). DID resolution. | Network secrets only |

No actor has more than one job. The wallet doesn't do KYC. The issuer doesn't
store holder secrets. The chain knows nothing about credential bodies.

### Communication channels

| # | From → To | Transport | Used for |
|---|---|---|---|
| 1 | Wallet → IssuerDIDIT-mock | HTTPS (over `adb reverse` or ngrok) | OID4VP `POST /authorize-response`, OID4VCI `POST /token` + `POST /credential` |
| 2 | Laptop browser → IssuerDIDIT-mock | HTTPS | Server-rendered HTML pages — `GET /authorize`, KYC form, `GET /credential-offer/:id` |
| 3 | IssuerDIDIT-mock → standalone env | HTTP (localhost) | Holder DID resolution via GraphQL indexer; issuer DID bootstrap |
| 4 | Wallet → standalone env | HTTP (localhost via emulator host bridge) | DID resolve, VC self-verify |

Communication is **pure pull from the wallet's side**: the wallet initiates
every exchange with the issuer. No websocket, no server-pushed channel.
Communication only happens when the user scans a QR.

### Data at rest

| Side | Store | Contents |
|---|---|---|
| `dioxus-wallet` | Existing `wallet.redb` + new `vc_store` tables (same redb file) | DIDs, keys, VC bodies, private claim openings, VC metadata |
| `IssuerDIDIT-mock` | SQLite `issuer.sqlite` + `issuer-keystore.json` (gitignored) | Sessions, issued VCs, issuer DID + Jubjub key |
| Standalone env | Docker volume — node DB + indexer DB | Chain state |

## Mobile architecture (`dioxus-wallet`)

### Screen tree

```
App
├─ Wallet              ← existing
├─ Identity            ← NEW top-level tab
│   ├─ VCs   (default sub-tab)
│   │   └─ <full-screen swipeable carousel>
│   │        ├─ VC card 1
│   │        ├─ VC card 2
│   │        └─ ...
│   ├─ DIDs (sub-tab — lifted from former top-level screen)
│   │   ├─ DID list
│   │   └─ DID detail
│   │        └─ Keys (lifted from former top-level Keys tab)
│   ├─ [Bootstrap] action button (shown until ≥1 bootstrapped DID exists)
│   └─ [FAB] Scan QR
├─ Bench               ← existing
└─ About               ← existing
```

The existing `Keys` tab disappears from the bottom bar entirely; its content
lives inside each DID's detail screen as "Keys for this DID", aligning with
the W3C semantic that verification methods are part of a DID document.

The FAB is scope-aware: on the `VCs` sub-tab it expects `oid4vci` payloads;
on `DIDs` it expects `oid4vp`. Both routes share the underlying scanner; the
distinction is only in the post-scan dispatch.

### Native QR scanner bridge

Single function per platform, returns a string (the scanned URL) or an
error. Trait surface in Rust:

```rust
// wallet-core/src/qr_scanner.rs
pub trait QrScanner: Send + Sync {
    /// Open a live camera preview, return when a QR is decoded or the
    /// user cancels. On iOS Sim the implementation MAY ignore the camera
    /// and return a string from a "paste URL" UI affordance instead.
    fn scan(&self) -> Pin<Box<dyn Future<Output = Result<String, QrScanError>>>>;
}
```

Android implementation (`android/.../QrScanner.kt`): CameraX + ML Kit
Barcode Scanning + JNI bridge to the Rust trait via the existing
`cargo-ndk` toolchain. ~200 Kotlin LOC + ~50 Rust trait LOC.

iOS implementation (`ios/App/QrScanner.swift`): deferred to a later
phase. The dev "paste URL" affordance lives in the Dioxus UI, not the
native bridge, so iOS sim is functional in Phase 1 via that path.

### New wallet-core modules

All in `mobile-bench/wallet-core/src/` so future non-Dioxus hosts (RN
demo, headless test harness) reuse the same primitives.

| Module | Responsibility | LOC est. |
|---|---|---|
| `vc_store/` | Three redb tables (`vcs`, `vc_openings`, `vc_metadata`) + serde codecs + CRUD API + iteration with metadata join. Generic over `VC<TClaims, TCommitments, _, _>`. | ~400 |
| `oid4vp_client/` | Parse `openid4vp://` deep-link payload. `presentation_definition` interpreter (Phase 1: ignored — pure auth). SIOPv2 id-token JWS builder (`EdDSA`, `kid = <did>#<frag>`, claims `{iss, aud, nonce, iat, exp}`). POST to `redirect_uri`. | ~250 |
| `oid4vci_client/` | Parse `openid-credential-offer://`. Pre-Authorized Code Flow token exchange. Credential request with `proof_type = jwt` carrying a fresh DID-bound JWS. Parse returned VC + openings. Hand to `vc_store`. | ~300 |
| `did_auth/` | "Given a DID, find its `authentication`-relation key, find the local `SecretKeyRef`, sign payload, return `(kid, signature)`." | ~50 |
| `vc_self_verify/` | Re-resolve issuer DID, pick `assertionMethod` Jubjub key, verify VC's signature. Returns `Valid` / `Stale` / `Invalid`. | ~150 |
| `did/bootstrap.rs` | `bootstrap_did_with_keys` — orchestrates create-DID + addVerificationMethod ×2 + addVerificationMethodRelation ×2. Deterministic from a seed. | ~150 |

Plus a small `did-bootstrap` CLI binary that wraps the bootstrap helper
for shell scripts and BDD harness setup.

### New Dioxus UI components

In `mobile-bench/dioxus-wallet/src/identity/`:

| Component | Job |
|---|---|
| `IdentityScreen` | Sub-tab router (VCs / DIDs). |
| `VcCarousel` | Full-screen pager driven by `vc_store::list_ordered()`. Swipe to advance. |
| `VcCard` | Renders one VC: issuer name + DID, type (`birth`), public claims as labelled rows, "View private fields" disclosure, "Self-verify" button → calls `vc_self_verify` → shows ✓/✗ + timestamp. |
| `DidList` | Replaces the existing top-level DIDs screen (lifted into this sub-tree). |
| `DidDetail` | Shows the resolved DID document + each verification relation's keys, with the existing add-key affordance. |
| `BootstrapPanel` | Shown when no bootstrapped DID exists. Single button → calls `bootstrap_did_with_keys` → progress indicator → on success, transitions to VC carousel (empty state). |
| `DidPickerPopup` | Shown during scan-flow dispatch when >1 DID is bootstrapped. User picks which DID to authenticate with. Single-DID case skips this entirely. |
| `QrScanFab` | The FAB; opens `QrScanModal`. |
| `QrScanModal` | Hosts the native QR scanner. Sim-mode adds an `<input type="text">` "Paste URL" affordance. Routes scanned URL to `oid4vp_client` or `oid4vci_client` based on scheme. |

### Happy-path flow (one cycle)

```
User taps FAB on Identity/VCs
  → QrScanModal opens
    → native scanner returns "openid-credential-offer://issuer.local/offer?..."
  → modal routes to oid4vci_client
    → parse offer → look up did_auth → mint JWS proof → POST token → POST credential
    → receive VC body + openings
    → vc_store::insert(vc, openings, metadata)
  → modal dismisses → VcCarousel re-renders → new card appears at the head
```

## Mock issuer (`IssuerDIDIT-mock`)

### Repo placement

```
~/iohk/midnight-identity-workspace/midnight-identity-solution-examples/
└─ IssuerDIDIT-mock/        ← new package
   ├─ package.json
   ├─ src/
   │   ├─ server.ts        ← Express entrypoint
   │   ├─ routes/
   │   │   ├─ login.ts             ← QR-1 (OID4VP auth)
   │   │   ├─ kyc.ts               ← operator form (the mock surface)
   │   │   └─ credential.ts        ← QR-2 (OID4VCI offer + issuance)
   │   ├─ services/
   │   │   ├─ issuerDid.ts         ← load issuer DID + Jubjub assertion key
   │   │   ├─ holderDidResolver.ts ← embedded resolver via @midnight-ntwrk/midnight-did
   │   │   ├─ oid4vpVerifier.ts    ← verify SIOPv2 id_token JWS
   │   │   ├─ oid4vciIssuer.ts     ← mint Pre-Authorized Code, serve VC
   │   │   └─ vcMinter.ts          ← assemble + sign the `birth` VC body
   │   ├─ storage/sessions.ts      ← SQLite via better-sqlite3
   │   └─ views/                   ← server-rendered HTML pages for the laptop
   ├─ scripts/
   │   └─ bootstrap-issuer-did.ts  ← one-time DID + keys via @midnight-ntwrk/midnight-did-api
   └─ e2e/                          ← Cucumber.js BDD specs + headless wallet
```

### HTTP contract (stable across mock and real issuer)

| Method + path | Phase of flow | Wallet sends | Issuer returns |
|---|---|---|---|
| `GET /authorize` | QR-1 contents — page rendered on the **laptop**, not the phone | — | HTML with QR-1: `openid4vp://...?request_uri=https://issuer/.../request/<id>` |
| `GET /request/:id` | After scan, wallet fetches the auth request object | — | `{client_id, nonce, presentation_definition: null, state}` — SIOPv2 only (no VC presentation required) |
| `POST /authorize-response` | Wallet posts signed id_token | `{id_token: <JWS>, state}` | `{session_id, status: "authenticated"}` |
| `GET /credential-offer/:session_id` | QR-2 contents — rendered after operator submits KYC form | — | HTML with QR-2: `openid-credential-offer://...?credential_offer=...` |
| `POST /token` | OID4VCI Pre-Authorized Code Flow token endpoint | `{grant_type: "...pre-authorized_code", pre-authorized_code}` | `{access_token, c_nonce}` |
| `POST /credential` | Wallet requests the actual VC | `{proof: {proof_type: "jwt", jwt: <DID-bound JWS over c_nonce>}, format: "midnight-vc-compact"}` | Signed Midnight `birth` VC body + opening blobs |

Six endpoints. Same contract for mock and real issuer.

### Session state

```typescript
type Session = {
  id: string;                       // server-minted UUID
  status: 'authorized' | 'kyc_done' | 'vc_issued' | 'failed';
  holder_did: string;               // bound at POST /authorize-response
  oid4vp_nonce: string;             // server-minted, returned in /request/:id
  vc_claims?: BirthVcClaims;        // populated at /kyc-form submit (mock)
                                    //  or webhook (real, future)
  pre_authorized_code?: string;     // minted at /credential-offer/:session_id
  c_nonce?: string;                 // minted at POST /token
  vc_uri?: string;                  // populated at POST /credential
  vc_body?: Uint8Array;             // the signed Compact VC
  created_at: number;
  updated_at: number;
};
```

`BirthVcClaims` matches the existing `birth` family schema in
`midnight-verifiable-credentials/midnight-did-credentials-birth/` — we
don't invent a new shape, we just bypass the KYC source.

### Mock surface — operator form

Phase 1's substitute for the real DIDIT flow. Single HTML form on
`/kyc-form` with these inputs:

```
firstName       (text)
lastName        (text)
dateOfBirth     (date, ISO 8601)
nationality     (ISO 3166-1 alpha-3 dropdown)
documentNumber  (text)
[Submit]
```

Submit handler:

1. Validate inputs (non-empty, plausible date).
2. `await sleep(KYC_DELAY_MS)` — default 2 000 ms for manual demos so the
   operator sees a brief "verifying..." state; 0 in CI; configurable.
3. Write claims into `sessions.vc_claims`.
4. Mark session `status = 'kyc_done'`.
5. Redirect laptop browser to `/credential-offer/:session_id`.

When the real `IssuerDIDIT` lands, only two methods change:

- `vcMinter.kycSourceClaims()` reads `decision.id_verifications[0]`
  instead of `sessions.vc_claims`.
- A new `POST /webhook/didit` route + canonical V2 signature verification
  (per the `integrate-didit` skill that lives in the issuer repo's
  `.claude/skills/`).

Everything else — the six HTTP endpoints, the storage shape, the bootstrap
script, the BDD harness — stays the same.

### Embedded DID resolution

The issuer's `holderDidResolver.ts` imports `@midnight-ntwrk/midnight-did`
directly (the `midnight-did-resolver.ts` + `offchain-midnight-did.ts`
modules in that package). In-process resolution against the standalone
indexer's GraphQL. No sidecar service.

### Issuer DID bootstrap

`scripts/bootstrap-issuer-did.ts` uses `@midnight-ntwrk/midnight-did-api`
to:

1. Connect to local indexer + node.
2. Create an empty Midnight DID.
3. Generate a Jubjub key pair + `addVerificationMethod` +
   `addVerificationMethodRelation(AssertionMethod)`.
4. Generate an Ed25519 key pair + `addVerificationMethod` +
   `addVerificationMethodRelation(Authentication)` (symmetry with holder,
   useful for future flows).
5. Write `{did, jubjubSecret, ed25519Secret}` into `issuer-keystore.json`
   (gitignored).

Deterministic from a seed (`ISSUER_BOOTSTRAP_SEED` env var, default
`"issuer-demo-seed"`).

Triggered either by a CLI command (`yarn bootstrap`) or by clicking the
**Bootstrap** button on the issuer's web UI (shown when
`issuer-keystore.json` is absent).

## DID bootstrap prerequisites

Standalone env has no `preprod-live`-style pre-seeding. Both wallet and
issuer start with an empty chain and must run their own bootstrap to get
into a usable state.

Both sides need DIDs with the **same shape**:

- Ed25519 in `authentication` relation → signs SIOPv2 id-tokens.
- Jubjub in `assertionMethod` relation → signs VCs (issuer) / VPs in
  Phase 2 (wallet).

### Shared helper

`wallet-core/src/did/bootstrap.rs`:

```rust
pub async fn bootstrap_did_with_keys(
    wallet: &Wallet,
    secret_store: &dyn SecretStorage,
    seed: &[u8; 32],
) -> Result<BootstrappedDid> {
    // 1. Derive Ed25519 + Jubjub keys from `seed` via deterministic KDF
    // 2. Wallet::create_did                                   (1 tx)
    // 3. addVerificationMethod(ed25519, "key-auth")           (1 tx)
    // 4. addVerificationMethodRelation("key-auth", Authentication)
    // 5. addVerificationMethod(jubjub, "key-assert")
    // 6. addVerificationMethodRelation("key-assert", AssertionMethod)
    // 7. Resolve + verify the DID document carries both relations
    Ok(BootstrappedDid { did, ed25519_ref, jubjub_ref })
}
```

### Two callers, two surfaces

| Caller | Surface | Implementation |
|---|---|---|
| Wallet | `Identity Centre → Bootstrap` button | Calls `bootstrap_did_with_keys` directly via `wallet-core` |
| Issuer | `IssuerDIDIT-mock → Bootstrap` button (and `yarn bootstrap` CLI) | Calls `@midnight-ntwrk/midnight-did-api` directly in TS |
| Scripts / BDD harness | shell | Calls `did-bootstrap` CLI binary out of `wallet-core` |

The `did-bootstrap` CLI binary ships out of `wallet-core` and accepts
`--indexer-url`, `--node-rpc-url`, `--seed-hex`, `--out path/to/output.json`.

### Fee funding (standalone env)

DID creation and each `addVerificationMethod` / `addVerificationMethodRelation`
call is an on-chain transaction and consumes NIGHT for fees. Standalone env
ships with a pre-funded operator wallet whose seed is published in the
midnight-did integration-test fixtures; both Bootstrap paths use the same
operator wallet to pay for the six bootstrap txs (DID create + 2×addVM +
2×addRelation, ×2 for symmetric Ed25519 + Jubjub). The existing
`Wallet::sync_unshielded()` path already provides the UTXO snapshot the
balancer needs (Subsystem A on this branch). No new fee-funding mechanism
is introduced by this design.

### Deterministic seeds (per midnight-did integration-test convention)

Bootstrap takes a seed input so:

- Standalone env reset → re-run bootstrap → same DID.
- BDD scenarios can predict the resulting DID for assertions.

Fixture seeds for the demo:

| Actor | Seed string (UTF-8) | Expected role |
|---|---|---|
| Holder | `holder-demo-seed` | Bootstrapped on wallet's first run |
| Issuer | `issuer-demo-seed` | Bootstrapped on issuer's first run |
| (BDD) Bob | `bob-demo-seed` | Reserved for multi-holder scenarios in Phase 2 |

### Multi-DID handling

Phase 1 ships single-DID by default. Bootstrap can be re-run to create
additional DIDs. When >1 DID exists, scan-flow dispatch shows a
`DidPickerPopup` before signing the SIOPv2 id-token. Single-DID case
skips the popup entirely.

## Self-verification mechanic

The user-visible feature that distinguishes Phase 1 from a vanilla
"download VC and show card" demo: tap a card → wallet re-resolves the
issuer DID against the chain → verifies the VC signature against the
issuer's `assertionMethod` Jubjub key → reports a three-state result.

### Result states

| State | Badge | Subtitle |
|---|---|---|
| `Valid { resolved_at }` | green check | "Signed by `did:midnight:abc…` — last checked `12:34:56`" |
| `Stale { age_seconds }` | grey clock | "Last check was N minutes ago; tap to re-verify" |
| `Invalid(reason)` | red ✗ | One of: "Issuer DID no longer resolves", "Signature does not match", "VC body tampered", "Issuer's assertionMethod key was revoked" |

The `Stale` state exists because re-resolving DIDs on every card render
is overkill. Results are cached with the resolution timestamp; "Stale"
means cached >60 s. Tap re-resolves.

### Rust path (`wallet-core::vc_self_verify`)

```rust
pub async fn self_verify(
    vc: &MidnightVc,
    wallet: &Wallet,
    indexer: &dyn Indexer,
) -> SelfVerifyResult {
    let issuer_doc = wallet.resolve_did(&vc.issuer).await?;
    let kid = vc.proof.verification_method.fragment();
    let vm = issuer_doc.assertion_method
        .iter()
        .find(|vm| vm.id.fragment() == kid)
        .ok_or(Invalid::IssuerKeyNotInAssertionRelation)?;
    let pk = jubjub::VerifyingKey::from_jwk(&vm.public_key_jwk)?;
    let canonical = midnight_vc_canonical_serialize(&vc.body)?;
    pk.verify(&canonical, &vc.proof.signature)
        .map(|_| Valid { resolved_at: now() })
        .map_err(|_| Invalid::SignatureMismatch)
}
```

Three external calls: `resolve_did` (chain), `from_jwk` + `verify` (pure
crypto). The chain call is the only thing that can be slow.

### UX framing

The badge says "Signed by `did:midnight:abc…` — last checked …", not
"Verified". Self-verify isn't W3C-formal verification (no verifier asking
the holder to prove anything); it's the holder asking the chain "is this
thing still signed by who I think". Calibrated language avoids overclaim.

### Explicit non-goals in Phase 1

- No revocation check (`birth` is non-revocable here; Phase 2 adds it).
- No holder-binding check (Phase 2 territory).
- No predicate evaluation (Phase 3).

## BDD integration tests

### Stack + location

```
~/iohk/midnight-identity-workspace/midnight-identity-solution-examples/IssuerDIDIT-mock/
└─ e2e/
   ├─ features/                    ← Gherkin specs
   │   ├─ bootstrap.feature
   │   ├─ issuance-happy-path.feature
   │   ├─ self-verify.feature
   │   └─ negative-paths.feature
   ├─ step-definitions/            ← TS Cucumber.js implementations
   │   ├─ wallet-steps.ts
   │   ├─ issuer-steps.ts
   │   └─ chain-steps.ts
   ├─ fixtures/
   │   ├─ headless-wallet-client.ts ← TS OID4VP/VCI client mirroring wallet-core's HTTP
   │   ├─ docker-compose.yml       ← standalone Midnight env
   │   └─ seeds.ts                 ← deterministic seeds + expected DIDs
   └─ support/
       └─ hooks.ts                 ← Before/After: spin up env, bootstrap, tear down
```

Stack: Cucumber.js + TypeScript + Playwright (Playwright drives the
laptop-browser side when scenarios need the operator form).

### Two clients, one issuer

- **Headless TS wallet client** (`headless-wallet-client.ts`, ~300 LOC) —
  implements the same 6-endpoint HTTP dance as the real Rust wallet, in
  pure TS. Uses `@midnight-ntwrk/midnight-did` for keys + signing. Fast
  (~5 s per scenario), runs in CI, no phone or simulator needed.
- **Real Rust `wallet-core` headless mode** (Phase 1.5, not blocking) —
  same scenarios driven against the actual Rust binary. Catches drift
  between the headless client and the real wallet.

Both clients hit the same mock issuer over the same HTTP surface.
Anything passing against the TS client must pass against the Rust client —
divergence indicates a bug in one of them.

### Mock-DIDIT step shape

```typescript
When('the operator submits KYC data for {string}', async function (holder: string) {
  const claims = fixtures.kycClaims[holder];
  await this.issuerForm.fill(claims);
  await this.issuerForm.submit();
  // mock issuer's kyc.ts sleeps for KYC_DELAY_MS (0 in CI, 2000 in demo)
  await this.waitForSession(holder, 'kyc_done');
});
```

### Scenario sketches

```gherkin
Feature: Issuance happy path
  Background:
    Given a clean standalone Midnight environment
    And the wallet has bootstrapped DID "alice" with both authn and assertion keys
    And the issuer has bootstrapped DID "issuer-demo"

  Scenario: Alice receives a birth VC from the demo issuer
    Given Alice's wallet is empty of VCs
    When the operator initiates an issuance session
    Then a QR-1 is rendered with a SIOPv2 authorization request
    When Alice's wallet scans QR-1
    Then Alice's wallet POSTs a SIOPv2 id_token signed with her Ed25519 authn key
    And the issuer verifies the id_token against Alice's DID document
    And the laptop browser is redirected to the KYC form
    When the operator submits KYC data for "alice"
    Then the session status is "kyc_done" within 100ms
    And a QR-2 is rendered with an OID4VCI credential offer
    When Alice's wallet scans QR-2
    Then Alice's wallet receives a signed birth VC issued by "issuer-demo"
    And Alice's vc_store contains 1 VC
    And the VC's holder field equals Alice's DID

Feature: Self-verify
  Scenario: Alice self-verifies a fresh VC
    Given Alice has a birth VC from a prior scenario
    When Alice taps Self-verify on the VC card
    Then the result is Valid
    And the badge subtitle includes the resolved_at timestamp

  Scenario: Self-verify after issuer rotates its assertion key
    Given Alice has a birth VC issued by "issuer-demo"
    When the issuer rotates its assertionMethod key on chain
    And Alice taps Self-verify on the VC card
    Then the result is Invalid because the signature does not match

Feature: Negative paths
  Scenario: Wrong nonce
    Given the issuer has issued a SIOPv2 nonce
    When the wallet signs a different nonce
    Then POST /authorize-response returns 401
    And the issuer's session is not advanced

  Scenario: Replay
    Given Alice has authenticated once
    When Alice's wallet replays the same id_token
    Then POST /authorize-response returns 401
    And the issuer's nonce is now consumed

  Scenario: Unbootstrapped DID
    Given Alice's wallet has bootstrapped a DID but never attached an authentication-relation key
    When Alice's wallet scans QR-1
    Then the wallet shows error "no authentication-relation key on this DID; run Bootstrap"
```

### Fixtures + hooks

Standalone env spin-up via `docker compose -f e2e/fixtures/docker-compose.yml
up -d`, healthcheck on indexer GraphQL, then teardown in `After`. ~30 s
per scenario for hot-start; ~5 s steady-state between scenarios that
share the env. Hooks honour a `@requires-fresh-chain` annotation for
scenarios that need a clean reset (e.g., the key-rotation scenario).

### Not covered in Phase 1 BDD

- UI tests against the real Dioxus wallet (manual via Android emulator +
  `adb input tap` per the existing session pattern).
- Real DIDIT integration (no DIDIT in our build).
- Network failure injection (deferred — happy path first).

## Build sequence + effort

| # | Step | What lands | Effort (eng-days) | Blocks |
|--:|---|---|--:|---|
| 1 | `bootstrap_did_with_keys` + CLI | `wallet-core/src/did/bootstrap.rs` (~150 LOC), `did-bootstrap` CLI binary, deterministic-seed fixtures. Unit tests against standalone env. | 2 | 2, 6 |
| 2 | `vc_store` module | Three redb tables + serde codecs + CRUD API + iteration. ~400 LOC. Unit tests in pure cargo. | 2 | 4, 5, 8 |
| 3 | `did_auth` + `oid4vp_client` | "Sign nonce with DID's authn key" + SIOPv2 id_token JWS builder + HTTP POST. ~300 LOC. Unit tests + integration against stub issuer. | 2 | 6, 8 |
| 4 | `oid4vci_client` | Parse offer, Pre-Authorized Code Flow token exchange, credential request with DID-bound JWS proof, parse `birth` VC + openings, hand to `vc_store`. ~300 LOC. | 2 | 6, 8 |
| 5 | `vc_self_verify` | ~150 LOC. Self-contained; testable against any stored VC. | 1 | 8 |
| 6 | `IssuerDIDIT-mock` skeleton + 6 routes | TS Express + 6 endpoints + SQLite + bootstrap script using `@midnight-ntwrk/midnight-did-api`. ~600 LOC TS. | 2 | 7, 10 |
| 7 | `vcMinter` + KYC form + nonce/token bindings | Midnight VC assembly + Jubjub signing + form-driven mock. ~400 LOC TS. | 2 | 10 |
| 8 | Identity Centre Dioxus UI | Top-level tab, sub-tab router, VC carousel, DID list + detail, Bootstrap panel, DID-picker popup, FAB, scan modal, sim-mode paste-URL affordance. ~800 LOC Rust+Dioxus. | 3 | 9, 10 |
| 9 | Android QR scanner JNI bridge | Kotlin + CameraX + ML Kit, JNI to wallet-core trait. ~200 Kotlin + ~50 Rust trait. | 2 | 10 |
| 10 | BDD harness + first scenarios | Cucumber.js setup, headless wallet client (~300 TS), 3-4 features covering happy path + 3 negative paths. | 3 | — |

**Total: ~21 eng-days.** Order is chosen so each step's tests can run
against the prior step's output.

**Critical-path bottleneck:** Step 8 (Identity Centre UI). Largest single
chunk and the only step that touches Dioxus directly. Can be parallelised
with step 6+7 once step 4 lands.

**Recommended parallel tracks** if compressing wall time:

- Track A (steps 1 → 2 → 3 → 4 → 5) and Track B (steps 6 → 7) run
  independently after step 1 lands; converge at step 8.
- Step 9 (QR bridge) is independent of everything except step 8's UI
  contract — can start once that contract is drafted.
- Step 10 (BDD) needs step 7 done; the headless TS client doesn't need
  the Dioxus UI — runs in parallel with step 8.

With two engineers (wallet + issuer) on parallel tracks: ~12 wall-days.

### Risk register

| Risk | Mitigation |
|---|---|
| Standalone env stability — flaky docker compose makes every scenario slower | Cache env image; healthcheck before scenarios; `@requires-fresh-chain` annotation for the few scenarios that need a reset |
| Compact-VC canonical serialisation must match exactly between wallet (verify) and issuer (sign) | Shared fixture tests using the same canonical bytes on both sides |
| Android emulator camera quirks for QR scanning | Dev paste-URL affordance always available; emulator camera is a stretch goal, not a requirement |
| Real `IssuerDIDIT` engineer diverges from the HTTP contract | This spec is the authority; HTTP contract section is the integration interface |
| `wallet-core::DidDocument` may not fully populate verification-relation iterables yet | First task of step 1 is to confirm and patch the existing fork branch if needed |

## Security considerations

Although this is a demo running against a local chain, the protocol surface
is the same one a production wallet would expose, so the threat model is
worth stating explicitly.

| Concern | Phase 1 stance |
|---|---|
| **Holder key storage** | Ed25519 + Jubjub secrets live in the existing wallet `SecretStorage` (redb-backed, OS-keystore-backed on real device; pure file on dev). No new storage surface introduced. |
| **SIOPv2 nonce replay** | Issuer mints a fresh `oid4vp_nonce` per session, binds it to the `sessions` row, and consumes it on first valid `POST /authorize-response`. A re-POST with the same `id_token` returns 401 (asserted by the `Replay` BDD scenario). |
| **id_token signature scope** | The signed payload includes `iss` (holder DID), `aud` (issuer's `client_id`), `nonce`, `iat`, `exp` — standard SIOPv2 framing. `aud` binding prevents an id_token minted for issuer A from being replayed against issuer B. |
| **OID4VCI `c_nonce` binding** | Each `POST /token` mints a fresh `c_nonce`; the wallet's credential-request JWS must sign over that `c_nonce`. Prevents an attacker who intercepts the access token from requesting a credential bound to a different DID. |
| **DID document integrity** | The issuer's `assertionMethod` key is published on-chain; rotating it invalidates prior VCs from the holder's perspective (asserted by the key-rotation BDD scenario). No off-chain key publication. |
| **BDD seed keys are not real keys** | Fixture seeds (`holder-demo-seed`, `issuer-demo-seed`) are committed to the repo. They MUST NOT be re-used outside the local standalone env. The Bootstrap UI on each side accepts a user-supplied seed for non-fixture runs. |
| **`issuer-keystore.json` is gitignored** | The committed `issuer-keystore.example.json` shows the shape with all-zero secrets; the real file is gitignored. Sims and CI populate it via the Bootstrap script, never from a checked-in file. |
| **didit.me PII handling** | Out of scope for Phase 1 (we don't talk to didit.me). When the real `IssuerDIDIT` lands, the `integrate-didit` skill in the issuer repo documents the canonical V2 signature verification + webhook idempotency requirements; that work is gated on the swap from mock to real, not on this design. |
| **Standalone env trust model** | Local-only, single-tenant, no network egress required for the wallet↔issuer↔chain triangle. The only external host that *can* be reached in Phase 1 is didit.me, and only when an engineer manually wires it up post-mock — which is explicitly out of scope here. |
| **Self-verify is not W3C verification** | The UI deliberately says "Signed by … — last checked …", never "Verified". The semantic distinction is preserved so a future verifier app doesn't inherit overclaim. |

## Acceptance criteria

Phase 1 is done when:

1. **All BDD scenarios green** on the standalone env, against both the
   headless TS wallet client and (stretch) the headless Rust wallet
   client.
2. **Manual demo flow works on Android emulator** end-to-end: launch the
   wallet, tap `Bootstrap`, wait for the DID to land, scan QR-1 from a
   browser on the same host, submit the KYC form, scan QR-2, see the VC
   card slide into the carousel, tap `Self-verify`, see a green
   `Valid { resolved_at }` badge.
3. **`vc_self_verify` works** for a freshly-issued VC and surfaces
   `Invalid` correctly when the issuer rotates its assertion key
   on-chain.
4. **6-endpoint HTTP contract is documented + stable** in this spec.
5. **`did-bootstrap` CLI binary** ships with `--help` output and the
   shell-script invocations used by both bootstrap paths.
6. **README updates** in both repos cover the demo setup (start
   standalone env, bootstrap wallet, bootstrap issuer, run flow).

## Open questions / future work

| Item | Phase | Note |
|---|---|---|
| Verifier app + plain-SD VP | 2 | The `B` of the roadmap |
| Compact verifier-contract circuits for predicate proofs | 3 | The `D` of the roadmap — the ultimate goal |
| iOS `AVCaptureMetadataOutput` QR scanner bridge | Later | Dev paste-URL works for sim in Phase 1 |
| Mobile WebView for didit.me KYC flow | Later | Phase 1 always uses laptop browser |
| Real `IssuerDIDIT` (DIDIT-driven) | Parallel workstream | Stable HTTP contract here makes the swap clean |
| Multi-issuer trust policy UI | Later | Phase 1 has no trust UI beyond "is the DID document valid" |
| Revocation status check in `vc_self_verify` | 2+ | `birth` here is non-revocable |
| DID-document patching UI (rotate keys, add relations) | Later | Phase 1 has Bootstrap only |
| Phase 2 verifier `IssuerDIDIT`-shaped HTTP contract design | 2 | Mirror Phase 1's contract approach |
| ngrok wiring once user provides account info | When needed | Demo on real device |
| Headless Rust `wallet-core` mode for BDD | 1.5 | Stretch — TS client first, Rust client nightly |

## Cross-doc references

- DIDIT integration skill: `~/iohk/midnight-identity-workspace/midnight-identity-solution-examples/.claude/skills/integrate-didit/SKILL.md` — for when the real `IssuerDIDIT` lands and we need to wire actual DIDIT webhooks.
- Midnight VC spec: `~/iohk/midnight-identity-workspace/midnight-verifiable-credentials/docs/spec/midnight-credentials.md`
- `birth` family schema: `~/iohk/midnight-identity-workspace/midnight-verifiable-credentials/midnight-did-credentials-birth/`
- Midnight DID resolver package: `~/iohk/midnight-identity-workspace/midnight-did/packages/did/src/midnight-did-resolver.ts`
- Wallet DID resolve path: `mobile-bench/wallet-core/src/wallet.rs::Wallet::resolve_did`
- Existing dioxus-wallet tab layout: `mobile-bench/dioxus-wallet/src/app.rs`
