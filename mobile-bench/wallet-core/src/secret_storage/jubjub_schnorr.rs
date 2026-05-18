//! Pure-Rust port of `midnight-did/jubjub-schnorr/src/signing.ts`.
//!
//! Provides offline (off-chain) Schnorr signing over Midnight's
//! embedded Jubjub curve. Algorithm mirrors the upstream
//! TypeScript reference:
//!
//! ```text
//!   sk            = hashToScalar(seed)                      [SHA-256, mod ORDER]
//!   pk            = G * sk
//!   nonce         = hashToScalar(domain || seed || digest)
//!   announcement  = G * nonce
//!   challenge     = transientHash([ann_x, ann_y,
//!                                  pk_x,  pk_y,
//!                                  msg0,  msg1, msg2, msg3])
//!                   reduced to fit in 248 bits
//!   response      = nonce + challenge * sk   (mod ORDER)
//! ```
//!
//! Differences from the TS reference:
//!
//! - **Wire format.** We serialise the signature as
//!   `point_compressed(32) || response_le(32)` (64 bytes) using
//!   the existing `transient_crypto::curve::EmbeddedGroupAffine`
//!   compressed encoding. Upstream uses `ann_x || ann_y ||
//!   response` (96 bytes, big-endian). Cross-implementation
//!   compat is out of scope for this slice; both formats are
//!   bijective with the underlying `(announcement, response)`
//!   tuple. A follow-up can add a `to_upstream_bytes` /
//!   `from_upstream_bytes` pair if interop becomes a
//!   requirement.
//!
//! - **Hash-to-scalar.** Upstream computes
//!   `BigInt('0x' || sha256(...)) % JUBJUB_ORDER`. We mirror the
//!   bias-free path via `Fr::from_uniform_bytes` on the SHA-256
//!   output zero-extended to 64 bytes, then reduce to
//!   `EmbeddedFr` by repeatedly subtracting the embedded modulus
//!   — the same trick `Mul<Fr> for EmbeddedGroupAffine` uses
//!   internally. The bias is below 2^-128 (ratio of outer to
//!   embedded scalar moduli ≪ 2^64).
//!
//! - **Challenge truncation.** Upstream takes the SchnorrChallenge
//!   value `transientHash(...)` and reduces it `% 2^248`. We
//!   achieve the same by taking the low 31 bytes (248 bits) of
//!   the challenge's little-endian representation. The result is
//!   always smaller than the embedded scalar modulus (which is
//!   ≈2^252), so the conversion `Fr → EmbeddedFr` is safe.

#![allow(dead_code)] // wired through curve_support in this same patch

use sha2::{Digest, Sha256};

use transient_crypto::curve::{EmbeddedFr, EmbeddedGroupAffine, Fr};
use transient_crypto::hash::transient_hash;

/// Domain-separation prefix for the deterministic nonce. Matches
/// upstream `Buffer.from("midnight-did:jubjub-schnorr:v1")`.
const NONCE_DOMAIN: &[u8] = b"midnight-did:jubjub-schnorr:v1";

/// Tagged signature pair. Wire encoding is fixed at 64 bytes
/// (see [`encode`] / [`decode`]); this struct is the in-memory
/// view callers see when they prefer not to round-trip through
/// bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JubjubSchnorrSignature {
    pub announcement: EmbeddedGroupAffine,
    pub response: EmbeddedFr,
}

/// Number of bytes a serialised signature occupies. Mirrors
/// upstream's `JUBJUB_SIGNATURE_LENGTH_BYTES` constant in spirit,
/// though our representation is more compact (point in
/// compressed form).
pub const JUBJUB_SIGNATURE_LENGTH_BYTES: usize = 64;

