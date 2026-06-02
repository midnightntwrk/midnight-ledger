# Login with DID — Implementation Plan (Phase 1: Mode A)

> Companion to `docs/superpowers/specs/2026-06-02-login-with-did-architecture.md`. Tasks land in this order; each ends in a signed commit (DCO + GPG) + an on-device smoke test of the migrated surface.

**Goal:** the wallet completes a SIOPv2 / OID4VP Mode-A "Login with DID" against the issuer-mock using the architecture spec'd above. No VP yet, but the seams for VP land in this phase.

**Test of completion:** the existing demo (Bootstrap → tap Scan QR → scan the issuer's `/authorize` QR → wallet POSTs `id_token` → issuer fires `redirect_to` → `/kyc-form`) still works end-to-end on the phone. Internally:
- exactly 1 `DidResolver` call + 1 `DidSigner` call per login
- `sub_jwk` in payload, no `jwk` in header
- POST goes to `response_uri`
- Issuer's `VerificationPipeline` ran 8 named steps in order and logged each

---

### Task 1: New ports in `wallet-core` — `DidResolver`, `DidAuthnDiscovery`, `DidSigner`

**Files:**
- Create: `mobile-bench/wallet-core/src/oid4vp_client/ports.rs`
- Modify: `mobile-bench/wallet-core/src/oid4vp_client/mod.rs` (add `pub mod ports;`)

- [ ] **Step 1: Write the ports module**

```rust
// mobile-bench/wallet-core/src/oid4vp_client/ports.rs
//! Ports the OID4VP client consumes — split out of the old
//! `did_auth::sign_for_authentication`'s monolithic signature.
//!
//! Adapters live on the dioxus-wallet side (Wallet-backed
//! resolver, RedbSecretStore-backed signer); wallet-core only
//! defines the traits so unit tests can mock each independently.

use async_trait::async_trait;

use crate::{DidId, PublicKeyJwk};

/// Discovery output: the kid + jwk for whichever VM the
/// implementation picked. Stable across the lifetime of the
/// returned value — caller is free to embed both in the JWS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthnKey {
    /// Full DID URL form: `did:midnight:abc#key-auth`.
    pub kid: String,
    pub public_jwk: PublicKeyJwk,
}

#[derive(Debug, thiserror::Error)]
pub enum DiscoverError {
    #[error("resolve failed: {0}")]
    Resolve(String),
    #[error("no authentication-relation verification method on {0}")]
    NoAuthnKey(String),
}

#[derive(Debug, thiserror::Error)]
pub enum SignError {
    #[error("no local secret for kid {0}")]
    NoLocalSecret(String),
    #[error("sign failed: {0}")]
    Sign(String),
}

/// Resolve a DID + return the authentication-relation key.
/// Implementations are free to cache (the wallet-side adapter
/// does, with a 30 s TTL).
#[async_trait]
pub trait DidAuthnDiscovery: Send + Sync {
    async fn authn_key(&self, did: &DidId) -> Result<AuthnKey, DiscoverError>;
}

/// Sign a payload with the local secret bound to `kid`. The kid
/// is what `DidAuthnDiscovery::authn_key` returned for the same
/// DID.
#[async_trait]
pub trait DidSigner: Send + Sync {
    async fn sign(&self, kid: &str, payload: &[u8]) -> Result<Vec<u8>, SignError>;
}
```

- [ ] **Step 2: Wire into `mod.rs`**

```rust
// At the top of wallet-core/src/oid4vp_client/mod.rs, alongside the
// existing `mod jws; mod parser; mod respond;`
pub mod ports;
```

- [ ] **Step 3: cargo check + cargo test wallet-core**

Run: `cargo check -p wallet-core && cargo test -p wallet-core --lib oid4vp`
Expected: builds cleanly, existing tests still pass (no callers yet).

- [ ] **Step 4: Commit**

```
feat(wallet-core): introduce OID4VP ports — DidAuthnDiscovery, DidSigner

Splits the over-broad `sign_for_authentication(wallet, store, did,
payload)` signature into two focused ports. Phase-1 callers
(build_id_token, OID4VCI proof builder) will be rewritten to
consume one of each, eliminating the double-resolve + double-sign
on every login.

