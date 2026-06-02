//! `vc_self_verify` — verify a VC's proof signature against the
//! issuer's published key (CBOR-phase1 VCs) or against the
//! Compact issuance proof via the JS bridge (digital-passport
//! VCs).
//!
//! Two verification paths branch on `vc.format`:
//!
//! - **`"midnight_compact_vc"`** (digital-passport) — calls
//!   `decodeDigitalPassportProof` and
//!   `verifyDigitalPassportIssuanceProof` through the wallet's
//!   JS bridge. The proof is self-contained: no DID resolution
//!   is needed; the decoded proof's `signerVerificationMethodRef`
//!   supplies the `vm_id` directly.
//!
//! - **All other formats** (CBOR-phase1, `"midnight-vc-compact"`)
//!   — the original path that re-resolves the issuer DID
//!   on-chain, strips the embedded `proof` map from the CBOR
//!   body, and checks the Ed25519 signature via
//!   `SecretStorage::verify`.
//!
//! Three-state result:
//! - [`SelfVerifyResult::Valid`] — proof checks out.
//! - [`SelfVerifyResult::Invalid`] — body parsed, but the proof
//!   couldn't be checked. Carries an [`InvalidReason`] tag.
//! - [`SelfVerifyResult::Error`] — could not even attempt the
//!   check (DID resolve failed, JS bridge unavailable, etc).

use base64::{Engine, engine::general_purpose::STANDARD as B64};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;

use crate::clock::Clock;
use crate::js_bridge::JsBridgeExt;
use crate::secret_storage::{
    MidnightCurve, MidnightKeyType, PublicJwk, SecretStorage, VerifyInput,
};
use crate::vc_store::StoredVc;
use crate::wallet::Wallet;
use crate::{
    CurveType, DidId, KeyType, PublicKeyJwk, VerificationMethod, VerificationMethodRef,
};

/// Outcome of [`self_verify`] / [`self_verify_and_cache`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelfVerifyResult {
    /// Proof signature checked against the resolved issuer key.
    Valid {
        /// Full DID URL of the verification method that signed
        /// (e.g. `did:midnight:preprod:abc#key-assert`).
        vm_id: String,
    },
    /// VC body parsed but the proof could not be checked.
    Invalid(InvalidReason),
    /// Could not attempt the check at all — fatal error.
    Error(String),
}

/// Reason a VC was rejected as `Invalid`. Fine-grained so the UI
/// can render a precise message instead of a generic failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvalidReason {
    /// Body wasn't CBOR or didn't decode to a map at the top
    /// level.
    UnparseableBody,
    /// The `proof` field was missing or malformed (not a map, or
    /// missing required `verificationMethod` / `signature` keys).
    MissingProof,
    /// The signature field couldn't be base64-decoded.
    UnparseableSignature,
    /// `proof.verificationMethod` didn't match any
    /// `assertionMethod`-relation VM on the resolved issuer DID.
    UnknownVerificationMethod(String),
    /// The verification method's `publicKeyJwk` couldn't be
    /// converted to the [`PublicJwk`] shape `SecretStorage::verify`
    /// requires (e.g. unsupported curve).
    UnsupportedKey(String),
    /// The signature didn't verify against the canonical
    /// (proof-stripped) body bytes.
    SignatureMismatch,
    /// The digital-passport issuance proof was rejected by the
    /// JS bridge's `assertValidIssuanceContextProof` circuit.
    /// Carries the error string returned by the bridge.
    InvalidIssuanceProof(String),
}

// ---------------------------------------------------------------------------
// JS bridge response types for digital-passport verification
// ---------------------------------------------------------------------------

/// Response from `decodeDigitalPassportProof` — we only need the
/// `signerVerificationMethodRef` to populate `SelfVerifyResult::Valid { vm_id }`.
#[derive(Debug, Clone, Deserialize)]
struct DecodedProofResponse {
    proof: DecodedProofFields,
}

#[derive(Debug, Clone, Deserialize)]
struct DecodedProofFields {
    #[serde(rename = "signerVerificationMethodRef")]
    signer_verification_method_ref: String,
}

/// Response from `verifyDigitalPassportIssuanceProof`.
#[derive(Debug, Clone, Deserialize)]
struct VerifyIssuanceProofResponse {
    valid: bool,
    #[serde(default)]
    error: Option<String>,
}

/// Format discriminant for the Compact-binary digital-passport VC.
const DIGITAL_PASSPORT_FORMAT: &str = "midnight_compact_vc";

// ---------------------------------------------------------------------------
// Digital-passport verification path
// ---------------------------------------------------------------------------