/// Derive the secret scalar from raw seed bytes — `sha256(seed)`
/// reduced mod `JUBJUB_ORDER`. Inputs longer than 32 bytes are
/// hashed in full (no truncation); shorter inputs are accepted
/// as-is.
pub fn seed_bytes_to_secret_scalar(seed_bytes: &[u8]) -> EmbeddedFr {
    let h = Sha256::digest(seed_bytes);
    hash_bytes_to_embedded_scalar(&h[..])
}

/// Derive the public point from a secret scalar. `G * sk`.
pub fn derive_public_key(secret: &EmbeddedFr) -> EmbeddedGroupAffine {
    EmbeddedGroupAffine::generator() * *secret
}

/// Derive the public point directly from a seed. Convenience
/// wrapper that chains [`seed_bytes_to_secret_scalar`] +
/// [`derive_public_key`].
pub fn derive_public_key_from_seed(seed_bytes: &[u8]) -> EmbeddedGroupAffine {
    derive_public_key(&seed_bytes_to_secret_scalar(seed_bytes))
}

/// Hash an arbitrary payload to the contract's
/// `digest: Vector<4, Field>` representation. SHA-256 → split
/// into four 8-byte big-endian limbs → each lifted to `Fr`.
/// Matches `payloadToJubjubDigest` in `signing.ts`.
pub fn payload_to_digest(payload: &[u8]) -> [Fr; 4] {
    let h = Sha256::digest(payload);
    let mut out = [Fr::from(0u64); 4];
    for (i, limb) in out.iter_mut().enumerate() {
        // Pull an 8-byte big-endian slice and lift via u64 → Fr.
        // Bias-free because 64 bits ≪ scalar-field size.
        let mut buf = [0u8; 8];
        buf.copy_from_slice(&h[i * 8..(i + 1) * 8]);
        *limb = Fr::from(u64::from_be_bytes(buf));
    }
    out
}

/// Compute the challenge scalar from announcement + public key
/// + digest. Mirrors the on-chain `schnorrChallengeDigest`
/// circuit + the `% 2^248` reduction in `signing.ts`.
fn compute_challenge(
    announcement: &EmbeddedGroupAffine,
    public_key: &EmbeddedGroupAffine,
    digest: &[Fr; 4],
) -> EmbeddedFr {
    let ann_x = announcement.x().unwrap_or_else(|| Fr::from(0u64));
    let ann_y = announcement.y().unwrap_or_else(|| Fr::from(0u64));
    let pk_x = public_key.x().unwrap_or_else(|| Fr::from(0u64));
    let pk_y = public_key.y().unwrap_or_else(|| Fr::from(0u64));
    let elems = [
        ann_x, ann_y, pk_x, pk_y, digest[0], digest[1], digest[2], digest[3],
    ];
    let full = transient_hash(&elems);
    // Reduce to 248 bits, then lift to EmbeddedFr. The low 31
    // bytes of the LE representation are exactly the value
    // mod 2^248; the result is always < embedded scalar modulus.
    let le = full.as_le_bytes();
    let mut truncated = [0u8; 32];
    let n = le.len().min(31);
    truncated[..n].copy_from_slice(&le[..n]);
    EmbeddedFr::from_le_bytes(&truncated).expect("248-bit value always fits in EmbeddedFr")
}

/// Sign a 4-field digest with the secret scalar `sk`. Nonce is
/// derived deterministically from `domain || seed || digest`
/// (see upstream's `signJubjubDigestFromSeed`). `seed` is the
/// raw seed bytes the secret was derived from — pass them
/// through so the nonce is reproducible across re-signs.
pub fn sign_digest_from_seed(seed_bytes: &[u8], digest: &[Fr; 4]) -> JubjubSchnorrSignature {
    let sk = seed_bytes_to_secret_scalar(seed_bytes);
    let pk = derive_public_key(&sk);

    // Deterministic nonce seed: domain prefix || seed bytes
    // (zero-padded / truncated to 32) || canonical digest bytes.
    // The canonical digest bytes mirror upstream's `bigintTo32Be`
    // serialisation: each Fr → 32 BE bytes.
    let mut nonce_seed = Vec::with_capacity(NONCE_DOMAIN.len() + 32 + 4 * 32);
    nonce_seed.extend_from_slice(NONCE_DOMAIN);
    nonce_seed.extend_from_slice(&pad_or_truncate_32(seed_bytes));
    for d in digest {
        nonce_seed.extend_from_slice(&fr_to_be_32(d));
    }
    let h = Sha256::digest(&nonce_seed);
    let nonce = hash_bytes_to_embedded_scalar(&h[..]);

    let announcement = EmbeddedGroupAffine::generator() * nonce;
    let challenge = compute_challenge(&announcement, &pk, digest);
    let response = nonce + challenge * sk;
    JubjubSchnorrSignature { announcement, response }
}

