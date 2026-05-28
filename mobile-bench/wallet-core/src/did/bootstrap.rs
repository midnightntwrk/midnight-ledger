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

use crate::secret_storage::{PublicJwk, SecretKeyRef, SecretStorage};
use crate::wallet::Wallet;
use crate::DidId;

/// Result of a successful bootstrap.
#[derive(Debug, Clone)]
pub struct BootstrappedDid {
    pub did: DidId,
    pub ed25519_ref: SecretKeyRef,
    pub jubjub_ref: SecretKeyRef,
    /// The per-DID random 32-byte controller secret minted by
    /// the chain at deploy time. The wallet needs this to drive
    /// any subsequent update / deactivate / VM-rotation circuit
    /// (`assertControllerCanUpdate` reads
    /// `controllerKey(localSecretKey())` against the on-chain
    /// `controllerPublicKey`, so the caller MUST persist this
    /// somewhere — typically `BridgeState::remember_controller_secret`).
    pub controller_sk: [u8; 32],
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

/// Public so the `did-bootstrap` CLI can re-derive the secrets for the output keystore without widening the `SecretStorage` trait with an `export_secret` method.
pub fn derive_keys(seed: &[u8; 32]) -> ([u8; 32], [u8; 32]) {
    let h = Hkdf::<Sha256>::new(Some(b"midnight-identity-centre-v1"), seed);
    let mut ed = [0u8; 32];
    let mut jb = [0u8; 32];
    h.expand(b"ed25519/authentication", &mut ed)
        .expect("HKDF expand for ed25519");
    h.expand(b"jubjub/assertionMethod", &mut jb)
        .expect("HKDF expand for jubjub");
    (ed, jb)
}

/// Compose the JSON arg the `setVerificationMethod` Compact circuit
/// expects. The schema lives in `~/iohk/midnight-did/packages/contract/src/did.compact`
/// (and its compiled JS view at
/// `packages/contract/dist/managed/did/contract/index.js`):
///
/// ```compact
/// export struct VerificationMethod {
///     id: Opaque<"string">,
///     typ: VerificationMethodType,
///     publicKeyJwk: PublicKeyJwk
/// };
/// export struct PublicKeyJwk {
///     kty: KeyType, crv: CurveType, x: Opaque<"string">, y: Opaque<"string">
/// }
/// export enum VerificationMethodType { Undefined, JsonWebKey }
/// export enum KeyType { EC, RSA, oct, OKP }
/// export enum CurveType { Ed25519, X25519, Jubjub, P256, Secp256k1 }
/// ```
///
/// 2026-05-28 schema refresh (`feat!: Redesign DID verification
/// method storage`): `x` / `y` reverted from `Bytes<32>` back to
/// `Opaque<"string">`, carrying the JWK base64url coordinates
/// directly. Sending them as `{"$bytes": "0x..."}` no longer
/// applies — the harness's `reviveBigints` would convert them to
/// a `Uint8Array`, but the contract now expects a JS string. The
/// `CurveType` enum keeps its post-2026-05-27 ordering: Jubjub at
/// tag 2, P256 at tag 3.
///
/// Wire format the harness's `prepareUnprovenCallTx` accepts:
///
/// ```json
/// {
///   "id": "<did>#<fragment>",
///   "typ": 1,
///   "publicKeyJwk": {
///     "kty": <enum tag, 0..=3>,
///     "crv": <enum tag, 0..=4>,
///     "x":   "<base64url-coord-string>",
///     "y":   "<base64url-coord-string>"
///   }
/// }
/// ```
///
/// For Ed25519 keys the secret store returns
/// `{ kty: OKP, crv: Ed25519, x: base64url(pub_32B) }` with no `y`;
/// we send the full 32-byte compressed pubkey's base64url as `x`
/// and an empty string as `y` — matches the upstream's
/// `publicKeyJwkToLedger` fallback (`y: jwk.y ?? ""`). Jubjub /
/// P-256 ship both coordinates as their JWK base64url strings.
fn build_verification_method_json(
    did: &DidId,
    fragment: &str,
    jwk: &PublicJwk,
) -> serde_json::Value {
    use crate::secret_storage::{MidnightCurve, MidnightKeyType};

    // .compact: `enum KeyType { EC, RSA, oct, OKP }` (0-indexed).
    // Our crate's MidnightKeyType has only OKP + EC, in OKP-first
    // order; map by name.
    let kty_tag: i32 = match jwk.kty {
        MidnightKeyType::EC => 0,
        // RSA = 1 (not in our enum)
        // oct = 2 (not in our enum)
        MidnightKeyType::OKP => 3,
    };
    // .compact (post-2026-05-27 refactor):
    // `enum CurveType { Ed25519, X25519, Jubjub, P256, Secp256k1 }`
    // — NOT `MidnightCurve`'s declaration order. Verified at
    // runtime via the compiled enum
    // (packages/contract/dist/managed/did/contract/index.js).
    let crv_tag: i32 = match jwk.crv {
        MidnightCurve::Ed25519 => 0,
        // X25519 = 1 (not in our enum)
        MidnightCurve::Jubjub => 2,
        MidnightCurve::P256 => 3,
        // Secp256k1 = 4 (not in our enum)
    };

    // Canonical methodId per the upstream's
    // `normalizeBoundFragmentId`
    // (`midnight-did/packages/domain/src/ledger-utils.ts`): a
    // hash-prefixed bare fragment. Sending the full DID URL
    // (`did:midnight:...#fragment`) is also accepted by the
    // normalizer when the subject matches, but the on-chain
    // contract stores the value normalized form, and
    // `setVerificationMethodRelation` later looks up the VM by
    // `#fragment` — passing the full URL there fails with "failed
    // assert: Verification method does not exist" because string
    // equality misses the stored short form.
    let _ = did; // kept for signature parity; the relation step
                 // does its own subject sanity-check.

    // `x` / `y` are passed verbatim as the JWK base64url strings.
    // No `$bytes` wrapper — that was needed only while the schema
    // briefly stored the coordinates as `Bytes<32>` (2026-05-27).
    // For Ed25519 the JWK omits `y`; we send the empty string,
    // matching the upstream `publicKeyJwkToLedger` fallback
    // (`y: jwk.y ?? ""`).
    let y_str: String = jwk.y.clone().unwrap_or_default();
    serde_json::json!({
        "id": format!("#{}", fragment),
        "typ": 1,
        "publicKeyJwk": {
            "kty": kty_tag,
            "crv": crv_tag,
            "x": jwk.x.clone(),
            "y": y_str,
        }
    })
}

/// Decode a Jubjub-curve JWK's `(x, y)` coordinates back into raw
/// 32-byte big-endian field elements — the same encoding
/// `jubjub_public_jwk_from_point` in `secret_storage/curve_support`
/// emits before base64url-encoding them. Used by the bootstrap to
/// route Jubjub keys through `setSchnorrJubjubVerificationMethod`
/// (the JWK form is no longer accepted by the contract's
/// `assertSupportedVerificationMethod` check).
fn decode_jubjub_coords(jwk: &PublicJwk) -> Result<([u8; 32], [u8; 32]), String> {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine as _;
    let y_str = jwk
        .y
        .as_ref()
        .ok_or_else(|| "Jubjub JWK missing y coordinate".to_string())?;
    let x = URL_SAFE_NO_PAD
        .decode(jwk.x.as_bytes())
        .map_err(|e| format!("decode jubjub x b64url: {e}"))?;
    let y = URL_SAFE_NO_PAD
        .decode(y_str.as_bytes())
        .map_err(|e| format!("decode jubjub y b64url: {e}"))?;
    if x.len() != 32 || y.len() != 32 {
        return Err(format!(
            "Jubjub coords must be 32 bytes each (x={}, y={})",
            x.len(),
            y.len()
        ));
    }
    let mut x_arr = [0u8; 32];
    let mut y_arr = [0u8; 32];
    x_arr.copy_from_slice(&x);
    y_arr.copy_from_slice(&y);
    Ok((x_arr, y_arr))
}

/// Atomically create a DID and attach the two Phase 1 verification
/// methods: Ed25519 in `authentication`, Jubjub in `assertionMethod`.
///
/// Six-step flow:
/// 1. Import both HKDF-derived secrets into the secret store
///    (keys-first: surviving a later crash leaves keys recoverable
///    from disk/redb).
/// 2. Deploy the DID contract, capturing the freshly-minted
///    `controller_sk` the wallet committed at deploy time.
/// 3. Fetch the public-key JWK for each key.
/// 4. Attach Ed25519 → register VM + push relation `authentication`.
/// 5. Attach Jubjub  → register VM + push relation `assertionMethod`.
/// 6. Resolve the DID and assert both relation arrays are populated.
///
/// On any step's failure the wallet's secret store retains both
/// imported keys (step 1 is idempotent against the seed); the
/// on-chain DID, if step 2 succeeded, is left in whatever partial
/// state the failing step reached.
pub async fn bootstrap_did_with_keys(
    wallet: &Wallet,
    secret_store: &mut dyn SecretStorage,
    seed: &[u8; 32],
) -> Result<BootstrappedDid, BootstrapError> {
    let (ed_bytes, jb_bytes) = derive_keys(seed);

    // 1. Create the DID on chain. We need its address before we
    //    can build the DID-URL-form kids (`<did>#key-auth`) that
    //    `did_auth::sign_for_authentication` will use to look the
    //    secret up later — the doc references VMs by full URL, so
    //    if the kid in the secret store were the bare
    //    "ed25519/authentication" tag, `SecretStorage::find_by_kid`
    //    would miss every lookup. Order here is therefore:
    //    create_did → indexer-settle → import keys with DID-URL
    //    kids → attach VMs/relations.
    let (did, controller_sk) = wallet
        .create_did_awaitable_with_controller()
        .await
        .map_err(|e| BootstrapError::CreateDid(e.to_string()))?;

    // 1.5 Wait for the indexer to ingest the freshly-deployed
    //     contract. `create_did_awaitable_with_controller` returns
    //     as soon as the node confirms block inclusion, but every
    //     follow-up `add_verification_method` call goes through
    //     `Wallet::call_did_circuit`, which queries
    //     `indexer.contract_state(addr_hex)` for the on-chain
    //     state. The indexer has its own ingestion lag (a few
    //     seconds on standalone, longer on PreProd), so without
    //     this poll, step 4 fails immediately with "no on-chain
    //     state for <addr> — was the DID deployed?". This was a
    //     real bug surfaced by `tests/did_bootstrap_standalone.rs`
    //     against the standalone docker stack on 2026-05-27.
    wait_for_indexer_settle(wallet, &did).await?;

    // 1.7 Load the two circuits we're about to call. A freshly-
    //     deployed DID contract has zero entries in its
    //     `operations` map; the first `ContractCall` for a circuit
    //     requires that circuit's verifier key to be loaded via
    //     a `MaintenanceUpdate` first. The dioxus-wallet's
    //     Operation Builder does this auto-load lazily before each
    //     call; here we do both circuits up front since bootstrap
    //     always needs them.
    //
    //     The maintenance-authority counter starts at 0 on a fresh
    //     deploy and bumps by 1 per accepted MaintenanceUpdate.
    //     Surfaces a clear error if either load fails before we
    //     spend time on the ContractCalls.
    wallet
        .load_did_circuit_awaitable(did.clone(), "setVerificationMethod".to_string(), 0)
        .await
        .map_err(|e| BootstrapError::CreateDid(format!("load setVerificationMethod VK: {e}")))?;
    // The indexer needs to ingest the counter bump from the first
    // MaintenanceUpdate before the next one is accepted — without
    // this wait, the second load lands with `counter: 1` but the
    // chain still sees `maintenance_authority.counter = 0` and
    // rejects with `Invalid Transaction (1010)`. Same shape as the
    // `wait_for_indexer_settle` we already do after create_did,
    // but the criterion is "counter == N" not "contract exists".
    wait_for_counter(wallet, &did, 1).await?;
    // 2026-05-28 schema refresh: the plain `setVerificationMethod`
    // map rejects Jubjub JWKs. The assertion VM goes through
    // `setSchnorrJubjubVerificationMethod` instead, so we need its
    // VK on-chain too before step 5 can land.
    wallet
        .load_did_circuit_awaitable(
            did.clone(),
            "setSchnorrJubjubVerificationMethod".to_string(),
            1,
        )
        .await
        .map_err(|e| {
            BootstrapError::CreateDid(format!(
                "load setSchnorrJubjubVerificationMethod VK: {e}"
            ))
        })?;
    wait_for_counter(wallet, &did, 2).await?;
    wallet
        .load_did_circuit_awaitable(
            did.clone(),
            "setVerificationMethodRelation".to_string(),
            2,
        )
        .await
        .map_err(|e| {
            BootstrapError::CreateDid(format!(
                "load setVerificationMethodRelation VK: {e}"
            ))
        })?;
    // Same wait before the first ContractCall — the call itself
    // doesn't care about counter; it cares about the `operations`
    // map carrying the verifier key for the circuit. Indexer needs
    // to surface all three VK loads in its decoded contract state
    // before the call.
    wait_for_counter(wallet, &did, 3).await?;

    // 2. Import keys into the secret store with DID-URL-form kids.
    //    The kid we register here is what `find_by_kid` will match
    //    on when `sign_for_authentication` walks the doc's
    //    `authentication[0]` reference (a `<did>#key-auth` string).
    //    Using the short tag form (`"ed25519/authentication"`) would
    //    work for the on-chain VM but not for off-chain auth flows
    //    like SIOPv2 / OID4VP.
    let did_str = did.to_did_string();
    let ed_kid = format!("{did_str}#key-auth");
    let jb_kid = format!("{did_str}#key-assert");
    let ed25519_ref = secret_store
        .import_ed25519(&ed_bytes, &ed_kid)
        .await
        .map_err(|e| BootstrapError::AttachAuthn(format!("import ed25519: {e}")))?;
    let jubjub_ref = secret_store
        .import_jubjub(&jb_bytes, &jb_kid)
        .await
        .map_err(|e| BootstrapError::AttachAssertion(format!("import jubjub: {e}")))?;

    // 3. Fetch the JWKs the secret store assembled from the imported
    //    private bytes. Encoding (base64url vs decimal bigint) is
    //    curve-specific and already baked into the JWK by
    //    `curve_support::from_private_bytes`.
    let ed_jwk = secret_store
        .get_public_key(ed25519_ref.uuid())
        .await
        .map_err(|e| BootstrapError::AttachAuthn(format!("get_public_key ed25519: {e}")))?;
    let jb_jwk = secret_store
        .get_public_key(jubjub_ref.uuid())
        .await
        .map_err(|e| {
            BootstrapError::AttachAssertion(format!("get_public_key jubjub: {e}"))
        })?;

    let ed_vm_json = build_verification_method_json(&did, "key-auth", &ed_jwk);
    // Jubjub no longer rides through `build_verification_method_json`
    // — the new contract's `assertSupportedVerificationMethod`
    // explicitly rejects `kty=EC, crv=Jubjub` with
    // "EC keys must use P-256 or secp256k1; use SchnorrJubjub
    // methods for Jubjub". Decode the JWK coords back to raw BE
    // bytes; `Wallet::set_schnorr_jubjub_verification_method`
    // translates them into the `{$bigint}` shape the
    // `JubjubPoint` circuit arg consumes.
    let (jb_x_be, jb_y_be) = decode_jubjub_coords(&jb_jwk).map_err(|e| {
        BootstrapError::AttachAssertion(format!("decode jubjub jwk coords: {e}"))
    })?;

    // 4. Attach Ed25519 → authentication. Relation methodIds are
    //    the hash-prefixed canonical form per
    //    `normalizeBoundFragmentId`; matching the `id` field the
    //    VM was registered with above.
    //
    //    Between every chain-write step we wait for the indexer to
    //    catch up. `prepareUnprovenCallTx` builds its proof against
    //    whatever the indexer's current view of the contract state
    //    is; if it hasn't ingested the prior tx yet, the next
    //    circuit's input assertions look at a stale state and fail
    //    (e.g. `setVerificationMethodRelation` requires the VM to
    //    already be in `verificationMethods`).
    wallet
        .add_verification_method(&did, &ed25519_ref, ed_vm_json, controller_sk)
        .await
        .map_err(|e| BootstrapError::AttachAuthn(e.to_string()))?;
    // 1 VK load + 1 VK load + 1 ContractCall = counter at 3 now.
    wait_for_vm_count(wallet, &did, 1).await?;
    wallet
        .add_verification_method_relation(
            &did,
            "#key-auth",
            crate::did::VerificationMethodRelation::Authentication,
            controller_sk,
        )
        .await
        .map_err(|e| BootstrapError::AttachAuthn(e.to_string()))?;
    wait_for_authentication_count(wallet, &did, 1).await?;

    // 5. Attach Jubjub → assertionMethod. Goes through the
    //    dedicated `setSchnorrJubjubVerificationMethod` map at
    //    ledger index 9 (2026-05-28 schema refresh); the JWK
    //    `verificationMethods` map no longer accepts Jubjub
    //    keys. The relation circuit (`setVerificationMethodRelation`)
    //    is SHARED between both VM maps — it looks the VM up via
    //    `verificationMethodExists`, which checks BOTH maps —
    //    so the relation insert below is unchanged from how
    //    Ed25519 attaches above.
    wallet
        .set_schnorr_jubjub_verification_method(
            &did,
            &jubjub_ref,
            "#key-assert".to_string(),
            jb_x_be,
            jb_y_be,
            controller_sk,
        )
        .await
        .map_err(|e| BootstrapError::AttachAssertion(e.to_string()))?;
    // The Jubjub VM lives in the SchnorrJubjub map, not the JWK
    // map; ledger_to_domain folds both into `verification_method[]`
    // on the resolved doc, so the original `wait_for_vm_count(2)`
    // still polls the right counter (1 JWK + 1 Schnorr/Jubjub).
    wait_for_vm_count(wallet, &did, 2).await?;
    wallet
        .add_verification_method_relation(
            &did,
            "#key-assert",
            crate::did::VerificationMethodRelation::AssertionMethod,
            controller_sk,
        )
        .await
        .map_err(|e| BootstrapError::AttachAssertion(e.to_string()))?;

    // 6. Verify the resolved doc carries both relations. The last
    //    `setVerificationMethodRelation` tx may not have surfaced
    //    in the indexer yet — wait for it before the assertion or
    //    we'd fail with `MissingRelation("assertionMethod")` on a
    //    successful bootstrap that's just one block behind.
    wait_for_assertion_count(wallet, &did, 1).await?;
    let doc = wallet
        .resolve_did(&did.to_did_string())
        .await
        .map_err(|e| BootstrapError::Resolve(e.to_string()))?;
    if doc.authentication.is_empty() {
        return Err(BootstrapError::MissingRelation("authentication"));
    }
    if doc.assertion_method.is_empty() {
        return Err(BootstrapError::MissingRelation("assertionMethod"));
    }

    Ok(BootstrappedDid {
        did,
        ed25519_ref,
        jubjub_ref,
        controller_sk,
    })
}

/// Poll `Wallet::resolve_did` until either the indexer reports the
/// contract or the 30 s deadline expires. Used between step 2 and
/// step 4 of `bootstrap_did_with_keys` so the first
/// `add_verification_method` call doesn't lose the race with the
/// indexer's ingestion lag.
///
/// "Transient" failure = `DidError::Indexer` whose message starts
/// with `"no contract action for address"` — that's the exact
/// string `Wallet::resolve_did` emits when the contract isn't yet
/// in the indexer's view. Anything else (network error, decode
/// failure, wrong network) is surfaced immediately.
///
/// In stub mode (`wallet.stub_did_state().is_some()`) the resolve
/// returns the stub doc instantly on the first attempt, so the
/// poll loop is a no-op there.
async fn wait_for_indexer_settle(
    wallet: &Wallet,
    did: &crate::DidId,
) -> Result<(), BootstrapError> {
    let did_str = did.to_did_string();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let mut backoff_ms = 500u64;
    loop {
        match wallet.resolve_did(&did_str).await {
            Ok(_) => return Ok(()),
            Err(e) => {
                let msg = e.to_string();
                let transient = msg.contains("no contract action for address");
                if !transient {
                    return Err(BootstrapError::CreateDid(format!(
                        "indexer-settle for {did_str}: {msg}"
                    )));
                }
                if std::time::Instant::now() >= deadline {
                    return Err(BootstrapError::CreateDid(format!(
                        "indexer-settle timeout (30s) for {did_str}"
                    )));
                }
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(backoff_ms)).await;
        backoff_ms = (backoff_ms * 2).min(2000);
    }
}

/// Poll `Wallet::resolve_did_full` until the contract's
/// `maintenance_authority.counter` reaches `target` or the 30 s
/// deadline expires. Used after every `MaintenanceUpdate` in the
/// auto-load step so successive loads don't race the indexer.
///
/// `Invalid Transaction (1010)` on a `MaintenanceUpdate` whose
/// counter LOOKS correct usually means the indexer's view of the
/// counter is stale — the chain has the newer one. Polling the
/// indexer's `resolve_did_full` (which decodes the contract state
/// the indexer has ingested) until it catches up is the easiest
/// fix. The bound on retries protects against a stuck indexer:
/// after 30 s we surface the failure with the most recent observed
/// counter so the operator knows where the lag started.
///
/// In stub mode this is a no-op — the stub DID-document map
/// doesn't model the maintenance counter, so any value of `target`
/// returns instantly.
async fn wait_for_counter(
    wallet: &Wallet,
    did: &crate::DidId,
    target: u32,
) -> Result<(), BootstrapError> {
    #[cfg(any(test, feature = "test-support"))]
    if wallet.stub_did_state().is_some() {
        return Ok(());
    }
    let did_str = did.to_did_string();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let mut backoff_ms = 500u64;
    let mut last_seen: u32 = 0;
    loop {
        match wallet.resolve_did_full(&did_str).await {
            Ok(view) => {
                last_seen = view.maintenance_counter;
                if last_seen >= target {
                    return Ok(());
                }
            }
            Err(e) => {
                // Transient "no contract action" surfaces while the
                // indexer hasn't ingested the contract yet — same
                // shape as wait_for_indexer_settle treats it. Other
                // errors fail fast.
                let msg = e.to_string();
                if !msg.contains("no contract action for address") {
                    return Err(BootstrapError::CreateDid(format!(
                        "wait_for_counter for {did_str}: {msg}"
                    )));
                }
            }
        }
        if std::time::Instant::now() >= deadline {
            return Err(BootstrapError::CreateDid(format!(
                "wait_for_counter timeout (30s) for {did_str}: saw counter \
                 {last_seen}, target {target}"
            )));
        }
        tokio::time::sleep(std::time::Duration::from_millis(backoff_ms)).await;
        backoff_ms = (backoff_ms * 2).min(2000);
    }
}

/// Poll `Wallet::resolve_did` until the document's
/// `verification_method` vec reaches `target` entries (or 30 s).
/// Used after each `setVerificationMethod` so the next call's
/// `prepareUnprovenCallTx` sees the freshly-inserted VM in the
/// indexer's view of the contract state.
async fn wait_for_vm_count(
    wallet: &Wallet,
    did: &crate::DidId,
    target: usize,
) -> Result<(), BootstrapError> {
    #[cfg(any(test, feature = "test-support"))]
    if wallet.stub_did_state().is_some() {
        return Ok(());
    }
    let did_str = did.to_did_string();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let mut backoff_ms = 500u64;
    let mut last_seen = 0usize;
    loop {
        match wallet.resolve_did(&did_str).await {
            Ok(doc) => {
                last_seen = doc.verification_method.len();
                if last_seen >= target {
                    return Ok(());
                }
            }
            Err(e) => {
                let msg = e.to_string();
                if !msg.contains("no contract action for address") {
                    return Err(BootstrapError::CreateDid(format!(
                        "wait_for_vm_count for {did_str}: {msg}"
                    )));
                }
            }
        }
        if std::time::Instant::now() >= deadline {
            return Err(BootstrapError::CreateDid(format!(
                "wait_for_vm_count timeout (30s) for {did_str}: saw {last_seen} \
                 VMs, target {target}"
            )));
        }
        tokio::time::sleep(std::time::Duration::from_millis(backoff_ms)).await;
        backoff_ms = (backoff_ms * 2).min(2000);
    }
}

/// Poll `Wallet::resolve_did` until the `authentication` relation
/// vec reaches `target` entries (or 30 s). Used after the first
/// `setVerificationMethodRelation` for the Ed25519 key so the
/// downstream verifier flows can see the relation populated.
async fn wait_for_authentication_count(
    wallet: &Wallet,
    did: &crate::DidId,
    target: usize,
) -> Result<(), BootstrapError> {
    #[cfg(any(test, feature = "test-support"))]
    if wallet.stub_did_state().is_some() {
        return Ok(());
    }
    let did_str = did.to_did_string();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let mut backoff_ms = 500u64;
    let mut last_seen = 0usize;
    loop {
        match wallet.resolve_did(&did_str).await {
            Ok(doc) => {
                last_seen = doc.authentication.len();
                if last_seen >= target {
                    return Ok(());
                }
            }
            Err(e) => {
                let msg = e.to_string();
                if !msg.contains("no contract action for address") {
                    return Err(BootstrapError::CreateDid(format!(
                        "wait_for_authentication_count for {did_str}: {msg}"
                    )));
                }
            }
        }
        if std::time::Instant::now() >= deadline {
            return Err(BootstrapError::CreateDid(format!(
                "wait_for_authentication_count timeout (30s) for {did_str}: \
                 saw {last_seen}, target {target}"
            )));
        }
        tokio::time::sleep(std::time::Duration::from_millis(backoff_ms)).await;
        backoff_ms = (backoff_ms * 2).min(2000);
    }
}

/// Poll `Wallet::resolve_did` until the `assertion_method` relation
/// vec reaches `target` entries (or 30 s). Used after the final
/// `setVerificationMethodRelation` for the Jubjub key so the
/// bootstrap success assertion sees the relation populated.
async fn wait_for_assertion_count(
    wallet: &Wallet,
    did: &crate::DidId,
    target: usize,
) -> Result<(), BootstrapError> {
    #[cfg(any(test, feature = "test-support"))]
    if wallet.stub_did_state().is_some() {
        return Ok(());
    }
    let did_str = did.to_did_string();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let mut backoff_ms = 500u64;
    let mut last_seen = 0usize;
    loop {
        match wallet.resolve_did(&did_str).await {
            Ok(doc) => {
                last_seen = doc.assertion_method.len();
                if last_seen >= target {
                    return Ok(());
                }
            }
            Err(e) => {
                let msg = e.to_string();
                if !msg.contains("no contract action for address") {
                    return Err(BootstrapError::CreateDid(format!(
                        "wait_for_assertion_count for {did_str}: {msg}"
                    )));
                }
            }
        }
        if std::time::Instant::now() >= deadline {
            return Err(BootstrapError::CreateDid(format!(
                "wait_for_assertion_count timeout (30s) for {did_str}: \
                 saw {last_seen}, target {target}"
            )));
        }
        tokio::time::sleep(std::time::Duration::from_millis(backoff_ms)).await;
        backoff_ms = (backoff_ms * 2).min(2000);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[tokio::test]
    async fn bootstrap_populates_both_relations_in_returned_struct() {
        use crate::test_support::{stub_secret_store, stub_wallet};
        let wallet = stub_wallet();
        let mut store = stub_secret_store();
        let seed = [7u8; 32];

        let out = bootstrap_did_with_keys(&wallet, &mut store, &seed)
            .await
            .expect("bootstrap should succeed against stub");

        // Kids are the full DID-URL form (`<did>#fragment`) so
        // off-chain auth flows (SIOPv2 / OID4VP) can look them up
        // by walking `DidDocument.authentication[*]` → kid →
        // `SecretStorage::find_by_kid`. The fragment names match
        // what `bootstrap_did_with_keys` passes to
        // `build_verification_method_json`.
        let did_str = out.did.to_did_string();
        assert_eq!(
            out.ed25519_ref.id(),
            format!("{did_str}#key-auth"),
            "ed25519 kid must be the authentication-VM DID URL",
        );
        assert_eq!(
            out.jubjub_ref.id(),
            format!("{did_str}#key-assert"),
            "jubjub kid must be the assertionMethod-VM DID URL",
        );
        assert!(
            did_str.starts_with("did:midnight:"),
            "DID must be in the midnight namespace",
        );
    }
}