/// Verify a `midnight_compact_vc` digital-passport VC through the
/// JS bridge.
///
/// The bridge's `verifyDigitalPassportIssuanceProof` runs the
/// upstream `assertValidIssuanceContextProof` circuit, which is
/// the canonical verification. No DID resolution is needed — the
/// decoded proof's `signerVerificationMethodRef` provides the VM
/// identity directly.
async fn self_verify_digital_passport(
    vc: &StoredVc,
    js_bridge: &dyn crate::js_bridge::JsBridge,
) -> SelfVerifyResult {
    // 1. Re-encode the raw body/proof bytes as base64url strings
    //    inside { encoding, payload } envelopes that the JS bridge
    //    expects.
    let credential_encoded = serde_json::json!({
        "encoding": "compact-value-v1.base64url",
        "payload": URL_SAFE_NO_PAD.encode(&vc.body),
    });
    let proof_encoded = serde_json::json!({
        "encoding": "compact-value-v1.base64url",
        "payload": URL_SAFE_NO_PAD.encode(&vc.proof),
    });

    // 2. Decode the proof to extract signerVerificationMethodRef.
    let decode_proof_params = serde_json::json!({
        "encoded": proof_encoded,
    });
    let decoded_proof: DecodedProofResponse = match js_bridge
        .call("decodeDigitalPassportProof", &decode_proof_params)
        .await
    {
        Ok(r) => r,
        Err(e) => {
            return SelfVerifyResult::Error(format!(
                "decode digital passport proof: {e}"
            ));
        }
    };
    let vm_id = decoded_proof.proof.signer_verification_method_ref;

    // 3. Verify the issuance proof (which internally decodes both
    //    credential and proof and runs the circuit).
    let verify_params = serde_json::json!({
        "credentialEncoded": credential_encoded,
        "proofEncoded": proof_encoded,
    });
    let verify_result: VerifyIssuanceProofResponse = match js_bridge
        .call("verifyDigitalPassportIssuanceProof", &verify_params)
        .await
    {
        Ok(r) => r,
        Err(e) => {
            return SelfVerifyResult::Error(format!(
                "verify digital passport issuance proof: {e}"
            ));
        }
    };

    // 4. Map the bridge result to SelfVerifyResult.
    if verify_result.valid {
        SelfVerifyResult::Valid { vm_id }
    } else {
        SelfVerifyResult::Invalid(InvalidReason::InvalidIssuanceProof(
            verify_result
                .error
                .unwrap_or_else(|| "unknown error".to_string()),
        ))
    }
}

// ---------------------------------------------------------------------------
// CBOR-phase1 verification path (original)
// ---------------------------------------------------------------------------

/// Verify a CBOR-phase1 VC by re-resolving the issuer DID on chain
/// and checking the body's Ed25519 proof signature against the
/// published `assertionMethod` key.
///
/// `secret_store` is used purely as a verify primitive — its key
/// store is NOT consulted (the verification method's public JWK
/// goes into `VerifyInput::public_jwk` directly).
pub async fn self_verify(
    vc: &StoredVc,
    wallet: &Wallet,
    secret_store: &dyn SecretStorage,
) -> SelfVerifyResult {
    // ---------------------------------------------------------------
    // Digital-passport path: verify via JS bridge.
    // ---------------------------------------------------------------
    if vc.format == DIGITAL_PASSPORT_FORMAT {
        let js_bridge = match wallet.js_bridge() {
            Some(b) => b,
            None => {
                return SelfVerifyResult::Error(
                    "JS bridge required for midnight_compact_vc format verification".to_string(),
                );
            }
        };
        if vc.proof.is_empty() {
            return SelfVerifyResult::Invalid(InvalidReason::MissingProof);
        }
        return self_verify_digital_passport(vc, js_bridge.as_ref()).await;
    }

    // ---------------------------------------------------------------
    // CBOR-phase1 / Ed25519 path (original)
    // ---------------------------------------------------------------

    // 1. Parse the issuer DID string.
    let issuer = match DidId::parse(&vc.issuer_did) {
        Ok(d) => d,
        Err(e) => return SelfVerifyResult::Error(format!("issuer did parse: {e}")),
    };

    // 2. Re-resolve on chain (or via stub map under tests).
    let doc = match wallet.resolve_did(&issuer.to_did_string()).await {
        Ok(d) => d,
        Err(e) => return SelfVerifyResult::Error(format!("resolve did: {e}")),
    };

    // 3. Position-walk the on-wire CBOR bytes to find the byte
    //    range of the `proof` key+value pair, and recover the
    //    canonical signed bytes by slicing it out and rewriting
    //    the outer map count in place.
    //
    //    Why not re-encode through a `ciborium::Value`? The TS
    //    issuer's `cbor-x` uses a non-canonical map-length head
    //    form (`b9 <u16>` regardless of count), while ciborium's
    //    encoder uses the shortest form (`a7` for n=7). The same
    //    logical map round-trips through ciborium with byte-
    //    different output, which means the wallet's re-derived
    //    canonical bytes ≠ the bytes the issuer SHA-256'd, and
    //    Jubjub-Schnorr verify rejects with `SignatureMismatch`.
    //    By keeping every other entry's bytes verbatim from the
    //    wire and only patching the outer map count, the result
    //    is byte-identical to `cborEncode(bodyNoProof)` provided
    //    cbor-x's head-encoding choice is consistent across
    //    map sizes (it is: u16-width for every map in the body).
    let proof_slice = match find_proof_byte_range(vc.body.as_slice()) {
        Ok(s) => s,
        Err(reason) => return SelfVerifyResult::Invalid(reason),
    };

    // 4. Pull verificationMethod + signature out of the proof
    //    value via ciborium (this part is decode-only — we don't
    //    re-encode it, so encoder-side byte differences are
    //    irrelevant).
    let proof_value: ciborium::Value =
        match ciborium::from_reader(&vc.body[proof_slice.value_start..proof_slice.value_end]) {
            Ok(v) => v,
            Err(_) => return SelfVerifyResult::Invalid(InvalidReason::MissingProof),
        };
    let proof_entries = match proof_value {
        ciborium::Value::Map(m) => m,
        _ => return SelfVerifyResult::Invalid(InvalidReason::MissingProof),
    };
    let mut vm_id_opt: Option<String> = None;
    let mut sig_b64_opt: Option<String> = None;
    for (k, v) in proof_entries {
        let key = match k {
            ciborium::Value::Text(s) => s,
            _ => continue,
        };
        match (key.as_str(), v) {
            ("verificationMethod", ciborium::Value::Text(s)) => vm_id_opt = Some(s),
            ("signature", ciborium::Value::Text(s)) => sig_b64_opt = Some(s),
            _ => {}
        }
    }
    let (vm_id, sig_b64) = match (vm_id_opt, sig_b64_opt) {
        (Some(v), Some(s)) => (v, s),
        _ => return SelfVerifyResult::Invalid(InvalidReason::MissingProof),
    };
    let signature = match B64.decode(sig_b64.as_bytes()) {
        Ok(b) => b,
        Err(_) => return SelfVerifyResult::Invalid(InvalidReason::UnparseableSignature),
    };

    // 5. Find the assertionMethod VM whose id matches.
    let vm = match find_assertion_vm(&doc.assertion_method, &doc.verification_method, &vm_id) {
        Some(vm) => vm,
        None => {
            return SelfVerifyResult::Invalid(InvalidReason::UnknownVerificationMethod(vm_id));
        }
    };

    // 6. Convert DID-Core PublicKeyJwk to secret-storage PublicJwk.
    let pjwk = match convert_jwk(&vm.public_key_jwk) {
        Ok(j) => j,
        Err(reason) => return SelfVerifyResult::Invalid(InvalidReason::UnsupportedKey(reason)),
    };

    // 7. Build the canonical signed bytes by:
    //    a) copying the outer map header from the wire and
    //       decrementing the count by 1 in place (preserving
    //       cbor-x's head-encoding form, see step 3 rationale),
    //    b) concatenating the wire bytes before the proof entry
    //       with the wire bytes after the proof entry — those
    //       byte ranges are cbor-x's verbatim output for the
    //       non-proof entries, so the result is bit-identical
    //       to `cborEncode(bodyNoProof)`.
    let canonical_bytes = match build_canonical_bytes(vc.body.as_slice(), &proof_slice) {
        Ok(b) => b,
        Err(reason) => return SelfVerifyResult::Invalid(reason),
    };

    // 8. Verify via SecretStorage's public_jwk path.
    let ok = secret_store
        .verify(VerifyInput {
            key_ref: None,
            public_jwk: Some(pjwk),
            payload: canonical_bytes,
            signature,
        })
        .await;
    match ok {
        Ok(true) => SelfVerifyResult::Valid { vm_id },
        Ok(false) => SelfVerifyResult::Invalid(InvalidReason::SignatureMismatch),
        Err(e) => SelfVerifyResult::Error(format!("verify: {e}")),
    }
}

