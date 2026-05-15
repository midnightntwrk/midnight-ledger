//! Per-curve generate / sign / verify dispatch.
//!
//! Today covers two of the three Midnight DID curves:
//! - **Ed25519** via [`ed25519_dalek`] (RFC 8032 EdDSA).
//! - **P-256** via [`p256`] (NIST ECDSA-SHA-256, raw `r||s` 64-byte
//!   signature — matches the upstream JS `node:crypto` `sign("sha256", …)`
//!   default).
//!
//! Jubjub Schnorr is the upstream's third curve; it uses a custom
//! Midnight package (`@midnight-ntwrk/midnight-did-jubjub-schnorr`)
//! that has no Rust counterpart in this tree. Stubbed to
//! [`SecretStoreError::SigningNotSupported`] for now — the secret
//! store still happily holds the keys, just can't sign with them.
//! Lands when we vendor the algorithm into a `transient-crypto`-
//! adjacent module.

// `file_secret_store` (next commit) is the only non-test caller;
// silence dead-code warnings on the intermediate API until then.
#![allow(dead_code)]

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ed25519_dalek::{Signer as _, Verifier as _};

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
        (MidnightKeyType::EC, MidnightCurve::Jubjub) => Err(jubjub_not_yet()),
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
        (MidnightKeyType::EC, MidnightCurve::Jubjub) => Err(jubjub_not_yet()),
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
        (MidnightKeyType::EC, MidnightCurve::Jubjub) => Err(jubjub_not_yet()),
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
        (MidnightKeyType::EC, MidnightCurve::Jubjub) => Err(jubjub_not_yet()),
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

/// Placeholder error for the Jubjub branch until we vendor the
/// algorithm into a Rust module.
fn jubjub_not_yet() -> SecretStoreError {
    SecretStoreError::SigningNotSupported(
        "Jubjub: vendored Schnorr algorithm not yet ported; see secret_storage::curve_support module docs".into(),
    )
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
    fn jubjub_is_unsupported_today() {
        let mut rng = deterministic_rng();
        let err = generate(MidnightKeyType::EC, MidnightCurve::Jubjub, &mut rng).unwrap_err();
        assert!(matches!(err, SecretStoreError::SigningNotSupported(_)));
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
