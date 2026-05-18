//! Per-curve generate / sign / verify dispatch.
//!
//! All three Midnight DID curves are now supported:
//! - **Ed25519** via [`ed25519_dalek`] (RFC 8032 EdDSA).
//! - **P-256** via [`p256`] (NIST ECDSA-SHA-256, raw `r||s` 64-byte
//!   signature — matches the upstream JS `node:crypto` `sign("sha256", …)`
//!   default).
//! - **Jubjub Schnorr** via the in-tree
//!   [`crate::secret_storage::jubjub_schnorr`] port of
//!   `midnight-did/jubjub-schnorr/src/signing.ts`. Off-chain
//!   sign / verify only — the on-chain `schnorrVerify` circuit
//!   is exercised via the JS bridge.

// `file_secret_store` (next commit) is the only non-test caller;
// silence dead-code warnings on the intermediate API until then.
#![allow(dead_code)]

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ed25519_dalek::{Signer as _, Verifier as _};
use transient_crypto::curve::{EmbeddedGroupAffine, Fr};

use crate::secret_storage::jubjub_schnorr::{self, JUBJUB_SIGNATURE_LENGTH_BYTES};
use crate::secret_storage::{MidnightCurve, MidnightKeyType, PublicJwk, SecretStoreError};

/// Internal record of a private key — what the store actually
/// persists per entry. Lives next to its public counterpart so
/// `sign(keyRef, ...)` doesn't need to round-trip through the
/// public JWK.
#[derive(Clone, Debug)]
pub(crate) struct StoredPrivateRecord {
    pub kty: MidnightKeyType,
    pub crv: MidnightCurve,
    /// Raw secret bytes. Encoding is curve-specific:
    /// - Ed25519: 32-byte seed (the input to the keypair derivation)
    /// - P-256: 32-byte scalar (big-endian) — the raw private key
    /// - Jubjub: 32-byte scalar (when supported)
    pub private_bytes: Vec<u8>,
}

/// Generate a fresh private/public pair for the given curve.
/// `rng` is borrowed so callers can plug in a deterministic source
/// in tests.
pub(crate) fn generate(
    kty: MidnightKeyType,
    crv: MidnightCurve,
    rng: &mut (impl rand::RngCore + rand::CryptoRng),
) -> Result<(StoredPrivateRecord, PublicJwk), SecretStoreError> {
    match (kty, crv) {
        (MidnightKeyType::OKP, MidnightCurve::Ed25519) => {
            let signing = ed25519_dalek::SigningKey::generate(rng);
            let verifying = signing.verifying_key();
            let record = StoredPrivateRecord {
                kty,
                crv,
                private_bytes: signing.to_bytes().to_vec(),
            };
            let public = PublicJwk {
                kty,
                crv,
                x: URL_SAFE_NO_PAD.encode(verifying.to_bytes()),
                y: None,
            };
            Ok((record, public))
        }
        (MidnightKeyType::EC, MidnightCurve::P256) => {
            let signing = p256::ecdsa::SigningKey::random(rng);
            let public = p256_public_jwk_from(&signing)?;
            let record = StoredPrivateRecord {
                kty,
                crv,
                private_bytes: signing.to_bytes().to_vec(),
            };
            Ok((record, public))
        }
        (MidnightKeyType::EC, MidnightCurve::Jubjub) => {
            // Match the upstream signing.ts convention: the
            // private "seed" is 32 fresh random bytes; the
            // secret scalar is `sha256(seed) mod ORDER`. Sampling
            // the seed (rather than the scalar directly) keeps
            // backups / re-imports stable across runtimes.
            let mut seed = [0u8; 32];
            rng.fill_bytes(&mut seed);
            let pk = jubjub_schnorr::derive_public_key_from_seed(&seed);
            Ok((
                StoredPrivateRecord {
                    kty,
                    crv,
                    private_bytes: seed.to_vec(),
                },
                jubjub_public_jwk_from_point(&pk)?,
            ))
        }
        (kty, crv) => Err(SecretStoreError::UnsupportedCurve(format!("{kty:?}/{crv:?}"))),
    }
}

