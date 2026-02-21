// This file is part of midnight-ledger.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");
// You may not use this file except in compliance with the License.
// You may obtain a copy of the License at
// http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Schnorr signatures over the embedded curve.
//!
//! This module provides Schnorr signature functionality using the native
//! embedded curve. The challenge is computed using a Poseidon hash over
//! the announcement coordinates, verifying key coordinates, and the message.

use crate::curve::{EmbeddedFr, EmbeddedGroupAffine, Fr, embedded};
use crate::hash::transient_hash;
use rand::{CryptoRng, Rng};

/// A Schnorr signature over the embedded curve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchnorrSignature {
    /// The announcement point, `R = k * G`.
    pub announcement: EmbeddedGroupAffine,
    /// The response scalar, `s = k + c * sk`.
    pub response: EmbeddedFr,
}

/// Computes the Schnorr challenge as Hash(ann_x || ann_y || pk_x || pk_y || msg).
///
/// The hash is computed using Poseidon over the outer curve scalar field (Fr),
/// then reduced modulo the embedded curve scalar field order.
fn compute_challenge(ann_x: Fr, ann_y: Fr, pk_x: Fr, pk_y: Fr, msg: &[Fr]) -> EmbeddedFr {
    let mut hash_input = vec![ann_x, ann_y, pk_x, pk_y];
    hash_input.extend_from_slice(msg);
    let hash = transient_hash(&hash_input);
    fr_to_embedded_fr(hash)
}

/// Converts a BLS12-381 scalar field element to a Jubjub scalar field element
/// by reducing modulo the Jubjub scalar field order.
fn fr_to_embedded_fr(fr: Fr) -> EmbeddedFr {
    let mut wide = [0u8; 64];
    wide[..32].copy_from_slice(&fr.0.to_bytes_le());
    EmbeddedFr(embedded::Scalar::from_bytes_wide(&wide))
}

/// Produces a Schnorr signature over the Jubjub curve.
pub fn sign<R: Rng + CryptoRng>(rng: &mut R, sk: EmbeddedFr, msg: &[Fr]) -> SchnorrSignature {
    let pk = EmbeddedGroupAffine::generator() * sk;
    // Unwraps cannot fail as the inner curve is an edwards curve, and the
    // identity is part of the curve (i.e., identity also has coordinates).
    let pk_x = pk.x().unwrap();
    let pk_y = pk.y().unwrap();

    let nonce: EmbeddedFr = rng.r#gen();
    let announcement = EmbeddedGroupAffine::generator() * nonce;
    let ann_x = announcement.x().unwrap();
    let ann_y = announcement.y().unwrap();

    let challenge = compute_challenge(ann_x, ann_y, pk_x, pk_y, msg);
    let response = nonce + challenge * sk;

    SchnorrSignature {
        announcement,
        response,
    }
}

/// Verifies a Schnorr signature over the Jubjub curve.
pub fn verify(pk: EmbeddedGroupAffine, msg: &[Fr], sig: &SchnorrSignature) -> bool {
    // Unwraps cannot fail as the inner curve is an edwards curve, and the
    // identity is part of the curve (i.e., identity also has coordinates).
    let pk_x = pk.x().unwrap();
    let pk_y = pk.y().unwrap();

    let ann_x = sig.announcement.x().unwrap();
    let ann_y = sig.announcement.y().unwrap();

    let challenge = compute_challenge(ann_x, ann_y, pk_x, pk_y, msg);

    let lhs = EmbeddedGroupAffine::generator() * sig.response;
    let rhs = sig.announcement + pk * challenge;

    lhs == rhs
}

/// Computes the verfying key from a secret key.
pub fn vk(sk: EmbeddedFr) -> EmbeddedGroupAffine {
    EmbeddedGroupAffine::generator() * sk
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{RngCore, SeedableRng, rngs::StdRng};

    fn generate_keypair(rng: &mut impl RngCore) -> (EmbeddedFr, EmbeddedGroupAffine) {
        let sk: EmbeddedFr = rng.r#gen();
        let pk = vk(sk);
        (sk, pk)
    }

    #[test]
    fn test_sign_verify_roundtrip() {
        let mut rng = StdRng::seed_from_u64(0x42);
        let (sk, vk) = generate_keypair(&mut rng);
        let msg = vec![Fr::from(1u64), Fr::from(2u64), Fr::from(3u64)];

        let sig = sign(&mut rng, sk, &msg);

        assert!(verify(vk, &msg, &sig), "signature should be valid");
    }

    #[test]
    fn test_wrong_message_fails() {
        let mut rng = StdRng::seed_from_u64(0x43);

        let (sk, vk) = generate_keypair(&mut rng);

        let msg = vec![Fr::from(1u64), Fr::from(2u64)];
        let wrong_msg = vec![Fr::from(1u64), Fr::from(3u64)];

        let sig = sign(&mut rng, sk, &msg);

        assert!(
            !verify(vk, &wrong_msg, &sig),
            "signature should be invalid for wrong message"
        );
    }

    #[test]
    fn test_wrong_key_fails() {
        let mut rng = StdRng::seed_from_u64(0x44);

        let (sk, _) = generate_keypair(&mut rng);
        let (_, wrong_vk) = generate_keypair(&mut rng);

        let msg = vec![Fr::from(42u64)];

        let sig = sign(&mut rng, sk, &msg);

        assert!(
            !verify(wrong_vk, &msg, &sig),
            "signature should be invalid for wrong public key"
        );
    }

    #[test]
    fn test_empty_message() {
        let mut rng = StdRng::seed_from_u64(0x45);

        let (sk, vk) = generate_keypair(&mut rng);

        let msg: Vec<Fr> = vec![];

        let sig = sign(&mut rng, sk, &msg);

        assert!(
            verify(vk, &msg, &sig),
            "signature should be valid for empty message"
        );
    }

    #[test]
    fn test_identity_pk_returns_false() {
        let mut rng = StdRng::seed_from_u64(0x46);
        let (sk, _) = generate_keypair(&mut rng);
        let msg = vec![Fr::from(1u64)];
        let sig = sign(&mut rng, sk, &msg);

        // Verify with identity verifying key should return false
        let identity_vk = EmbeddedGroupAffine::identity();
        assert!(
            !verify(identity_vk, &msg, &sig),
            "verification with identity pk should return false"
        );
    }
}
