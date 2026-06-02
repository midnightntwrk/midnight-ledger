# Login with DID — Architecture (Phase 1: DID-only / Mode A)

**Date:** 2026-06-02
**Status:** spec → planning
**Scope:** wallet-core OID4VP client + IssuerDIDIT-mock OID4VP verifier. Phase 1 implements SIOPv2 / OID4VP Mode A (id_token only). The architecture must absorb Mode B (id_token + vp_token) and Mode C (vp_token only) in a later phase without touching Phase-1 code.

**Normative reference:** the implementation guide at
`/Users/ysh/Downloads/login_with_did_oid4vp_siop2_implementation_guide.md`
(canonical OID4VP 1.0, SIOP v2, DID Core, VC DM 2.0, DIF PEX 2.0
links inside). Section numbers below refer to that document.

## Goals

1. Replace the ad-hoc `build_id_token + verifyIdToken` pair with a coordinator-based pipeline on both ends — every "proof" the guide enumerates (signature, DID binding, nonce binding, audience binding, freshness) is a named, individually-testable component.
2. Introduce three new wallet-side ports — `DidResolver`, `DidAuthnDiscovery`, `DidSigner` — that replace the over-broad `&Wallet` parameter and the wasteful sign-twice probe in `build_id_token`.
3. Cut down to **one** DID resolution + **one** signing call per login (down from 2 + 2 today).
4. Bring the wire format in line with the normative guide:
   - `sub_jwk` lives in the id_token **payload** (not the JWS header).
   - `response_uri` (not `redirect_uri`) for `direct_post` mode.
   - `response_type` + `response_mode` modelled on the wire even if Phase 1 only handles `id_token` / `direct_post`.
5. Use the guide's error taxonomy across wallet + issuer.
6. Land the full negative test matrix from the guide §"Minimum test matrix".
7. Keep the Phase-1 holder-binding posture (issuer trusts the self-asserted JWK) but **structure the verifier so the on-chain check is one swap-in** — single-method change in `holderDidResolver.ts`, no caller updates.

## Non-Goals

- Phase 2 (VP, presentation_submission, credential holder binding).
- Indexer-backed DID resolution on the issuer (Phase 2 spec).
- Multi-VM rotation policy (a separate spec — defer until `MaintenanceUpdate` adds a second authentication VM to the demo).
- Account linking on the RP (Phase 1 issuer-mock doesn't persist a user table; only `holder_did` on the session row).
- Per-RP `client_metadata` resolution / signed-request-object verification.
- Native `wallet-core::oid4vp_verifier` (Rust mirror of the TS verifier) — flagged as future work; not on this branch.

## Compatible mode matrix

`LoginCoordinator` carries a `Vec<Box<dyn ResponseBuilder>>`. Modes are configurations of that vec:

| Mode | builders                                                                 |
|------|--------------------------------------------------------------------------|
| A    | `[IdTokenBuilder]`                                                       |
| B    | `[IdTokenBuilder, VpTokenBuilder, PresentationSubmissionBuilder]`        |
| C    | `[VpTokenBuilder, PresentationSubmissionBuilder]`                        |

Phase 1 only instantiates Mode A. The trait + coordinator shape is the same.

The verifier side mirrors:

| Mode | pipeline                                                                                                                |
|------|-------------------------------------------------------------------------------------------------------------------------|
| A    | `[State, JwsStructure, DidResolution, KeyAuthorization, JwsSignature, Audience, Freshness, Nonce]`                       |
| B    | `[…Mode A…, VpToken, PresentationSubmission, CredentialHolderBinding, CredentialStatus, TrustedIssuer]`                  |
| C    | `[State, VpToken, PresentationSubmission, CredentialHolderBinding, CredentialStatus, TrustedIssuer, Audience, Freshness, Nonce]` |

Phase 1 only instantiates Mode A. Each verifier is independently unit-tested.

## Architecture

### Wallet-core (the hex core)

```
wallet-core/src/oid4vp_client/                ← rename keep, current name is fine
├── mod.rs                  // public surface: run_authentication, LoginCoordinator
├── request.rs              // AuthorizationRequest struct + parser
├── response.rs             // AuthorizationResponse struct + post_response
├── builders/
│   ├── mod.rs              // ResponseBuilder trait
│   └── id_token.rs         // IdTokenBuilder (Phase 1)
│   //  Phase 2 adds: vp_token.rs, presentation_submission.rs
├── id_token.rs             // JWS construction primitives (header + payload typed)
├── ports.rs                // DidResolver, DidAuthnDiscovery, DidSigner
└── errors.rs               // unified LoginError enum
```

### Adapters (dioxus-wallet)

```
mobile-bench/dioxus-wallet/src/
├── did_ports/             // new module
│   ├── mod.rs
│   ├── wallet_resolver.rs            // impl DidResolver via Wallet::resolve_did
│   ├── cached_authn_discovery.rs     // impl DidAuthnDiscovery: 30s TTL cache + authn[0]
│   └── redb_signer.rs                // impl DidSigner via RedbSecretStore
```

### Ports (the new traits)

```rust
// wallet-core/src/oid4vp_client/ports.rs

#[async_trait]
pub trait DidResolver: Send + Sync {
    /// Phase-1: returns the wallet's local view of the DID
    /// document (from the indexer or a cached snapshot). Phase-2
    /// adds a `freshness_hint` parameter to force re-resolution.
    async fn resolve(&self, did: &DidId) -> Result<DidDocument, ResolveError>;
}

#[async_trait]
pub trait DidAuthnDiscovery: Send + Sync {
    /// Resolve the DID, pick the verification method authorized
    /// for `authentication`, return its kid + public JWK. NO
    /// signing — this is a discovery step. Caching lives in the
    /// adapter.
    async fn authn_key(&self, did: &DidId) -> Result<AuthnKey, DiscoverError>;
}

#[async_trait]
pub trait DidSigner: Send + Sync {
    /// Sign `payload` with the local secret bound to `kid`. The
    /// kid is the full DID URL (`did:midnight:abc#key-auth`).
    async fn sign(&self, kid: &str, payload: &[u8]) -> Result<Vec<u8>, SignError>;
}