/// Reconstruct the [`StoredPrivateRecord`] + matching public JWK
/// from raw secret bytes. Used by `importKey` and
/// `deriveKeyFromSeed`. Validates `private_bytes` is the right
/// length / range for the curve.
pub(crate) fn from_private_bytes(
    kty: MidnightKeyType,
    crv: MidnightCurve,
    private_bytes: &[u8],
) -> Result<(StoredPrivateRecord, PublicJwk), SecretStoreError> {
    match (kty, crv) {
        (MidnightKeyType::OKP, MidnightCurve::Ed25519) => {
            let bytes: [u8; 32] = private_bytes
                .try_into()
                .map_err(|_| SecretStoreError::InvalidInput(format!(
                    "Ed25519 private key must be 32 bytes, got {}",
                    private_bytes.len(),
                )))?;
            let signing = ed25519_dalek::SigningKey::from_bytes(&bytes);
            let verifying = signing.verifying_key();
            Ok((
                StoredPrivateRecord {
                    kty,
                    crv,
                    private_bytes: bytes.to_vec(),
                },
                PublicJwk {
                    kty,
                    crv,
                    x: URL_SAFE_NO_PAD.encode(verifying.to_bytes()),
                    y: None,
                },
            ))
        }
        (MidnightKeyType::EC, MidnightCurve::P256) => {
            let signing = p256::ecdsa::SigningKey::from_slice(private_bytes)
                .map_err(|e| SecretStoreError::InvalidInput(format!("P-256 scalar: {e}")))?;
            let public = p256_public_jwk_from(&signing)?;
            Ok((
                StoredPrivateRecord {
                    kty,
                    crv,
                    private_bytes: signing.to_bytes().to_vec(),
                },
                public,
            ))
        }
        (MidnightKeyType::EC, MidnightCurve::Jubjub) => {
            let seed: [u8; 32] = private_bytes.try_into().map_err(|_| {
                SecretStoreError::InvalidInput(format!(
                    "Jubjub seed must be 32 bytes, got {}",
                    private_bytes.len(),
                ))
            })?;
            let pk = jubjub_schnorr::derive_public_key_from_seed(&seed);
            Ok((
                StoredPrivateRecord {
                    kty,
                    crv,
                    private_bytes: seed.to_vec(),
                },
                jubjub_public_jwk_from_point(&pk)?,
            ))
        }
        (kty, crv) => Err(SecretStoreError::UnsupportedCurve(format!("{kty:?}/{crv:?}"))),
    }
}

/// Sign `payload` with the stored private record. Returns the
/// raw signature bytes (no DER wrapping) — matches the upstream's
/// `format: "raw"` convention.
pub(crate) fn sign(
    record: &StoredPrivateRecord,
    payload: &[u8],
) -> Result<Vec<u8>, SecretStoreError> {
    match (record.kty, record.crv) {
        (MidnightKeyType::OKP, MidnightCurve::Ed25519) => {
            let bytes: [u8; 32] = record
                .private_bytes
                .as_slice()
                .try_into()
                .map_err(|_| SecretStoreError::Crypto("Ed25519 stored key wrong length".into()))?;
            let signing = ed25519_dalek::SigningKey::from_bytes(&bytes);
            let sig = signing.sign(payload);
            Ok(sig.to_bytes().to_vec())
        }
        (MidnightKeyType::EC, MidnightCurve::P256) => {
            let signing = p256::ecdsa::SigningKey::from_slice(&record.private_bytes)
                .map_err(|e| SecretStoreError::Crypto(format!("P-256 sk decode: {e}")))?;
            let sig: p256::ecdsa::Signature = signing.sign(payload);
            // ecdsa::Signature::to_bytes() is fixed-size (32 + 32)
            // big-endian `r || s`. Matches Node's
            // `sign("sha256", …, key)` default for P-256.
            Ok(sig.to_bytes().to_vec())
        }
        (MidnightKeyType::EC, MidnightCurve::Jubjub) => {
            let seed: [u8; 32] = record.private_bytes.as_slice().try_into().map_err(|_| {
                SecretStoreError::Crypto("Jubjub stored seed wrong length".into())
            })?;
            let sig = jubjub_schnorr::sign_payload_from_seed(&seed, payload);
            Ok(jubjub_schnorr::encode(&sig).to_vec())
        }
        (kty, crv) => Err(SecretStoreError::UnsupportedCurve(format!("{kty:?}/{crv:?}"))),
    }
}

