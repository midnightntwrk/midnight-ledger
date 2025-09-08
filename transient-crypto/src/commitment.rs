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

//! (Homomorphic) commitment schemes used in Midnight.
//!
//! Note that the trivial commitment schemes of [`persistent_commit`] and
//! [`transient_commit`](crate::hash::transient_commit) are instead defined in [`base_crypto::hash`].

use crate::curve::Fr;
use crate::curve::{EmbeddedFr, EmbeddedGroupAffine, embedded};
use crate::hash::hash_to_curve;
use crate::macros::wrap_display;
use crate::repr::FieldRepr;
use base_crypto::hash::{HashOutput, persistent_commit};
use base_crypto::repr::MemWrite;
use group::GroupEncoding;
#[cfg(feature = "proptest")]
use proptest_derive::Arbitrary;
use rand::{CryptoRng, Rng};
use serde::Serialize;
use serialize::{Deserializable, Serializable, Tagged, tag_enforcement_test};
use std::ops::{Add, Neg, Sub};
use storage::Storable;
use storage::db::DB;
use storage::{arena::ArenaKey, storable::Loader};

/// Homomorphic Pedersen commitment.
/// a) Summed commitments should verify against their summed randomness.
/// b) Summed commitments should be equal to a sum of (for each type) the value sum.
#[derive(
    Default, Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serializable, Serialize, Storable,
)]
#[storable(base)]
#[tag = "pedersen[v1]"]
#[serde(transparent)]
#[cfg_attr(feature = "proptest", derive(Arbitrary))]
pub struct Pedersen(pub EmbeddedGroupAffine);
wrap_display!(Pedersen);
tag_enforcement_test!(Pedersen);

/// The randomness used in the Pedersen commitments is the embedded curves prime
/// field.
pub type PedersenRandomness = EmbeddedFr;

impl From<PedersenRandomness> for Pedersen {
    fn from(rand: PedersenRandomness) -> Pedersen {
        Pedersen(EmbeddedGroupAffine::generator() * rand)
    }
}

impl FieldRepr for Pedersen {
    fn field_repr<W: MemWrite<Fr>>(&self, writer: &mut W) {
        writer.write(&[
            self.0.x().unwrap_or(0.into()),
            self.0.y().unwrap_or(0.into()),
        ]);
    }
    fn field_size(&self) -> usize {
        2
    }
}

impl Pedersen {
    /// Create a Pedersen commitment purely for randomizing powers of
    /// independent generators.
    ///
    /// Returns a random `(g^r, r)`.
    pub fn blinding_component<R: Rng + CryptoRng + ?Sized>(
        rng: &mut R,
    ) -> (Self, PedersenRandomness) {
        let rand: PedersenRandomness = rng.r#gen();
        (rand.into(), rand)
    }
}

impl Add<Pedersen> for Pedersen {
    type Output = Pedersen;
    fn add(self, other: Self) -> Self {
        Pedersen(self.0 + other.0)
    }
}

impl Neg for Pedersen {
    type Output = Pedersen;
    fn neg(self) -> Self {
        Pedersen(-self.0)
    }
}

impl Sub<Pedersen> for Pedersen {
    type Output = Pedersen;
    fn sub(self, other: Self) -> Self {
        Pedersen(self.0 - other.0)
    }
}

// Basic idea: Our type `type_: P::BaseField` is combined with a counter `ctr: P::BaseField` using
// a two-to-one hash. The result should be in `x: P::ScalarField` (conversion check needed). Find
// `y: P::ScalarField` such that `(x, y)` is a valid curve point. `(ctr, y)` are witnesses to
// `type_`.
impl Pedersen {
    /// Homomorphically commits to a value of a type.
    ///
    /// Produces: `H(type_)^v g^r`, where `H` is [`hash_to_curve`] over a
    /// [`crate::hash::transient_hash`]-reduced `type_`.
    pub fn commit<T: FieldRepr + ?Sized>(type_: &T, v: &EmbeddedFr, r: &EmbeddedFr) -> Self {
        // What we want: Given a hash-to-curve H:
        // Commit(type, v, r) = g^r H(type)^v
        let h = hash_to_curve(type_);
        let g = EmbeddedGroupAffine::generator();
        let com = g * *r + h * *v;
        Pedersen(com)
    }
}