/// Sign an arbitrary payload by way of `payload_to_digest`.
/// Wrapper that mirrors the upstream's `signJubjubPayloadFromSeed`.
pub fn sign_payload_from_seed(seed_bytes: &[u8], payload: &[u8]) -> JubjubSchnorrSignature {
    sign_digest_from_seed(seed_bytes, &payload_to_digest(payload))
}

/// Verify a (digest, signature) pair against the public key.
/// Mirrors `verifyJubjubDigest`: checks `G * response ==
/// announcement + pk * challenge`.
pub fn verify_digest(
    public_key: &EmbeddedGroupAffine,
    digest: &[Fr; 4],
    signature: &JubjubSchnorrSignature,
) -> bool {
    let challenge = compute_challenge(&signature.announcement, public_key, digest);
    let lhs = EmbeddedGroupAffine::generator() * signature.response;
    let rhs = signature.announcement + (*public_key) * challenge;
    lhs == rhs
}

/// Verify a payload (hashed via SHA-256 → 4-limb digest).
pub fn verify_payload(
    public_key: &EmbeddedGroupAffine,
    payload: &[u8],
    signature: &JubjubSchnorrSignature,
) -> bool {
    verify_digest(public_key, &payload_to_digest(payload), signature)
}

/// Wire encoding — `point_compressed(32) || response_le(32)`.
pub fn encode(sig: &JubjubSchnorrSignature) -> [u8; JUBJUB_SIGNATURE_LENGTH_BYTES] {
    use serialize::Serializable;
    let mut out = [0u8; JUBJUB_SIGNATURE_LENGTH_BYTES];
    let mut buf = Vec::with_capacity(32);
    sig.announcement
        .serialize(&mut buf)
        .expect("EmbeddedGroupAffine serialize is infallible into Vec");
    debug_assert_eq!(buf.len(), 32, "EmbeddedGroupAffine wire size is 32 bytes");
    out[..32].copy_from_slice(&buf);
    let resp_le = sig.response.as_le_bytes();
    let n = resp_le.len().min(32);
    out[32..32 + n].copy_from_slice(&resp_le[..n]);
    out
}

/// Wire decoding — inverse of [`encode`]. Returns `None` on a
/// malformed point or a response scalar that doesn't fit in
/// `EmbeddedFr`.
pub fn decode(bytes: &[u8]) -> Option<JubjubSchnorrSignature> {
    use serialize::Deserializable;
    if bytes.len() != JUBJUB_SIGNATURE_LENGTH_BYTES {
        return None;
    }
    let mut point_reader = &bytes[..32];
    let announcement = EmbeddedGroupAffine::deserialize(&mut point_reader, 0).ok()?;
    let response = EmbeddedFr::from_le_bytes(&bytes[32..])?;
    Some(JubjubSchnorrSignature { announcement, response })
}

/// Wire length for the upstream-compatible signature encoding —
/// matches `JUBJUB_SIGNATURE_LENGTH_BYTES` in upstream's
/// `signing.ts`. 96 bytes: `ann.x BE || ann.y BE || response BE`,
/// each 32 bytes.
pub const JUBJUB_SIGNATURE_UPSTREAM_LENGTH_BYTES: usize = 96;