/// Detached verification — public key supplied via JWK. Returns
/// `true` on a valid signature, `false` on a mismatch (caller
/// translates to [`SecretStoreError::VerificationFailed`] if it
/// wants strict semantics; we keep the boolean here for parity
/// with the upstream).
pub(crate) fn verify(
    public_jwk: &PublicJwk,
    payload: &[u8],
    signature: &[u8],
) -> Result<bool, SecretStoreError> {
    match (public_jwk.kty, public_jwk.crv) {
        (MidnightKeyType::OKP, MidnightCurve::Ed25519) => {
            let x_bytes = URL_SAFE_NO_PAD
                .decode(public_jwk.x.as_bytes())
                .map_err(|e| SecretStoreError::InvalidInput(format!("x b64url: {e}")))?;
            let x: [u8; 32] = x_bytes
                .as_slice()
                .try_into()
                .map_err(|_| SecretStoreError::InvalidInput("Ed25519 x must be 32 bytes".into()))?;
            let verifying = ed25519_dalek::VerifyingKey::from_bytes(&x)
                .map_err(|e| SecretStoreError::InvalidInput(format!("Ed25519 vk: {e}")))?;
            let sig_bytes: [u8; 64] = signature
                .try_into()
                .map_err(|_| SecretStoreError::InvalidInput("Ed25519 sig must be 64 bytes".into()))?;
            let sig = ed25519_dalek::Signature::from_bytes(&sig_bytes);
            Ok(verifying.verify(payload, &sig).is_ok())
        }
        (MidnightKeyType::EC, MidnightCurve::P256) => {
            let y_str = public_jwk.y.as_ref().ok_or_else(|| {
                SecretStoreError::InvalidInput("P-256 JWK missing y coordinate".into())
            })?;
            let x_bytes = URL_SAFE_NO_PAD
                .decode(public_jwk.x.as_bytes())
                .map_err(|e| SecretStoreError::InvalidInput(format!("P-256 x b64url: {e}")))?;
            let y_bytes = URL_SAFE_NO_PAD
                .decode(y_str.as_bytes())
                .map_err(|e| SecretStoreError::InvalidInput(format!("P-256 y b64url: {e}")))?;
            // Reconstruct via SEC1 uncompressed form `04 || x || y`.
            let mut sec1 = Vec::with_capacity(1 + 32 + 32);
            sec1.push(0x04);
            sec1.extend_from_slice(&x_bytes);
            sec1.extend_from_slice(&y_bytes);
            let verifying = p256::ecdsa::VerifyingKey::from_sec1_bytes(&sec1)
                .map_err(|e| SecretStoreError::InvalidInput(format!("P-256 vk decode: {e}")))?;
            let sig = p256::ecdsa::Signature::from_slice(signature)
                .map_err(|e| SecretStoreError::InvalidInput(format!("P-256 sig: {e}")))?;
            use p256::ecdsa::signature::Verifier;
            Ok(verifying.verify(payload, &sig).is_ok())
        }
        (MidnightKeyType::EC, MidnightCurve::Jubjub) => {
            let pk = jubjub_public_point_from_jwk(public_jwk)?;
            if signature.len() != JUBJUB_SIGNATURE_LENGTH_BYTES {
                return Err(SecretStoreError::InvalidInput(format!(
                    "Jubjub sig must be {JUBJUB_SIGNATURE_LENGTH_BYTES} bytes, got {}",
                    signature.len(),
                )));
            }
            let sig = jubjub_schnorr::decode(signature).ok_or_else(|| {
                SecretStoreError::InvalidInput("Jubjub sig: malformed wire bytes".into())
            })?;
            Ok(jubjub_schnorr::verify_payload(&pk, payload, &sig))
        }
        (kty, crv) => Err(SecretStoreError::UnsupportedCurve(format!("{kty:?}/{crv:?}"))),
    }
}

/// Helper: build a P-256 `PublicJwk` from its `SigningKey`. The
/// public key is encoded as `(x, y)` in big-endian base64url —
/// the JWK convention.
fn p256_public_jwk_from(signing: &p256::ecdsa::SigningKey) -> Result<PublicJwk, SecretStoreError> {
    let verifying = signing.verifying_key();
    let point = verifying.to_encoded_point(false); // uncompressed: 04 || x || y
    let x_bytes = point
        .x()
        .ok_or_else(|| SecretStoreError::Crypto("P-256 x extract".into()))?;
    let y_bytes = point
        .y()
        .ok_or_else(|| SecretStoreError::Crypto("P-256 y extract".into()))?;
    Ok(PublicJwk {
        kty: MidnightKeyType::EC,
        crv: MidnightCurve::P256,
        x: URL_SAFE_NO_PAD.encode(x_bytes),
        y: Some(URL_SAFE_NO_PAD.encode(y_bytes)),
    })
}