pub struct AuthnKey {
    pub kid: String,
    pub public_jwk: PublicKeyJwk,
}
```

### Coordinator pattern (wallet)

```rust
// wallet-core/src/oid4vp_client/builders/mod.rs

#[async_trait]
pub trait ResponseBuilder: Send + Sync {
    /// Augment the response with this builder's contribution.
    /// Builders run in the order they're registered; each one
    /// can read fields earlier builders populated.
    async fn build(
        &self,
        req: &AuthorizationRequest,
        ctx: &BuildContext,
        resp: &mut AuthorizationResponse,
    ) -> Result<(), LoginError>;
}

// wallet-core/src/oid4vp_client/mod.rs

pub struct LoginCoordinator {
    builders: Vec<Box<dyn ResponseBuilder>>,
}

impl LoginCoordinator {
    pub fn new(builders: Vec<Box<dyn ResponseBuilder>>) -> Self { … }

    /// Convenience constructor for Mode A.
    pub fn mode_a(
        discovery: Arc<dyn DidAuthnDiscovery>,
        signer: Arc<dyn DidSigner>,
        clock: Arc<dyn Clock>,
        holder: DidId,
        lifetime_secs: u64,
    ) -> Self {
        Self::new(vec![Box::new(IdTokenBuilder { … })])
    }
}

pub async fn run_authentication(
    http: &dyn HttpClient,
    coordinator: &LoginCoordinator,
    qr_url: &str,
) -> Result<PostResponseResult, AuthFlowError> {
    let request_uri = request::parse_request_url(qr_url)?;
    let req = request::fetch_request_object(http, &request_uri).await?;
    let mut resp = AuthorizationResponse::new(req.state.clone());
    let ctx = BuildContext { /* … */ };
    for b in &coordinator.builders {
        b.build(&req, &ctx, &mut resp).await?;
    }
    let result = response::post_response(http, &req.response_uri, &resp).await?;
    Ok(result)
}
```

### `IdTokenBuilder` flow (Phase 1)

```rust
async fn build(&self, req, _ctx, resp) -> Result<(), LoginError> {
    // 1. Discover kid + jwk (one DID resolve)
    let key = self.discovery.authn_key(&self.holder).await?;

    // 2. Compose JOSE header + claims
    let iat = self.clock.now_ms() / 1_000;
    let header = JwsHeader { alg: "EdDSA", typ: "JWT", kid: key.kid.clone() };
    let payload = IdTokenPayload {
        iss: self.holder.to_did_string(),
        sub: self.holder.to_did_string(),
        aud: req.client_id.clone(),
        nonce: req.nonce.clone(),
        iat,
        exp: iat + self.lifetime_secs,
        sub_jwk: Some(key.public_jwk.clone()),    // ⬅ in payload, per normative
    };
    let sign_input = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header)?),
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload)?),
    );

    // 3. Sign (one signing call)
    let sig = self.signer.sign(&key.kid, sign_input.as_bytes()).await?;
    let id_token = format!("{sign_input}.{}", URL_SAFE_NO_PAD.encode(sig));

    resp.id_token = Some(id_token);
    Ok(())
}
```

### Verifier pipeline (issuer-mock, TS)

```ts
// IssuerDIDIT-mock/src/services/oid4vpVerifier/pipeline.ts