Reference: docs/superpowers/specs/2026-06-02-login-with-did-architecture.md §"Ports".
```

---

### Task 2: Adapter — `WalletDidAuthnDiscovery` with cache

**Files:**
- Create: `mobile-bench/dioxus-wallet/src/did_ports/mod.rs`
- Create: `mobile-bench/dioxus-wallet/src/did_ports/cached_authn_discovery.rs`
- Modify: `mobile-bench/dioxus-wallet/src/lib.rs` (add `mod did_ports;`)

- [ ] **Step 1: Write the cached adapter**

```rust
// mobile-bench/dioxus-wallet/src/did_ports/cached_authn_discovery.rs
//! `DidAuthnDiscovery` impl backed by `wallet_core::Wallet`
//! resolution + a 30 s TTL cache keyed on the DID string.
//!
//! The cache halves the indexer roundtrips on the rapid-fire
//! "scan → authenticate → scan again" flow without holding stale
//! documents long enough to matter (Phase-1 doesn't rotate
//! authentication VMs in the demo).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use async_trait::async_trait;

use wallet_core::oid4vp_client::ports::{AuthnKey, DidAuthnDiscovery, DiscoverError};
use wallet_core::{DidId, VerificationMethodRef, Wallet};

const TTL: Duration = Duration::from_secs(30);

struct Entry {
    key: AuthnKey,
    inserted: Instant,
}

pub struct CachedWalletAuthnDiscovery {
    wallet: Wallet,
    cache: Mutex<HashMap<String, Entry>>,
}

impl CachedWalletAuthnDiscovery {
    pub fn new(wallet: Wallet) -> Self {
        Self { wallet, cache: Mutex::new(HashMap::new()) }
    }
}

#[async_trait]
impl DidAuthnDiscovery for CachedWalletAuthnDiscovery {
    async fn authn_key(&self, did: &DidId) -> Result<AuthnKey, DiscoverError> {
        let did_str = did.to_did_string();
        // Cache hit (still fresh) — short-circuit.
        if let Some(hit) = self.cache.lock().expect("poisoned").get(&did_str) {
            if hit.inserted.elapsed() < TTL {
                return Ok(hit.key.clone());
            }
        }

        let doc = self
            .wallet
            .resolve_did(&did_str)
            .await
            .map_err(|e| DiscoverError::Resolve(e.to_string()))?;

        let (kid, public_jwk) = match doc
            .authentication
            .first()
            .ok_or_else(|| DiscoverError::NoAuthnKey(did_str.clone()))?
        {
            VerificationMethodRef::Inline(vm) => (vm.id.clone(), vm.public_key_jwk.clone()),
            VerificationMethodRef::Id(id) => {
                let vm = doc
                    .verification_method
                    .iter()
                    .find(|v| v.id == *id)
                    .ok_or_else(|| {
                        DiscoverError::Resolve(format!(
                            "authentication kid {id} not present in verificationMethod[]"
                        ))
                    })?;
                (vm.id.clone(), vm.public_key_jwk.clone())
            }
        };

        let key = AuthnKey { kid, public_jwk };
        self.cache
            .lock()
            .expect("poisoned")
            .insert(did_str, Entry { key: key.clone(), inserted: Instant::now() });
        Ok(key)
    }
}
```

- [ ] **Step 2: mod.rs re-exports**

```rust
// did_ports/mod.rs
mod cached_authn_discovery;
pub use cached_authn_discovery::CachedWalletAuthnDiscovery;
```

- [ ] **Step 3: lib.rs**

```rust
// after `mod worker;`
mod did_ports;
```

- [ ] **Step 4: cargo check both targets**

```
cargo check -p dioxus-wallet --lib
# + arm64
cargo ndk -t arm64-v8a build --release -p dioxus-wallet --lib
```

- [ ] **Step 5: Commit**

```
feat(dioxus-wallet): WalletDidAuthnDiscovery adapter with 30s cache

Wraps the Wallet's resolve_did + first-authentication-VM picking
in the new DidAuthnDiscovery port. Cache halves the indexer
roundtrips during a rapid-fire demo session.
```

---

### Task 3: Adapter — `RedbDidSigner`

**Files:**
- Create: `mobile-bench/dioxus-wallet/src/did_ports/redb_signer.rs`
- Modify: `mobile-bench/dioxus-wallet/src/did_ports/mod.rs` (re-export)

- [ ] **Step 1: Write the adapter**

```rust
// mobile-bench/dioxus-wallet/src/did_ports/redb_signer.rs
//! `DidSigner` impl backed by RedbSecretStore.