/// A commitment of type `PedersenCom`, with only the randomization part (of base `g`),
/// and *not* the value part (of base `H(ty)`). To ensure this, a Fiat-Shamir proof of knowledge of
/// exponent is used, guaranteeing that only an exponent of `g` is known.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serializable, Serialize, Storable)]
#[storable(base)]
#[tag = "pedersen-schnorr[v1]"]
#[cfg_attr(feature = "proptest", derive(Arbitrary))]
pub struct PureGeneratorPedersen {
    /// The underlying Pedersen commitment.
    pub commitment: Pedersen,
    target: EmbeddedGroupAffine,
    reply: EmbeddedFr,
}
tag_enforcement_test!(PureGeneratorPedersen);

impl From<PureGeneratorPedersen> for Pedersen {
    fn from(com: PureGeneratorPedersen) -> Pedersen {
        com.commitment
    }
}

impl PureGeneratorPedersen {
    /// Returns an instance of the largest representable instance of this type, for use in
    /// estimating fee computations down the line.
    pub fn largest_representable() -> Self {
        let m1 = EmbeddedFr::from(0) - 1.into();
        let p = EmbeddedGroupAffine::generator();
        PureGeneratorPedersen {
            commitment: Pedersen(p),
            target: p,
            reply: m1,
        }
    }

    /// Creates a new, Fiat-Shamir evidenced Pedersen commitment with no second bases.
    /// Takes `wit`, the preimage of the commitment, and `challenge_pre`,
    /// arbitrary data that is bound in the Fiat-Shamir.
    pub fn new_from<R: Rng>(rng: &mut R, wit: &PedersenRandomness, challenge_pre: &[u8]) -> Self {
        let commitment = (*wit).into();
        let rand: EmbeddedFr = rng.r#gen();
        let target = EmbeddedGroupAffine::generator() * rand;
        let reply = rand + Self::challenge(&commitment, &target, challenge_pre) * *wit;
        PureGeneratorPedersen {
            commitment,
            target,
            reply,
        }
    }

    fn challenge(
        commitment: &Pedersen,
        target: &EmbeddedGroupAffine,
        challenge_pre: &[u8],
    ) -> EmbeddedFr {
        let mut data = Vec::<u8>::new();
        data.extend(commitment.0.0.to_bytes().as_ref());
        data.extend(target.0.to_bytes().as_ref());
        data.extend(challenge_pre);
        const DOMAIN_SEP: HashOutput = HashOutput(*b"midnight:schnorr_challenge\0\0\0\0\0\0");
        let hash_bytes: HashOutput = persistent_commit(&data[..], DOMAIN_SEP);
        let mut raw_le = [0u8; 64];
        raw_le[..32].copy_from_slice(&hash_bytes.0);
        // Yes, I know it's not uniform, but this is essentially a modular from_bytes_le
        EmbeddedFr(embedded::Scalar::from_bytes_wide(&raw_le))
    }

    /// Checks if the Fiat-Shamir proof is valid against arbitrary challenge data.
    pub fn valid(&self, challenge_pre: &[u8]) -> bool {
        let test_left = EmbeddedGroupAffine::generator() * self.reply;
        let test_right = self.target
            + self.commitment.0 * Self::challenge(&self.commitment, &self.target, challenge_pre);
        test_left == test_right
    }
}

#[cfg(test)]
mod tests {
    use rand::{Rng, RngCore, SeedableRng, rngs::StdRng};
    use serialize::Serializable;

    use super::PureGeneratorPedersen;

    #[test]
    fn test_largest_representable() {
        let claimed = PureGeneratorPedersen::largest_representable().serialized_size();
        let mut rng = StdRng::seed_from_u64(0x42);
        for _ in 0..100_000 {
            let rand = rng.r#gen();
            let mut challenge_pre = [0u8; 1024];
            rng.fill_bytes(&mut challenge_pre);
            let actual = PureGeneratorPedersen::new_from(&mut rng, &rand, &challenge_pre);
            assert!(claimed >= actual.serialized_size());
        }
    }
}