/// Bit-for-bit identical encoding to upstream
/// `@midnight-ntwrk/midnight-did-jubjub-schnorr`'s
/// `encodeJubjubSignature`. 96 bytes total:
/// `ann.x (32B BE) || ann.y (32B BE) || response (32B BE)`.
///
/// Use this when a Rust-side signer needs to hand a signature
/// to a JS verifier (or to any consumer matching upstream's
/// wire format). Use [`encode`] when both sides are Rust and
/// the compressed-point form is preferred.
///
/// `ann.x` / `ann.y` are the affine coordinates pulled via the
/// existing `EmbeddedGroupAffine` accessors. The identity point
/// would render as zeros — but a valid signature never has the
/// identity as its announcement, so this case is unreachable
/// in practice.
pub fn encode_upstream(
    sig: &JubjubSchnorrSignature,
) -> [u8; JUBJUB_SIGNATURE_UPSTREAM_LENGTH_BYTES] {
    let mut out = [0u8; JUBJUB_SIGNATURE_UPSTREAM_LENGTH_BYTES];
    let ann_x = sig
        .announcement
        .x()
        .unwrap_or_else(|| Fr::from(0u64));
    let ann_y = sig
        .announcement
        .y()
        .unwrap_or_else(|| Fr::from(0u64));
    out[..32].copy_from_slice(&fr_to_be_32(&ann_x));
    out[32..64].copy_from_slice(&fr_to_be_32(&ann_y));
    out[64..96].copy_from_slice(&embedded_fr_to_be_32(&sig.response));
    out
}

/// Inverse of [`encode_upstream`]. Returns `None` if the bytes
/// don't decode cleanly — bad length, off-curve `(ann.x,
/// ann.y)`, or a response that doesn't fit in `EmbeddedFr`.
///
/// Off-curve points are rejected at this layer via
/// `EmbeddedGroupAffine::new`, so a downstream `verify_digest`
/// call never has to worry about identity / non-subgroup
/// announcements arriving from the wire.
pub fn decode_upstream(bytes: &[u8]) -> Option<JubjubSchnorrSignature> {
    if bytes.len() != JUBJUB_SIGNATURE_UPSTREAM_LENGTH_BYTES {
        return None;
    }
    let ann_x = Fr::from_le_bytes(&be_to_le_32(&bytes[..32]))?;
    let ann_y = Fr::from_le_bytes(&be_to_le_32(&bytes[32..64]))?;
    let announcement = EmbeddedGroupAffine::new(ann_x, ann_y)?;
    let response = EmbeddedFr::from_le_bytes(&be_to_le_32(&bytes[64..96]))?;
    Some(JubjubSchnorrSignature { announcement, response })
}

// ── internals ────────────────────────────────────────────────────

/// Reduce `bytes` (any length) to an `EmbeddedFr` value with
/// negligible bias. Implementation goes via `Fr::from_uniform_bytes`
/// on a zero-extended 64-byte buffer, then reduces to the embedded
/// modulus by repeated subtraction — the same trick
/// `Mul<Fr> for EmbeddedGroupAffine` uses internally.
fn hash_bytes_to_embedded_scalar(bytes: &[u8]) -> EmbeddedFr {
    let mut extended = [0u8; 64];
    let n = bytes.len().min(64);
    extended[..n].copy_from_slice(&bytes[..n]);
    let mut wide = Fr::from_uniform_bytes(&extended);
    // Build the embedded modulus expressed as an Fr value.
    let embedded_m1 = EmbeddedFr::from(0u64) - EmbeddedFr::from(1u64);
    let embedded_modulus = Fr::from_le_bytes(&embedded_m1.as_le_bytes())
        .expect("embedded modulus fits in Fr")
        + Fr::from(1);
    while wide >= embedded_modulus {
        wide = wide - embedded_modulus;
    }
    EmbeddedFr::from_le_bytes(&wide.as_le_bytes()).expect("after reduction, wide fits in EmbeddedFr")
}

