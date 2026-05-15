//! Deterministic key derivation: seed → curve-specific private
//! bytes. Mirrors `secret-storage/src/hd-derivation.ts`.
//!
//! Pipeline:
//! 1. BIP32 HD walk: `m/44'/2400'/<account>'/Metadata/<index>` via
//!    [`crate::hd::derive_child_priv`] — same code path the
//!    wallet's controller/dust/zswap key derivation uses, with
//!    [`crate::hd::Role::Metadata`] as the role slot.
//! 2. HKDF-SHA256 from the resulting 32-byte child key, with a
//!    fixed salt and a per-call info string capturing
//!    `(kty, crv, account, index, candidate)`.
//! 3. Curve-specific normalisation: P-256 reduces modulo
//!    `n - 1` and adds 1 so the scalar is in `[1, n - 1]`.
//!
//! `candidate` is the retry slot. `file_secret_store`
//! (next commit) increments it and re-derives if a key fails a
//! curve-specific validity check (e.g. P-256 returns a non-zero
//! scalar but ed25519-dalek rejects it — happens with negligible
//! probability but the retry loop matches the upstream).

#![allow(dead_code)] // wired by file_secret_store next.

use hkdf::Hkdf;
use sha2::Sha256;

use crate::secret_storage::{
    DeriveKeyFromSeedInput, MidnightCurve, MidnightKeyType, SecretStoreError,
};

/// Per-call HKDF salt. Matches the upstream's
/// `"midnight-did-secret-storage-v1"` ASCII string.
const HKDF_SALT: &[u8] = b"midnight-did-secret-storage-v1";

/// `n - 1` for the P-256 curve order, big-endian 32 bytes. Used
/// to reduce a HKDF output into `[1, n - 1]`.
const P256_ORDER_MINUS_ONE_BE: [u8; 32] = [
    0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
    0xbc, 0xe6, 0xfa, 0xad, 0xa7, 0x17, 0x9e, 0x84, 0xf3, 0xb9, 0xca, 0xc2, 0xfc, 0x63, 0x25, 0x50,
];

/// Output of [`derive_curve_private_from_seed`] — the raw secret
/// bytes plus the curve labels (echo of the inputs, kept together
/// for the caller's convenience).
#[derive(Debug, Clone)]
pub(crate) struct DerivedPrivateKey {
    pub kty: MidnightKeyType,
    pub crv: MidnightCurve,
    pub private_bytes: Vec<u8>,
}

/// Derive a curve-specific 32-byte private key from `seedHex` +
/// the given `(account, index)` BIP32 path + a `candidate` retry
/// slot.
pub(crate) fn derive_curve_private_from_seed(
    params: &DeriveKeyFromSeedInput,
    candidate: u32,
) -> Result<DerivedPrivateKey, SecretStoreError> {
    let account = params.account.unwrap_or(0);
    let index = params.index.unwrap_or(0);

    // 32-byte seed.
    let seed_bytes = hex::decode(params.seed_hex.trim()).map_err(|e| {
        SecretStoreError::InvalidInput(format!("seedHex: {e}"))
    })?;
    let seed: [u8; 32] = seed_bytes.as_slice().try_into().map_err(|_| {
        SecretStoreError::InvalidInput(format!(
            "seedHex must decode to 32 bytes, got {}",
            seed_bytes.len()
        ))
    })?;

    // BIP32 walk via the existing wallet `hd` module — same path
    // shape as the wallet's other roles.
    let metadata_key = crate::hd::derive_child_priv(
        &seed,
        account,
        crate::hd::Role::Metadata,
        index,
    )
    .map_err(|e| SecretStoreError::Crypto(format!("BIP32 derive: {e}")))?;

    // HKDF-SHA256 with a per-call info string. Match the upstream's
    // exact byte layout so a TS-side and Rust-side derivation from
    // the SAME seed/account/index/candidate produce IDENTICAL
    // private bytes.
    let kty_str = match params.kty {
        MidnightKeyType::OKP => "OKP",
        MidnightKeyType::EC => "EC",
    };
    let crv_str = match params.crv {
        MidnightCurve::Ed25519 => "Ed25519",
        MidnightCurve::Jubjub => "Jubjub",
        MidnightCurve::P256 => "P-256",
    };
    let info = format!(
        "midnight-did:key:v1:{kty_str}:{crv_str}:{account}:{index}:{candidate}",
    );

    let hk = Hkdf::<Sha256>::new(Some(HKDF_SALT), &metadata_key);
    let mut derived = [0u8; 32];
    hk.expand(info.as_bytes(), &mut derived)
        .map_err(|e| SecretStoreError::Crypto(format!("HKDF expand: {e}")))?;

    // Curve-specific final normalisation. Ed25519 takes the 32
    // bytes raw (will be clamped inside ed25519-dalek). Jubjub
    // also takes raw 32 bytes (when we wire it). P-256 needs to
    // be reduced into `[1, n - 1]`.
    let private_bytes = match (params.kty, params.crv) {
        (MidnightKeyType::OKP, MidnightCurve::Ed25519) => derived.to_vec(),
        (MidnightKeyType::EC, MidnightCurve::Jubjub) => derived.to_vec(),
        (MidnightKeyType::EC, MidnightCurve::P256) => normalize_p256_private(&derived),
        (kty, crv) => {
            return Err(SecretStoreError::UnsupportedCurve(format!("{kty:?}/{crv:?}")));
        }
    };
    Ok(DerivedPrivateKey {
        kty: params.kty,
        crv: params.crv,
        private_bytes,
    })
}