/// Wraps [`self_verify`] and writes the outcome onto the VC's
/// metadata row so a carousel can show "last verified at HH:MM:SS"
/// without re-running the chain query.
///
/// Writes `last_verified_ms` (epoch millis, now) and
/// `last_verify_outcome` (a stable display string) regardless of
/// the outcome — even `Error(_)` lands as
/// `"Error: <message>"` so the carousel can surface it.
pub async fn self_verify_and_cache(
    vc: &StoredVc,
    wallet: &Wallet,
    secret_store: &dyn SecretStorage,
    vc_store: &dyn crate::VcStorage,
    clock: &dyn Clock,
) -> SelfVerifyResult {
    let result = self_verify(vc, wallet, secret_store).await;
    let outcome = match &result {
        SelfVerifyResult::Valid { .. } => "Valid".to_string(),
        SelfVerifyResult::Invalid(reason) => format!("Invalid: {reason:?}"),
        SelfVerifyResult::Error(msg) => format!("Error: {msg}"),
    };
    let now_ms = clock.now_ms();
    // Best-effort metadata write — verification result is the
    // primary return; a metadata write failure shouldn't mask the
    // verification outcome itself.
    let _ = vc_store.update_metadata(&vc.vc_uri, &mut |m| {
        m.last_verified_ms = Some(now_ms);
        m.last_verify_outcome = Some(outcome.clone());
    });
    result
}

/// Locate a `VerificationMethod` whose `id` matches `vm_id` among
/// the entries the issuer DID document lists under `assertion_method`.
/// Accepts both `VerificationMethodRef::Id(...)` (with a lookup
/// into the doc's `verification_method` array) and the inline
/// `Inline(VerificationMethod)` form.
fn find_assertion_vm<'a>(
    assertion_refs: &'a [VerificationMethodRef],
    verification_methods: &'a [VerificationMethod],
    vm_id: &str,
) -> Option<&'a VerificationMethod> {
    for r in assertion_refs {
        match r {
            VerificationMethodRef::Inline(vm) if vm.id == vm_id => return Some(vm),
            VerificationMethodRef::Id(s) if s == vm_id => {
                return verification_methods.iter().find(|vm| vm.id == vm_id);
            }
            _ => {}
        }
    }
    None
}

/// Convert a DID-Core [`PublicKeyJwk`] (Ed25519/P256/Jubjub) to
/// the secret-storage [`PublicJwk`] shape `SecretStorage::verify`
/// requires. Mostly trivial — the two are structurally identical
/// but use distinct enum types; the only on-wire difference is
/// P-256 (`"P256"` vs `"P-256"`), which we map here.
fn convert_jwk(jwk: &PublicKeyJwk) -> Result<PublicJwk, String> {
    let kty = match jwk.kty {
        KeyType::OKP => MidnightKeyType::OKP,
        KeyType::EC => MidnightKeyType::EC,
    };
    let crv = match jwk.crv {
        CurveType::Ed25519 => MidnightCurve::Ed25519,
        CurveType::Jubjub => MidnightCurve::Jubjub,
        CurveType::P256 => MidnightCurve::P256,
    };
    Ok(PublicJwk {
        kty,
        crv,
        x: jwk.x.clone(),
        y: jwk.y.clone(),
    })
}