/// Build a `PublicJwk` from a Jubjub point. JWK `x` / `y` are
/// 32-byte big-endian base64url, matching upstream's
/// `bigintTo32Be` serialisation. The conversion goes via the
/// `Fr` (outer base field) representation since Jubjub
/// coordinates live there.
fn jubjub_public_jwk_from_point(pk: &EmbeddedGroupAffine) -> Result<PublicJwk, SecretStoreError> {
    let x = pk
        .x()
        .ok_or_else(|| SecretStoreError::Crypto("Jubjub pk has no affine x (identity?)".into()))?;
    let y = pk
        .y()
        .ok_or_else(|| SecretStoreError::Crypto("Jubjub pk has no affine y (identity?)".into()))?;
    Ok(PublicJwk {
        kty: MidnightKeyType::EC,
        crv: MidnightCurve::Jubjub,
        x: URL_SAFE_NO_PAD.encode(fr_to_be_32(&x)),
        y: Some(URL_SAFE_NO_PAD.encode(fr_to_be_32(&y))),
    })
}

/// Inverse of [`jubjub_public_jwk_from_point`]. Decodes `x` /
/// `y` from the JWK and reconstructs the `EmbeddedGroupAffine`
/// via `EmbeddedGroupAffine::new`. Rejects malformed
/// coordinates and off-curve points.
fn jubjub_public_point_from_jwk(jwk: &PublicJwk) -> Result<EmbeddedGroupAffine, SecretStoreError> {
    let y_str = jwk.y.as_ref().ok_or_else(|| {
        SecretStoreError::InvalidInput("Jubjub JWK missing y coordinate".into())
    })?;
    let x_bytes = URL_SAFE_NO_PAD
        .decode(jwk.x.as_bytes())
        .map_err(|e| SecretStoreError::InvalidInput(format!("Jubjub x b64url: {e}")))?;
    let y_bytes = URL_SAFE_NO_PAD
        .decode(y_str.as_bytes())
        .map_err(|e| SecretStoreError::InvalidInput(format!("Jubjub y b64url: {e}")))?;
    if x_bytes.len() != 32 || y_bytes.len() != 32 {
        return Err(SecretStoreError::InvalidInput(format!(
            "Jubjub coords must be 32 bytes each (x={}, y={})",
            x_bytes.len(),
            y_bytes.len(),
        )));
    }
    let x = Fr::from_le_bytes(&be_to_le_32(&x_bytes))
        .ok_or_else(|| SecretStoreError::InvalidInput("Jubjub x not in field".into()))?;
    let y = Fr::from_le_bytes(&be_to_le_32(&y_bytes))
        .ok_or_else(|| SecretStoreError::InvalidInput("Jubjub y not in field".into()))?;
    EmbeddedGroupAffine::new(x, y)
        .ok_or_else(|| SecretStoreError::InvalidInput("Jubjub (x,y) off curve / off subgroup".into()))
}

/// Encode an `Fr` value as 32 big-endian bytes. Used for JWK
/// `x` / `y` so the byte order matches upstream `bigintTo32Be`.
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