use async_trait::async_trait;

use wallet_core::oid4vp_client::ports::{DidSigner, SignError};
use wallet_core::secret_storage::redb_secret_store::RedbSecretStore;
use wallet_core::secret_storage::SecretStorage;

pub struct RedbDidSigner {
    store: RedbSecretStore,
}

impl RedbDidSigner {
    pub fn new(store: RedbSecretStore) -> Self {
        Self { store }
    }
}

#[async_trait]
impl DidSigner for RedbDidSigner {
    async fn sign(&self, kid: &str, payload: &[u8]) -> Result<Vec<u8>, SignError> {
        let key_ref = self
            .store
            .find_by_kid(kid)
            .await
            .ok_or_else(|| SignError::NoLocalSecret(kid.to_string()))?;
        let out = self
            .store
            .sign(key_ref.uuid(), payload)
            .await
            .map_err(|e| SignError::Sign(e.to_string()))?;
        Ok(out.signature)
    }
}
```

- [ ] **Step 2: Re-export from `did_ports/mod.rs`**
- [ ] **Step 3: cargo check both targets**
- [ ] **Step 4: Commit** — `feat(dioxus-wallet): RedbDidSigner adapter for DidSigner port`

---

### Task 4: New `AuthorizationRequest` / `AuthorizationResponse` types + request parser

**Files:**
- Create: `mobile-bench/wallet-core/src/oid4vp_client/request.rs`
- Create: `mobile-bench/wallet-core/src/oid4vp_client/response.rs`
- Modify: `mobile-bench/wallet-core/src/oid4vp_client/mod.rs`
- Modify (delete content of): `mobile-bench/wallet-core/src/oid4vp_client/parser.rs` (becomes a thin re-export shim, removed in Task 9)

- [ ] **Step 1: Write `request.rs`**

```rust
//! Authorization request — the issuer-sent object the wallet
//! parses out of `request_uri`. Phase 1 only fills the id_token
//! fields; the struct shape is the full normative OID4VP request
//! so adding vp_token + presentation_definition later is a
//! field-level change, not a type-replacement.

use serde::{Deserialize, Serialize};
use url::Url;