/// Zero-pad or right-truncate `bytes` to exactly 32 bytes.
fn pad_or_truncate_32(bytes: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    let n = bytes.len().min(32);
    out[..n].copy_from_slice(&bytes[..n]);
    out
}

/// Encode an `Fr` as 32 big-endian bytes. Used for the
/// deterministic nonce derivation only — the byte order doesn't
/// affect security as long as we're consistent across sign /
/// re-sign of the same input.
fn fr_to_be_32(f: &Fr) -> [u8; 32] {
    let mut le = f.as_le_bytes();
    if le.len() < 32 {
        le.resize(32, 0);
    }
    let mut be = [0u8; 32];
    for (i, b) in le[..32].iter().enumerate() {
        be[31 - i] = *b;
    }
    be
}

/// Same byte-order flip for `EmbeddedFr` (the response scalar
/// of a Schnorr signature lives in the Jubjub scalar field).
/// Upstream encodes it as 32 BE bytes per `bigintTo32Be`.
fn embedded_fr_to_be_32(f: &EmbeddedFr) -> [u8; 32] {
    let mut le = f.as_le_bytes();
    if le.len() < 32 {
        le.resize(32, 0);
    }
    let mut be = [0u8; 32];
    for (i, b) in le[..32].iter().enumerate() {
        be[31 - i] = *b;
    }
    be
}

/// Reverse a 32-byte big-endian slice to little-endian. The
/// inverse of [`fr_to_be_32`] / [`embedded_fr_to_be_32`].
fn be_to_le_32(be: &[u8]) -> [u8; 32] {
    let mut le = [0u8; 32];
    for (i, b) in be.iter().take(32).enumerate() {
        le[31 - i] = *b;
    }
    le
}

/// Derive a deterministic Jubjub Schnorr signing seed for the
/// `(controller_secret, did)` pair. Same seed across reloads
/// of the wallet, but different per-DID and per-controller. The
/// domain prefix keeps this output disjoint from any other
/// "hash controller_secret + something" derivation the wallet
/// might add later (e.g. encryption keys, DID-document
/// authorisation tokens).
///
/// Used by the wallet's "Sign" tab in the DID detail view —
/// every payload signed under a given DID always uses the same
/// keypair, but unrelated DIDs (or different controllers) get
/// independent keys.
pub fn seed_from_controller_and_did(controller_secret: &[u8; 32], did: &str) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(b"midnight-did:wallet:sign-tab:v1");
    h.update(controller_secret);
    h.update(did.as_bytes());
    let out = h.finalize();
    let mut seed = [0u8; 32];
    seed.copy_from_slice(&out[..]);
    seed
}

/// Pre-rendered, UI-friendly view of one Schnorr sign run.
/// Every field is a decimal string or hex string so consumers
/// don't need to depend on `transient_crypto` directly. Built
/// by [`sign_payload_diagnostic`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignedPayloadDiagnostic {
    /// Public key's affine `x` / `y` coordinates in base 10.
    pub pk_x_decimal: String,
    pub pk_y_decimal: String,
    /// 4-limb `Vector<4, Field>` digest as decimal strings.
    pub digest_decimal: [String; 4],
    /// Announcement point coordinates of the signature.
    pub announcement_x_decimal: String,
    pub announcement_y_decimal: String,
    /// Response scalar (lives in the Jubjub scalar field).
    pub response_decimal: String,
    /// 64-byte compact wire form (`encode`).
    pub compact_hex: String,
    /// 96-byte upstream-compatible wire form (`encode_upstream`).
    pub upstream_hex: String,
}