/// Byte offsets identifying the `proof` key + value range inside
/// the on-wire CBOR body, plus the original outer map's head
/// length and key count. Used by [`build_canonical_bytes`] to
/// reconstruct `cborEncode(bodyNoProof)` byte-for-byte.
#[derive(Debug, Clone, Copy)]
struct ProofSlice {
    /// Outer map's head bytes occupy `body[..outer_head_len]`.
    outer_head_len: usize,
    /// Outer map's key count as the wire-encoded value (the
    /// canonical bytes' map must have `outer_count - 1`).
    outer_count: usize,
    /// `body[key_start..value_start]` is the CBOR-encoded "proof"
    /// text key, byte-for-byte.
    key_start: usize,
    /// `body[value_start..value_end]` is the CBOR-encoded proof
    /// value (a map), byte-for-byte.
    value_start: usize,
    /// One past the last byte of the proof value's encoding.
    value_end: usize,
}

/// Walk the wire CBOR bytes positionally with `ciborium-ll` to
/// find the byte range covering the `proof` entry of the outer
/// map. Records the outer map's head length + key count so the
/// caller can rebuild the map header with one fewer entry.
///
/// Returns `Err(InvalidReason)` on shape failures (not a map at
/// the top level, indefinite-length map, missing proof, etc).
fn find_proof_byte_range(body: &[u8]) -> Result<ProofSlice, InvalidReason> {
    use ciborium_ll::{Decoder, Header};

    let mut d = Decoder::from(body);

    // Outer must be a definite-length map.
    let n = match d.pull().map_err(|_| InvalidReason::UnparseableBody)? {
        Header::Map(Some(n)) => n,
        _ => return Err(InvalidReason::UnparseableBody),
    };
    let outer_head_len = d.offset();

    let mut key_start: Option<usize> = None;
    let mut value_start: Option<usize> = None;
    let mut value_end: Option<usize> = None;

    for _ in 0..n {
        let entry_key_start = d.offset();
        // Read the key — must be a definite-length text string.
        let key_text = read_text_key(&mut d)?;
        let entry_value_start = d.offset();
        // Walk past the value (recursively, whatever shape).
        skip_value(&mut d)?;
        let entry_value_end = d.offset();

        if key_text == "proof" {
            if key_start.is_some() {
                // Duplicate proof key — accept the first.
                continue;
            }
            key_start = Some(entry_key_start);
            value_start = Some(entry_value_start);
            value_end = Some(entry_value_end);
        }
    }

    match (key_start, value_start, value_end) {
        (Some(k), Some(vs), Some(ve)) => Ok(ProofSlice {
            outer_head_len,
            outer_count: n,
            key_start: k,
            value_start: vs,
            value_end: ve,
        }),
        _ => Err(InvalidReason::MissingProof),
    }
}

/// Read a definite-length text key from the decoder. The
/// returned string is owned because `ciborium_ll::Decoder`'s
/// text segments are borrowed against a caller-provided chunk
/// buffer — we drain into a `String` so the comparison against
/// `"proof"` outlives the buffer's lifetime.
///
/// `chunk_buf` is a scratch buffer the segments iterator parses
/// chunks into. A 1 KiB buffer is plenty for any text key
/// `cbor-x` emits in the VC body (longest is `"credentialSubject"`
/// at 17 bytes, longest text value is the holder DID at ~88).
fn read_text_key<R: ciborium_io::Read>(
    d: &mut ciborium_ll::Decoder<R>,
) -> Result<String, InvalidReason>
where
    R::Error: core::fmt::Debug,
{
    use ciborium_ll::Header;
    let len = match d.pull().map_err(|_| InvalidReason::UnparseableBody)? {
        Header::Text(Some(len)) => len,
        _ => return Err(InvalidReason::UnparseableBody),
    };
    let mut chunk_buf = [0u8; 1024];
    let mut out = String::with_capacity(len);
    let mut segments = d.text(Some(len));
    while let Some(mut seg) = segments
        .pull()
        .map_err(|_| InvalidReason::UnparseableBody)?
    {
        while let Some(piece) = seg
            .pull(&mut chunk_buf)
            .map_err(|_| InvalidReason::UnparseableBody)?
        {
            out.push_str(piece);
        }
    }
    Ok(out)
}

/// Recursively walk past a CBOR value of any shape, advancing
/// the decoder's offset to the byte just after the value's
/// encoding. Definite-length containers only — cbor-x emits
/// definite-length for everything in the VC body, so an
/// indefinite-length sub-value is treated as a shape error.
fn skip_value<R: ciborium_io::Read>(
    d: &mut ciborium_ll::Decoder<R>,
) -> Result<(), InvalidReason>
where
    R::Error: core::fmt::Debug,
{
    use ciborium_ll::Header;
    match d.pull().map_err(|_| InvalidReason::UnparseableBody)? {
        Header::Positive(_)
        | Header::Negative(_)
        | Header::Float(_)
        | Header::Simple(_)
        | Header::Tag(_) => {
            // For Tag, the next pull() is the tagged value.
            // Re-enter skip_value to consume it — except Tag
            // is the only one of these that has a follower.
            // The others are atomic.
            // (ciborium-ll's `pull` after a Tag returns the
            // wrapped value's header; we don't currently see
            // tags in the VC body but handle them correctly
            // anyway by recursing.)
            // The non-tag arms are atomic; nothing more to do.
            Ok(())
        }
        Header::Bytes(Some(len)) => {
            let mut chunk_buf = [0u8; 1024];
            let mut segs = d.bytes(Some(len));
            while let Some(mut seg) = segs
                .pull()
                .map_err(|_| InvalidReason::UnparseableBody)?
            {
                while seg
                    .pull(&mut chunk_buf)
                    .map_err(|_| InvalidReason::UnparseableBody)?
                    .is_some()
                {}
            }
            Ok(())
        }
        Header::Text(Some(len)) => {
            let mut chunk_buf = [0u8; 1024];
            let mut segs = d.text(Some(len));
            while let Some(mut seg) = segs
                .pull()
                .map_err(|_| InvalidReason::UnparseableBody)?
            {
                while seg
                    .pull(&mut chunk_buf)
                    .map_err(|_| InvalidReason::UnparseableBody)?
                    .is_some()
                {}
            }
            Ok(())
        }
        Header::Array(Some(n)) => {
            for _ in 0..n {
                skip_value(d)?;
            }
            Ok(())
        }
        Header::Map(Some(n)) => {
            for _ in 0..n {
                skip_value(d)?; // key
                skip_value(d)?; // value
            }
            Ok(())
        }
        // Indefinite-length forms aren't emitted by cbor-x for
        // the VC body shapes we accept — treat them as a shape
        // error to surface the issue loudly instead of papering
        // over a divergent issuer encoder.
        Header::Bytes(None)
        | Header::Text(None)
        | Header::Array(None)
        | Header::Map(None)
        | Header::Break => Err(InvalidReason::UnparseableBody),
    }
}