use crate::http::{HttpClient, HttpError};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResponseType {
    IdToken,
    VpToken,                       // Phase 2
    #[serde(rename = "vp_token id_token")]
    VpTokenIdToken,                // Phase 2
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResponseMode {
    DirectPost,
    DirectPostJwt,                 // Phase 3
}

/// Phase-1 placeholder. Models the full normative shape so
/// adding `input_descriptors` later is a struct-field extension.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[allow(dead_code)] // Phase 2 fills these in.
pub struct PresentationDefinition {
    pub id: String,
    pub input_descriptors: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorizationRequest {
    pub client_id: String,
    /// Phase 1 only handles `id_token`. The struct accepts the
    /// full set so we can reject unsupported modes with a clear
    /// error rather than a parse failure.
    pub response_type: ResponseType,
    pub response_mode: ResponseMode,
    /// `direct_post` target. Older mock issuer revisions used
    /// `redirect_uri`; the parser falls back to that during the
    /// transition (#TODO drop in Task 9).
    pub response_uri: String,
    pub scope: String,
    pub nonce: String,
    pub state: Option<String>,
    pub presentation_definition: Option<PresentationDefinition>,
}

// Wire-level helper struct for the transitional dual-read.
#[derive(Debug, Deserialize)]
struct RawRequest {
    client_id: String,
    response_type: Option<ResponseType>,
    response_mode: Option<ResponseMode>,
    response_uri: Option<String>,
    redirect_uri: Option<String>,        // legacy
    scope: Option<String>,
    nonce: String,
    state: Option<String>,
    presentation_definition: Option<PresentationDefinition>,
}

impl From<RawRequest> for AuthorizationRequest {
    fn from(r: RawRequest) -> Self {
        Self {
            client_id: r.client_id,
            response_type: r.response_type.unwrap_or(ResponseType::IdToken),
            response_mode: r.response_mode.unwrap_or(ResponseMode::DirectPost),
            response_uri: r.response_uri.or(r.redirect_uri).unwrap_or_default(),
            scope: r.scope.unwrap_or_else(|| "openid".into()),
            nonce: r.nonce,
            state: r.state,
            presentation_definition: r.presentation_definition,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RequestParseError {
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
    #[error("unsupported response_type {0:?}; this build only handles id_token (Phase 1)")]
    UnsupportedMode(ResponseType),
}

impl From<HttpError> for RequestParseError {
    fn from(e: HttpError) -> Self {
        RequestParseError::Http(e.to_string())
    }
}

pub fn parse_request_url(url: &str) -> Result<String, RequestParseError> {
    let u = Url::parse(url)?;
    if u.scheme() != "openid4vp" {
        return Err(RequestParseError::BadScheme(u.scheme().into()));
    }
    u.query_pairs()
        .find(|(k, _)| k == "request_uri")
        .map(|(_, v)| v.into_owned())
        .ok_or(RequestParseError::MissingParam("request_uri"))
}

pub async fn fetch_request_object(
    http: &dyn HttpClient,
    request_uri: &str,
) -> Result<AuthorizationRequest, RequestParseError> {
    let resp = http.get(request_uri).await?;
    if !resp.is_success() {
        return Err(RequestParseError::Http(format!(
            "non-2xx status {} fetching request_uri",
            resp.status
        )));
    }
    let body = resp.body_text()?;
    let raw: RawRequest = serde_json::from_str(body)?;
    let req: AuthorizationRequest = raw.into();
    if req.response_type != ResponseType::IdToken {
        return Err(RequestParseError::UnsupportedMode(req.response_type));
    }
    Ok(req)
}
```

- [ ] **Step 2: Write `response.rs`**

```rust
//! AuthorizationResponse — wallet → issuer body. Phase 1 only
//! sends id_token + state; the struct holds vp_token /
//! presentation_submission as Option<…> so Phase 2 builders fill
//! them without changing this type.

use serde::{Deserialize, Serialize};

use crate::http::{HttpClient, HttpError};

#[derive(Debug, Clone, Serialize, Default)]
pub struct AuthorizationResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id_token: Option<String>,
    /// Phase 2.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vp_token: Option<serde_json::Value>,
    /// Phase 2.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presentation_submission: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
}

impl AuthorizationResponse {
    pub fn new(state: Option<String>) -> Self {
        Self { state, ..Default::default() }
    }
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

impl From<HttpError> for PostResponseError {
    fn from(e: HttpError) -> Self {
        PostResponseError::Http(e.to_string())
    }
}

pub async fn post_response(
    http: &dyn HttpClient,
    response_uri: &str,
    resp: &AuthorizationResponse,
) -> Result<PostResponseResult, PostResponseError> {
    let body = serde_json::to_value(resp).map_err(PostResponseError::Decode)?;
    let r = http.post_json(response_uri, &body, None).await?;
    let body_text = r
        .body_text()
        .map_err(|e| PostResponseError::Http(e.to_string()))?
        .to_string();
    if !r.is_success() {
        return Err(PostResponseError::Status { status: r.status, body: body_text });
    }
    let parsed: PostResponseResult = serde_json::from_str(&body_text)?;
    Ok(parsed)
}
```

- [ ] **Step 3: Re-export from mod.rs**

```rust
pub mod request;
pub mod response;
```

- [ ] **Step 4: Tests**

Add unit tests in each file mirroring the existing `parser.rs::tests` + `respond.rs::tests`, plus:
- `parse_request_url_unchanged` — same input as old, same output.
- `fetch_request_object_dual_reads_redirect_uri` — legacy mock issuer body with `redirect_uri` gets mapped to `response_uri`.
- `fetch_request_object_rejects_vp_token_mode` — Phase-1 guard against accidental Mode-B activation.
- `post_response_omits_null_vp_fields` — `vp_token`/`presentation_submission` absent on the wire when None.

- [ ] **Step 5: Commit** — `feat(wallet-core): typed AuthorizationRequest + AuthorizationResponse with normative shape`

---

### Task 5: `id_token` typed primitive + new `IdTokenBuilder` (uses ports)

**Files:**
- Create: `mobile-bench/wallet-core/src/oid4vp_client/id_token.rs` (JOSE header + claims types)
- Create: `mobile-bench/wallet-core/src/oid4vp_client/builders/mod.rs` (`ResponseBuilder` trait)
- Create: `mobile-bench/wallet-core/src/oid4vp_client/builders/id_token_builder.rs`
- Modify: `mobile-bench/wallet-core/src/oid4vp_client/mod.rs`

- [ ] **Step 1: id_token.rs**

```rust
//! SIOPv2 id_token JOSE header + payload types.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};

use crate::PublicKeyJwk;

#[derive(Debug, Serialize)]
pub struct JwsHeader<'a> {
    pub alg: &'a str,
    pub typ: &'a str,
    pub kid: &'a str,
    // No `jwk` — per normative (sub_jwk lives in the payload).
}

#[derive(Debug, Serialize, Deserialize)]
pub struct IdTokenPayload {
    pub iss: String,
    pub sub: String,
    pub aud: String,
    pub nonce: String,
    pub iat: u64,
    pub exp: u64,
    /// Per OID4VP / SIOPv2 §"Self-issued ID Token claims".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub_jwk: Option<PublicKeyJwk>,
}

pub fn encode_segment<T: Serialize>(value: &T) -> Result<String, serde_json::Error> {
    Ok(URL_SAFE_NO_PAD.encode(serde_json::to_vec(value)?))
}
```

- [ ] **Step 2: builders/mod.rs**

```rust
use async_trait::async_trait;

use super::errors::LoginError;
use super::request::AuthorizationRequest;
use super::response::AuthorizationResponse;

#[async_trait]
pub trait ResponseBuilder: Send + Sync {
    /// Mutate `resp` to add this builder's contribution. Run in
    /// declaration order; later builders see fields earlier ones
    /// populated.
    async fn build(
        &self,
        req: &AuthorizationRequest,
        resp: &mut AuthorizationResponse,
    ) -> Result<(), LoginError>;
}

mod id_token_builder;
pub use id_token_builder::IdTokenBuilder;
```

- [ ] **Step 3: builders/id_token_builder.rs**

```rust
use std::sync::Arc;

use async_trait::async_trait;

use super::ResponseBuilder;
use crate::clock::Clock;
use crate::oid4vp_client::errors::LoginError;
use crate::oid4vp_client::id_token::{encode_segment, IdTokenPayload, JwsHeader};
use crate::oid4vp_client::ports::{DidAuthnDiscovery, DidSigner};
use crate::oid4vp_client::request::AuthorizationRequest;
use crate::oid4vp_client::response::AuthorizationResponse;
use crate::DidId;

pub struct IdTokenBuilder {
    pub discovery: Arc<dyn DidAuthnDiscovery>,
    pub signer: Arc<dyn DidSigner>,
    pub clock: Arc<dyn Clock>,
    pub holder: DidId,
    pub lifetime_secs: u64,
}

#[async_trait]
impl ResponseBuilder for IdTokenBuilder {
    async fn build(
        &self,
        req: &AuthorizationRequest,
        resp: &mut AuthorizationResponse,
    ) -> Result<(), LoginError> {
        // ① one discovery call
        let key = self
            .discovery
            .authn_key(&self.holder)
            .await
            .map_err(|e| LoginError::DiscoverFailed(e.to_string()))?;

        // ② compose
        let iat = self.clock.now_ms() / 1_000;
        let header = JwsHeader { alg: "EdDSA", typ: "JWT", kid: &key.kid };
        let payload = IdTokenPayload {
            iss: self.holder.to_did_string(),
            sub: self.holder.to_did_string(),
            aud: req.client_id.clone(),
            nonce: req.nonce.clone(),
            iat,
            exp: iat + self.lifetime_secs,
            sub_jwk: Some(key.public_jwk),
        };
        let header_b64 = encode_segment(&header).map_err(|e| LoginError::Internal(e.to_string()))?;
        let payload_b64 = encode_segment(&payload).map_err(|e| LoginError::Internal(e.to_string()))?;
        let sign_input = format!("{header_b64}.{payload_b64}");

        // ③ one sign call
        let sig = self
            .signer
            .sign(&key.kid, sign_input.as_bytes())
            .await
            .map_err(|e| LoginError::SignFailed(e.to_string()))?;
        let sig_b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&sig);

        resp.id_token = Some(format!("{sign_input}.{sig_b64}"));
        Ok(())
    }
}
```

- [ ] **Step 4: `errors.rs`** with the unified `LoginError` enum.

- [ ] **Step 5: Unit tests**:
  - happy path round-trip — builder fills `resp.id_token`, decoded JWS has correct shape (typ, alg, kid in header; iss/sub/aud/nonce/iat/exp/sub_jwk in payload)
  - **count-assert**: exactly 1 call to discovery, 1 call to signer (use a counting mock)
  - discovery error → LoginError::DiscoverFailed
  - sign error → LoginError::SignFailed
  - `sub_jwk` Some(jwk) with kty=OKP

- [ ] **Step 6: Commit** — `feat(wallet-core): IdTokenBuilder with single-resolve / single-sign + sub_jwk in payload`

---

### Task 6: `LoginCoordinator` + new `run_authentication`

**Files:**
- Modify: `mobile-bench/wallet-core/src/oid4vp_client/mod.rs`

- [ ] **Step 1: Rewrite mod.rs**

```rust
mod builders;
pub mod errors;
mod id_token;
pub mod ports;
pub mod request;
pub mod response;
mod jws;       // ← retained for transitional compat, removed in Task 9
mod parser;    // ← retained for transitional compat, removed in Task 9
mod respond;   // ← retained for transitional compat, removed in Task 9

pub use builders::{IdTokenBuilder, ResponseBuilder};
pub use errors::LoginError;
pub use ports::{AuthnKey, DidAuthnDiscovery, DidSigner, DiscoverError, SignError};
pub use request::{AuthorizationRequest, RequestParseError, ResponseType, ResponseMode};
pub use response::{AuthorizationResponse, PostResponseError, PostResponseResult};

// Legacy re-exports for transitional compat.
pub use jws::{build_id_token, IdTokenError};
pub use parser::{parse_request_url as legacy_parse_request_url, AuthRequest, Oid4vpParseError};
pub use respond::{post_response as legacy_post_response};

pub struct LoginCoordinator {
    builders: Vec<Box<dyn ResponseBuilder>>,
}

impl LoginCoordinator {
    pub fn new(builders: Vec<Box<dyn ResponseBuilder>>) -> Self {
        Self { builders }
    }

    /// Mode A convenience: id_token only.
    pub fn mode_a(builder: IdTokenBuilder) -> Self {
        Self::new(vec![Box::new(builder)])
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AuthFlowError {
    #[error(transparent)]
    Request(#[from] RequestParseError),
    #[error(transparent)]
    Build(#[from] LoginError),
    #[error(transparent)]
    Post(#[from] PostResponseError),
}

pub async fn run_authentication(
    http: &dyn crate::HttpClient,
    coordinator: &LoginCoordinator,
    qr_url: &str,
) -> Result<PostResponseResult, AuthFlowError> {
    let request_uri = request::parse_request_url(qr_url)?;
    let req = request::fetch_request_object(http, &request_uri).await?;
    let mut resp = AuthorizationResponse::new(req.state.clone());
    for b in &coordinator.builders {
        b.build(&req, &mut resp).await?;
    }
    let result = response::post_response(http, &req.response_uri, &resp).await?;
    Ok(result)
}
```

- [ ] **Step 2: Flow tests**

Replace the existing `mod flow_tests` with one matching the new shape — happy path using mocks against the new request/response types. Keep the old test marked `#[ignore]` so the diff is visible.

- [ ] **Step 3: Commit** — `feat(wallet-core): LoginCoordinator + new run_authentication (Mode A)`

---

### Task 7: Update issuer-mock — typed pipeline + `sub_jwk` payload + `response_uri`

**Files:**
- Create: `IssuerDIDIT-mock/src/services/oid4vpVerifier/pipeline.ts`
- Create: `IssuerDIDIT-mock/src/services/oid4vpVerifier/verifiers/*.ts` (one file per step)
- Modify: `IssuerDIDIT-mock/src/services/oid4vpVerifier.ts` (becomes a thin facade)
- Modify: `IssuerDIDIT-mock/src/routes/login.ts` (return `response_uri` + `response_type`)
- Modify: `IssuerDIDIT-mock/src/services/holderDidResolver.ts` (read `sub_jwk` payload claim; fall back to `jwk` header during transition)

- [ ] **Step 1: Per-verifier files**, each with a unit test:
  - `stateVerifier.ts`
  - `jwsStructureVerifier.ts`
  - `didResolverVerifier.ts`        (Phase 1: self-asserted; the swap-point for the indexer in Phase 2)
  - `keyAuthorizationVerifier.ts`
  - `jwsSignatureVerifier.ts`
  - `audienceVerifier.ts`
  - `freshnessVerifier.ts`
  - `nonceVerifier.ts`
- [ ] **Step 2: pipeline.ts** — orchestrates, logs `step=name status=ok` on info.
- [ ] **Step 3: Refactor `oid4vpVerifier.ts`** to call the pipeline; keep the old behaviour by default; flag-gated dual-read of header `jwk` vs payload `sub_jwk` (drop header support in Task 9).
- [ ] **Step 4: Update `login.ts`** — return `response_uri` + `response_type: "id_token"` + `response_mode: "direct_post"` + accept legacy `redirect_uri` field too during transition.
- [ ] **Step 5: jest tests** — full negative matrix from the guide.
- [ ] **Step 6: Commit** — `feat(issuer-mock): typed verification pipeline + normative wire format` (DCO+GPG, repo's own convention).

This task lives in the issuer-mock repo (`midnight-identity-solution-examples` on `develop`), not in this worktree.

---

### Task 8: Wire the new ports into the dioxus-wallet click site

**Files:**
- Modify: `mobile-bench/dioxus-wallet/src/identity_centre.rs` — `run_oid4vp_authenticate` is rewritten to build `LoginCoordinator::mode_a` from `CachedWalletAuthnDiscovery + RedbDidSigner + SystemClock + did` and call the new `run_authentication`.

- [ ] **Step 1: Replace the body of `run_oid4vp_authenticate`** with the coordinator-based version.
- [ ] **Step 2: Remove unused imports** (`oid4vp_run_authentication` legacy re-export goes away once the call site stops referencing it).
- [ ] **Step 3: Build + install + on-phone smoke test** — scan the issuer's QR, expect:
  - one `op indexer.contract_state` (not two)
  - one `secret_store.sign` (not two)
  - issuer pipeline log shows 8 named steps, all `status=ok`
  - VC flow continues unchanged
- [ ] **Step 4: Commit** — `feat(dioxus-wallet): route OID4VP click through LoginCoordinator + new ports`

---

### Task 9: Cleanup — drop the old `did_auth::sign_for_authentication`, old `jws`, old `parser`, old `respond`

**Files:**
- Delete: `mobile-bench/wallet-core/src/oid4vp_client/jws.rs`
- Delete: `mobile-bench/wallet-core/src/oid4vp_client/parser.rs`
- Delete: `mobile-bench/wallet-core/src/oid4vp_client/respond.rs`
- Delete or shrink: `mobile-bench/wallet-core/src/did_auth/mod.rs` (only if no OID4VCI caller remains; otherwise it stays for Phase-2 OID4VCI cleanup)
- Modify: `mobile-bench/wallet-core/src/oid4vp_client/mod.rs` (drop legacy re-exports)
- Modify: issuer-mock — drop legacy `jwk` header read + `redirect_uri` field.

- [ ] **Step 1: Confirm no callers** — grep `build_id_token`, `legacy_parse_request_url`, etc. should return nothing in `dioxus-wallet` after Task 8.
- [ ] **Step 2: Delete + grep again**.
- [ ] **Step 3: Drop dual-read on the issuer** — single source of truth for the wire format.
- [ ] **Step 4: Commit** — `chore(oid4vp): remove transitional compat layers`

---

### Task 10: Integration test — full negative matrix

**Files:**
- New: `mobile-bench/wallet-core/tests/oid4vp_login_e2e.rs` (integration test)
- Issuer-side: existing jest matrix from Task 7.

- [ ] **Step 1: Stand up a `MockHttpClient` fixture that emulates the issuer pipeline**, configurable per-test (which step fails).
- [ ] **Step 2: Code each of the 9 Phase-1 cases from the spec's test matrix.**
- [ ] **Step 3: Commit** — `test(wallet-core): OID4VP login negative test matrix`

---

## Acceptance criteria

- [ ] Demo flow (scan QR → authenticated → KYC → VC) still works on the phone.
- [ ] **Exactly one** `indexer.contract_state` op + one `secret_store.sign` op per login (verified via logcat `wallet_core::metrics` events).
- [ ] id_token payload contains `sub_jwk`; JWS header contains `kid` but no `jwk`.
- [ ] POST goes to `response_uri`; `redirect_uri` no longer present on the wire.
- [ ] Issuer logs show 8 named pipeline steps per login, each `status=ok`.
- [ ] Negative test matrix passes on both ends.
- [ ] No compile warnings on either target.

## Rollback per task

Each task lands as a single signed commit with no dependents until the next one. To roll back, `git revert <sha>`. Tasks 4-6 introduce new code without removing the old — that intermediate state is the safety net for Tasks 7-8.