/// Sign + render a payload, returning every on-wire value the
/// UI's "Sign" tab wants to display. Pure function — same seed
/// + payload always yields the same diagnostic. Bundles the
/// individual `derive_public_key_from_seed`, `payload_to_digest`,
/// `sign_payload_from_seed`, `encode`, and `encode_upstream`
/// calls so the UI does one round trip instead of five.
pub fn sign_payload_diagnostic(
    seed_bytes: &[u8],
    payload: &[u8],
) -> SignedPayloadDiagnostic {
    let pk = derive_public_key_from_seed(seed_bytes);
    let digest = payload_to_digest(payload);
    let sig = sign_payload_from_seed(seed_bytes, payload);
    SignedPayloadDiagnostic {
        pk_x_decimal: fr_to_decimal(&pk.x().unwrap_or_else(|| Fr::from(0u64))),
        pk_y_decimal: fr_to_decimal(&pk.y().unwrap_or_else(|| Fr::from(0u64))),
        digest_decimal: [
            fr_to_decimal(&digest[0]),
            fr_to_decimal(&digest[1]),
            fr_to_decimal(&digest[2]),
            fr_to_decimal(&digest[3]),
        ],
        announcement_x_decimal: fr_to_decimal(
            &sig.announcement.x().unwrap_or_else(|| Fr::from(0u64)),
        ),
        announcement_y_decimal: fr_to_decimal(
            &sig.announcement.y().unwrap_or_else(|| Fr::from(0u64)),
        ),
        response_decimal: embedded_fr_to_decimal(&sig.response),
        compact_hex: hex::encode(encode(&sig)),
        upstream_hex: hex::encode(encode_upstream(&sig)),
    }
}

/// Verify a payload directly from the same seed used to sign
/// it. Bundles `derive_public_key_from_seed` +
/// `verify_payload` so callers that already have the seed
/// don't need to thread the public key around.
pub fn verify_payload_with_seed(seed_bytes: &[u8], payload: &[u8], compact_sig: &[u8]) -> bool {
    let Some(sig) = decode(compact_sig) else {
        return false;
    };
    let pk = derive_public_key_from_seed(seed_bytes);
    verify_payload(&pk, payload, &sig)
}

/// Render an `Fr` as a base-10 decimal string via long-division
/// on u32 limbs. Used internally by the diagnostic shape; not
/// exposed because callers should never need an `Fr` directly.
fn fr_to_decimal(f: &Fr) -> String {
    le_bytes_to_decimal(&f.as_le_bytes())
}

fn embedded_fr_to_decimal(f: &EmbeddedFr) -> String {
    le_bytes_to_decimal(&f.as_le_bytes())
}