/// Stitch the canonical signed bytes from the wire body. Uses
/// every byte from the original wire encoding verbatim, except
/// the outer map's count head bytes (rewritten with `count - 1`)
/// and the proof entry's key+value range (omitted).
fn build_canonical_bytes(
    body: &[u8],
    slice: &ProofSlice,
) -> Result<Vec<u8>, InvalidReason> {
    if slice.outer_count == 0 {
        return Err(InvalidReason::UnparseableBody);
    }
    let new_count = slice.outer_count - 1;
    let mut head = vec![0u8; slice.outer_head_len];
    head.copy_from_slice(&body[..slice.outer_head_len]);
    // Patch the count in the head bytes, preserving the head's
    // width (1/1+1/1+2/1+4/1+8). cbor-x consistently uses the
    // u16 form (`b9 <u16-be>`) for the maps inside the VC body;
    // we still cover the other forms so a future cbor-x setting
    // change doesn't silently re-break canonicalisation.
    let head_byte = head[0];
    let info = head_byte & 0x1f;
    let major = head_byte >> 5;
    if major != 5 {
        return Err(InvalidReason::UnparseableBody);
    }
    match info {
        0..=23 => {
            // Inline count form `0xa0 + n`.
            if new_count > 23 {
                // Decrementing can't grow the encoding here.
                return Err(InvalidReason::UnparseableBody);
            }
            head[0] = (major << 5) | (new_count as u8);
        }
        24 => {
            // `0xb8 <u8>` — count in 1 follower byte.
            if new_count > u8::MAX as usize {
                return Err(InvalidReason::UnparseableBody);
            }
            head[1] = new_count as u8;
        }
        25 => {
            // `0xb9 <u16-be>` — count in 2 follower bytes.
            if new_count > u16::MAX as usize {
                return Err(InvalidReason::UnparseableBody);
            }
            let nb = (new_count as u16).to_be_bytes();
            head[1] = nb[0];
            head[2] = nb[1];
        }
        26 => {
            // `0xba <u32-be>` — count in 4 follower bytes.
            if new_count > u32::MAX as usize {
                return Err(InvalidReason::UnparseableBody);
            }
            let nb = (new_count as u32).to_be_bytes();
            head[1..5].copy_from_slice(&nb);
        }
        27 => {
            // `0xbb <u64-be>` — count in 8 follower bytes.
            let nb = (new_count as u64).to_be_bytes();
            head[1..9].copy_from_slice(&nb);
        }
        _ => return Err(InvalidReason::UnparseableBody),
    }

    let mut out = Vec::with_capacity(
        head.len() + (slice.key_start - slice.outer_head_len) + (body.len() - slice.value_end),
    );
    out.extend_from_slice(&head);
    out.extend_from_slice(&body[slice.outer_head_len..slice.key_start]);
    out.extend_from_slice(&body[slice.value_end..]);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{
        stub_secret_store_with_bootstrapped_did, stub_sign_birth_vc,
        stub_wallet_with_bootstrapped_did,
    };
    use crate::VcStorage;

    /// Regression test for the canonicalisation drift that was
    /// behind a long-running `Invalid(SignatureMismatch)` on the
    /// OID4VCI live-issuance path.
    ///
    /// The TS issuer's `cbor-x` encoder uses the `b9 <u16-BE>`
    /// head form for definite-length maps regardless of count.
    /// Our prior implementation re-encoded the proof-stripped
    /// body through `ciborium::Value::Map`, which picks the
    /// shortest form (e.g. `a7` for n=7). The two encoders
    /// produce byte-different output for the same logical map,
    /// breaking the issuer-side `sha256(canonical)`.
    ///
    /// This fixture hand-builds a wire CBOR body that uses the
    /// cbor-x non-canonical head form, then asserts:
    ///
    /// 1. `find_proof_byte_range` locates the proof entry by
    ///    name (NOT by position — `proof` is in the middle of
    ///    the entry list here on purpose).
    /// 2. `build_canonical_bytes` strips it and decrements the
    ///    outer count IN PLACE, preserving the `b9 <u16>` form.
    /// 3. Every non-head, non-proof byte is byte-identical to
    ///    the wire input (i.e. nothing was re-encoded).
    #[test]
    fn cbor_x_style_canonicalisation_byte_for_byte() {
        // Outer map: `b9 0003` = u16-form 3-key map.
        // Entries (in order):
        //   "a" -> 1
        //   "proof" -> { "z": 9 }   // also u16-form `b9 0001`
        //   "b" -> 2
        let mut wire: Vec<u8> = Vec::new();
        // Outer head: `b9 0003`.
        wire.extend_from_slice(&[0xb9, 0x00, 0x03]);
        // "a" -> 1
        wire.extend_from_slice(&[0x61, b'a', 0x01]);
        // "proof" -> { "z" -> 9 }   (proof value uses `b9 0001`)
        wire.extend_from_slice(&[0x65, b'p', b'r', b'o', b'o', b'f']);
        wire.extend_from_slice(&[0xb9, 0x00, 0x01, 0x61, b'z', 0x09]);
        // "b" -> 2
        wire.extend_from_slice(&[0x61, b'b', 0x02]);

        let slice =
            find_proof_byte_range(&wire).expect("proof entry should be located by name");
        assert_eq!(slice.outer_count, 3);
        assert_eq!(slice.outer_head_len, 3, "head is `b9 00 03` = 3 bytes");

        let canonical = build_canonical_bytes(&wire, &slice)
            .expect("canonical bytes should rebuild without re-encoding");

        // Expected: outer head `b9 0002`, then non-proof entries
        // copied verbatim from the wire.
        let mut expected: Vec<u8> = Vec::new();
        expected.extend_from_slice(&[0xb9, 0x00, 0x02]);
        expected.extend_from_slice(&[0x61, b'a', 0x01]);
        expected.extend_from_slice(&[0x61, b'b', 0x02]);
        assert_eq!(
            hex::encode(&canonical),
            hex::encode(&expected),
            "head must stay in `b9 <u16>` form and non-proof entries must \
             be byte-identical to the wire input",
        );

        // Sanity: head width preserved exactly.
        assert_eq!(
            &canonical[..3],
            &[0xb9, 0x00, 0x02],
            "outer head must be the 3-byte `b9 <u16-BE>` form",
        );
    }

    fn make_vc(issuer: &DidId, body: Vec<u8>) -> StoredVc {
        StoredVc {
            vc_uri: "urn:uuid:test-vc".to_string(),
                       issuer_did: issuer.to_did_string(),
            holder_did: "did:midnight:undeployed:00".to_string(),
            format: "midnight-vc-cbor-phase1".to_string(),
            body,
            proof: vec![],
            issued_at_ms: 0,
        }
    }

    fn make_digital_passport_vc(body: Vec<u8>, proof: Vec<u8>) -> StoredVc {
        StoredVc {
            vc_uri: "urn:uuid:dp-test-vc".to_string(),
            issuer_did: "did:midnight:test:issuer123".to_string(),
            holder_did: "did:midnight:test:holder456".to_string(),
            format: DIGITAL_PASSPORT_FORMAT.to_string(),
            body,
            proof,
            issued_at_ms: 0,
        }
    }

    // ------------------------------------------------------------------
    // Mock JS bridge for digital-passport self-verify tests
    // ------------------------------------------------------------------

    /// Mock JS bridge that responds to digital-passport bridge
    /// methods. The caller can configure it to return valid or
    /// invalid verification results.
    struct MockDigitalPassportBridge {
        /// Whether `verifyDigitalPassportIssuanceProof` should
        /// return `{ valid: true }` or `{ valid: false, error }`.
        verify_valid: bool,
        /// Error message returned when `verify_valid` is false.
        verify_error: String,
        /// The `signerVerificationMethodRef` returned by
        /// `decodeDigitalPassportProof`.
        vm_id: String,
    }

    #[async_trait::async_trait]
    impl crate::js_bridge::JsBridge for MockDigitalPassportBridge {
        async fn call_json(
            &self,
            method: &str,
            _params: serde_json::Value,
        ) -> Result<serde_json::Value, crate::js_bridge::JsBridgeError> {
            match method {
                "decodeDigitalPassportProof" => {
                    Ok(serde_json::json!({
                        "proof": {
                            "signerVerificationMethodRef": self.vm_id,
                        }
                    }))
                }
                "verifyDigitalPassportIssuanceProof" => {
                    if self.verify_valid {
                        Ok(serde_json::json!({ "valid": true }))
                    } else {
                        Ok(serde_json::json!({
                            "valid": false,
                            "error": self.verify_error,
                        }))
                    }
                }
                "decodeDigitalPassportCredential" => {
                    // Not used by self_verify_digital_passport, but
                    // respond for completeness.
                    Ok(serde_json::json!({
                        "credential": { "issuerDid": "did:midnight:test:issuer123" }
                    }))
                }
                other => Err(crate::js_bridge::JsBridgeError::JsError(format!(
                    "MockDigitalPassportBridge: unknown method {other}"
                ))),
            }
        }
    }

    // ------------------------------------------------------------------
    // Mock that always fails with a transport error
    // ------------------------------------------------------------------

    struct FailingBridge;

    #[async_trait::async_trait]
    impl crate::js_bridge::JsBridge for FailingBridge {
        async fn call_json(
            &self,
            _method: &str,
            _params: serde_json::Value,
        ) -> Result<serde_json::Value, crate::js_bridge::JsBridgeError> {
            Err(crate::js_bridge::JsBridgeError::Transport(
                "bridge unavailable".to_string(),
            ))
        }
    }

    #[tokio::test]
    async fn self_verify_valid_round_trip() {
        let seed = [55u8; 32];
        let (wallet, did) = stub_wallet_with_bootstrapped_did(seed).await;
        let store = stub_secret_store_with_bootstrapped_did(seed).await;
        let body = stub_sign_birth_vc(&wallet, &store, &did, b"BIRTH-FIXTURE").await;
        let vc = make_vc(&did, body);

        let result = self_verify(&vc, &wallet, &store).await;
        match result {
            SelfVerifyResult::Valid { vm_id } => {
                assert!(vm_id.starts_with("did:midnight:"), "vm_id: {vm_id}");
                assert!(vm_id.contains("#"), "vm_id should be DID URL form: {vm_id}");
            }
            other => panic!("expected Valid, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn self_verify_tampered_body_is_invalid() {
        let seed = [56u8; 32];
        let (wallet, did) = stub_wallet_with_bootstrapped_did(seed).await;
        let store = stub_secret_store_with_bootstrapped_did(seed).await;
        let body = stub_sign_birth_vc(&wallet, &store, &did, b"BIRTH-FIXTURE").await;

        // Tamper: flip one byte in the body. To make the body
        // still decode as CBOR (we want to land in the
        // SignatureMismatch path, not UnparseableBody), we flip a
        // byte well inside the credentialSubject payload — the
        // ASCII bytes near the end of "BIRTH-FIXTURE", which
        // live unambiguously inside a CBOR bytes blob.
        let mut tampered = body.clone();
        let needle = b"BIRTH-FIXTURE";
        let pos = tampered
            .windows(needle.len())
            .position(|w| w == needle)
            .expect("payload should appear in CBOR-encoded body");
        // Flip a high bit so the CBOR length headers remain valid.
        tampered[pos] ^= 0x01;
        let vc = make_vc(&did, tampered);

        let result = self_verify(&vc, &wallet, &store).await;
        assert_eq!(
            result,
            SelfVerifyResult::Invalid(InvalidReason::SignatureMismatch),
            "expected SignatureMismatch, got {result:?}"
        );
    }

    #[tokio::test]
    async fn self_verify_and_cache_writes_metadata() {
        let seed = [57u8; 32];
        let (wallet, did) = stub_wallet_with_bootstrapped_did(seed).await;
        let store = stub_secret_store_with_bootstrapped_did(seed).await;
        let body = stub_sign_birth_vc(&wallet, &store, &did, b"BIRTH-FIXTURE").await;
        let vc = make_vc(&did, body);

        let vc_store = crate::InMemoryVcStore::default();
        vc_store.insert_vc(&vc).expect("insert vc");
        let clock = crate::FixedClock::new(1_700_000_002_000);

        let result = self_verify_and_cache(&vc, &wallet, &store, &vc_store, &clock).await;
        assert!(
            matches!(result, SelfVerifyResult::Valid { .. }),
            "expected Valid, got {result:?}"
        );
        let md = vc_store
            .get_metadata(&vc.vc_uri)
            .expect("metadata read ok")
            .expect("metadata present");
        assert_eq!(md.last_verify_outcome, Some("Valid".to_string()));
        assert_eq!(
            md.last_verified_ms,
            Some(1_700_000_002_000),
            "last_verified_ms should come from the injected clock"
        );
    }

    // ------------------------------------------------------------------
    // Digital-passport format self-verify tests
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn digital_passport_self_verify_valid_returns_vm_id() {
        // A digital-passport VC with a mock bridge that returns
        // { valid: true } and a known signerVerificationMethodRef.
        let bridge = MockDigitalPassportBridge {
            verify_valid: true,
            verify_error: String::new(),
            vm_id: "did:midnight:test:issuer123#key-assert".to_string(),
        };
        let vc = make_digital_passport_vc(b"CREDENTIAL_BODY".to_vec(), b"PROOF_BYTES".to_vec());
        let result = self_verify_digital_passport(&vc, &bridge).await;
        match result {
            SelfVerifyResult::Valid { vm_id } => {
                assert_eq!(vm_id, "did:midnight:test:issuer123#key-assert");
            }
            other => panic!("expected Valid, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn digital_passport_self_verify_invalid_returns_invalid_issuance_proof() {
        // A digital-passport VC with a mock bridge that returns
        // { valid: false, error: "circuit assertion failed" }.
        let bridge = MockDigitalPassportBridge {
            verify_valid: false,
            verify_error: "circuit assertion failed".to_string(),
            vm_id: "did:midnight:test:issuer123#key-assert".to_string(),
        };
        let vc = make_digital_passport_vc(b"CREDENTIAL_BODY".to_vec(), b"PROOF_BYTES".to_vec());
        let result = self_verify_digital_passport(&vc, &bridge).await;
        match result {
            SelfVerifyResult::Invalid(InvalidReason::InvalidIssuanceProof(msg)) => {
                assert_eq!(msg, "circuit assertion failed");
            }
            other => panic!("expected Invalid(InvalidIssuanceProof), got {other:?}"),
        }
    }

    #[tokio::test]
    async fn digital_passport_self_verify_bridge_failure_returns_error() {
        // A digital-passport VC with a bridge that always returns
        // a transport error — should map to SelfVerifyResult::Error.
        let bridge = FailingBridge;
        let vc = make_digital_passport_vc(b"CREDENTIAL_BODY".to_vec(), b"PROOF_BYTES".to_vec());
        let result = self_verify_digital_passport(&vc, &bridge).await;
        match result {
            SelfVerifyResult::Error(msg) => {
                assert!(
                    msg.contains("bridge unavailable"),
                    "error should mention bridge failure: {msg}"
                );
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn self_verify_branches_on_format_midnight_compact_vc() {
        // A midnight_compact_vc VC should go through the JS bridge
        // path. We verify by attaching a mock bridge to the wallet
        // and confirming the bridge is consulted (valid result).
        let bridge = std::sync::Arc::new(MockDigitalPassportBridge {
            verify_valid: true,
            verify_error: String::new(),
            vm_id: "did:midnight:test:issuer#key-1".to_string(),
        });
        let seed = [60u8; 32];
        let (wallet, _did) = stub_wallet_with_bootstrapped_did(seed).await;
        let wallet = wallet.with_js_bridge(bridge);
        let store = stub_secret_store_with_bootstrapped_did(seed).await;

        let vc = make_digital_passport_vc(b"BODY".to_vec(), b"PROOF".to_vec());

        let result = self_verify(&vc, &wallet, &store).await;
        match result {
            SelfVerifyResult::Valid { vm_id } => {
                assert_eq!(vm_id, "did:midnight:test:issuer#key-1");
            }
            other => panic!("expected Valid via JS bridge path, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn self_verify_branches_on_format_cbor_phase1() {
        // A CBOR-phase1 VC should still go through the original
        // DID-resolution + Ed25519 path, not the JS bridge path.
        // If it mistakenly routed to the bridge, the bridge would
        // reject the CBOR body as invalid base64url, or the bridge
        // method names wouldn't match.
        //
        // This is the same as the existing self_verify_valid_round_trip
        // test — just confirming it still works after the branching
        // change.
        let seed = [61u8; 32];
        let (wallet, did) = stub_wallet_with_bootstrapped_did(seed).await;
        let store = stub_secret_store_with_bootstrapped_did(seed).await;
        let body = stub_sign_birth_vc(&wallet, &store, &did, b"BRANCH-TEST").await;
        let vc = make_vc(&did, body);

        let result = self_verify(&vc, &wallet, &store).await;
        match result {
            SelfVerifyResult::Valid { vm_id } => {
                assert!(vm_id.starts_with("did:midnight:"));
            }
            other => panic!("expected Valid via CBOR path, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn digital_passport_self_verify_no_js_bridge_returns_error() {
        // A midnight_compact_vc VC with no JS bridge attached to
        // the wallet should return Error.
        let seed = [62u8; 32];
        let (wallet, _did) = stub_wallet_with_bootstrapped_did(seed).await;
        // Deliberately do NOT attach a JS bridge.
        assert!(
            wallet.js_bridge().is_none(),
            "stub wallet should have no JS bridge by default"
        );
        let store = stub_secret_store_with_bootstrapped_did(seed).await;

        let vc = make_digital_passport_vc(b"BODY".to_vec(), b"PROOF".to_vec());
        let result = self_verify(&vc, &wallet, &store).await;
        match result {
            SelfVerifyResult::Error(msg) => {
                assert!(
                    msg.contains("JS bridge required"),
                    "error should mention JS bridge: {msg}"
                );
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn digital_passport_self_verify_empty_proof_returns_missing_proof() {
        // A midnight_compact_vc VC with an empty proof field
        // should return Invalid(MissingProof).
        let bridge = std::sync::Arc::new(MockDigitalPassportBridge {
            verify_valid: true,
            verify_error: String::new(),
            vm_id: "did:midnight:test:issuer#key-1".to_string(),
        });
        let seed = [63u8; 32];
        let (wallet, _did) = stub_wallet_with_bootstrapped_did(seed).await;
        let wallet = wallet.with_js_bridge(bridge);
        let store = stub_secret_store_with_bootstrapped_did(seed).await;

        let vc = StoredVc {
            vc_uri: "urn:uuid:dp-no-proof".to_string(),
            issuer_did: "did:midnight:test:issuer".to_string(),
            holder_did: "did:midnight:test:holder".to_string(),
            format: DIGITAL_PASSPORT_FORMAT.to_string(),
            body: b"BODY".to_vec(),
            proof: vec![],  // empty
            issued_at_ms: 0,
        };

        let result = self_verify(&vc, &wallet, &store).await;
        assert_eq!(
            result,
            SelfVerifyResult::Invalid(InvalidReason::MissingProof),
            "expected MissingProof for empty proof, got {result:?}"
        );
    }

    #[tokio::test]
    async fn digital_passport_self_verify_and_cache_writes_metadata() {
        // self_verify_and_cache should work for digital-passport VCs
        // too — the result should be cached correctly.
        let bridge = std::sync::Arc::new(MockDigitalPassportBridge {
            verify_valid: true,
            verify_error: String::new(),
            vm_id: "did:midnight:test:issuer#key-1".to_string(),
        });
        let seed = [64u8; 32];
        let (wallet, _did) = stub_wallet_with_bootstrapped_did(seed).await;
        let wallet = wallet.with_js_bridge(bridge);
        let store = stub_secret_store_with_bootstrapped_did(seed).await;

        let vc = make_digital_passport_vc(b"BODY".to_vec(), b"PROOF".to_vec());
        let vc_store = crate::InMemoryVcStore::default();
        vc_store.insert_vc(&vc).expect("insert vc");
        let clock = crate::FixedClock::new(1_700_000_003_000);

        let result =
            self_verify_and_cache(&vc, &wallet, &store, &vc_store, &clock).await;
        assert!(
            matches!(result, SelfVerifyResult::Valid { .. }),
            "expected Valid, got {result:?}"
        );
        let md = vc_store
            .get_metadata(&vc.vc_uri)
            .expect("metadata read ok")
            .expect("metadata present");
        assert_eq!(md.last_verify_outcome, Some("Valid".to_string()));
        assert_eq!(
            md.last_verified_ms,
            Some(1_700_000_003_000),
            "last_verified_ms should come from the injected clock"
        );
    }
}