export interface Verifier {
  name: string;
  run(ctx: VerificationContext): Promise<void>;   // throws LoginError on fail
}

export class VerificationPipeline {
  constructor(private steps: Verifier[]) {}
  async run(ctx: VerificationContext): Promise<VerifiedAuth> {
    for (const step of this.steps) {
      await step.run(ctx);
    }
    return ctx.verified();
  }
}

// Mode A wiring:
export function modeAPipeline(deps: VerifierDeps): VerificationPipeline {
  return new VerificationPipeline([
    new StateVerifier(),
    new JwsStructureVerifier(),
    new DidResolverVerifier(deps.holderDidResolver),     // ⬅ Phase 1 self-asserted; swap to on-chain in Phase 2
    new KeyAuthorizationVerifier(),
    new JwsSignatureVerifier(),
    new AudienceVerifier(deps.expectedAud),
    new FreshnessVerifier(deps.maxTokenAgeSeconds),
    new NonceVerifier(deps.nonceStore),                  // atomic consume
  ]);
}
```

Each `Verifier` is a class with a single `run()` method and a unit test file. Order matters — the pipeline runs in declaration order, fails on the first error.

### Wire format (Phase 1, normative-aligned)

**Request object (issuer → wallet):**

```json
{
  "client_id": "did:midnight:issuer-mock",
  "response_type": "id_token",
  "response_mode": "direct_post",
  "response_uri": "https://issuer.local/oid4vp/callback",
  "scope": "openid",
  "nonce": "256-bit-random-b64url",
  "state": "session-uuid",
  "presentation_definition": null
}
```

Today's request has `redirect_uri` instead of `response_uri` and is missing `response_type` / `response_mode`. Phase 1 adds them; the wallet parser accepts both during transition.

**id_token (wallet → issuer):**

```
JOSE header: { "typ": "JWT", "alg": "EdDSA", "kid": "did:midnight:holder#key-auth" }
Payload: {
  "iss": "did:midnight:holder",
  "sub": "did:midnight:holder",
  "aud": "did:midnight:issuer-mock",
  "nonce": "…",
  "iat": …,
  "exp": …,
  "sub_jwk": { "kty": "OKP", "crv": "Ed25519", "x": "…" }
}
```

`sub_jwk` moves from JOSE header into payload (per guide §"Self-issued ID Token claims"). Issuer side reads it from there.

**Authorization response (wallet → issuer):**

```json
{
  "state": "session-uuid",
  "id_token": "eyJ…"
}
```

No `vp_token`, no `presentation_submission` (Phase 1).

### Error taxonomy

`LoginError` (Rust enum + matching TS string codes). Maps to the guide §"Error handling":

| code                                   | source side | meaning |
|----------------------------------------|-------------|---------|
| `invalid_state`                        | RP          | state cookie mismatch |
| `invalid_nonce`                        | RP          | payload nonce != session nonce |
| `expired_challenge`                    | RP          | session.expires_at < now |
| `reused_nonce`                         | RP          | atomic consume returned null |
| `invalid_signature`                    | RP          | JWS sig didn't verify |
| `did_resolution_failed`                | RP          | DidResolver port error |
| `verification_method_not_authorized`   | RP          | kid not in `authentication[]` |
| `invalid_audience`                     | RP          | aud != client_id |
| `expired_token`                        | RP          | exp < now or iat too old |
| `invalid_request`                      | wallet      | bad URL / shape |
| `discover_failed`                      | wallet      | DidAuthnDiscovery returned error |
| `sign_failed`                          | wallet      | DidSigner returned error |
| `http_error`                           | both        | transport-level failure |

Wallet surfaces these as `LoginError`. Issuer surfaces them as response JSON `{ error: "…" }`. Test matrix uses these codes to assert specific failure shapes (not just "any error").

## Test matrix (from §"Minimum test matrix")

Each listed test gets a dedicated integration test on both ends.

**wallet-core tests** (unit + mocked HTTP):

- happy path — Mode A end-to-end against mock issuer
- one resolve + one sign assertion (count `DidAuthnDiscovery` + `DidSigner` calls)
- payload `sub_jwk` present, header `jwk` absent
- `response_uri` honoured (POST goes there, not `redirect_uri`)
- malformed request URL → `LoginError::InvalidRequest`
- DidAuthnDiscovery returns NoAuthnKey → `LoginError::DiscoverFailed`
- DidSigner returns error → `LoginError::SignFailed`

**issuer tests** (TS + jest + mock JWS fixtures):

| Guide test                                     | Code under test       | Expected |
|-----------------------------------------------|-----------------------|----------|
| Valid DID login with fresh nonce              | pipeline.run          | `VerifiedAuth` |
| Replay same response                          | NonceVerifier         | `reused_nonce` |
| Wrong nonce                                   | NonceVerifier         | `invalid_nonce` |
| Missing nonce                                 | NonceVerifier         | `invalid_nonce` |
| Wrong audience                                | AudienceVerifier      | `invalid_audience` |
| Expired ID Token                              | FreshnessVerifier     | `expired_token` |
| Signature by unknown key                      | JwsSignatureVerifier  | `invalid_signature` |
| DID key not authorized for authentication     | KeyAuthorizationVerifier | `verification_method_not_authorized` |
| DID resolution failure                        | DidResolverVerifier   | `did_resolution_failed` |

Phase-2-only tests deferred:

- VP missing requested credential
- VP valid signature but no holder binding
- Credential expired
- Credential revoked
- Credential issuer not trusted
- Valid VP with claims + holder binding

## Migration strategy (no breakage during transition)

The existing demo works. We keep it working at every step. Two compatibility hinges:

1. **Wire format dual-read on the issuer**: during Tasks 5-8 the issuer accepts both
   - `sub_jwk` in payload (new) **and** `jwk` in header (old)
   - `response_uri` **and** `redirect_uri`
   When both ends are upgraded, the legacy reads can come out (Task 9).

2. **Old `sign_for_authentication` kept alongside the new ports** until callers are migrated. Single deprecation commit removes it last.

## Risks

| Risk                                                                                  | Mitigation |
|--------------------------------------------------------------------------------------|------------|
| Refactor breaks the working demo                                                     | Tasks land in small commits, each smoke-tested on phone before the next |
| `Box<dyn ResponseBuilder>` future has `.await`s that capture per-call state and isn't `Send` | Already verified the worker thread can hold a `Send` future; builders are `Send + Sync` |
| Adding `sub_jwk` payload + removing header `jwk` breaks Phase-1 issuer mid-migration | Issuer accepts both forms during the transition tasks |
| Coordinator pattern adds boilerplate for the simple Mode-A case                      | One-line constructor (`LoginCoordinator::mode_a(…)`); no extra friction at the call site |
| Pipeline order errors silently pass invalid tokens                                   | Each verifier carries a `name`; the pipeline logs `step=… status=ok` at info on every run; missing steps surface in tests |
| Indexer roundtrip elimination cache returns stale doc after `MaintenanceUpdate`     | Phase 1's `CachedDidAuthnDiscovery` 30s TTL is short enough that demo-time changes are tolerable; Phase 2's policy may need invalidation hooks |

## What this spec does NOT change

- The on-the-wire request method (still `GET /request/<id>`). Phase 2 may push for signed request objects + PAR.
- The wallet's bootstrap path (separate concern).
- The OID4VCI flow (separate sister spec — same patterns will apply there in Phase 2 cleanup).
- The Box::pin defensives or the worker-thread refactor. Those are orthogonal stack-overflow fixes; the work here lands inside the same worker once Task 1-3 of the worker-migration plan are done.

## Coexistence with the worker-thread refactor

The worker-migration plan (`2026-06-02-wallet-worker-thread.md`) will route every heavy chain op through `worker.send(WorkMsg::…)`. The login pipeline lives one layer below that: the `WorkMsg::Oid4vp { … }` handler on the worker thread calls `run_authentication(http, &coordinator, qr_url)` with a coordinator the worker built once at boot.

So the order is:

1. Worker thread Task 1-2 land (already done — skeleton + Bootstrap migration).
2. **This spec's Tasks 1-9 land** alongside Tasks 3-4 of the worker plan (OID4VP + OID4VCI migrations). Each worker task uses the new ports / coordinator.
3. Worker Task 10 (cleanup) drops the `spawn(Box::pin(…))` defensives.

## Follow-ups (not in this spec)

- **Phase 2 — VP**: `VpTokenBuilder` + `PresentationSubmissionBuilder` + format-router (jwt_vp_json, sd-jwt, json-ld, mdoc). Adds `DidAssertionDiscovery` port + `VcSelection` port. Separate spec.
- **Phase 2 — issuer DID resolution via indexer**: replace `buildHolderDocumentFromJwsHeader` with an on-chain lookup. Affects only `DidResolverVerifier`'s deps. Separate spec.
- **Phase 3 — signed request objects (JAR)**: verify the request_object JWS on the wallet side; rotates trust from the URL TLS to the issuer's signing key. Separate spec.
- **Phase 3 — PAR (Pushed Authorization Request)**: the issuer pushes the request to a wallet endpoint instead of QR-encoding a URL. Separate spec.
- **Account-linking metadata on the RP**: persist `local_account_id ↔ did` after first successful login. Issuer-mock concern.
- **Rust-side `oid4vp_verifier`**: mirror the TS verifier so other Rust relying parties can reuse it. Separate spec.