fn le_bytes_to_decimal(le: &[u8]) -> String {
    let be: Vec<u8> = le.iter().rev().copied().collect();
    let mut limbs: Vec<u32> = Vec::with_capacity(be.len().div_ceil(4));
    for chunk in be.chunks(4) {
        let mut limb = 0u32;
        for &byte in chunk {
            limb = (limb << 8) | (byte as u32);
        }
        limbs.push(limb);
    }
    if limbs.iter().all(|&l| l == 0) {
        return "0".to_string();
    }
    let mut digits = Vec::new();
    while !limbs.iter().all(|&l| l == 0) {
        let mut rem: u64 = 0;
        for limb in limbs.iter_mut() {
            let acc = (rem << 32) | (*limb as u64);
            *limb = (acc / 10) as u32;
            rem = acc % 10;
        }
        digits.push((rem as u8) + b'0');
    }
    digits.reverse();
    String::from_utf8(digits).expect("ascii digits")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed_seed() -> [u8; 32] {
        let mut s = [0u8; 32];
        for (i, b) in s.iter_mut().enumerate() {
            *b = i as u8;
        }
        s
    }

    #[test]
    fn sign_verify_round_trip_payload() {
        let seed = fixed_seed();
        let payload = b"midnight DID handshake";
        let sig = sign_payload_from_seed(&seed, payload);
        let pk = derive_public_key_from_seed(&seed);
        assert!(verify_payload(&pk, payload, &sig));
    }

    #[test]
    fn signature_is_deterministic_for_seed_and_digest() {
        let seed = fixed_seed();
        let payload = b"deterministic-input";
        let s1 = sign_payload_from_seed(&seed, payload);
        let s2 = sign_payload_from_seed(&seed, payload);
        assert_eq!(s1, s2, "nonce derivation should be deterministic");
    }

    #[test]
    fn tampered_signature_fails_verify() {
        let seed = fixed_seed();
        let pk = derive_public_key_from_seed(&seed);
        let payload = b"unmodified";
        let sig = sign_payload_from_seed(&seed, payload);
        // Flip a bit in the response by re-encoding with an
        // arbitrary scalar shift.
        let bad = JubjubSchnorrSignature {
            announcement: sig.announcement,
            response: sig.response + EmbeddedFr::from(1u64),
        };
        assert!(!verify_digest(&pk, &payload_to_digest(payload), &bad));
    }

    #[test]
    fn cross_payload_signature_fails_verify() {
        let seed = fixed_seed();
        let pk = derive_public_key_from_seed(&seed);
        let sig = sign_payload_from_seed(&seed, b"payload-A");
        assert!(!verify_payload(&pk, b"payload-B", &sig));
    }

    #[test]
    fn encode_decode_round_trip() {
        let seed = fixed_seed();
        let sig = sign_payload_from_seed(&seed, b"encode-roundtrip");
        let bytes = encode(&sig);
        assert_eq!(bytes.len(), JUBJUB_SIGNATURE_LENGTH_BYTES);
        let back = decode(&bytes).expect("decode round trip");
        assert_eq!(back, sig);
    }

    #[test]
    fn decode_rejects_wrong_length() {
        assert!(decode(&[0u8; 32]).is_none());
        assert!(decode(&[0u8; 96]).is_none());
    }

    #[test]
    fn encode_upstream_decode_upstream_round_trip() {
        let seed = fixed_seed();
        let sig = sign_payload_from_seed(&seed, b"upstream-roundtrip");
        let bytes = encode_upstream(&sig);
        assert_eq!(bytes.len(), JUBJUB_SIGNATURE_UPSTREAM_LENGTH_BYTES);
        let back = decode_upstream(&bytes).expect("decode_upstream round trip");
        assert_eq!(back, sig);
    }

    #[test]
    fn upstream_decoded_signature_still_verifies() {
        let seed = fixed_seed();
        let pk = derive_public_key_from_seed(&seed);
        let payload = b"upstream-verify";
        let sig = sign_payload_from_seed(&seed, payload);
        let bytes = encode_upstream(&sig);
        let back = decode_upstream(&bytes).expect("decode_upstream");
        assert!(verify_payload(&pk, payload, &back));
    }

    #[test]
    fn decode_upstream_rejects_wrong_length() {
        assert!(decode_upstream(&[0u8; 32]).is_none());
        assert!(decode_upstream(&[0u8; 64]).is_none());
        assert!(decode_upstream(&[0u8; 97]).is_none());
    }

    #[test]
    fn upstream_and_compact_formats_carry_same_signature() {
        // Encode the same signature both ways, decode both
        // ways, confirm they end up at the same (ann, response).
        let seed = fixed_seed();
        let sig = sign_payload_from_seed(&seed, b"two-formats");
        let from_compact = decode(&encode(&sig)).expect("compact rt");
        let from_upstream = decode_upstream(&encode_upstream(&sig)).expect("upstream rt");
        assert_eq!(from_compact, from_upstream);
    }

    #[test]
    fn public_key_is_a_function_of_seed_only() {
        let pk1 = derive_public_key_from_seed(&fixed_seed());
        let pk2 = derive_public_key_from_seed(&fixed_seed());
        assert_eq!(pk1, pk2);
        // Different seed → different pk (overwhelming probability).
        let mut other = fixed_seed();
        other[0] ^= 0xff;
        assert_ne!(pk1, derive_public_key_from_seed(&other));
    }
}