/// Reduce a 32-byte big-endian integer modulo `(n - 1)` and add 1.
/// Result is in `[1, n - 1]` — the valid private-key range for
/// P-256. Matches the upstream's `normalizeP256Private`.
fn normalize_p256_private(input: &[u8; 32]) -> Vec<u8> {
    let n_minus_one = ru256_be(&P256_ORDER_MINUS_ONE_BE);
    let mut x = ru256_be(input);
    x = umod(&x, &n_minus_one);
    // Add 1 (no carry possible because x < n - 1).
    let one = ru256_be(&{
        let mut a = [0u8; 32];
        a[31] = 1;
        a
    });
    x = uadd(&x, &one);
    be_bytes(&x).to_vec()
}

// ── Minimal 256-bit big-int helpers ─────────────────────────────
//
// Avoid pulling in a full bignum crate just for `mod (n - 1)` + 1.
// We carry the value as a fixed-size little-endian `[u64; 4]`.

#[derive(Clone, Copy)]
struct U256([u64; 4]);

fn ru256_be(bytes: &[u8; 32]) -> U256 {
    let mut limbs = [0u64; 4];
    for i in 0..4 {
        let mut b = [0u8; 8];
        b.copy_from_slice(&bytes[(3 - i) * 8..(3 - i) * 8 + 8]);
        limbs[i] = u64::from_be_bytes(b);
    }
    U256(limbs)
}

fn be_bytes(x: &U256) -> [u8; 32] {
    let mut out = [0u8; 32];
    for i in 0..4 {
        out[(3 - i) * 8..(3 - i) * 8 + 8].copy_from_slice(&x.0[i].to_be_bytes());
    }
    out
}

fn cmp(a: &U256, b: &U256) -> std::cmp::Ordering {
    for i in (0..4).rev() {
        match a.0[i].cmp(&b.0[i]) {
            std::cmp::Ordering::Equal => continue,
            ord => return ord,
        }
    }
    std::cmp::Ordering::Equal
}

fn uadd(a: &U256, b: &U256) -> U256 {
    let mut carry: u128 = 0;
    let mut out = [0u64; 4];
    for i in 0..4 {
        let s = a.0[i] as u128 + b.0[i] as u128 + carry;
        out[i] = s as u64;
        carry = s >> 64;
    }
    U256(out)
}

fn usub(a: &U256, b: &U256) -> U256 {
    let mut borrow: i128 = 0;
    let mut out = [0u64; 4];
    for i in 0..4 {
        let d = a.0[i] as i128 - b.0[i] as i128 - borrow;
        if d < 0 {
            out[i] = (d + (1i128 << 64)) as u64;
            borrow = 1;
        } else {
            out[i] = d as u64;
            borrow = 0;
        }
    }
    U256(out)
}