/// Reverse a 32-byte slice — the inverse of [`fr_to_be_32`]'s
/// byte-order flip.
fn be_to_le_32(be: &[u8]) -> [u8; 32] {
    let mut le = [0u8; 32];
    for (i, b) in be.iter().take(32).enumerate() {
        le[31 - i] = *b;
    }
    le
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    fn deterministic_rng() -> rand::rngs::StdRng {
        rand::rngs::StdRng::seed_from_u64(0xc0ffee)
    }

    #[test]
    fn ed25519_generate_sign_verify_roundtrip() {
        let mut rng = deterministic_rng();
        let (record, pk) = generate(MidnightKeyType::OKP, MidnightCurve::Ed25519, &mut rng).unwrap();
        let msg = b"hello, Ed25519";
        let sig = sign(&record, msg).unwrap();
        assert_eq!(sig.len(), 64, "Ed25519 sig must be 64 bytes");
        assert!(verify(&pk, msg, &sig).unwrap());
    }

    #[test]
    fn ed25519_verify_fails_on_tampered_signature() {
        let mut rng = deterministic_rng();
        let (record, pk) = generate(MidnightKeyType::OKP, MidnightCurve::Ed25519, &mut rng).unwrap();
        let mut sig = sign(&record, b"msg").unwrap();
        sig[0] ^= 0x01;
        assert!(!verify(&pk, b"msg", &sig).unwrap());
    }

    #[test]
    fn p256_generate_sign_verify_roundtrip() {
        let mut rng = deterministic_rng();
        let (record, pk) = generate(MidnightKeyType::EC, MidnightCurve::P256, &mut rng).unwrap();
        let msg = b"hello, P-256";
        let sig = sign(&record, msg).unwrap();
        assert_eq!(sig.len(), 64, "P-256 raw sig must be 64 bytes (r||s)");
        assert!(verify(&pk, msg, &sig).unwrap());
    }

    #[test]
    fn from_private_bytes_round_trip_ed25519() {
        let mut rng = deterministic_rng();
        let (rec1, pk1) = generate(MidnightKeyType::OKP, MidnightCurve::Ed25519, &mut rng).unwrap();
        let (rec2, pk2) =
            from_private_bytes(MidnightKeyType::OKP, MidnightCurve::Ed25519, &rec1.private_bytes)
                .unwrap();
        assert_eq!(rec1.private_bytes, rec2.private_bytes);
        assert_eq!(pk1, pk2);
    }

    #[test]
    fn jubjub_generate_sign_verify_roundtrip() {
        let mut rng = deterministic_rng();
        let (record, pk) = generate(MidnightKeyType::EC, MidnightCurve::Jubjub, &mut rng).unwrap();
        let msg = b"hello, Jubjub Schnorr";
        let sig = sign(&record, msg).unwrap();
        assert_eq!(
            sig.len(),
            JUBJUB_SIGNATURE_LENGTH_BYTES,
            "Jubjub Schnorr sig wire size is {JUBJUB_SIGNATURE_LENGTH_BYTES} bytes",
        );
        assert!(verify(&pk, msg, &sig).unwrap());
    }

    #[test]
    fn jubjub_verify_fails_on_tampered_signature() {
        let mut rng = deterministic_rng();
        let (record, pk) = generate(MidnightKeyType::EC, MidnightCurve::Jubjub, &mut rng).unwrap();
        let mut sig = sign(&record, b"jubjub-msg").unwrap();
        // Flip a bit in the response half (back of the wire).
        sig[JUBJUB_SIGNATURE_LENGTH_BYTES - 1] ^= 0x01;
        match verify(&pk, b"jubjub-msg", &sig) {
            Ok(false) => {}
            Err(_) => {}
            Ok(true) => panic!("tampered Jubjub signature must not verify"),
        }
    }

    #[test]
    fn jubjub_from_private_bytes_round_trip() {
        let mut rng = deterministic_rng();
        let (rec1, pk1) = generate(MidnightKeyType::EC, MidnightCurve::Jubjub, &mut rng).unwrap();
        let (rec2, pk2) =
            from_private_bytes(MidnightKeyType::EC, MidnightCurve::Jubjub, &rec1.private_bytes)
                .unwrap();
        assert_eq!(rec1.private_bytes, rec2.private_bytes);
        assert_eq!(pk1, pk2);
        // And the re-imported record still produces a valid signature.
        let sig = sign(&rec2, b"reimported").unwrap();
        assert!(verify(&pk2, b"reimported", &sig).unwrap());
    }

    #[test]
    fn cross_curve_pk_rejects_wrong_signature() {
        // An Ed25519 signature shouldn't validate against a P-256 pk.
        let mut rng = deterministic_rng();
        let (ed_rec, _ed_pk) = generate(MidnightKeyType::OKP, MidnightCurve::Ed25519, &mut rng).unwrap();
        let (_p_rec, p_pk) = generate(MidnightKeyType::EC, MidnightCurve::P256, &mut rng).unwrap();
        let sig = sign(&ed_rec, b"msg").unwrap();
        // verify() may surface this as an error (length mismatch) or
        // a false — either is acceptable; the key thing is it doesn't
        // accept the signature.
        let outcome = verify(&p_pk, b"msg", &sig);
        match outcome {
            Ok(false) => {} // good
            Err(_) => {}    // also good
            Ok(true) => panic!("cross-curve signature must NOT verify"),
        }
    }
}