/// `a mod m` via repeated subtraction. Fine here because input is
/// 256 bits and `m` is on the order of 2^256: at most one
/// subtraction. We loop for safety.
fn umod(a: &U256, m: &U256) -> U256 {
    let mut x = *a;
    while cmp(&x, m) != std::cmp::Ordering::Less {
        x = usub(&x, m);
    }
    x
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secret_storage::types::DeriveKeyFromSeedInput;

    fn input(crv: MidnightCurve, kty: MidnightKeyType) -> DeriveKeyFromSeedInput {
        // 32 bytes of zeros for a stable, deterministic seed.
        DeriveKeyFromSeedInput {
            id: "test-key".into(),
            seed_hex: "0".repeat(64),
            kty,
            crv,
            account: Some(0),
            index: Some(0),
            did: None,
            purpose: None,
        }
    }

    #[test]
    fn ed25519_derivation_is_deterministic() {
        let p = input(MidnightCurve::Ed25519, MidnightKeyType::OKP);
        let a = derive_curve_private_from_seed(&p, 0).unwrap();
        let b = derive_curve_private_from_seed(&p, 0).unwrap();
        assert_eq!(a.private_bytes, b.private_bytes);
        assert_eq!(a.private_bytes.len(), 32);
    }

    #[test]
    fn different_candidates_produce_different_keys() {
        let p = input(MidnightCurve::Ed25519, MidnightKeyType::OKP);
        let k0 = derive_curve_private_from_seed(&p, 0).unwrap();
        let k1 = derive_curve_private_from_seed(&p, 1).unwrap();
        assert_ne!(k0.private_bytes, k1.private_bytes);
    }

    #[test]
    fn different_curves_produce_different_keys() {
        let mut p_ed = input(MidnightCurve::Ed25519, MidnightKeyType::OKP);
        p_ed.id = "ed".into();
        let mut p_p256 = input(MidnightCurve::P256, MidnightKeyType::EC);
        p_p256.id = "p256".into();
        let ed = derive_curve_private_from_seed(&p_ed, 0).unwrap();
        let p256 = derive_curve_private_from_seed(&p_p256, 0).unwrap();
        // HKDF info string differs by kty + crv → different output.
        assert_ne!(ed.private_bytes, p256.private_bytes);
    }

    #[test]
    fn p256_derivation_is_in_valid_range() {
        let p = input(MidnightCurve::P256, MidnightKeyType::EC);
        let k = derive_curve_private_from_seed(&p, 0).unwrap();
        assert_eq!(k.private_bytes.len(), 32);
        // Not zero (mod-then-+1 guarantee).
        assert!(k.private_bytes.iter().any(|&b| b != 0));
        // Less than n: the high byte is < 0xff (since n-1 starts
        // with 0xffffffff00000000…, x = (input mod (n-1)) + 1 is
        // strictly less than n).
        // Cheap proxy: feed into p256 ECDSA SigningKey, which
        // checks the range internally.
        let _ = p256::ecdsa::SigningKey::from_slice(&k.private_bytes)
            .expect("P-256 normalized key must be a valid scalar");
    }

    #[test]
    fn jubjub_derivation_returns_raw_bytes() {
        let p = input(MidnightCurve::Jubjub, MidnightKeyType::EC);
        let k = derive_curve_private_from_seed(&p, 0).unwrap();
        assert_eq!(k.private_bytes.len(), 32);
    }

    #[test]
    fn rejects_bad_hex_seed() {
        let mut p = input(MidnightCurve::Ed25519, MidnightKeyType::OKP);
        p.seed_hex = "not-hex".into();
        let err = derive_curve_private_from_seed(&p, 0).unwrap_err();
        assert!(matches!(err, SecretStoreError::InvalidInput(_)));
    }

    #[test]
    fn rejects_wrong_seed_length() {
        let mut p = input(MidnightCurve::Ed25519, MidnightKeyType::OKP);
        p.seed_hex = "00".repeat(16); // 16 bytes — half a seed
        let err = derive_curve_private_from_seed(&p, 0).unwrap_err();
        assert!(matches!(err, SecretStoreError::InvalidInput(_)));
    }
}
